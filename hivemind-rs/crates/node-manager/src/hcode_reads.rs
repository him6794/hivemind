#![allow(clippy::result_large_err, reason = "gRPC uses tonic::Status")]

//! Nodepool-only exchange of execution credentials for live, single-chunk reads.
//! Read tokens use the platform Ed25519 key but a disjoint type/purpose/schema.
//! Neither RPC accepts a caller-selected URL, extends readiness or reads bytes.

use anyhow::Context;
use hivemind_auth::{
    artifact_read::{ArtifactReadScope, ArtifactReadTokenSigner, ArtifactReadTokenVerifier},
    worker_execution::WorkerExecutionVerifier,
};
use hivemind_proto::{
    hcode_artifact_read_lease_service_server::HcodeArtifactReadLeaseService,
    HcodeArtifactChunkAuthorization, HcodeArtifactReadLease, HcodePeerReadAuthorization,
    HcodePeerReadRequest, IssueHcodeReadLeaseRequest,
};
use hivemind_task_scheduler::{hcode_read_leases::HcodeReadLease, task_repository::TaskRepository};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

pub const HCODE_READ_LEASE_RPC_MAX_BYTES: usize = 32 * 1024;

pub struct GrpcHcodeArtifactReadLeaseService {
    repo: TaskRepository,
    execution: WorkerExecutionVerifier,
    signer: ArtifactReadTokenSigner,
    verifier: ArtifactReadTokenVerifier,
}

fn source_id(value: &str) -> Result<Uuid, Status> {
    let id = Uuid::parse_str(value).map_err(|_| Status::invalid_argument("invalid source ID"))?;
    if id.is_nil() || id.to_string() != value {
        return Err(Status::invalid_argument("invalid source ID"));
    }
    Ok(id)
}

fn denied() -> Status {
    Status::permission_denied("live artifact read authorization is unavailable")
}

fn storage_error(error: anyhow::Error) -> Status {
    tracing::error!(%error, "hcode read lease operation failed");
    Status::internal("hcode read lease operation failed")
}

fn authorization(row: &HcodeReadLease, token_expiry: u64) -> HcodePeerReadAuthorization {
    let scope = &row.scope.0;
    HcodePeerReadAuthorization {
        source_id: row.source_id.to_string(),
        registration_id: row.registration_id.to_string(),
        task_id: scope.task_id.clone(),
        artifact_id: scope.artifact_id.clone(),
        offset: scope.offset,
        chunk: Some(HcodeArtifactChunkAuthorization {
            artifact_sha256: scope.artifact_sha256.clone(),
            artifact_size_bytes: scope.artifact_size_bytes as i64,
            chunk_sha256: scope.chunk_sha256.clone(),
            chunk_size_bytes: scope.chunk_size_bytes as i64,
        }),
        expires_at_unix_ms: row
            .expires_at
            .timestamp_millis()
            .min(i64::try_from(token_expiry.saturating_mul(1000)).unwrap_or(i64::MAX)),
    }
}

impl GrpcHcodeArtifactReadLeaseService {
    pub fn new(pool: PgPool, private_key: &str, public_key: &str) -> anyhow::Result<Self> {
        let signer = ArtifactReadTokenSigner::from_pem(private_key)?;
        let verifier = ArtifactReadTokenVerifier::from_pem(public_key)?;
        // Fail startup on mismatched operator keys rather than issuing unusable
        // capabilities. This token is never persisted or exposed on the wire.
        let id = Uuid::new_v4();
        let probe = signer.sign(
            id,
            Uuid::new_v4(),
            ArtifactReadScope {
                task_id: "key-check".into(),
                artifact_id: "key-check".into(),
                artifact_sha256: format!("sha256:{}", "0".repeat(64)),
                artifact_size_bytes: 1,
                offset: 0,
                chunk_sha256: format!("sha256:{}", "0".repeat(64)),
                chunk_size_bytes: 1,
            },
            u64::MAX,
        )?;
        verifier
            .verify(&probe, id)
            .context("artifact-read signing and verification keys do not match")?;
        Ok(Self {
            repo: TaskRepository::new(pool),
            execution: WorkerExecutionVerifier::from_pem(public_key)?,
            signer,
            verifier,
        })
    }
}

#[tonic::async_trait]
impl HcodeArtifactReadLeaseService for GrpcHcodeArtifactReadLeaseService {
    async fn issue_read_lease(
        &self,
        request: Request<IssueHcodeReadLeaseRequest>,
    ) -> Result<Response<HcodeArtifactReadLease>, Status> {
        let request = request.into_inner();
        let source = source_id(&request.source_id)?;
        let chunk = request
            .chunk
            .ok_or_else(|| Status::invalid_argument("missing chunk identity"))?;
        if chunk.task_id.is_empty()
            || chunk.task_id.len() > hivemind_proto::TASK_ID_MAX_BYTES
            || chunk.task_id == "."
            || chunk.task_id.contains("..")
            || !chunk
                .task_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || chunk.offset < 0
            || chunk.token.is_empty()
            || chunk.token.len() > hivemind_proto::HCODE_ARTIFACT_EXECUTION_TOKEN_MAX_BYTES
        {
            return Err(Status::invalid_argument("invalid artifact read request"));
        }
        general_compute_runtime::validate_artifact_id(&chunk.artifact_id)
            .map_err(Status::invalid_argument)?;
        let claims = self
            .execution
            .decode_execution_claims(&chunk.token)
            .map_err(|_| Status::unauthenticated("invalid execution token"))?;
        if claims.claims.role.as_deref() != Some("worker-execution")
            || claims.claims.task_id.as_deref() != Some(chunk.task_id.as_str())
        {
            return Err(denied());
        }
        let lease = self
            .repo
            .issue_hcode_read_lease(&claims, &chunk.artifact_id, chunk.offset, source)
            .await
            .map_err(storage_error)?
            .ok_or_else(denied)?;
        let valid_until = u64::try_from(lease.expires_at.timestamp()).map_err(|_| denied())?;
        let token = self
            .signer
            .sign(source, lease.lease_id, lease.scope.0.clone(), valid_until)
            .map_err(|_| denied())?;
        let read = self
            .verifier
            .verify(&token, source)
            .map_err(|_| Status::internal("artifact-read key verification failed"))?;
        Ok(Response::new(HcodeArtifactReadLease {
            authorization: Some(authorization(&lease, read.exp)),
            token,
            overlay_ipv4: lease.overlay_ipv4,
            port: u32::from(crate::hcode_sources::HCODE_SOURCE_PORT),
        }))
    }

    async fn validate_read_lease(
        &self,
        request: Request<HcodePeerReadRequest>,
    ) -> Result<Response<HcodePeerReadAuthorization>, Status> {
        let request = request.into_inner();
        let source = source_id(&request.source_id)?;
        let read = self
            .verifier
            .verify(&request.token, source)
            .map_err(|_| Status::unauthenticated("invalid artifact-read token"))?;
        let lease = self
            .repo
            .validate_hcode_read_lease(&read)
            .await
            .map_err(storage_error)?
            .ok_or_else(denied)?;
        Ok(Response::new(authorization(&lease, read.exp)))
    }
}

#[cfg(test)]
mod tests;

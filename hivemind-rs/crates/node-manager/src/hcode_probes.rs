#![allow(clippy::result_large_err, reason = "gRPC uses tonic::Status")]
//! Owner-authorized availability sampling, separate from execution reads.

use anyhow::Context;
use general_compute_runtime::sha256_digest;
use hivemind_auth::{
    artifact_probe::{ArtifactProbeClaims, ArtifactProbeTokenSigner, ArtifactProbeTokenVerifier},
    artifact_read::ArtifactReadScope,
};
use hivemind_proto::{
    hcode_artifact_peer_service_client::HcodeArtifactPeerServiceClient,
    hcode_artifact_probe_service_server::HcodeArtifactProbeService,
    HcodeArtifactChunkAuthorization, HcodePeerProbeRequest, HcodePeerReadAuthorization,
    HcodeSourceAvailability, HcodeSourceRegistration, HcodeSourceRequest,
};
use hivemind_task_scheduler::{hcode_probes::HcodeProbe, task_repository::TaskRepository};
use hivemind_vpn_service::VpnService;
use sqlx::PgPool;
use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tonic::{transport::Endpoint, Request, Response, Status};
use uuid::Uuid;

pub const HCODE_PROBE_RPC_MAX_BYTES: usize = 32 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct GrpcHcodeArtifactProbeService {
    repo: TaskRepository,
    vpn: Arc<VpnService>,
    signer: ArtifactProbeTokenSigner,
    verifier: ArtifactProbeTokenVerifier,
    concurrent: Semaphore,
    #[cfg(test)]
    peer_url: Option<String>,
}

fn unavailable() -> Status {
    Status::failed_precondition("live source probe is unavailable or was attempted too recently")
}
fn storage_error(error: anyhow::Error) -> Status {
    tracing::error!(%error, "hcode source probe operation failed");
    Status::internal("hcode source probe operation failed")
}
fn source_id(value: &str) -> Result<Uuid, Status> {
    let id = Uuid::parse_str(value).map_err(|_| Status::invalid_argument("invalid source ID"))?;
    if id.is_nil() || id.to_string() != value {
        return Err(Status::invalid_argument("invalid source ID"));
    }
    Ok(id)
}
fn peer_endpoint(value: &str) -> Result<String, Status> {
    let ip: Ipv4Addr = value.parse().map_err(|_| unavailable())?;
    if u32::from(ip) & 0xffc0_0000 != u32::from(Ipv4Addr::new(100, 64, 0, 0))
        || ip.to_string() != value
    {
        return Err(unavailable());
    }
    Ok(format!(
        "http://{ip}:{}",
        crate::hcode_sources::HCODE_SOURCE_PORT
    ))
}

pub(crate) fn availability(row: HcodeProbe) -> Result<HcodeSourceAvailability, Status> {
    let checked = row.verified_at.ok_or_else(unavailable)?;
    let until = row.available_until.ok_or_else(unavailable)?;
    Ok(HcodeSourceAvailability {
        source: Some(HcodeSourceRegistration {
            source_id: row.source_id.to_string(),
            registration_id: row.registration_id.to_string(),
            task_id: row.scope.task_id.clone(),
            artifact_id: row.scope.artifact_id.clone(),
            artifact_sha256: row.scope.artifact_sha256.clone(),
            artifact_size_bytes: row.scope.artifact_size_bytes as i64,
            overlay_ipv4: row.overlay_ipv4,
            port: u32::from(crate::hcode_sources::HCODE_SOURCE_PORT),
            expires_at_unix_ms: row.source_expires_at.timestamp_millis(),
            state: "registered_pending".into(),
        }),
        checked_at_unix_ms: checked.timestamp_millis(),
        available_until_unix_ms: until.timestamp_millis(),
        sampled_offset: row.scope.offset,
    })
}
fn authorization(row: HcodeProbe, claims: &ArtifactProbeClaims) -> HcodePeerReadAuthorization {
    HcodePeerReadAuthorization {
        source_id: row.source_id.to_string(),
        registration_id: row.registration_id.to_string(),
        task_id: row.scope.task_id.clone(),
        artifact_id: row.scope.artifact_id.clone(),
        offset: row.scope.offset,
        chunk: Some(HcodeArtifactChunkAuthorization {
            artifact_sha256: row.scope.artifact_sha256.clone(),
            artifact_size_bytes: row.scope.artifact_size_bytes as i64,
            chunk_sha256: row.scope.chunk_sha256.clone(),
            chunk_size_bytes: row.scope.chunk_size_bytes as i64,
        }),
        expires_at_unix_ms: row
            .expires_at
            .timestamp_millis()
            .min(i64::try_from(claims.exp.saturating_mul(1000)).unwrap_or(i64::MAX)),
    }
}

impl GrpcHcodeArtifactProbeService {
    pub fn new(
        pool: PgPool,
        vpn: Arc<VpnService>,
        private: &str,
        public: &str,
    ) -> anyhow::Result<Self> {
        let signer = ArtifactProbeTokenSigner::from_pem(private)?;
        let verifier = ArtifactProbeTokenVerifier::from_pem(public)?;
        let id = Uuid::new_v4();
        let scope = ArtifactReadScope {
            task_id: "key-check".into(),
            artifact_id: "key-check".into(),
            artifact_sha256: format!("sha256:{}", "0".repeat(64)),
            artifact_size_bytes: 1,
            offset: 0,
            chunk_sha256: format!("sha256:{}", "0".repeat(64)),
            chunk_size_bytes: 1,
        };
        verifier
            .verify(&signer.sign(id, Uuid::new_v4(), scope, u64::MAX)?, id)
            .context("artifact-probe signing and verification keys do not match")?;
        Ok(Self {
            repo: TaskRepository::new(pool),
            vpn,
            signer,
            verifier,
            concurrent: Semaphore::new(8),
            #[cfg(test)]
            peer_url: None,
        })
    }

    #[cfg(test)]
    fn with_peer_url(mut self, url: String) -> Self {
        self.peer_url = Some(url);
        self
    }

    async fn sample(&self, row: &HcodeProbe, request: HcodePeerProbeRequest) -> Result<(), Status> {
        let endpoint = peer_endpoint(&row.overlay_ipv4)?;
        // The loopback override exists only in test binaries. Production always
        // dials the literal, controller-verified overlay IPv4 and reserved port.
        #[cfg(test)]
        let endpoint = self.peer_url.clone().unwrap_or(endpoint);
        let endpoint = Endpoint::from_shared(endpoint)
            .map_err(|_| unavailable())?
            .connect_timeout(Duration::from_secs(2));
        let channel = endpoint
            .connect()
            .await
            .map_err(|_| Status::unavailable("source connection failed"))?;
        let bytes = HcodeArtifactPeerServiceClient::new(channel)
            .max_encoding_message_size(8192)
            .max_decoding_message_size(1048576 + 1024)
            .probe_chunk(request)
            .await?
            .into_inner()
            .bytes;
        if bytes.len() as u64 != row.scope.chunk_size_bytes {
            return Err(unavailable());
        }
        let digest = tokio::task::spawn_blocking(move || sha256_digest(&bytes))
            .await
            .map_err(|_| Status::internal("source probe hashing failed"))?;
        if digest != row.scope.chunk_sha256 {
            return Err(unavailable());
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl HcodeArtifactProbeService for GrpcHcodeArtifactProbeService {
    async fn probe_source(
        &self,
        request: Request<HcodeSourceRequest>,
    ) -> Result<Response<HcodeSourceAvailability>, Status> {
        let request = request.into_inner();
        let source = source_id(&request.source_id)?;
        if request.token.is_empty()
            || request.token.len() > 8192
            || !request.token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Status::invalid_argument("invalid account token"));
        }
        let owner = self
            .vpn
            .authenticate_vpn_owner(&request.token)
            .await
            .map_err(|_| Status::unauthenticated("active account authorization required"))?;
        let _permit = self
            .concurrent
            .try_acquire()
            .map_err(|_| Status::resource_exhausted("source probe limit reached"))?;
        let row = self
            .repo
            .issue_hcode_probe(source, owner.id(), owner.username(), owner.expires_at())
            .await
            .map_err(storage_error)?
            .ok_or_else(unavailable)?;
        let expiry = u64::try_from(row.expires_at.timestamp()).map_err(|_| unavailable())?;
        let token = self
            .signer
            .sign(source, row.probe_id, row.scope.0.clone(), expiry)
            .map_err(|_| unavailable())?;
        let claims = self
            .verifier
            .verify(&token, source)
            .map_err(|_| unavailable())?;
        tokio::time::timeout(
            PROBE_TIMEOUT,
            self.sample(
                &row,
                HcodePeerProbeRequest {
                    source_id: source.to_string(),
                    token,
                },
            ),
        )
        .await
        .map_err(|_| Status::unavailable("source probe timed out"))??;
        let completed = self
            .repo
            .complete_hcode_probe(&claims)
            .await
            .map_err(storage_error)?
            .ok_or_else(unavailable)?;
        Ok(Response::new(availability(completed)?))
    }

    async fn validate_probe_lease(
        &self,
        request: Request<HcodePeerProbeRequest>,
    ) -> Result<Response<HcodePeerReadAuthorization>, Status> {
        let request = request.into_inner();
        let source = source_id(&request.source_id)?;
        let claims = self
            .verifier
            .verify(&request.token, source)
            .map_err(|_| Status::unauthenticated("invalid artifact-probe token"))?;
        let row = self
            .repo
            .validate_hcode_probe(&claims)
            .await
            .map_err(storage_error)?
            .ok_or_else(unavailable)?;
        Ok(Response::new(authorization(row, &claims)))
    }
}

#[cfg(test)]
mod tests;

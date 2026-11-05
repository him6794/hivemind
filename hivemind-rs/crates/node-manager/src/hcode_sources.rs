#![allow(clippy::result_large_err, reason = "gRPC uses tonic::Status")]

//! Owner-controlled, metadata-only hcode source registration.
//!
//! A registration is a short-lived endpoint observation, not proof that bytes
//! exist and not permission to download them. It never changes artifact
//! completeness, scheduling state, execution holds or compatibility byte storage.

use chrono::{DateTime, Utc};
use hivemind_proto::{
    hcode_artifact_source_service_server::HcodeArtifactSourceService, HcodeOwnerVpnNode,
    HcodeOwnerVpnNodeRequest, HcodeSourceRegistration, HcodeSourceRequest,
    RegisterHcodeSourceRequest, RevokeHcodeSourceResponse,
};
use hivemind_vpn_service::{
    enrollment::{AuthenticatedOwner, VerifiedVpnPeer, VpnEnrollmentError},
    peer_verifier::PeerVerificationError,
    VpnService,
};
use sqlx::PgPool;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use uuid::Uuid;

/// This port is reserved for the overlay-only artifact service, not a local UI.
/// Clients cannot nominate arbitrary ports, URLs or non-overlay destinations.
pub const HCODE_SOURCE_PORT: u16 = 50054;
pub const HCODE_SOURCE_RPC_MAX_BYTES: usize = 16 * 1024;
const MAX_ACCOUNT_TOKEN_BYTES: usize = 8 * 1024;
const MAX_SOURCES_PER_ARTIFACT: i64 = 8;

pub struct GrpcHcodeArtifactSourceService {
    pool: PgPool,
    vpn: Arc<VpnService>,
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    source_id: Uuid,
    registration_id: Uuid,
    task_id: String,
    artifact_id: String,
    artifact_sha256: String,
    artifact_size_bytes: i64,
    overlay_ipv4: String,
    expires_at: DateTime<Utc>,
}

impl From<SourceRow> for HcodeSourceRegistration {
    fn from(row: SourceRow) -> Self {
        Self {
            source_id: row.source_id.to_string(),
            registration_id: row.registration_id.to_string(),
            task_id: row.task_id,
            artifact_id: row.artifact_id,
            artifact_sha256: row.artifact_sha256,
            artifact_size_bytes: row.artifact_size_bytes,
            overlay_ipv4: row.overlay_ipv4,
            port: u32::from(HCODE_SOURCE_PORT),
            expires_at_unix_ms: row.expires_at.timestamp_millis(),
            state: "registered_pending".into(),
        }
    }
}

fn storage_error(error: sqlx::Error) -> Status {
    tracing::error!(%error, "hcode source metadata operation failed");
    Status::internal("hcode source metadata operation failed")
}

fn unavailable() -> Status {
    Status::not_found("source or owned hcode artifact is unavailable")
}

fn vpn_status(error: VpnEnrollmentError) -> Status {
    match error {
        VpnEnrollmentError::Unauthorized(_) => {
            Status::unauthenticated("active account authorization required")
        }
        VpnEnrollmentError::Database(_) | VpnEnrollmentError::Configuration(_) => {
            Status::internal("VPN identity lookup failed")
        }
        VpnEnrollmentError::Controller(PeerVerificationError::NodeNotFound) => {
            Status::failed_precondition("enrolled VPN node has not joined yet")
        }
        _ => Status::failed_precondition("verified VPN enrollment required"),
    }
}

fn validate_token(token: &str) -> Result<(), Status> {
    if token.is_empty()
        || token.len() > MAX_ACCOUNT_TOKEN_BYTES
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(Status::invalid_argument(
            "invalid account token size or encoding",
        ));
    }
    Ok(())
}

fn validate_registration(request: &RegisterHcodeSourceRequest) -> Result<(), Status> {
    validate_token(&request.token)?;
    let task = &request.task_id;
    if task.is_empty()
        || task.len() > hivemind_proto::TASK_ID_MAX_BYTES
        || task == "."
        || task.contains("..")
        || !task
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        || request.vpn_client_id.is_empty()
        || request.vpn_client_id.len() > 255
        || request.headscale_node_id == 0
        || request.headscale_node_id > i64::MAX as u64
    {
        return Err(Status::invalid_argument("invalid hcode source identity"));
    }
    general_compute_runtime::validate_artifact_id(&request.artifact_id)
        .map_err(Status::invalid_argument)
}

fn source_id(request: &HcodeSourceRequest) -> Result<Uuid, Status> {
    validate_token(&request.token)?;
    let id = Uuid::parse_str(&request.source_id)
        .map_err(|_| Status::invalid_argument("invalid hcode source ID"))?;
    if id.is_nil() || id.to_string() != request.source_id {
        return Err(Status::invalid_argument("invalid hcode source ID"));
    }
    Ok(id)
}

impl GrpcHcodeArtifactSourceService {
    pub fn new(pool: PgPool, vpn: Arc<VpnService>) -> Self {
        Self { pool, vpn }
    }

    async fn owner(&self, token: &str) -> Result<AuthenticatedOwner, Status> {
        self.vpn
            .authenticate_vpn_owner(token)
            .await
            .map_err(|_| Status::unauthenticated("active account authorization required"))
    }

    // A cheap ownership check precedes the controller request. The transaction
    // below repeats every gate after the network observation and artifact lock.
    async fn owned_artifact(
        &self,
        owner: &AuthenticatedOwner,
        task: &str,
        artifact: &str,
    ) -> Result<Uuid, Status> {
        sqlx::query_scalar(
            "SELECT f.id FROM general_compute_artifacts f
             JOIN tasks t ON t.task_id = f.task_id JOIN users u ON u.username = t.owner AND u.id = f.owner_user_id
             WHERE u.id = $1 AND u.username = $2 AND u.is_active
               AND $3 > clock_timestamp() AND t.task_id = $4 AND f.artifact_id = $5
               AND t.runtime = 'hcode' AND t.status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
               AND f.expires_at > clock_timestamp()
               AND (t.deadline IS NULL OR t.deadline > clock_timestamp())",
        )
        .bind(owner.id())
        .bind(owner.username())
        .bind(owner.expires_at())
        .bind(task)
        .bind(artifact)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?
        .ok_or_else(unavailable)
    }

    async fn persist(
        &self,
        owner: &AuthenticatedOwner,
        peer: &VerifiedVpnPeer,
        artifact_row_id: Uuid,
    ) -> Result<SourceRow, Status> {
        if owner.id() != peer.owner_id() {
            return Err(Status::permission_denied(
                "source owner does not match VPN enrollment",
            ));
        }
        let node_id = i64::try_from(peer.node().node_id())
            .map_err(|_| Status::invalid_argument("invalid Headscale node ID"))?;
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        // Serialize renewals and the source-count limit for one immutable file.
        let locked: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM general_compute_artifacts WHERE id = $1 FOR UPDATE")
                .bind(artifact_row_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(storage_error)?;
        if locked.is_none() {
            return Err(unavailable());
        }
        sqlx::query(
            "DELETE FROM hcode_artifact_sources WHERE artifact_row_id = $1
             AND (expires_at <= clock_timestamp() OR revoked_at IS NOT NULL)",
        )
        .bind(artifact_row_id)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM hcode_artifact_sources
             WHERE artifact_row_id = $1 AND peer_id != $2",
        )
        .bind(artifact_row_id)
        .bind(peer.peer_id())
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
        if count >= MAX_SOURCES_PER_ARTIFACT {
            return Err(Status::resource_exhausted("artifact source limit reached"));
        }
        let row: SourceRow = sqlx::query_as(
            "WITH saved AS (
                INSERT INTO hcode_artifact_sources
                    (registration_id, artifact_row_id, owner_user_id, peer_id, enrollment_id,
                     headscale_node_id, overlay_ipv4, observed_at, expires_at)
                SELECT $1, f.id, u.id, p.id, e.enrollment_id, $2, $3, $4,
                       LEAST(clock_timestamp() + INTERVAL '60 seconds', $5, $6, f.expires_at, t.deadline)
                FROM general_compute_artifacts f JOIN tasks t ON t.task_id = f.task_id
                JOIN users u ON u.username = t.owner AND u.id = f.owner_user_id
                JOIN vpn_peer_enrollments e ON e.user_id = u.id
                JOIN vpn_peers p ON p.id = e.peer_id
                WHERE f.id = $7 AND u.id = $8 AND u.username = $9 AND u.is_active
                  AND e.peer_id = $10 AND e.enrollment_id = $11
                  AND t.runtime = 'hcode' AND t.status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
                  AND f.expires_at > clock_timestamp() AND $5 > clock_timestamp() AND $6 > clock_timestamp()
                  AND (t.deadline IS NULL OR t.deadline > clock_timestamp())
                  AND $4 <= clock_timestamp() AND $4 > clock_timestamp() - INTERVAL '15 seconds'
                ON CONFLICT (artifact_row_id, peer_id) DO UPDATE SET
                    registration_id = EXCLUDED.registration_id,
                    owner_user_id = EXCLUDED.owner_user_id, enrollment_id = EXCLUDED.enrollment_id,
                    headscale_node_id = EXCLUDED.headscale_node_id, overlay_ipv4 = EXCLUDED.overlay_ipv4,
                    observed_at = EXCLUDED.observed_at, expires_at = EXCLUDED.expires_at,
                    revoked_at = NULL, updated_at = clock_timestamp()
                RETURNING *
             )
             SELECT s.source_id, s.registration_id, t.task_id, f.artifact_id,
                    f.sha256 AS artifact_sha256, f.size_bytes AS artifact_size_bytes,
                    s.overlay_ipv4, s.expires_at
             FROM saved s JOIN general_compute_artifacts f ON f.id = s.artifact_row_id
             JOIN tasks t ON t.task_id = f.task_id",
        )
        .bind(Uuid::new_v4())
        .bind(node_id)
        .bind(peer.node().ipv4().to_string())
        .bind(peer.node().observed_at())
        .bind(peer.authorization_expires_at())
        .bind(owner.expires_at())
        .bind(artifact_row_id)
        .bind(owner.id())
        .bind(owner.username())
        .bind(peer.peer_id())
        .bind(peer.enrollment_id())
        .fetch_optional(&mut *tx)
        .await
        .map_err(storage_error)?
        .ok_or_else(unavailable)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(row)
    }

    async fn current(&self, owner: &AuthenticatedOwner, id: Uuid) -> Result<SourceRow, Status> {
        sqlx::query_as(
            "SELECT s.source_id, s.registration_id, t.task_id, f.artifact_id,
                    f.sha256 AS artifact_sha256, f.size_bytes AS artifact_size_bytes,
                    s.overlay_ipv4, s.expires_at
             FROM hcode_artifact_sources s
             JOIN general_compute_artifacts f ON f.id = s.artifact_row_id
             JOIN tasks t ON t.task_id = f.task_id
             JOIN users u ON u.id = s.owner_user_id AND u.id = f.owner_user_id AND u.username = t.owner
             JOIN vpn_peer_enrollments e ON e.peer_id = s.peer_id
                AND e.enrollment_id = s.enrollment_id AND e.user_id = u.id
             WHERE s.source_id = $1 AND u.id = $2 AND u.username = $3 AND u.is_active
               AND $4 > clock_timestamp() AND s.revoked_at IS NULL AND s.expires_at > clock_timestamp()
               AND f.expires_at > clock_timestamp() AND t.runtime = 'hcode'
               AND t.status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
               AND (t.deadline IS NULL OR t.deadline > clock_timestamp())",
        )
        .bind(id)
        .bind(owner.id())
        .bind(owner.username())
        .bind(owner.expires_at())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?
        .ok_or_else(unavailable)
    }
}

#[tonic::async_trait]
impl HcodeArtifactSourceService for GrpcHcodeArtifactSourceService {
    async fn register_source(
        &self,
        request: Request<RegisterHcodeSourceRequest>,
    ) -> Result<Response<HcodeSourceRegistration>, Status> {
        let request = request.into_inner();
        validate_registration(&request)?;
        let owner = self.owner(&request.token).await?;
        let artifact = self
            .owned_artifact(&owner, &request.task_id, &request.artifact_id)
            .await?;
        let peer = self
            .vpn
            .verify_user_vpn_peer(
                &request.token,
                &request.vpn_client_id,
                request.headscale_node_id,
            )
            .await
            .map_err(vpn_status)?;
        let row = self.persist(&owner, &peer, artifact).await?;
        Ok(Response::new(row.into()))
    }

    async fn get_source(
        &self,
        request: Request<HcodeSourceRequest>,
    ) -> Result<Response<HcodeSourceRegistration>, Status> {
        let request = request.into_inner();
        let id = source_id(&request)?;
        let owner = self.owner(&request.token).await?;
        Ok(Response::new(self.current(&owner, id).await?.into()))
    }

    async fn revoke_source(
        &self,
        request: Request<HcodeSourceRequest>,
    ) -> Result<Response<RevokeHcodeSourceResponse>, Status> {
        let request = request.into_inner();
        let id = source_id(&request)?;
        let owner = self.owner(&request.token).await?;
        let result = sqlx::query(
            "UPDATE hcode_artifact_sources s SET revoked_at = clock_timestamp(), updated_at = clock_timestamp()
             FROM users u WHERE s.source_id = $1 AND s.owner_user_id = $2 AND s.revoked_at IS NULL
               AND u.id = s.owner_user_id AND u.username = $3 AND u.is_active AND $4 > clock_timestamp()",
        )
        .bind(id)
        .bind(owner.id())
        .bind(owner.username())
        .bind(owner.expires_at())
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(Response::new(RevokeHcodeSourceResponse {
            revoked: result.rows_affected() == 1,
        }))
    }

    /// Owner-authenticated identity query. The node comes from the persisted
    /// enrollment key and a bounded control-plane listing, so the reply is a
    /// registration hint. Registration verifies the node again.
    async fn resolve_owner_vpn_node(
        &self,
        request: Request<HcodeOwnerVpnNodeRequest>,
    ) -> Result<Response<HcodeOwnerVpnNode>, Status> {
        let request = request.into_inner();
        validate_token(&request.token)?;
        if request.vpn_client_id.is_empty() || request.vpn_client_id.len() > 255 {
            return Err(Status::invalid_argument("invalid VPN client identity"));
        }
        let peer = self
            .vpn
            .resolve_user_vpn_node(&request.token, &request.vpn_client_id)
            .await
            .map_err(vpn_status)?;
        Ok(Response::new(HcodeOwnerVpnNode {
            vpn_client_id: request.vpn_client_id,
            headscale_node_id: peer.node().node_id(),
            overlay_ipv4: peer.node().ipv4().to_string(),
        }))
    }
}

#[cfg(test)]
mod tests;

//! Bind controller-issued enrollment IDs to authenticated account and VPN rows.
//! These records authenticate an overlay endpoint, not artifact availability or
//! a data-plane lease. Labels and client-reported VPN addresses are not evidence.

use crate::headscale_client::IdentifiedPreAuthKey;
use crate::peer_verifier::VerifiedOverlayNode;
use crate::VpnService;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use hivemind_auth::jwt_service::JwtService;
use uuid::Uuid;

#[derive(Debug)]
pub enum VpnEnrollmentError {
    InvalidRequest,
    Unauthorized(anyhow::Error),
    NotEnrolled,
    ControllerMismatch,
    ControllerUnavailable,
    InvalidStoredIdentity,
    StaleEnrollment,
    ExpiredObservation,
    Controller(crate::peer_verifier::PeerVerificationError),
    Database(sqlx::Error),
    Configuration(anyhow::Error),
}

impl std::fmt::Display for VpnEnrollmentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRequest => "invalid VPN peer lookup identity",
            Self::Unauthorized(_) => "VPN account authorization failed",
            Self::NotEnrolled => "no authenticated VPN enrollment for this account and client",
            Self::ControllerMismatch => {
                "VPN enrollment belongs to a different controller or namespace"
            }
            Self::ControllerUnavailable => "Headscale peer verification is not configured",
            Self::InvalidStoredIdentity => "invalid persisted VPN enrollment key ID",
            Self::StaleEnrollment => "VPN enrollment changed or expired during peer verification",
            Self::ExpiredObservation => "VPN peer authorization expired during verification",
            Self::Controller(_) => "Headscale rejected the enrolled VPN peer",
            Self::Database(_) => "VPN enrollment database operation failed",
            Self::Configuration(_) => "invalid VPN controller configuration",
        })
    }
}

impl std::error::Error for VpnEnrollmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unauthorized(error) | Self::Configuration(error) => Some(error.as_ref()),
            Self::Controller(error) => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for VpnEnrollmentError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

pub(super) struct AuthenticatedOwner {
    pub(super) id: Uuid,
    pub(super) username: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct VerifiedVpnPeer {
    enrollment_id: Uuid,
    peer_id: Uuid,
    owner_id: Uuid,
    node: VerifiedOverlayNode,
    authorization_expires_at: DateTime<Utc>,
}

impl VerifiedVpnPeer {
    pub fn enrollment_id(&self) -> Uuid {
        self.enrollment_id
    }

    pub fn peer_id(&self) -> Uuid {
        self.peer_id
    }

    pub fn owner_id(&self) -> Uuid {
        self.owner_id
    }

    pub fn node(&self) -> &VerifiedOverlayNode {
        &self.node
    }

    /// A later lease must also respect task, source, session and assignment state.
    pub fn authorization_expires_at(&self) -> DateTime<Utc> {
        self.authorization_expires_at
    }
}

#[derive(sqlx::FromRow)]
struct EnrollmentSnapshot {
    enrollment_id: Uuid,
    peer_id: Uuid,
    controller_url: String,
    namespace: String,
    headscale_key_id: i64,
}

impl VpnService {
    pub(super) async fn authenticate_vpn_owner(&self, token: &str) -> Result<AuthenticatedOwner> {
        let claims = JwtService::new(
            &self.config.auth.jwt_secret,
            self.config.auth.token_expiry_hours,
        )
        .validate(token)?;
        if !matches!(claims.role.as_deref(), None | Some("user") | Some("admin"))
            || claims.task_id.is_some()
            || claims.worker_id.is_some()
        {
            anyhow::bail!("VPN enrollment requires an account token");
        }
        let id = Uuid::parse_str(&claims.user_id).context("invalid VPN account identity")?;
        let expires_at = i64::try_from(claims.exp)
            .ok()
            .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
            .context("invalid VPN account token expiry")?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users
             WHERE id = $1 AND username = $2 AND is_active = true
               AND $3 > clock_timestamp())",
        )
        .bind(id)
        .bind(&claims.sub)
        .bind(expires_at)
        .fetch_one(&self.db.pool)
        .await?;
        if !active {
            anyhow::bail!("VPN account is inactive, mismatched or expired");
        }
        Ok(AuthenticatedOwner {
            id,
            username: claims.sub,
            expires_at,
        })
    }

    pub(super) async fn persist_user_vpn_enrollment(
        &self,
        owner: &AuthenticatedOwner,
        client_id: &str,
        hostname: &str,
        suggested_ip: &str,
        issued: &IdentifiedPreAuthKey,
    ) -> Result<()> {
        let controller_url = self.client.controller_url()?;
        let mut tx = self.db.pool.begin().await?;
        // Serialize with account deletion/deactivation, including changes made
        // while the Headscale issuance request was in flight.
        let active: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM users WHERE id = $1 AND username = $2
             AND is_active = true FOR UPDATE",
        )
        .bind(owner.id)
        .bind(&owner.username)
        .fetch_optional(&mut *tx)
        .await?;
        let unexpired: bool = sqlx::query_scalar("SELECT $1 > clock_timestamp()")
            .bind(owner.expires_at)
            .fetch_one(&mut *tx)
            .await?;
        if active.is_none() || !unexpired {
            anyhow::bail!("VPN account changed or expired during enrollment");
        }
        let peer_id: Uuid = sqlx::query_scalar(
            "INSERT INTO vpn_peers (worker_id, hostname, virtual_ip, auth_key, online, last_seen)
             VALUES ($1, $2, $3, $4, false, NOW())
             ON CONFLICT (worker_id) DO UPDATE SET
                 hostname = EXCLUDED.hostname, virtual_ip = EXCLUDED.virtual_ip,
                 auth_key = EXCLUDED.auth_key, online = false, last_seen = NOW()
             RETURNING id",
        )
        .bind(client_id)
        .bind(hostname)
        .bind(suggested_ip)
        .bind(issued.key())
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO vpn_peer_enrollments
                (enrollment_id, peer_id, user_id, controller_url, namespace, headscale_key_id)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (peer_id) DO UPDATE SET
                 enrollment_id = EXCLUDED.enrollment_id, user_id = EXCLUDED.user_id,
                 controller_url = EXCLUDED.controller_url, namespace = EXCLUDED.namespace,
                 headscale_key_id = EXCLUDED.headscale_key_id, issued_at = NOW()",
        )
        .bind(Uuid::new_v4())
        .bind(peer_id)
        .bind(owner.id)
        .bind(controller_url)
        .bind(&self.headscale_user)
        .bind(issued.id())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Resolve only a controller-enrolled node belonging to the authenticated
    /// owner. Re-issuance/revocation during the HTTP lookup invalidates its result.
    /// No caller-supplied key ID, URL, namespace or IP enters this lookup.
    pub async fn verify_user_vpn_peer(
        &self,
        token: &str,
        client_id: &str,
        node_id: u64,
    ) -> std::result::Result<VerifiedVpnPeer, VpnEnrollmentError> {
        if client_id.is_empty() || client_id.len() > 255 || node_id == 0 {
            return Err(VpnEnrollmentError::InvalidRequest);
        }
        let owner = self
            .authenticate_vpn_owner(token)
            .await
            .map_err(VpnEnrollmentError::Unauthorized)?;
        let snapshot: EnrollmentSnapshot = sqlx::query_as(
            "SELECT e.enrollment_id, e.peer_id, e.controller_url, e.namespace, e.headscale_key_id
             FROM vpn_peer_enrollments e JOIN vpn_peers p ON p.id = e.peer_id
             WHERE p.worker_id = $1 AND e.user_id = $2",
        )
        .bind(client_id)
        .bind(owner.id)
        .fetch_optional(&self.db.pool)
        .await?
        .ok_or(VpnEnrollmentError::NotEnrolled)?;
        let controller_url = self
            .client
            .controller_url()
            .map_err(VpnEnrollmentError::Configuration)?;
        if snapshot.controller_url != controller_url || snapshot.namespace != self.headscale_user {
            return Err(VpnEnrollmentError::ControllerMismatch);
        }
        let key_id = u64::try_from(snapshot.headscale_key_id)
            .map_err(|_| VpnEnrollmentError::InvalidStoredIdentity)?;
        let verifier = self
            .peer_verifier
            .as_ref()
            .ok_or(VpnEnrollmentError::ControllerUnavailable)?;
        let node = verifier
            .verify_node(node_id, key_id)
            .await
            .map_err(VpnEnrollmentError::Controller)?;
        let still_current: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM vpn_peer_enrollments e
                JOIN vpn_peers p ON p.id = e.peer_id JOIN users u ON u.id = e.user_id
                WHERE e.enrollment_id = $1 AND e.peer_id = $2 AND e.user_id = $3
                  AND e.controller_url = $4 AND e.namespace = $5 AND e.headscale_key_id = $6
                  AND p.worker_id = $7 AND u.username = $8 AND u.is_active = true
                  AND $9 > clock_timestamp()
             )",
        )
        .bind(snapshot.enrollment_id)
        .bind(snapshot.peer_id)
        .bind(owner.id)
        .bind(&snapshot.controller_url)
        .bind(&snapshot.namespace)
        .bind(snapshot.headscale_key_id)
        .bind(client_id)
        .bind(&owner.username)
        .bind(owner.expires_at)
        .fetch_one(&self.db.pool)
        .await?;
        if !still_current {
            return Err(VpnEnrollmentError::StaleEnrollment);
        }
        let authorization_expires_at = node
            .expires_at()
            .map(|expiry| expiry.min(owner.expires_at))
            .unwrap_or(owner.expires_at);
        if authorization_expires_at <= Utc::now() {
            return Err(VpnEnrollmentError::ExpiredObservation);
        }
        Ok(VerifiedVpnPeer {
            enrollment_id: snapshot.enrollment_id,
            peer_id: snapshot.peer_id,
            owner_id: owner.id,
            node,
            authorization_expires_at,
        })
    }

    /// Revoke only the binding, not the remote node or an execution/billing hold.
    pub async fn revoke_user_vpn_enrollment(
        &self,
        token: &str,
        client_id: &str,
    ) -> std::result::Result<bool, VpnEnrollmentError> {
        if client_id.is_empty() || client_id.len() > 255 {
            return Err(VpnEnrollmentError::InvalidRequest);
        }
        let owner = self
            .authenticate_vpn_owner(token)
            .await
            .map_err(VpnEnrollmentError::Unauthorized)?;
        let result = sqlx::query(
            "DELETE FROM vpn_peer_enrollments e USING vpn_peers p, users u
             WHERE p.id = e.peer_id AND p.worker_id = $1 AND e.user_id = $2
               AND u.id = e.user_id AND u.username = $3 AND u.is_active = true
               AND $4 > clock_timestamp()",
        )
        .bind(client_id)
        .bind(owner.id)
        .bind(owner.username)
        .bind(owner.expires_at)
        .execute(&self.db.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests;

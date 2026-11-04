//! Pre-dispatch sampling and short-lived source observations. No artifact
//! completeness, task state, execution hold or compatibility content is changed.

use crate::task_repository::TaskRepository;
use anyhow::Result;
use chrono::{DateTime, Utc};
use hivemind_auth::{artifact_probe::ArtifactProbeClaims, artifact_read::ArtifactReadScope};
use sqlx::{types::Json, FromRow};
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub struct HcodeProbe {
    pub probe_id: Uuid,
    pub source_id: Uuid,
    pub registration_id: Uuid,
    pub scope: Json<ArtifactReadScope>,
    pub overlay_ipv4: String,
    pub expires_at: DateTime<Utc>,
    pub source_expires_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub available_until: Option<DateTime<Utc>>,
}

const SCOPE: &str = "jsonb_build_object(
    'task_id', t.task_id, 'artifact_id', f.artifact_id,
    'artifact_sha256', f.sha256, 'artifact_size_bytes', f.size_bytes,
    'offset', m.offset_bytes, 'chunk_sha256', m.sha256, 'chunk_size_bytes', m.size_bytes)";
const SOURCE_FROM: &str = "FROM hcode_artifact_sources s
    JOIN general_compute_artifacts f ON f.id = s.artifact_row_id
    JOIN tasks t ON t.task_id = f.task_id
    JOIN users u ON u.id = s.owner_user_id AND u.id = f.owner_user_id AND u.username = t.owner
    JOIN vpn_peer_enrollments e ON e.peer_id = s.peer_id
        AND e.enrollment_id = s.enrollment_id AND e.user_id = u.id
    JOIN general_compute_artifact_manifest_chunks m ON m.task_id = f.task_id AND m.artifact_id = f.artifact_id";
const SOURCE_GATE: &str = "u.is_active AND t.runtime = 'hcode'
    AND t.status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
    AND s.revoked_at IS NULL AND s.expires_at > clock_timestamp()
    AND f.expires_at > clock_timestamp()
    AND (t.deadline IS NULL OR t.deadline > clock_timestamp())
    AND m.offset_bytes >= 0 AND m.size_bytes BETWEEN 1 AND 1048576";
const COLUMNS: &str = "p.probe_id, p.source_id, p.registration_id, p.scope,
    p.overlay_ipv4, p.expires_at, p.source_expires_at, p.verified_at, p.available_until";

fn live_from() -> String {
    format!(
        "{SOURCE_FROM} JOIN hcode_artifact_probes p
        ON p.source_id = s.source_id AND p.registration_id = s.registration_id
        AND p.enrollment_id = s.enrollment_id AND p.owner_user_id = u.id
        AND p.artifact_row_id = f.id AND p.overlay_ipv4 = s.overlay_ipv4
        AND p.scope = {SCOPE}"
    )
}

/// Reused by execution-source discovery. Probe expiry is deliberately different
/// from observation expiry; each result still checks current source/owner state.
pub(crate) fn available_sources_sql() -> String {
    let from = live_from();
    format!("SELECT {COLUMNS}, f.id AS artifact_row_id,
            LEAST(p.available_until, s.expires_at, f.expires_at, t.deadline, p.owner_expires_at) AS live_until
        {from} WHERE {SOURCE_GATE}
        AND p.owner_expires_at > clock_timestamp()
        AND p.verified_at <= clock_timestamp() AND p.available_until > clock_timestamp()")
}

impl TaskRepository {
    /// Caller must supply an owner authenticated by Nodepool. No caller-provided
    /// offset or endpoint: a random full persisted manifest chunk is sampled.
    pub async fn issue_hcode_probe(
        &self,
        source: Uuid,
        owner: Uuid,
        username: &str,
        owner_expiry: DateTime<Utc>,
    ) -> Result<Option<HcodeProbe>> {
        if source.is_nil() || owner.is_nil() {
            return Ok(None);
        }
        let sql = format!("WITH candidate AS (
            SELECT s.source_id, s.registration_id, s.enrollment_id, f.id AS artifact_row_id,
                u.id AS owner_user_id, {SCOPE} AS scope, s.overlay_ipv4, s.expires_at AS source_expires_at,
                LEAST(clock_timestamp() + INTERVAL '15 seconds', $4, s.expires_at, f.expires_at, t.deadline) AS expires_at
            {SOURCE_FROM} WHERE {SOURCE_GATE}
                AND s.source_id = $1 AND u.id = $2 AND u.username = $3 AND $4 > clock_timestamp()
            ORDER BY random() LIMIT 1
        ) INSERT INTO hcode_artifact_probes AS p
            (source_id, probe_id, registration_id, enrollment_id, artifact_row_id, owner_user_id,
             scope, overlay_ipv4, owner_expires_at, source_expires_at, started_at, expires_at)
          SELECT source_id, $5, registration_id, enrollment_id, artifact_row_id, owner_user_id,
             scope, overlay_ipv4, $4, source_expires_at, clock_timestamp(), expires_at FROM candidate
          ON CONFLICT (source_id) DO UPDATE SET
             probe_id = EXCLUDED.probe_id, registration_id = EXCLUDED.registration_id,
             enrollment_id = EXCLUDED.enrollment_id, artifact_row_id = EXCLUDED.artifact_row_id,
             owner_user_id = EXCLUDED.owner_user_id, scope = EXCLUDED.scope,
             overlay_ipv4 = EXCLUDED.overlay_ipv4, owner_expires_at = EXCLUDED.owner_expires_at,
             source_expires_at = EXCLUDED.source_expires_at,
             started_at = EXCLUDED.started_at, expires_at = EXCLUDED.expires_at,
             verified_at = NULL, available_until = NULL
          WHERE p.started_at <= clock_timestamp() - INTERVAL '3 seconds'
          RETURNING {COLUMNS}");
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(source)
            .bind(owner)
            .bind(username)
            .bind(owner_expiry)
            .bind(Uuid::new_v4())
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn validate_hcode_probe(
        &self,
        claims: &ArtifactProbeClaims,
    ) -> Result<Option<HcodeProbe>> {
        let Some(expiry) = i64::try_from(claims.exp)
            .ok()
            .and_then(|v| DateTime::from_timestamp(v, 0))
        else {
            return Ok(None);
        };
        let from = live_from();
        let sql = format!(
            "SELECT {COLUMNS} {from} WHERE {SOURCE_GATE}
            AND p.source_id = $1 AND p.probe_id = $2 AND p.scope = $3
            AND p.expires_at > clock_timestamp() AND p.owner_expires_at > clock_timestamp()
            AND $4 > clock_timestamp() AND $4 <= p.expires_at"
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(claims.aud)
            .bind(claims.lease_id)
            .bind(Json(&claims.scope))
            .bind(expiry)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Only Nodepool calls this after hashing the complete sampled chunk. Repeat
    /// all fences after the network await, and never revive a superseded probe.
    pub async fn complete_hcode_probe(
        &self,
        claims: &ArtifactProbeClaims,
    ) -> Result<Option<HcodeProbe>> {
        let Some(expiry) = i64::try_from(claims.exp)
            .ok()
            .and_then(|v| DateTime::from_timestamp(v, 0))
        else {
            return Ok(None);
        };
        let from = live_from();
        let sql = format!(
            "WITH current_probe AS (
            SELECT p.probe_id, LEAST(clock_timestamp() + INTERVAL '30 seconds',
                p.owner_expires_at, s.expires_at, f.expires_at, t.deadline) AS available_until
            {from} WHERE {SOURCE_GATE}
            AND p.source_id = $1 AND p.probe_id = $2 AND p.scope = $3
            AND p.expires_at > clock_timestamp() AND p.owner_expires_at > clock_timestamp()
            AND $4 > clock_timestamp() AND $4 <= p.expires_at
        ) UPDATE hcode_artifact_probes p SET verified_at = clock_timestamp(),
            available_until = current_probe.available_until
          FROM current_probe WHERE p.probe_id = current_probe.probe_id
          RETURNING {COLUMNS}"
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(claims.aud)
            .bind(claims.lease_id)
            .bind(Json(&claims.scope))
            .bind(expiry)
            .fetch_optional(&self.pool)
            .await?)
    }
}

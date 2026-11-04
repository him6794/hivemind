//! Persisted peer-read capabilities. Every read repeats the current-assignment
//! predicate and the source/owner/enrollment fences in a single SQL snapshot.
//! Neither issuing nor validating a lease reads artifact content or changes holds.

use crate::task_repository::{hcode_artifact_chunk_sql, HcodeChunkRead, TaskRepository};
use anyhow::Result;
use chrono::{DateTime, Utc};
use hivemind_auth::{
    artifact_read::{ArtifactReadClaims, ArtifactReadScope},
    worker_execution::WorkerExecutionClaims,
};
use sqlx::{postgres::PgArguments, query::QueryAs, types::Json, FromRow, Postgres};
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub struct HcodeReadLease {
    pub lease_id: Uuid,
    pub source_id: Uuid,
    pub registration_id: Uuid,
    pub scope: Json<ArtifactReadScope>,
    pub overlay_ipv4: String,
    pub expires_at: DateTime<Utc>,
}

// This adds identity and strict wall-clock fences without weakening the shared
// compatibility predicate. Pending-only artifacts still cannot obtain leases.
/// `source_predicate` is a fixed fragment chosen by this module; every caller
/// value is bound as a parameter. Execution reads always pin one source ID.
fn candidate_sql(source_predicate: &str) -> String {
    let assignment = hcode_artifact_chunk_sql(HcodeChunkRead::PeerLease);
    format!(
        "SELECT a.*, s.source_id, s.registration_id, s.enrollment_id,
                s.overlay_ipv4, s.expires_at AS source_deadline
         FROM ({assignment}) a
         JOIN hcode_artifact_sources s ON s.artifact_row_id = a.artifact_row_id
         JOIN users u ON u.id = a.owner_user_id AND u.id = s.owner_user_id
                     AND u.username = a.owner AND u.is_active
         JOIN vpn_peer_enrollments e ON e.peer_id = s.peer_id
                     AND e.enrollment_id = s.enrollment_id AND e.user_id = u.id
         WHERE {source_predicate} AND s.revoked_at IS NULL
           AND s.expires_at > clock_timestamp()
           AND a.artifact_deadline > clock_timestamp()
           AND a.attempt_deadline > clock_timestamp()
           AND (a.task_deadline IS NULL OR a.task_deadline > clock_timestamp())
           AND COALESCE(a.transfer_generation, 1) = 1"
    )
}

fn bind_assignment<'q, T>(
    query: QueryAs<'q, Postgres, T, PgArguments>,
    claims: &'q WorkerExecutionClaims,
    artifact: &'q str,
    offset: i64,
) -> QueryAs<'q, Postgres, T, PgArguments> {
    query
        .bind(&claims.claims.task_id)
        .bind(&claims.claims.sub)
        .bind(&claims.claims.worker_id)
        .bind(&claims.execution_id)
        .bind(&claims.attempt_id)
        .bind(&claims.request_digest)
        .bind(&claims.idempotency_key)
        .bind(artifact)
        .bind(offset)
        .bind(&claims.consensus_round_id)
        .bind(&claims.replica_id)
        .bind(claims.consensus_protocol_version.map(i32::from))
        .bind(claims.transfer_generation)
}

impl TaskRepository {
    /// Only sources with a live sampled observation are disclosed, and only
    /// through the same current-assignment gates as execution reads.
    pub async fn discover_hcode_sources(
        &self,
        claims: &WorkerExecutionClaims,
        artifact: &str,
        offset: i64,
    ) -> Result<Vec<crate::hcode_probes::HcodeProbe>> {
        if claims.claims.role.as_deref() != Some("worker-execution") || offset < 0 {
            return Ok(Vec::new());
        }
        let Some(expiry) = i64::try_from(claims.claims.exp)
            .ok()
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        else {
            return Ok(Vec::new());
        };
        let candidate = candidate_sql("($14::uuid IS NULL OR s.source_id = $14)");
        let available = crate::hcode_probes::available_sources_sql();
        let sql = format!(
            "SELECT p.probe_id, p.source_id, p.registration_id,
                p.scope, p.overlay_ipv4, p.expires_at, p.source_expires_at, p.verified_at,
                LEAST(p.live_until, $15, a.attempt_deadline) AS available_until
            FROM ({candidate}) a JOIN ({available}) p ON p.source_id = a.source_id
                AND p.registration_id = a.registration_id AND p.artifact_row_id = a.artifact_row_id
            WHERE $15 > clock_timestamp() ORDER BY p.live_until DESC, p.source_id LIMIT 8"
        );
        Ok(bind_assignment(
            sqlx::query_as(sqlx::AssertSqlSafe(sql)),
            claims,
            artifact,
            offset,
        )
        .bind(None::<Uuid>)
        .bind(expiry)
        .fetch_all(&self.pool)
        .await?)
    }

    /// `claims` must come from Nodepool's execution-token verifier. As with other
    /// repository authorization methods, decoding a JWT is the RPC boundary's job.
    pub async fn issue_hcode_read_lease(
        &self,
        claims: &WorkerExecutionClaims,
        artifact: &str,
        offset: i64,
        source: Uuid,
    ) -> Result<Option<HcodeReadLease>> {
        if claims.claims.role.as_deref() != Some("worker-execution")
            || offset < 0
            || source.is_nil()
        {
            return Ok(None);
        }
        let Some(execution_expiry) = i64::try_from(claims.claims.exp)
            .ok()
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        else {
            return Ok(None);
        };
        let candidate = candidate_sql("s.source_id = $14");
        let sql = format!(
            "WITH candidate AS ({candidate})
             INSERT INTO hcode_artifact_read_leases
                 (lease_id, source_id, registration_id, enrollment_id, artifact_row_id,
                  replica_row_id, owner_user_id, offset_bytes, execution_claims,
                  scope, overlay_ipv4, expires_at)
             SELECT $15, source_id, registration_id, enrollment_id, artifact_row_id,
                    replica_row_id, owner_user_id, offset_bytes, $16, scope, overlay_ipv4,
                    LEAST(clock_timestamp() + INTERVAL '60 seconds', $17,
                          source_deadline, artifact_deadline, attempt_deadline, task_deadline)
             FROM candidate WHERE $17 > clock_timestamp()
             ON CONFLICT (source_id, replica_row_id, offset_bytes) DO UPDATE SET
                 lease_id = EXCLUDED.lease_id, registration_id = EXCLUDED.registration_id,
                 enrollment_id = EXCLUDED.enrollment_id, artifact_row_id = EXCLUDED.artifact_row_id,
                 owner_user_id = EXCLUDED.owner_user_id, execution_claims = EXCLUDED.execution_claims,
                 scope = EXCLUDED.scope, overlay_ipv4 = EXCLUDED.overlay_ipv4,
                 expires_at = EXCLUDED.expires_at
             RETURNING lease_id, source_id, registration_id, scope, overlay_ipv4, expires_at"
        );
        // Only fixed internal SQL fragments are composed; all request values bind.
        Ok(bind_assignment(
            sqlx::query_as(sqlx::AssertSqlSafe(sql)),
            claims,
            artifact,
            offset,
        )
        .bind(source)
        .bind(Uuid::new_v4())
        .bind(Json(claims))
        .bind(execution_expiry)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// The read token must already have passed purpose, signature and audience
    /// validation. Loading saved claims is not authorization: the final query
    /// rechecks that the same lease still exists and all live gates still hold.
    pub async fn validate_hcode_read_lease(
        &self,
        read: &ArtifactReadClaims,
    ) -> Result<Option<HcodeReadLease>> {
        let claims: Option<Json<WorkerExecutionClaims>> = sqlx::query_scalar(
            "SELECT execution_claims FROM hcode_artifact_read_leases
             WHERE lease_id = $1 AND source_id = $2",
        )
        .bind(read.lease_id)
        .bind(read.aud)
        .fetch_optional(&self.pool)
        .await?;
        let Some(Json(claims)) = claims else {
            return Ok(None);
        };
        if claims.claims.role.as_deref() != Some("worker-execution") {
            return Ok(None);
        }
        let (Ok(offset), Some(expiry)) = (
            i64::try_from(read.scope.offset),
            i64::try_from(read.exp)
                .ok()
                .and_then(|seconds| DateTime::from_timestamp(seconds, 0)),
        ) else {
            return Ok(None);
        };
        let candidate = candidate_sql("s.source_id = $14");
        let sql = format!(
            "WITH candidate AS ({candidate})
             SELECT l.lease_id, l.source_id, l.registration_id, l.scope,
                    l.overlay_ipv4, LEAST(l.expires_at, $17) AS expires_at
             FROM candidate c JOIN hcode_artifact_read_leases l
               ON l.source_id = c.source_id AND l.registration_id = c.registration_id
              AND l.enrollment_id = c.enrollment_id AND l.artifact_row_id = c.artifact_row_id
              AND l.replica_row_id = c.replica_row_id AND l.owner_user_id = c.owner_user_id
              AND l.overlay_ipv4 = c.overlay_ipv4 AND l.scope = c.scope
             WHERE l.lease_id = $15 AND l.scope = $16
               AND l.expires_at > clock_timestamp()
               AND $17 > clock_timestamp() AND $17 <= l.expires_at"
        );
        Ok(bind_assignment(
            sqlx::query_as(sqlx::AssertSqlSafe(sql)),
            &claims,
            &read.scope.artifact_id,
            offset,
        )
        .bind(read.aud)
        .bind(read.lease_id)
        .bind(Json(&read.scope))
        .bind(expiry)
        .fetch_optional(&self.pool)
        .await?)
    }
}

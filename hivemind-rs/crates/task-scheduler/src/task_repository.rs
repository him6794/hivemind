use anyhow::Result;
use chrono::{DateTime, Utc};
use general_compute_runtime::{GeneralComputeRequest, GeneralComputeResult, ResultStatus};
use hivemind_models::{Task, TaskStatus, WorkerNode, PUBLIC_DYNAMIC_ADMISSION_MODE};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use sqlx::{FromRow, PgPool, Postgres, Row, Transaction};

use crate::{scheduler::PUBLIC_DYNAMIC_CAPABILITY_MAX_AGE_SECS, BatchTaskReport};

pub struct TaskRepository {
    pub pool: PgPool,
}

/// Nodepool-owned immutable identity and lifecycle state for one general-
/// compute artifact. Attempt manifests may rotate, but this row does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneralComputeArtifactState {
    pub artifact_id: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub expected_chunk_count: u64,
    pub availability_status: String,
    pub complete: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Nodepool-owned lease for one general-compute transfer attempt. A Worker
/// never chooses the generation; it is allocated transactionally when the
/// task assignment changes and is revoked before redispatch.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct GeneralComputeTransferLease {
    pub task_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub state: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl GeneralComputeTransferLease {
    pub fn matches_assignment(
        &self,
        task_id: &str,
        execution_id: &str,
        attempt_id: &str,
        worker_id: &str,
    ) -> bool {
        self.state == "active"
            && self.task_id == task_id
            && self.execution_id == execution_id
            && self.attempt_id == attempt_id
            && self.worker_id == worker_id
            && self.generation > 0
            && self.expires_at.is_none_or(|expiry| expiry > Utc::now())
    }
}

/// Nodepool-owned metadata for one managed-proof authorization. The bearer
/// token and task payload are intentionally not represented here; only the
/// immutable binding, issuance metadata, and non-reusable audit hash are
/// persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedProofAuthorizationRecord {
    pub task_id: String,
    pub protocol_version: u16,
    pub proof_task_id: String,
    pub owner: String,
    pub worker_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub lease_generation: i64,
    pub runtime: String,
    pub backend_id: String,
    pub semantics_manifest_sha256: String,
    pub proof_scheme: String,
    pub image_id_json: String,
    pub deadline_unix_ms: i64,
    pub token_jti: String,
    pub token_iat: i64,
    pub token_exp: i64,
    pub token_sha256: String,
}

/// The issuance metadata selected for an authorization attempt. Callers use
/// this to reconstruct the token in memory and compare its hash with the
/// Nodepool-owned fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedProofAuthorizationIssuance {
    pub token_jti: String,
    pub token_iat: i64,
    pub token_exp: i64,
    pub token_sha256: String,
}

/// Complete identity and target state for one authorization lifecycle update.
/// Keeping these fields together prevents callers from accidentally mixing
/// identity from one attempt with the state of another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedProofAuthorizationStateUpdate<'a> {
    pub task_id: &'a str,
    pub lease_generation: i64,
    pub attempt_id: &'a str,
    pub worker_id: &'a str,
    pub execution_id: &'a str,
    pub idempotency_key: &'a str,
    pub request_digest: &'a str,
    pub state: &'a str,
}

const PLATFORM_FEE_BPS: i64 = 1000; // 10%
const MANAGED_BASE_INVOCATION_CPT: i64 = 1;
const GENERAL_COMPUTE_BILLING_VERSION: &str = "billing-v1";
const GENERAL_COMPUTE_COST_MODEL_VERSION: &str = "cost-v1";
pub(crate) const MIN_WORKER_REPUTATION_SCORE: i32 = 20;

struct ManagedCompletionReceipt<'a> {
    executed_ops: i64,
    output_bytes: i64,
    receipt_json: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedCompletionEvidence {
    /// The Nodepool dispatcher independently verified and persisted a receipt.
    VerifiedReceipt,
    /// Observe mode verified a receipt but intentionally settles through the
    /// legacy path; this is recorded separately from receipt-backed settlement.
    ObservedVerified,
    /// An explicitly selected compatibility path settled without a receipt.
    LegacyFallback,
    /// Generic repository completion has no managed-proof authority.
    Untrusted,
}

/// Nodepool-owned settlement evidence for an alpha general-compute result.
/// Worker usage remains an unverified claim; the amount is the task's fixed
/// reservation and never a worker-selected variable price.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneralComputeSettlement {
    worker_id: String,
    execution_id: String,
    attempt_id: String,
    idempotency_key: String,
    request_digest: String,
    billing_version: String,
    cost_model_version: String,
    usage_claim_json: Vec<u8>,
    evidence_level: String,
    basis: String,
    amount_cpt: i64,
}

impl TaskRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn is_worker_trusted(score: i32, banned: bool) -> bool {
        !banned && score >= MIN_WORKER_REPUTATION_SCORE
    }

    pub(crate) async fn trusted_workers(&self, workers: &[WorkerNode]) -> Result<Vec<WorkerNode>> {
        if workers.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<String> = workers.iter().map(|w| w.worker_id.clone()).collect();
        let rows: Vec<(String, i32, bool)> = sqlx::query_as(
            "SELECT worker_id, score, banned
             FROM worker_reputation
             WHERE worker_id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;

        let trust_map: std::collections::HashMap<String, (i32, bool)> = rows
            .into_iter()
            .map(|(worker_id, score, banned)| (worker_id, (score, banned)))
            .collect();

        Ok(workers
            .iter()
            .filter(|worker| match trust_map.get(&worker.worker_id) {
                Some((score, banned)) => Self::is_worker_trusted(*score, *banned),
                None => false,
            })
            .cloned()
            .collect())
    }

    pub async fn create(&self, task: &Task) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        if task.runtime.as_deref() == Some("production_sandboxed_dsl")
            && (task
                .managed_dsl_backend_id
                .as_deref()
                .is_none_or(|backend_id| backend_id.trim().is_empty())
                || task.managed_dsl_semantics_manifest_sha256.as_deref()
                    != Some(general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256))
        {
            anyhow::bail!("production_sandboxed_dsl task identity is incomplete or invalid");
        }
        let created = sqlx::query_as::<_, Task>(
            "INSERT INTO tasks (task_id, owner, status, status_message, torrent_source, runtime, task_source, general_compute_manifest_json, managed_dsl_backend_id, managed_dsl_semantics_manifest_sha256, expected_btih,
             req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
             host_count, max_cpt, max_retries, deadline,
             deterministic, side_effects, priority, created_at, last_update)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,NOW(),NOW()) RETURNING *",
        )
        .bind(&task.task_id).bind(&task.owner)
        .bind(task.status.as_str()).bind(&task.status_message)
        .bind(&task.torrent_source).bind(&task.runtime).bind(&task.task_source)
        .bind(&task.general_compute_manifest_json)
        .bind(&task.managed_dsl_backend_id)
        .bind(&task.managed_dsl_semantics_manifest_sha256)
        .bind(&task.expected_btih)
        .bind(task.req_cpu_score).bind(task.req_gpu_score)
        .bind(task.req_memory_gb).bind(task.req_gpu_memory_gb)
        .bind(task.req_storage_gb)
        .bind(task.host_count).bind(task.max_cpt).bind(task.max_retries)
        .bind(task.deadline).bind(task.deterministic).bind(task.side_effects).bind(task.priority)
        .fetch_one(&mut *tx).await?;

        if task.runtime.as_deref() == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
        {
            let manifest = task
                .general_compute_manifest_json
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("general-compute request manifest is missing"))?;
            let request: GeneralComputeRequest = serde_json::from_slice(manifest)
                .map_err(|_| anyhow::anyhow!("general-compute request manifest is malformed"))?;
            request.validate().map_err(|error| {
                anyhow::anyhow!("general-compute request manifest is invalid: {error:?}")
            })?;
            for artifact in
                std::iter::once(&request.source_artifact).chain(request.input_artifacts.iter())
            {
                let artifact_size = i64::try_from(artifact.size_bytes).map_err(|_| {
                    anyhow::anyhow!("general-compute artifact size exceeds database range")
                })?;
                let expected_chunk_count = i64::try_from(artifact.chunks.len()).map_err(|_| {
                    anyhow::anyhow!("general-compute artifact chunk count exceeds database range")
                })?;
                sqlx::query(
                    "INSERT INTO general_compute_artifacts
                        (task_id, artifact_id, sha256, size_bytes, expected_chunk_count,
                         availability_status, complete, expires_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                )
                .bind(&task.task_id)
                .bind(&artifact.artifact_id)
                .bind(&artifact.sha256)
                .bind(artifact_size)
                .bind(expected_chunk_count)
                .bind(if artifact.inline_bytes.is_some() {
                    "available"
                } else {
                    "pending"
                })
                .bind(artifact.inline_bytes.is_some())
                .bind(task.deadline)
                .execute(&mut *tx)
                .await?;
                for chunk in &artifact.chunks {
                    sqlx::query(
                        "INSERT INTO general_compute_artifact_manifest_chunks
                            (task_id, artifact_id, offset_bytes, size_bytes, sha256)
                         VALUES ($1, $2, $3, $4, $5)",
                    )
                    .bind(&task.task_id)
                    .bind(&artifact.artifact_id)
                    .bind(i64::try_from(chunk.offset).map_err(|_| {
                        anyhow::anyhow!("general-compute chunk offset exceeds database range")
                    })?)
                    .bind(i64::try_from(chunk.size_bytes).map_err(|_| {
                        anyhow::anyhow!("general-compute chunk size exceeds database range")
                    })?)
                    .bind(&chunk.sha256)
                    .execute(&mut *tx)
                    .await?;
                }
                let Some(bytes) = artifact.inline_bytes.as_deref() else {
                    continue;
                };
                sqlx::query(
                    "INSERT INTO general_compute_artifact_sources
                        (task_id, artifact_id, sha256, size_bytes, content, expires_at)
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(&task.task_id)
                .bind(&artifact.artifact_id)
                .bind(&artifact.sha256)
                .bind(artifact_size)
                .bind(bytes)
                .bind(task.deadline)
                .execute(&mut *tx)
                .await?;
                for chunk in &artifact.chunks {
                    let start = usize::try_from(chunk.offset).map_err(|_| {
                        anyhow::anyhow!("general-compute chunk offset is too large")
                    })?;
                    let end = start
                        .checked_add(usize::try_from(chunk.size_bytes).map_err(|_| {
                            anyhow::anyhow!("general-compute chunk size is too large")
                        })?)
                        .ok_or_else(|| anyhow::anyhow!("general-compute chunk range overflows"))?;
                    let chunk_bytes = bytes.get(start..end).ok_or_else(|| {
                        anyhow::anyhow!("general-compute chunk range exceeds inline bytes")
                    })?;
                    sqlx::query(
                        "INSERT INTO general_compute_artifact_chunks
                            (task_id, artifact_id, offset_bytes, size_bytes, sha256, content)
                         VALUES ($1, $2, $3, $4, $5, $6)",
                    )
                    .bind(&task.task_id)
                    .bind(&artifact.artifact_id)
                    .bind(i64::try_from(chunk.offset).map_err(|_| {
                        anyhow::anyhow!("general-compute chunk offset exceeds database range")
                    })?)
                    .bind(i64::try_from(chunk.size_bytes).map_err(|_| {
                        anyhow::anyhow!("general-compute chunk size exceeds database range")
                    })?)
                    .bind(&chunk.sha256)
                    .bind(chunk_bytes)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        tx.commit().await?;
        Ok(created)
    }

    /// Read a Nodepool-owned inline artifact source only when its immutable
    /// manifest coordinates match the persisted source row. This is the sole
    /// scheduler raw-byte source for repopulating a Worker CAS.
    #[allow(clippy::type_complexity)]
    pub async fn general_compute_artifact_bytes(
        &self,
        task_id: &str,
        artifact_id: &str,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<Option<Vec<u8>>> {
        self.expire_general_compute_artifact(task_id, artifact_id)
            .await?;
        let Some(state) = self
            .general_compute_artifact_state(task_id, artifact_id)
            .await?
        else {
            return Ok(None);
        };
        if !state.complete
            || state.availability_status != "available"
            || state.sha256 != sha256
            || state.size_bytes != size_bytes
        {
            return Ok(None);
        }
        let expected_size = i64::try_from(size_bytes)
            .map_err(|_| anyhow::anyhow!("general-compute artifact size exceeds database range"))?;
        let row: Option<(String, i64, Vec<u8>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT sha256, size_bytes, content, expires_at
             FROM general_compute_artifact_sources
             WHERE task_id = $1 AND artifact_id = $2",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((stored_sha256, stored_size, content, expires_at)) = row {
            if stored_sha256 != sha256
                || stored_size != expected_size
                || expires_at.is_some_and(|expiry| expiry <= Utc::now())
            {
                return Ok(None);
            }
            let digest = general_compute_runtime::sha256_digest(&content);
            return Ok((content.len() as u64 == size_bytes && digest == sha256).then_some(content));
        }

        let chunks: Vec<(i64, i64, String, Vec<u8>)> = sqlx::query_as(
            "SELECT offset_bytes, size_bytes, sha256, content
             FROM general_compute_artifact_chunks
             WHERE task_id = $1 AND artifact_id = $2
             ORDER BY offset_bytes ASC",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_all(&self.pool)
        .await?;
        if chunks.is_empty() {
            let manifest: Option<(Vec<u8>,)> = sqlx::query_as(
                "SELECT general_compute_manifest_json
                 FROM tasks
                 WHERE task_id = $1 AND runtime = $2",
            )
            .bind(task_id)
            .bind(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
            .fetch_optional(&self.pool)
            .await?;
            let Some((manifest,)) = manifest else {
                return Ok(None);
            };
            let request: GeneralComputeRequest = serde_json::from_slice(&manifest)
                .map_err(|_| anyhow::anyhow!("general-compute task manifest is malformed"))?;
            request.validate().map_err(|error| {
                anyhow::anyhow!("general-compute task manifest is invalid: {error:?}")
            })?;
            let Some(artifact) = std::iter::once(&request.source_artifact)
                .chain(request.input_artifacts.iter())
                .find(|artifact| artifact.artifact_id == artifact_id)
            else {
                return Ok(None);
            };
            if artifact.sha256 != sha256 || artifact.size_bytes != size_bytes {
                return Ok(None);
            }
            let Some(content) = artifact.inline_bytes.clone() else {
                return Ok(None);
            };
            return Ok((content.len() as u64 == size_bytes
                && general_compute_runtime::sha256_digest(&content) == sha256)
                .then_some(content));
        }
        let mut assembled = Vec::with_capacity(size_bytes as usize);
        let mut expected_offset = 0u64;
        for (offset, chunk_size, chunk_sha256, content) in chunks {
            let offset =
                u64::try_from(offset).map_err(|_| anyhow::anyhow!("negative chunk offset"))?;
            let chunk_size =
                u64::try_from(chunk_size).map_err(|_| anyhow::anyhow!("negative chunk size"))?;
            if offset != expected_offset
                || chunk_size != content.len() as u64
                || general_compute_runtime::sha256_digest(&content) != chunk_sha256
            {
                return Ok(None);
            }
            expected_offset = expected_offset
                .checked_add(chunk_size)
                .ok_or_else(|| anyhow::anyhow!("general-compute artifact size overflows"))?;
            assembled.extend_from_slice(&content);
        }
        Ok((expected_offset == size_bytes
            && general_compute_runtime::sha256_digest(&assembled) == sha256)
            .then_some(assembled))
    }

    /// Read the immutable Nodepool-owned artifact identity and its persisted
    /// availability state. Expiry is materialized before the row is returned
    /// so callers never treat an expired source as available.
    #[allow(clippy::type_complexity)]
    pub async fn general_compute_artifact_state(
        &self,
        task_id: &str,
        artifact_id: &str,
    ) -> Result<Option<GeneralComputeArtifactState>> {
        self.expire_general_compute_artifact(task_id, artifact_id)
            .await?;
        let row: Option<(
            String,
            String,
            i64,
            i64,
            String,
            bool,
            Option<DateTime<Utc>>,
        )> = sqlx::query_as(
            "SELECT artifact_id, sha256, size_bytes, expected_chunk_count,
                        availability_status, complete, expires_at
                 FROM general_compute_artifacts
                 WHERE task_id = $1 AND artifact_id = $2",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(
            |(
                artifact_id,
                sha256,
                size_bytes,
                expected_chunk_count,
                availability_status,
                complete,
                expires_at,
            )| {
                Ok(GeneralComputeArtifactState {
                    artifact_id,
                    sha256,
                    size_bytes: u64::try_from(size_bytes)
                        .map_err(|_| anyhow::anyhow!("negative general-compute artifact size"))?,
                    expected_chunk_count: u64::try_from(expected_chunk_count)
                        .map_err(|_| anyhow::anyhow!("negative general-compute chunk count"))?,
                    availability_status,
                    complete,
                    expires_at,
                })
            },
        )
        .transpose()
    }

    /// Verify that a current attempt's artifact coordinates still match the
    /// task-bound immutable identity and chunk manifest. The attempt may
    /// rotate, but it cannot redefine which bytes an upload refers to.
    pub async fn general_compute_artifact_coordinates_match(
        &self,
        task_id: &str,
        artifact_id: &str,
        size_bytes: u64,
        sha256: &str,
        chunks: &[general_compute_runtime::ArtifactChunk],
    ) -> Result<bool> {
        let state = if let Some(state) = self
            .general_compute_artifact_state(task_id, artifact_id)
            .await?
        {
            state
        } else {
            self.ensure_general_compute_artifact_identity(task_id, artifact_id)
                .await?;
            let Some(state) = self
                .general_compute_artifact_state(task_id, artifact_id)
                .await?
            else {
                return Ok(false);
            };
            if state.size_bytes != size_bytes
                || state.sha256 != sha256
                || state.expected_chunk_count != chunks.len() as u64
                || state.availability_status == "expired"
            {
                return Ok(false);
            }
            state
        };
        if state.size_bytes != size_bytes
            || state.sha256 != sha256
            || state.expected_chunk_count != chunks.len() as u64
            || state.availability_status == "expired"
        {
            return Ok(false);
        }
        let persisted: Vec<(i64, i64, String)> = sqlx::query_as(
            "SELECT offset_bytes, size_bytes, sha256
             FROM general_compute_artifact_manifest_chunks
             WHERE task_id = $1 AND artifact_id = $2
             ORDER BY offset_bytes ASC",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(persisted.len() == chunks.len()
            && persisted
                .iter()
                .zip(chunks.iter())
                .all(|((offset, size, digest), chunk)| {
                    u64::try_from(*offset).ok() == Some(chunk.offset)
                        && u64::try_from(*size).ok() == Some(chunk.size_bytes)
                        && digest == &chunk.sha256
                }))
    }

    async fn expire_general_compute_artifact(
        &self,
        task_id: &str,
        artifact_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE general_compute_artifacts
             SET availability_status = 'expired', complete = false, updated_at = NOW()
             WHERE task_id = $1 AND artifact_id = $2
               AND expires_at IS NOT NULL AND expires_at <= NOW()
               AND availability_status <> 'expired'",
        )
        .bind(task_id)
        .bind(artifact_id)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE general_compute_artifact_sources
             SET expires_at = COALESCE(expires_at, (
                 SELECT expires_at FROM general_compute_artifacts
                 WHERE task_id = $1 AND artifact_id = $2
             ))
             WHERE task_id = $1 AND artifact_id = $2",
        )
        .bind(task_id)
        .bind(artifact_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Persist one Nodepool-owned, manifest-bound chunk. Identical retries are
    /// idempotent; a conflicting payload is rejected before later resume.
    pub async fn put_general_compute_artifact_chunk(
        &self,
        task_id: &str,
        artifact_id: &str,
        offset: u64,
        size_bytes: u64,
        sha256: &str,
        content: &[u8],
    ) -> Result<()> {
        self.expire_general_compute_artifact(task_id, artifact_id)
            .await?;
        let state = sqlx::query_as::<
            _,
            (
                String,
                String,
                i64,
                i64,
                String,
                bool,
                Option<DateTime<Utc>>,
            ),
        >(
            "SELECT artifact_id, sha256, size_bytes, expected_chunk_count,
                    availability_status, complete, expires_at
             FROM general_compute_artifacts
             WHERE task_id = $1 AND artifact_id = $2",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_optional(&self.pool)
        .await?;
        if state.is_none() {
            self.ensure_general_compute_artifact_identity(task_id, artifact_id)
                .await?;
        }
        let mut tx = self.pool.begin().await?;
        let state = sqlx::query_as::<
            _,
            (
                String,
                String,
                i64,
                i64,
                String,
                bool,
                Option<DateTime<Utc>>,
            ),
        >(
            "SELECT artifact_id, sha256, size_bytes, expected_chunk_count,
                    availability_status, complete, expires_at
             FROM general_compute_artifacts
             WHERE task_id = $1 AND artifact_id = $2
             FOR UPDATE",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((
            _artifact_id,
            expected_sha256,
            expected_size,
            expected_chunk_count,
            status,
            complete,
            expires_at,
        )) = state
        else {
            anyhow::bail!("general-compute artifact identity is missing");
        };
        if status == "expired" || expires_at.is_some_and(|expires_at| expires_at <= Utc::now()) {
            anyhow::bail!("general-compute artifact is expired");
        }
        if expected_sha256.trim().is_empty() {
            anyhow::bail!("general-compute artifact identity is invalid");
        }
        let expected_size = u64::try_from(expected_size)
            .map_err(|_| anyhow::anyhow!("negative general-compute artifact size"))?;
        if size_bytes > expected_size {
            anyhow::bail!("general-compute artifact chunk exceeds artifact size");
        }
        let chunk: Option<(i64, i64, String)> =
            sqlx::query_as(
                "SELECT offset_bytes, size_bytes, sha256
             FROM general_compute_artifact_manifest_chunks
             WHERE task_id = $1 AND artifact_id = $2 AND offset_bytes = $3",
            )
            .bind(task_id)
            .bind(artifact_id)
            .bind(i64::try_from(offset).map_err(|_| {
                anyhow::anyhow!("general-compute chunk offset exceeds database range")
            })?)
            .fetch_optional(&mut *tx)
            .await?;
        let Some((manifest_offset, manifest_size, manifest_sha256)) = chunk else {
            anyhow::bail!("artifact chunk coordinates do not match immutable manifest");
        };
        if u64::try_from(manifest_offset).ok() != Some(offset)
            || u64::try_from(manifest_size).ok() != Some(size_bytes)
            || manifest_sha256 != sha256
        {
            anyhow::bail!("artifact chunk coordinates do not match immutable manifest");
        }
        if size_bytes > general_compute_runtime::transport::MAX_CHUNK_UPLOAD_BYTES as u64 {
            anyhow::bail!("general-compute artifact chunk exceeds the upload limit");
        }
        let size = i64::try_from(size_bytes)
            .map_err(|_| anyhow::anyhow!("general-compute chunk size exceeds database range"))?;
        if content.len() as u64 != size_bytes
            || general_compute_runtime::sha256_digest(content) != sha256
        {
            anyhow::bail!("general-compute artifact chunk content does not match its digest");
        }
        let offset = i64::try_from(offset)
            .map_err(|_| anyhow::anyhow!("general-compute chunk offset exceeds database range"))?;
        sqlx::query(
            "INSERT INTO general_compute_artifact_chunks
                (task_id, artifact_id, offset_bytes, size_bytes, sha256, content)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (task_id, artifact_id, offset_bytes) DO NOTHING",
        )
        .bind(task_id)
        .bind(artifact_id)
        .bind(offset)
        .bind(size)
        .bind(sha256)
        .bind(content)
        .execute(&mut *tx)
        .await?;
        let existing: Option<(String, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT sha256, size_bytes, content
             FROM general_compute_artifact_chunks
             WHERE task_id = $1 AND artifact_id = $2 AND offset_bytes = $3",
        )
        .bind(task_id)
        .bind(artifact_id)
        .bind(offset)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((existing_sha256, existing_size, existing_content)) = existing else {
            anyhow::bail!("general-compute artifact chunk was not persisted");
        };
        if existing_sha256 != sha256 || existing_size != size || existing_content != content {
            anyhow::bail!("general-compute artifact chunk conflicts with persisted content");
        }
        let stored_chunks: Vec<(i64, i64, String, Vec<u8>)> = sqlx::query_as(
            "SELECT offset_bytes, size_bytes, sha256, content
             FROM general_compute_artifact_chunks
             WHERE task_id = $1 AND artifact_id = $2
             ORDER BY offset_bytes ASC",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_all(&mut *tx)
        .await?;
        let expected_chunks: Vec<(i64, i64, String)> = sqlx::query_as(
            "SELECT offset_bytes, size_bytes, sha256
             FROM general_compute_artifact_manifest_chunks
             WHERE task_id = $1 AND artifact_id = $2
             ORDER BY offset_bytes ASC",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_all(&mut *tx)
        .await?;
        let is_complete = expected_chunk_count
            == i64::try_from(expected_chunks.len()).unwrap_or(-1)
            && expected_chunks.len() == stored_chunks.len()
            && expected_chunks.iter().zip(stored_chunks.iter()).all(
                |((expected_offset, expected_size, expected_sha), (offset, size, sha, bytes))| {
                    expected_offset == offset
                        && expected_size == size
                        && expected_sha == sha
                        && u64::try_from(*size).ok() == Some(bytes.len() as u64)
                        && general_compute_runtime::sha256_digest(bytes) == *sha
                },
            );
        let content_complete = if is_complete {
            let mut assembled = Vec::with_capacity(expected_size as usize);
            for (_, _, _, bytes) in &stored_chunks {
                assembled.extend_from_slice(bytes);
            }
            assembled.len() as u64 == expected_size
                && general_compute_runtime::sha256_digest(&assembled) == expected_sha256
        } else {
            false
        };
        sqlx::query(
            "UPDATE general_compute_artifacts
             SET complete = complete OR $1,
                 availability_status = CASE
                     WHEN complete OR $1 THEN 'available'
                     ELSE 'pending'
                 END,
                 updated_at = NOW()
             WHERE task_id = $2 AND artifact_id = $3 AND availability_status <> 'expired'",
        )
        .bind(content_complete || complete)
        .bind(task_id)
        .bind(artifact_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Backfill the immutable identity for a task written by an older schema
    /// or an operator migration that populated the task manifest directly.
    /// Normal task creation writes this row in the same transaction; this
    /// fallback only establishes it once, and never replaces an existing row.
    async fn ensure_general_compute_artifact_identity(
        &self,
        task_id: &str,
        artifact_id: &str,
    ) -> Result<()> {
        let manifest: Option<(Vec<u8>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT general_compute_manifest_json, deadline
             FROM tasks
             WHERE task_id = $1 AND runtime = $2",
        )
        .bind(task_id)
        .bind(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
        .fetch_optional(&self.pool)
        .await?;
        let Some((manifest, deadline)) = manifest else {
            anyhow::bail!("general-compute task manifest is missing");
        };
        let request: GeneralComputeRequest = serde_json::from_slice(&manifest)
            .map_err(|_| anyhow::anyhow!("general-compute task manifest is malformed"))?;
        request.validate().map_err(|error| {
            anyhow::anyhow!("general-compute task manifest is invalid: {error:?}")
        })?;
        let artifact = std::iter::once(&request.source_artifact)
            .chain(request.input_artifacts.iter())
            .find(|artifact| artifact.artifact_id == artifact_id)
            .ok_or_else(|| anyhow::anyhow!("artifact is not present in task manifest"))?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO general_compute_artifacts
                (task_id, artifact_id, sha256, size_bytes, expected_chunk_count,
                 availability_status, complete, expires_at)
             VALUES ($1, $2, $3, $4, $5, 'pending', false, $6)
             ON CONFLICT (task_id, artifact_id) DO NOTHING",
        )
        .bind(task_id)
        .bind(&artifact.artifact_id)
        .bind(&artifact.sha256)
        .bind(
            i64::try_from(artifact.size_bytes).map_err(|_| {
                anyhow::anyhow!("general-compute artifact size exceeds database range")
            })?,
        )
        .bind(i64::try_from(artifact.chunks.len()).map_err(|_| {
            anyhow::anyhow!("general-compute artifact chunk count exceeds database range")
        })?)
        .bind(deadline)
        .execute(&mut *tx)
        .await?;
        for chunk in &artifact.chunks {
            sqlx::query(
                "INSERT INTO general_compute_artifact_manifest_chunks
                    (task_id, artifact_id, offset_bytes, size_bytes, sha256)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (task_id, artifact_id, offset_bytes) DO NOTHING",
            )
            .bind(task_id)
            .bind(&artifact.artifact_id)
            .bind(i64::try_from(chunk.offset).map_err(|_| {
                anyhow::anyhow!("general-compute chunk offset exceeds database range")
            })?)
            .bind(i64::try_from(chunk.size_bytes).map_err(|_| {
                anyhow::anyhow!("general-compute chunk size exceeds database range")
            })?)
            .bind(&chunk.sha256)
            .execute(&mut *tx)
            .await?;
        }
        let inline_source: Option<(Vec<u8>,)> =
            sqlx::query_as(
                "SELECT content
             FROM general_compute_artifact_sources
             WHERE task_id = $1 AND artifact_id = $2
               AND sha256 = $3 AND size_bytes = $4
               AND (expires_at IS NULL OR expires_at > NOW())",
            )
            .bind(task_id)
            .bind(&artifact.artifact_id)
            .bind(&artifact.sha256)
            .bind(i64::try_from(artifact.size_bytes).map_err(|_| {
                anyhow::anyhow!("general-compute artifact size exceeds database range")
            })?)
            .fetch_optional(&mut *tx)
            .await?;
        let verified_inline = inline_source.as_ref().is_some_and(|(content,)| {
            content.len() as u64 == artifact.size_bytes
                && general_compute_runtime::sha256_digest(content) == artifact.sha256
        });
        let stored_chunks: Vec<(i64, i64, String, Vec<u8>)> = sqlx::query_as(
            "SELECT offset_bytes, size_bytes, sha256, content
             FROM general_compute_artifact_chunks
             WHERE task_id = $1 AND artifact_id = $2
             ORDER BY offset_bytes ASC",
        )
        .bind(task_id)
        .bind(&artifact.artifact_id)
        .fetch_all(&mut *tx)
        .await?;
        let verified_chunks = artifact.chunks.len() == stored_chunks.len()
            && artifact.chunks.iter().zip(stored_chunks.iter()).all(
                |(expected, (offset, size, sha256, content))| {
                    u64::try_from(*offset).ok() == Some(expected.offset)
                        && u64::try_from(*size).ok() == Some(expected.size_bytes)
                        && sha256 == &expected.sha256
                        && content.len() as u64 == expected.size_bytes
                        && general_compute_runtime::sha256_digest(content) == expected.sha256
                },
            );
        let verified_chunk_content = if verified_chunks {
            let mut assembled = Vec::with_capacity(artifact.size_bytes as usize);
            for (_, _, _, content) in &stored_chunks {
                assembled.extend_from_slice(content);
            }
            assembled.len() as u64 == artifact.size_bytes
                && general_compute_runtime::sha256_digest(&assembled) == artifact.sha256
        } else {
            false
        };
        if verified_inline || verified_chunk_content {
            sqlx::query(
                "UPDATE general_compute_artifacts
                 SET availability_status = 'available', complete = true, updated_at = NOW()
                 WHERE task_id = $1 AND artifact_id = $2",
            )
            .bind(task_id)
            .bind(&artifact.artifact_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn general_compute_artifact_chunks(
        &self,
        task_id: &str,
        artifact_id: &str,
    ) -> Result<Vec<(u64, u64, String, Vec<u8>)>> {
        let rows: Vec<(i64, i64, String, Vec<u8>)> = sqlx::query_as(
            "SELECT offset_bytes, size_bytes, sha256, content
             FROM general_compute_artifact_chunks
             WHERE task_id = $1 AND artifact_id = $2
             ORDER BY offset_bytes ASC",
        )
        .bind(task_id)
        .bind(artifact_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(offset, size, sha256, content)| {
                Ok((
                    u64::try_from(offset).map_err(|_| anyhow::anyhow!("negative chunk offset"))?,
                    u64::try_from(size).map_err(|_| anyhow::anyhow!("negative chunk size"))?,
                    sha256,
                    content,
                ))
            })
            .collect()
    }

    pub async fn find_by_task_id(&self, task_id: &str) -> Result<Option<Task>> {
        sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn general_compute_capability_snapshot(
        &self,
        worker_id: &str,
    ) -> Result<Option<String>> {
        let row = sqlx::query_as::<_, (Option<String>,)>(
            "SELECT CASE WHEN admission_mode = 'private_static'
                         THEN general_compute_capabilities_json
                         ELSE NULL END
             FROM worker_nodes
             WHERE worker_id = $1",
        )
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(snapshot,)| snapshot))
    }

    pub async fn managed_dsl_capability_snapshot(&self, worker_id: &str) -> Result<Option<String>> {
        let row = sqlx::query_as::<_, (Option<String>, Option<String>, String)>(
            "SELECT CASE
                         WHEN admission_mode = 'public_dynamic'
                              AND dynamic_admission_ready
                              AND dynamic_observed_at IS NOT NULL
                              AND dynamic_observed_at <= NOW()
                              AND dynamic_observed_at >= NOW() - ($2 * INTERVAL '1 second')
                           THEN dynamic_capabilities_json
                         WHEN admission_mode = 'private_static'
                           THEN managed_dsl_capabilities_json
                         ELSE NULL
                       END,
                       CASE WHEN admission_mode = 'public_dynamic'
                            THEN dynamic_capabilities_digest
                            ELSE NULL
                       END,
                       admission_mode
             FROM worker_nodes
             WHERE worker_id = $1",
        )
        .bind(worker_id)
        .bind(PUBLIC_DYNAMIC_CAPABILITY_MAX_AGE_SECS)
        .fetch_optional(&self.pool)
        .await?;
        let Some((snapshot, digest, admission_mode)) =
            row.and_then(|(snapshot, digest, admission_mode)| {
                snapshot.map(|value| (value, digest, admission_mode))
            })
        else {
            return Ok(None);
        };
        if admission_mode == PUBLIC_DYNAMIC_ADMISSION_MODE {
            let Some(digest) = digest else {
                return Ok(None);
            };
            let expected_digest = format!("sha256:{:x}", Sha256::digest(snapshot.as_bytes()));
            if digest != expected_digest {
                return Ok(None);
            }
        }
        Ok(Some(snapshot))
    }

    pub async fn find_by_owner(&self, owner: &str) -> Result<Vec<Task>> {
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE owner = $1 ORDER BY created_at DESC LIMIT 100",
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_pending(&self) -> Result<Vec<Task>> {
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE status IN ('PENDING', 'QUEUED') ORDER BY priority DESC, created_at ASC LIMIT 100"
        ).fetch_all(&self.pool).await.map_err(Into::into)
    }

    pub async fn update_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        message: Option<&str>,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = $1, status_message = $2, last_update = NOW() WHERE task_id = $3 RETURNING *"
        ).bind(status.as_str()).bind(message).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn assign_to_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        worker_ip: &str,
    ) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let assigned = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET worker_id = $1, worker_ip = $2, status = 'ASSIGNED', last_update = NOW()
             WHERE task_id = $3 AND status IN ('PENDING', 'QUEUED')
             RETURNING *",
        )
        .bind(worker_id)
        .bind(worker_ip)
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
        self.activate_general_compute_transfer_lease(&mut tx, &assigned, worker_id)
            .await?;
        tx.commit().await?;
        Ok(assigned)
    }

    /// Return the active Nodepool transfer authority after materializing
    /// expiry. Revoked, expired, and terminal-task generations are never
    /// returned.
    pub async fn general_compute_transfer_lease(
        &self,
        task_id: &str,
    ) -> Result<Option<GeneralComputeTransferLease>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE general_compute_transfer_leases l
             SET state = CASE
                     WHEN l.expires_at IS NOT NULL AND l.expires_at <= NOW()
                         THEN 'expired'
                     ELSE 'revoked'
                 END,
                 updated_at = NOW()
             FROM tasks t
             WHERE l.task_id = $1 AND l.task_id = t.task_id AND l.state = 'active'
               AND (
                   (l.expires_at IS NOT NULL AND l.expires_at <= NOW())
                   OR t.status NOT IN ('ASSIGNED', 'RUNNING')
                   OR t.worker_id IS DISTINCT FROM l.worker_id
               )",
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
        let lease = sqlx::query_as::<_, GeneralComputeTransferLease>(
            "SELECT l.task_id, l.execution_id, l.attempt_id, l.worker_id,
                    l.generation, l.state, l.expires_at
             FROM general_compute_transfer_leases l
             JOIN tasks t ON t.task_id = l.task_id
             WHERE l.task_id = $1 AND l.state = 'active'
               AND t.status IN ('ASSIGNED', 'RUNNING')
               AND t.worker_id = l.worker_id
               AND (l.expires_at IS NULL OR l.expires_at > NOW())",
        )
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(lease)
    }

    /// Persist only the immutable binding, issuance metadata, and token
    /// fingerprint for a managed-proof attempt. Repeating the exact binding is
    /// idempotent and returns the original issuance metadata; a conflicting
    /// binding for the same task/generation/attempt is rejected.
    pub async fn record_managed_proof_authorization(
        &self,
        record: &ManagedProofAuthorizationRecord,
    ) -> Result<ManagedProofAuthorizationIssuance> {
        if record.protocol_version == 0
            || record.proof_task_id.trim().is_empty()
            || record.token_jti.trim().is_empty()
            || record.token_iat <= 0
            || record.token_exp < record.token_iat
            || record.token_sha256.trim().is_empty()
        {
            anyhow::bail!("managed-proof authorization metadata is invalid");
        }

        let mut tx = self.pool.begin().await?;
        let active_task = sqlx::query(
            "SELECT t.task_id
             FROM tasks t
             WHERE t.task_id = $1
               AND t.owner = $2
               AND t.worker_id = $3
               AND t.status IN ('ASSIGNED', 'RUNNING')
               AND t.runtime = $7
               AND (
                   ($7 = 'general-compute-v1alpha1' AND EXISTS (
                       SELECT 1
                       FROM general_compute_transfer_leases l
                       WHERE l.task_id = t.task_id
                         AND l.execution_id = $4
                         AND l.attempt_id = $5
                         AND l.worker_id = $3
                         AND l.generation = $6
                         AND l.state = 'active'
                         AND (l.expires_at IS NULL OR l.expires_at > NOW())
                   ))
                   OR ($7 <> 'general-compute-v1alpha1' AND t.retry_count = $6 - 1)
               )
             FOR UPDATE",
        )
        .bind(&record.task_id)
        .bind(&record.owner)
        .bind(&record.worker_id)
        .bind(&record.execution_id)
        .bind(&record.attempt_id)
        .bind(record.lease_generation)
        .bind(&record.runtime)
        .fetch_optional(&mut *tx)
        .await?;
        if active_task.is_none() {
            anyhow::bail!(
                "managed-proof authorization targets a stale or inactive task assignment"
            );
        }

        let existing = sqlx::query(
            "SELECT protocol_version, proof_task_id, owner, worker_id,
                    execution_id, attempt_id, idempotency_key, request_digest,
                    lease_generation, runtime, backend_id,
                    semantics_manifest_sha256, proof_scheme, image_id_json,
                    deadline_unix_ms, token_jti, token_iat, token_exp,
                    token_sha256
             FROM managed_proof_authorizations
             WHERE task_id = $1 AND lease_generation = $2 AND attempt_id = $3
             FOR UPDATE",
        )
        .bind(&record.task_id)
        .bind(record.lease_generation)
        .bind(&record.attempt_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(existing) = existing {
            let protocol_version = existing.try_get::<Option<i32>, _>("protocol_version")?;
            let proof_task_id = existing.try_get::<Option<String>, _>("proof_task_id")?;
            let binding_matches = protocol_version == Some(i32::from(record.protocol_version))
                && proof_task_id.as_deref() == Some(record.proof_task_id.as_str())
                && existing.try_get::<String, _>("owner")? == record.owner
                && existing.try_get::<String, _>("worker_id")? == record.worker_id
                && existing.try_get::<String, _>("execution_id")? == record.execution_id
                && existing.try_get::<String, _>("attempt_id")? == record.attempt_id
                && existing.try_get::<String, _>("idempotency_key")? == record.idempotency_key
                && existing.try_get::<String, _>("request_digest")? == record.request_digest
                && existing.try_get::<i64, _>("lease_generation")? == record.lease_generation
                && existing.try_get::<String, _>("runtime")? == record.runtime
                && existing.try_get::<String, _>("backend_id")? == record.backend_id
                && existing.try_get::<String, _>("semantics_manifest_sha256")?
                    == record.semantics_manifest_sha256
                && existing.try_get::<String, _>("proof_scheme")? == record.proof_scheme
                && existing.try_get::<String, _>("image_id_json")? == record.image_id_json
                && existing.try_get::<i64, _>("deadline_unix_ms")? == record.deadline_unix_ms;
            if !binding_matches {
                anyhow::bail!(
                    "managed-proof authorization conflicts with the existing attempt binding"
                );
            }

            let token_iat = existing
                .try_get::<Option<i64>, _>("token_iat")?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "existing managed-proof authorization cannot be regenerated safely"
                    )
                })?;
            let token_exp = existing
                .try_get::<Option<i64>, _>("token_exp")?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "existing managed-proof authorization cannot be regenerated safely"
                    )
                })?;
            if token_iat <= 0 || token_exp < token_iat {
                anyhow::bail!("existing managed-proof authorization has invalid issuance metadata");
            }
            let issuance = ManagedProofAuthorizationIssuance {
                token_jti: existing.try_get("token_jti")?,
                token_iat,
                token_exp,
                token_sha256: existing.try_get("token_sha256")?,
            };
            tx.commit().await?;
            return Ok(issuance);
        }

        sqlx::query(
            "INSERT INTO managed_proof_authorizations (
                task_id, protocol_version, proof_task_id, owner, worker_id,
                execution_id, attempt_id, idempotency_key, request_digest,
                lease_generation, runtime, backend_id,
                semantics_manifest_sha256, proof_scheme, image_id_json,
                deadline_unix_ms, token_jti, token_iat, token_exp,
                token_sha256, state
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                     $12, $13, $14, $15, $16, $17, $18, $19, $20, 'issued')",
        )
        .bind(&record.task_id)
        .bind(i32::from(record.protocol_version))
        .bind(&record.proof_task_id)
        .bind(&record.owner)
        .bind(&record.worker_id)
        .bind(&record.execution_id)
        .bind(&record.attempt_id)
        .bind(&record.idempotency_key)
        .bind(&record.request_digest)
        .bind(record.lease_generation)
        .bind(&record.runtime)
        .bind(&record.backend_id)
        .bind(&record.semantics_manifest_sha256)
        .bind(&record.proof_scheme)
        .bind(&record.image_id_json)
        .bind(record.deadline_unix_ms)
        .bind(&record.token_jti)
        .bind(record.token_iat)
        .bind(record.token_exp)
        .bind(&record.token_sha256)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ManagedProofAuthorizationIssuance {
            token_jti: record.token_jti.clone(),
            token_iat: record.token_iat,
            token_exp: record.token_exp,
            token_sha256: record.token_sha256.clone(),
        })
    }

    pub async fn managed_proof_authorization_deadline(
        &self,
        task_id: &str,
        lease_generation: i64,
        attempt_id: &str,
    ) -> Result<Option<i64>> {
        sqlx::query_scalar(
            "SELECT deadline_unix_ms
             FROM managed_proof_authorizations
             WHERE task_id = $1 AND lease_generation = $2 AND attempt_id = $3",
        )
        .bind(task_id)
        .bind(lease_generation)
        .bind(attempt_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_managed_proof_authorization_state(
        &self,
        update: &ManagedProofAuthorizationStateUpdate<'_>,
    ) -> Result<()> {
        if !matches!(
            update.state,
            "issued"
                | "submitted"
                | "running"
                | "succeeded"
                | "observed_verified"
                | "failed"
                | "cancelled"
                | "expired"
                | "revoked"
        ) {
            anyhow::bail!("managed-proof authorization state is invalid");
        }
        let result = sqlx::query(
            "UPDATE managed_proof_authorizations
             SET state = $8, updated_at = NOW()
             WHERE task_id = $1 AND lease_generation = $2 AND attempt_id = $3
               AND worker_id = $4
               AND execution_id = $5
               AND idempotency_key = $6
               AND request_digest = $7
               AND (
                   state = $8
                   OR ($8 = 'submitted' AND state = 'issued')
                   OR ($8 = 'running' AND state IN ('submitted', 'running'))
                   OR ($8 IN (
                       'succeeded', 'observed_verified', 'failed',
                       'cancelled', 'expired', 'revoked'
                   ) AND state IN ('issued', 'submitted', 'running'))
               )",
        )
        .bind(update.task_id)
        .bind(update.lease_generation)
        .bind(update.attempt_id)
        .bind(update.worker_id)
        .bind(update.execution_id)
        .bind(update.idempotency_key)
        .bind(update.request_digest)
        .bind(update.state)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!(
                "managed-proof authorization lifecycle transition was stale or already terminal"
            );
        }
        Ok(())
    }

    async fn activate_general_compute_transfer_lease(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        task: &Task,
        worker_id: &str,
    ) -> Result<()> {
        if task.runtime.as_deref() != Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
        {
            return Ok(());
        }
        let manifest = task
            .general_compute_manifest_json
            .as_deref()
            .ok_or_else(|| {
                anyhow::anyhow!("general-compute task is missing its request manifest")
            })?;
        let request: GeneralComputeRequest = serde_json::from_slice(manifest)
            .map_err(|_| anyhow::anyhow!("general-compute request manifest is malformed"))?;
        request
            .validate()
            .map_err(|error| anyhow::anyhow!("general-compute request is invalid: {error:?}"))?;

        sqlx::query(
            "UPDATE general_compute_transfer_leases
             SET state = 'revoked', updated_at = NOW()
             WHERE task_id = $1 AND state = 'active'",
        )
        .bind(&task.task_id)
        .execute(&mut **tx)
        .await?;
        let generation: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(generation), 0) + 1
             FROM general_compute_transfer_leases
             WHERE task_id = $1",
        )
        .bind(&task.task_id)
        .fetch_one(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO general_compute_transfer_leases
                (task_id, execution_id, attempt_id, worker_id, generation, state, expires_at)
             VALUES ($1, $2, $3, $4, $5, 'active', $6)",
        )
        .bind(&task.task_id)
        .bind(&request.execution_id)
        .bind(&request.attempt_id)
        .bind(worker_id)
        .bind(generation)
        .bind(task.deadline)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn revoke_general_compute_transfer_lease(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        task_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE general_compute_transfer_leases
             SET state = 'revoked', updated_at = NOW()
             WHERE task_id = $1 AND state = 'active'",
        )
        .bind(task_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn mark_worker_execution_running(
        &self,
        task_id: &str,
        worker_id: &str,
    ) -> Result<Option<Task>> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'RUNNING', last_update = NOW()
             WHERE task_id = $1 AND worker_id = $2
               AND status IN ('ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(task_id)
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn refresh_worker_endpoint(
        &self,
        task_id: &str,
        worker_id: &str,
        worker_ip: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tasks
             SET worker_ip = $1, last_update = NOW()
             WHERE task_id = $2 AND worker_id = $3 AND status IN ('ASSIGNED', 'RUNNING')",
        )
        .bind(worker_ip)
        .bind(task_id)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn claim_pending_for_worker(
        &self,
        worker_id: &str,
        worker_ip: &str,
        limit: i64,
    ) -> Result<Vec<Task>> {
        let trust = sqlx::query_as::<_, (i32, bool)>(
            "SELECT score, banned FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await?;
        match trust {
            Some((score, banned)) if Self::is_worker_trusted(score, banned) => {}
            Some((score, banned)) => {
                tracing::warn!(
                    "Worker {} blocked from claiming tasks (banned={}, score={})",
                    worker_id,
                    banned,
                    score
                );
                return Ok(vec![]);
            }
            None => {
                tracing::warn!(
                    "Worker {} blocked from claiming tasks because reputation row is missing",
                    worker_id
                );
                return Ok(vec![]);
            }
        }

        let limit = limit.max(1);
        let mut tx = self.pool.begin().await?;
        let claimed = sqlx::query_as::<_, Task>(
            "WITH picked AS (
                SELECT id
                FROM tasks
                WHERE status IN ('PENDING', 'QUEUED')
                ORDER BY priority DESC, created_at ASC
                FOR UPDATE SKIP LOCKED
                LIMIT $3
             )
             UPDATE tasks t
             SET worker_id = $1, worker_ip = $2, status = 'ASSIGNED', last_update = NOW()
             FROM picked
             WHERE t.id = picked.id
             RETURNING t.*",
        )
        .bind(worker_id)
        .bind(worker_ip)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        for task in &claimed {
            self.activate_general_compute_transfer_lease(&mut tx, task, worker_id)
                .await?;
        }
        tx.commit().await?;
        Ok(claimed)
    }

    pub async fn complete(
        &self,
        task_id: &str,
        result_torrent: Option<&str>,
        output: Option<&str>,
    ) -> Result<Task> {
        self.complete_guarded(
            task_id,
            None,
            result_torrent,
            output,
            None,
            None,
            None,
            ManagedCompletionEvidence::Untrusted,
        )
        .await
    }

    pub async fn complete_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        result_torrent: Option<&str>,
        output: Option<&str>,
    ) -> Result<Task> {
        self.complete_guarded(
            task_id,
            Some(worker_id),
            result_torrent,
            output,
            None,
            None,
            None,
            ManagedCompletionEvidence::Untrusted,
        )
        .await
    }

    /// Explicitly selected observe/off compatibility completion for a managed
    /// task. This is never used by the generic Worker result APIs, which must
    /// not bypass the enforce proof gate.
    pub async fn complete_for_worker_legacy_managed(
        &self,
        task_id: &str,
        worker_id: &str,
        output: Option<&str>,
    ) -> Result<Task> {
        self.complete_guarded(
            task_id,
            Some(worker_id),
            None,
            output,
            None,
            None,
            None,
            ManagedCompletionEvidence::LegacyFallback,
        )
        .await
    }

    /// Observe mode retains legacy settlement after a proof was independently
    /// verified. The authorization row records `observed_verified` so audit
    /// consumers can distinguish this from receipt-backed settlement.
    pub async fn complete_for_worker_observed_verified(
        &self,
        task_id: &str,
        worker_id: &str,
        output: Option<&str>,
    ) -> Result<Task> {
        self.complete_guarded(
            task_id,
            Some(worker_id),
            None,
            output,
            None,
            None,
            None,
            ManagedCompletionEvidence::ObservedVerified,
        )
        .await
    }

    pub async fn complete_general_compute_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        output: Option<&str>,
    ) -> Result<Task> {
        self.complete_guarded(
            task_id,
            Some(worker_id),
            None,
            output,
            None,
            Some(expected_manifest),
            Some(result_json),
            ManagedCompletionEvidence::Untrusted,
        )
        .await
    }

    /// Persist a validated typed general-compute failure without treating it
    /// as a billable completion. The dispatcher validates the result against
    /// the Nodepool-owned capability snapshot before calling this method; the
    /// repository still rejects a completed status so a success envelope can
    /// never be recorded on the failure path by mistake.
    pub async fn fail_general_compute_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        reason: &str,
    ) -> Result<Task> {
        let result = serde_json::from_slice::<GeneralComputeResult>(result_json)
            .map_err(|error| anyhow::anyhow!("general-compute result is malformed: {error}"))?;
        if result.status == ResultStatus::Completed {
            anyhow::bail!("completed general-compute result cannot use the failure path");
        }

        let mut tx = self.pool.begin().await?;
        let failed = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'FAILED', status_message = $1, last_update = NOW(), completed_at = NOW()
             WHERE task_id = $2 AND worker_id = $3 AND status IN ('ASSIGNED', 'RUNNING')
               AND general_compute_manifest_json = $4
             RETURNING *",
        )
        .bind(reason)
        .bind(task_id)
        .bind(worker_id)
        .bind(expected_manifest)
        .fetch_one(&mut *tx)
        .await?;

        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;

        sqlx::query(
            "INSERT INTO general_compute_results (task_id, worker_id, result_json)
             VALUES ($1, $2, $3)
             ON CONFLICT (task_id) DO UPDATE
             SET worker_id = EXCLUDED.worker_id,
                 result_json = EXCLUDED.result_json,
                 created_at = NOW()",
        )
        .bind(task_id)
        .bind(worker_id)
        .bind(result_json)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, failed_tasks, score, updated_at)
             VALUES ($1, 1, 95, NOW())
             ON CONFLICT (worker_id) DO UPDATE SET
                failed_tasks = worker_reputation.failed_tasks + 1,
                score = GREATEST(0, worker_reputation.score - 5),
                updated_at = NOW()",
        )
        .bind(worker_id)
        .execute(&mut *tx)
        .await?;
        insert_task_attestation(&mut tx, task_id, worker_id, "rejected", 100, reason).await?;
        tx.commit().await?;
        Ok(failed)
    }

    pub async fn complete_for_worker_with_managed_receipt(
        &self,
        task_id: &str,
        worker_id: &str,
        output: Option<&str>,
        executed_ops: i64,
        output_bytes: i64,
        receipt_json: &str,
    ) -> Result<Task> {
        self.complete_guarded(
            task_id,
            Some(worker_id),
            None,
            output,
            Some(ManagedCompletionReceipt {
                executed_ops,
                output_bytes,
                receipt_json,
            }),
            None,
            None,
            ManagedCompletionEvidence::VerifiedReceipt,
        )
        .await
    }

    pub async fn complete_result_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        result_torrent: &str,
    ) -> Result<Task> {
        self.complete_for_worker(task_id, worker_id, Some(result_torrent), None)
            .await
    }

    pub async fn record_output_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        output: &str,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET output = $1, last_update = NOW()
             WHERE task_id = $2 AND worker_id = $3
               AND (status IN ('ASSIGNED', 'RUNNING') OR (status = 'COMPLETED' AND output IS NULL))
             RETURNING *",
        )
        .bind(output)
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    async fn complete_guarded(
        &self,
        task_id: &str,
        worker_id: Option<&str>,
        result_torrent: Option<&str>,
        output: Option<&str>,
        managed_receipt: Option<ManagedCompletionReceipt<'_>>,
        expected_manifest: Option<&[u8]>,
        general_compute_result: Option<&[u8]>,
        managed_evidence: ManagedCompletionEvidence,
    ) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let runtime: Option<String> =
            sqlx::query_scalar("SELECT runtime FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?;
        let managed_runtime = matches!(
            runtime.as_deref(),
            Some("managed-function-v0") | Some("production_sandboxed_dsl")
        );
        if managed_runtime && managed_evidence == ManagedCompletionEvidence::Untrusted {
            anyhow::bail!(
                "managed task completion requires a Nodepool-verified proof or an explicit rollout compatibility path"
            );
        }
        let deterministic: bool =
            sqlx::query_scalar("SELECT deterministic FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?;
        if deterministic
            && result_torrent
                .map(|value| value.trim().is_empty())
                .unwrap_or(true)
        {
            anyhow::bail!("deterministic task completion requires a result reference");
        }

        let mut completed = if let Some(worker_id) = worker_id {
            sqlx::query_as::<_, Task>(
                "UPDATE tasks
                 SET status = 'COMPLETED', result_torrent = $1, output = COALESCE($2, output), last_update = NOW(), completed_at = NOW()
                 WHERE task_id = $3 AND worker_id = $4 AND status IN ('ASSIGNED', 'RUNNING')
                   AND ($5::bytea IS NULL OR general_compute_manifest_json = $5)
                 RETURNING *",
            )
            .bind(result_torrent)
            .bind(output)
            .bind(task_id)
            .bind(worker_id)
            .bind(expected_manifest.map(|manifest| manifest.to_vec()))
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query_as::<_, Task>(
                "UPDATE tasks
                 SET status = 'COMPLETED', result_torrent = $1, output = COALESCE($2, output), last_update = NOW(), completed_at = NOW()
                 WHERE task_id = $3 AND ($4::bytea IS NULL OR general_compute_manifest_json = $4)
                 RETURNING *",
            )
            .bind(result_torrent)
            .bind(output)
            .bind(task_id)
            .bind(expected_manifest.map(|manifest| manifest.to_vec()))
            .fetch_one(&mut *tx)
            .await?
        };

        let general_compute_settlement = if let Some(result_json) = general_compute_result {
            let manifest = expected_manifest.ok_or_else(|| {
                anyhow::anyhow!("general-compute settlement is missing its request manifest")
            })?;
            let request =
                serde_json::from_slice::<GeneralComputeRequest>(manifest).map_err(|error| {
                    anyhow::anyhow!("general-compute request is malformed: {error}")
                })?;
            let result = serde_json::from_slice::<GeneralComputeResult>(result_json)
                .map_err(|error| anyhow::anyhow!("general-compute result is malformed: {error}"))?;
            let worker_id = completed.worker_id.as_deref().ok_or_else(|| {
                anyhow::anyhow!("general-compute settlement requires an assigned worker")
            })?;
            Some(trusted_general_compute_settlement(
                &request,
                &result,
                worker_id,
                completed.max_cpt,
            )?)
        } else {
            None
        };

        if let Some(result_json) = general_compute_result {
            sqlx::query(
                "INSERT INTO general_compute_results (task_id, worker_id, result_json)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (task_id) DO UPDATE
                 SET worker_id = EXCLUDED.worker_id,
                     result_json = EXCLUDED.result_json,
                     created_at = NOW()",
            )
            .bind(task_id)
            .bind(worker_id)
            .bind(result_json)
            .execute(&mut *tx)
            .await?;
        }

        if let Some(settlement) = general_compute_settlement {
            sqlx::query(
                "INSERT INTO general_compute_settlements (
                    task_id, worker_id, execution_id, attempt_id, idempotency_key,
                    request_digest, billing_version, cost_model_version,
                    usage_claim_json, evidence_level, settlement_basis, amount_cpt
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            )
            .bind(task_id)
            .bind(settlement.worker_id)
            .bind(settlement.execution_id)
            .bind(settlement.attempt_id)
            .bind(settlement.idempotency_key)
            .bind(settlement.request_digest)
            .bind(settlement.billing_version)
            .bind(settlement.cost_model_version)
            .bind(settlement.usage_claim_json)
            .bind(settlement.evidence_level)
            .bind(settlement.basis)
            .bind(settlement.amount_cpt)
            .execute(&mut *tx)
            .await?;
        }

        if let Some(receipt) = managed_receipt {
            completed = sqlx::query_as::<_, Task>(
                "UPDATE tasks
                 SET managed_executed_ops = $1,
                     managed_output_bytes = $2,
                     managed_receipt_json = $3,
                     last_update = NOW()
                 WHERE task_id = $4
                 RETURNING *",
            )
            .bind(receipt.executed_ops.max(0))
            .bind(receipt.output_bytes.max(0))
            .bind(receipt.receipt_json)
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
        }

        if let Some(worker_id) = completed.worker_id.as_deref() {
            increment_worker_success(&mut tx, worker_id).await?;
            insert_task_attestation(
                &mut tx,
                task_id,
                worker_id,
                "accepted",
                100,
                "primary execution",
            )
            .await?;
            if completed.deterministic {
                let proof =
                    checksum_proof_details(completed.result_torrent.as_deref().unwrap_or(""));
                insert_task_attestation(&mut tx, task_id, worker_id, "checksum_proof", 80, &proof)
                    .await?;
            }
        }

        let managed_proof_state = if managed_runtime {
            Some(match managed_evidence {
                ManagedCompletionEvidence::VerifiedReceipt => "succeeded",
                ManagedCompletionEvidence::ObservedVerified => "observed_verified",
                ManagedCompletionEvidence::LegacyFallback
                | ManagedCompletionEvidence::Untrusted => "failed",
            })
        } else {
            None
        };

        let managed_attempt_id = managed_proof_attempt_id(&completed);

        if completed.max_cpt <= 0 || completed.billing_settled {
            if let Some(state) = managed_proof_state {
                update_active_managed_proof_state(
                    &mut tx,
                    task_id,
                    managed_attempt_id.as_deref(),
                    state,
                )
                .await?;
            }
            self.revoke_general_compute_transfer_lease(&mut tx, task_id)
                .await?;
            tx.commit().await?;
            return Ok(completed);
        }

        let billable_cpt = billable_amount_cpt(&completed);

        let charged = sqlx::query(
            "UPDATE users SET balance = balance - $1, updated_at = NOW()
             WHERE username = $2 AND balance >= $1",
        )
        .bind(billable_cpt)
        .bind(&completed.owner)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;

        if charged {
            let platform_fee_cpt = (billable_cpt * PLATFORM_FEE_BPS) / 10_000;
            let provider_credit_cpt = (billable_cpt - platform_fee_cpt).max(0);
            let provider_user: Option<String> = match completed.worker_id.as_deref() {
                Some(worker_id) => {
                    sqlx::query_scalar("SELECT username FROM worker_nodes WHERE worker_id = $1")
                        .bind(worker_id)
                        .fetch_optional(&mut *tx)
                        .await?
                }
                None => None,
            };

            insert_ledger_entry(
                &mut tx,
                task_id,
                &completed.owner,
                completed.worker_id.as_deref(),
                provider_user.as_deref(),
                "payer_debit",
                billable_cpt,
            )
            .await?;
            insert_ledger_entry(
                &mut tx,
                task_id,
                &completed.owner,
                completed.worker_id.as_deref(),
                provider_user.as_deref(),
                "provider_credit",
                provider_credit_cpt,
            )
            .await?;
            insert_ledger_entry(
                &mut tx,
                task_id,
                &completed.owner,
                completed.worker_id.as_deref(),
                provider_user.as_deref(),
                "platform_fee",
                platform_fee_cpt,
            )
            .await?;

            let settled = sqlx::query_as::<_, Task>(
                "UPDATE tasks SET billing_settled = true, billed_amount = $1, last_update = NOW()
                 WHERE task_id = $2 RETURNING *",
            )
            .bind(billable_cpt)
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
            if let Some(state) = managed_proof_state {
                update_active_managed_proof_state(
                    &mut tx,
                    task_id,
                    managed_attempt_id.as_deref(),
                    state,
                )
                .await?;
            }
            self.revoke_general_compute_transfer_lease(&mut tx, task_id)
                .await?;
            tx.commit().await?;
            Ok(settled)
        } else {
            tracing::warn!(
                "Task {} completed but billing is pending: owner {} has insufficient balance for {}",
                task_id,
                completed.owner,
                billable_cpt
            );
            if let Some(state) = managed_proof_state {
                update_active_managed_proof_state(
                    &mut tx,
                    task_id,
                    managed_attempt_id.as_deref(),
                    state,
                )
                .await?;
            }
            self.revoke_general_compute_transfer_lease(&mut tx, task_id)
                .await?;
            tx.commit().await?;
            Ok(completed)
        }
    }

    pub async fn fail(&self, task_id: &str, reason: &str) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let failed = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'FAILED', status_message = $1, last_update = NOW(), completed_at = NOW()
             WHERE task_id = $2 AND status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(reason)
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;

        update_active_managed_proof_state(
            &mut tx,
            task_id,
            managed_proof_attempt_id(&failed).as_deref(),
            "failed",
        )
        .await?;
        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;

        if let Some(result_json) = nodepool_general_compute_terminal_result(
            &failed,
            ResultStatus::Failed,
            "nodepool_task_failed",
            reason,
            b"general-compute-nodepool-failure-input-v1",
        )? {
            sqlx::query(
                "INSERT INTO general_compute_results (task_id, worker_id, result_json)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (task_id) DO NOTHING",
            )
            .bind(&failed.task_id)
            .bind(failed.worker_id.as_deref().unwrap_or("nodepool"))
            .bind(result_json)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(failed)
    }

    pub async fn fail_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        reason: &str,
    ) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let failed = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'FAILED', status_message = $1, last_update = NOW(), completed_at = NOW()
             WHERE task_id = $2 AND worker_id = $3 AND status IN ('ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(reason)
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&mut *tx)
        .await?;

        update_active_managed_proof_state(
            &mut tx,
            task_id,
            managed_proof_attempt_id(&failed).as_deref(),
            "failed",
        )
        .await?;
        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;

        if let Some(result_json) = nodepool_general_compute_terminal_result(
            &failed,
            ResultStatus::Failed,
            "nodepool_task_failed",
            reason,
            b"general-compute-nodepool-failure-input-v1",
        )? {
            sqlx::query(
                "INSERT INTO general_compute_results (task_id, worker_id, result_json)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (task_id) DO NOTHING",
            )
            .bind(&failed.task_id)
            .bind(worker_id)
            .bind(result_json)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        increment_worker_failure(&self.pool, worker_id).await?;
        insert_task_attestation_pool(&self.pool, task_id, worker_id, "rejected", 100, reason)
            .await?;
        Ok(failed)
    }

    /// Mark an assigned task failed without attributing the failure to the
    /// worker. This is used for operator-side admission failures such as a
    /// CAS-only artifact that Nodepool cannot yet source.
    pub async fn fail_for_worker_without_penalty(
        &self,
        task_id: &str,
        worker_id: &str,
        reason: &str,
    ) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let failed = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'FAILED', status_message = $1, last_update = NOW(), completed_at = NOW()
             WHERE task_id = $2 AND worker_id = $3 AND status IN ('ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(reason)
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&mut *tx)
        .await?;
        update_active_managed_proof_state(
            &mut tx,
            task_id,
            managed_proof_attempt_id(&failed).as_deref(),
            "failed",
        )
        .await?;
        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;
        if let Some(result_json) = nodepool_general_compute_terminal_result(
            &failed,
            ResultStatus::Failed,
            "nodepool_task_failed",
            reason,
            b"general-compute-nodepool-failure-input-v1",
        )? {
            sqlx::query(
                "INSERT INTO general_compute_results (task_id, worker_id, result_json)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (task_id) DO NOTHING",
            )
            .bind(&failed.task_id)
            .bind(worker_id)
            .bind(result_json)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(failed)
    }

    pub async fn cancel(&self, task_id: &str) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let cancelled = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'CANCELLED', last_update = NOW(), completed_at = NOW()
             WHERE task_id = $1 AND status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;

        update_active_managed_proof_state(
            &mut tx,
            task_id,
            managed_proof_attempt_id(&cancelled).as_deref(),
            "cancelled",
        )
        .await?;
        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;

        if let Some(result_json) = nodepool_general_compute_terminal_result(
            &cancelled,
            ResultStatus::Cancelled,
            "task_cancelled",
            "task cancelled by owner",
            b"general-compute-cancellation-input-v1",
        )? {
            sqlx::query(
                "INSERT INTO general_compute_results (task_id, worker_id, result_json)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (task_id) DO NOTHING",
            )
            .bind(&cancelled.task_id)
            .bind(cancelled.worker_id.as_deref().unwrap_or("nodepool"))
            .bind(result_json)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(cancelled)
    }

    pub async fn mark_stale_running(&self) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let timed_out = sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = 'TIMED_OUT', status_message = 'Worker heartbeat lost', completed_at = NOW()
             WHERE status = 'RUNNING' AND last_update < NOW() - INTERVAL '120 seconds'
             RETURNING *",
        )
        .fetch_all(&mut *tx)
        .await?;
        for task in &timed_out {
            update_active_managed_proof_state(
                &mut tx,
                &task.task_id,
                managed_proof_attempt_id(task).as_deref(),
                "expired",
            )
            .await?;
            self.revoke_general_compute_transfer_lease(&mut tx, &task.task_id)
                .await?;
            if let Some(result_json) = nodepool_general_compute_terminal_result(
                task,
                ResultStatus::TimedOut,
                "worker_heartbeat_lost",
                "worker heartbeat lost",
                b"general-compute-timeout-input-v1",
            )? {
                sqlx::query(
                    "INSERT INTO general_compute_results (task_id, worker_id, result_json)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (task_id) DO NOTHING",
                )
                .bind(&task.task_id)
                .bind(task.worker_id.as_deref().unwrap_or("nodepool"))
                .bind(result_json)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(u64::try_from(timed_out.len())?)
    }

    pub async fn find_stale_dispatched(&self, timeout_secs: u64) -> Result<Vec<Task>> {
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE status = 'ASSIGNED' AND last_update < NOW() - make_interval(secs => $1::double precision) ORDER BY priority DESC, created_at ASC"
        ).bind(timeout_secs as f64).fetch_all(&self.pool).await.map_err(Into::into)
    }

    pub async fn reset_to_pending(&self, task_id: &str) -> Result<Task> {
        self.reset_to_pending_inner(task_id, None).await
    }

    pub async fn reset_to_pending_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
    ) -> Result<Task> {
        self.reset_to_pending_inner(task_id, Some(worker_id)).await
    }

    async fn reset_to_pending_inner(&self, task_id: &str, worker_id: Option<&str>) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let current = if let Some(worker_id) = worker_id {
            sqlx::query_as::<_, Task>(
                "SELECT * FROM tasks
                 WHERE task_id = $1 AND worker_id = $2 AND status IN ('ASSIGNED', 'RUNNING')
                 FOR UPDATE",
            )
            .bind(task_id)
            .bind(worker_id)
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE task_id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?
        };

        let rotated_manifest = rotate_general_compute_attempt(&current)?;
        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;
        update_active_managed_proof_state(
            &mut tx,
            task_id,
            managed_proof_attempt_id(&current).as_deref(),
            "revoked",
        )
        .await?;
        let updated = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'PENDING', status_message = 'Redispatched', worker_id = NULL, worker_ip = NULL,
                 general_compute_manifest_json = $1,
                 retry_count = retry_count + 1, last_update = NOW()
             WHERE task_id = $2
             RETURNING *",
        )
        .bind(rotated_manifest)
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn update_resource_usage(
        &self,
        task_id: &str,
        cpu: f64,
        memory: f64,
        gpu: f64,
        gpu_mem: f64,
    ) -> Result<()> {
        sqlx::query("UPDATE tasks SET cpu_usage = $1, memory_usage = $2, gpu_usage = $3, gpu_memory_usage = $4, last_update = NOW() WHERE task_id = $5")
            .bind(cpu).bind(memory).bind(gpu).bind(gpu_mem).bind(task_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn update_resource_usage_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        cpu: f64,
        memory: f64,
        gpu: f64,
        gpu_mem: f64,
    ) -> Result<()> {
        let result = sqlx::query(
            "UPDATE tasks
             SET cpu_usage = $1, memory_usage = $2, gpu_usage = $3, gpu_memory_usage = $4, last_update = NOW()
             WHERE task_id = $5 AND worker_id = $6
               AND (
                   status IN ('ASSIGNED', 'RUNNING')
                   OR (
                       status = 'COMPLETED'
                       AND cpu_usage = 0
                       AND memory_usage = 0
                       AND gpu_usage = 0
                       AND gpu_memory_usage = 0
                   )
               )",
        )
        .bind(cpu)
        .bind(memory)
        .bind(gpu)
        .bind(gpu_mem)
        .bind(task_id)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("task is not assigned to this worker or is no longer active");
        }
        Ok(())
    }

    pub async fn record_batch_report_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        report: BatchTaskReport<'_>,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET output = COALESCE($1, output),
                 cpu_time_ms = $2,
                 wall_time_ms = $3,
                 peak_memory_mb = $4,
                 download_bytes = $5,
                 cache_hits = $6,
                 last_update = NOW()
             WHERE task_id = $7 AND worker_id = $8 AND status IN ('COMPLETED', 'FAILED')
             RETURNING *",
        )
        .bind(report.output)
        .bind(report.cpu_time_ms)
        .bind(report.wall_time_ms)
        .bind(report.peak_memory_mb)
        .bind(report.download_bytes)
        .bind(report.cache_hits)
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }
}

async fn update_active_managed_proof_state(
    tx: &mut Transaction<'_, Postgres>,
    task_id: &str,
    attempt_id: Option<&str>,
    state: &str,
) -> Result<()> {
    let Some(attempt_id) = attempt_id else {
        return Ok(());
    };
    if !matches!(
        state,
        "succeeded" | "observed_verified" | "failed" | "cancelled" | "expired" | "revoked"
    ) {
        anyhow::bail!("managed-proof terminal state is invalid");
    }
    sqlx::query(
        "UPDATE managed_proof_authorizations AS authorization
         SET state = $1, updated_at = NOW()
         FROM tasks AS task
         WHERE authorization.task_id = $2
           AND task.task_id = authorization.task_id
           AND authorization.attempt_id = $3
           AND authorization.state IN ('issued', 'submitted', 'running')
           AND (
               (
                   authorization.runtime = 'general-compute-v1alpha1'
                   AND EXISTS (
                       SELECT 1
                       FROM general_compute_transfer_leases lease
                       WHERE lease.task_id = authorization.task_id
                         AND lease.attempt_id = authorization.attempt_id
                         AND lease.generation = authorization.lease_generation
                         AND lease.state = 'active'
                         AND (lease.expires_at IS NULL OR lease.expires_at > NOW())
                   )
               )
               OR (
                   authorization.runtime <> 'general-compute-v1alpha1'
                   AND authorization.lease_generation = task.retry_count + 1
               )
           )",
    )
    .bind(state)
    .bind(task_id)
    .bind(attempt_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn managed_proof_attempt_id(task: &Task) -> Option<String> {
    if !matches!(
        task.runtime.as_deref(),
        Some("managed-function-v0") | Some("production_sandboxed_dsl")
    ) {
        return None;
    }
    let attempt_number = u32::try_from(task.retry_count).ok()?;
    Some(format!(
        "managed-attempt-v1:{}:{attempt_number}",
        task.id.simple()
    ))
}

async fn insert_ledger_entry(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: &str,
    payer_user: &str,
    provider_worker_id: Option<&str>,
    provider_user: Option<&str>,
    kind: &str,
    amount_cpt: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO ledger_entries (
            task_id, payer_user, provider_worker_id, provider_user,
            kind, amount_cpt, currency, status, idempotency_key
         )
         VALUES ($1, $2, $3, $4, $5, $6, 'CPT', 'settled', $7)
         ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(task_id)
    .bind(payer_user)
    .bind(provider_worker_id)
    .bind(provider_user)
    .bind(kind)
    .bind(amount_cpt)
    .bind(format!("{task_id}:{kind}"))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn trusted_general_compute_settlement(
    request: &GeneralComputeRequest,
    result: &GeneralComputeResult,
    worker_id: &str,
    reservation_cpt: i64,
) -> Result<GeneralComputeSettlement> {
    if worker_id.trim().is_empty() {
        anyhow::bail!("general-compute settlement requires a worker identity");
    }
    if reservation_cpt <= 0 {
        anyhow::bail!("general-compute settlement reservation must be positive");
    }
    request.validate().map_err(|error| {
        anyhow::anyhow!("general-compute request failed settlement validation: {error:?}")
    })?;
    if request.billing_version != GENERAL_COMPUTE_BILLING_VERSION
        || request.cost_model_version != GENERAL_COMPUTE_COST_MODEL_VERSION
    {
        anyhow::bail!(
            "general-compute billing/cost model is not Nodepool-approved: {}/{}",
            request.billing_version,
            request.cost_model_version
        );
    }
    if result.status != ResultStatus::Completed
        || result.exit_code != Some(0)
        || result.error_code.is_some()
    {
        anyhow::bail!("only a successfully completed general-compute result can settle");
    }
    if result.execution_id != request.execution_id
        || result.attempt_id != request.attempt_id
        || result.idempotency_key != request.idempotency_key
        || result.request_digest != request.request_digest
        || result.runtime_version != request.runtime_version
        || result.backend_id != request.backend_id
        || result.guest_image_digest != request.guest_image_digest
        || result.determinism != request.determinism
    {
        anyhow::bail!("general-compute result identity does not match the request");
    }
    if result.evidence.level != general_compute_runtime::EvidenceLevel::Unverified {
        anyhow::bail!("worker general-compute evidence cannot establish trusted settlement");
    }
    if result.output_manifest_root
        != general_compute_runtime::canonical_artifact_root(&result.output_artifacts)
    {
        anyhow::bail!("general-compute output manifest root is not canonical");
    }
    let output_artifact_bytes = result
        .output_artifacts
        .iter()
        .try_fold(0u64, |total, artifact| {
            if artifact.role != general_compute_runtime::ArtifactRole::Output {
                return None;
            }
            artifact.validate().ok()?;
            total.checked_add(artifact.size_bytes)
        })
        .ok_or_else(|| anyhow::anyhow!("general-compute output artifacts are invalid"))?;
    let input_bytes = request
        .input_artifacts
        .iter()
        .try_fold(0u64, |total, artifact| {
            total.checked_add(artifact.size_bytes)
        })
        .ok_or_else(|| anyhow::anyhow!("general-compute input sizes overflow"))?;
    if output_artifact_bytes > request.execution_policy.output_bytes
        || result.usage.cpu_time_ms > request.execution_policy.cpu_millis
        || result.usage.wall_time_ms > request.execution_policy.wall_time_ms
        || result.usage.peak_memory_bytes > request.execution_policy.memory_bytes
        || result.usage.output_bytes > request.execution_policy.output_bytes
        || result.usage.input_bytes > input_bytes
        || (!request.execution_policy.gpu_required
            && (result.usage.gpu_time_ms != 0 || result.usage.gpu_memory_bytes != 0))
    {
        anyhow::bail!("general-compute usage claim exceeds the request policy");
    }

    Ok(GeneralComputeSettlement {
        worker_id: worker_id.to_owned(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        billing_version: request.billing_version.clone(),
        cost_model_version: request.cost_model_version.clone(),
        usage_claim_json: serde_json::to_vec(&result.usage)?,
        evidence_level: "unverified".into(),
        basis: "fixed_reservation".into(),
        amount_cpt: reservation_cpt,
    })
}

fn managed_receipt_amount_cpt(task: &Task) -> i64 {
    MANAGED_BASE_INVOCATION_CPT + task.managed_executed_ops.max(0)
}

fn nodepool_general_compute_terminal_result(
    task: &Task,
    status: ResultStatus,
    error_code: &str,
    stderr: &str,
    input_domain: &[u8],
) -> Result<Option<Vec<u8>>> {
    if task.runtime.as_deref() != Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION) {
        return Ok(None);
    }
    let manifest = task
        .general_compute_manifest_json
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("general-compute task is missing its request manifest"))?;
    let request: GeneralComputeRequest = serde_json::from_slice(manifest)
        .map_err(|error| anyhow::anyhow!("general-compute request is malformed: {error}"))?;
    request
        .validate()
        .map_err(|error| anyhow::anyhow!("general-compute request is invalid: {error:?}"))?;

    let all_inline = request
        .input_artifacts
        .iter()
        .all(|artifact| artifact.inline_bytes.is_some())
        && request.source_artifact.inline_bytes.is_some();
    let input_sha256 = if all_inline {
        let source = request
            .source_artifact
            .inline_bytes
            .as_deref()
            .expect("all_inline guarantees source bytes");
        let inputs = request
            .input_artifacts
            .iter()
            .map(|artifact| {
                artifact
                    .inline_bytes
                    .as_deref()
                    .expect("all_inline guarantees input bytes")
            })
            .collect::<Vec<_>>();
        general_compute_runtime::canonical_input_digest(source, &inputs)
    } else {
        // A Nodepool terminal transition can happen before a Worker
        // materializes CAS bytes.
        // Bind the envelope to the immutable manifest coordinates instead of
        // claiming that unobserved execution inputs were read.
        let mut coordinates = Vec::new();
        coordinates.extend_from_slice(input_domain);
        for artifact in
            std::iter::once(&request.source_artifact).chain(request.input_artifacts.iter())
        {
            coordinates.extend_from_slice(artifact.artifact_id.as_bytes());
            coordinates.push(0);
            coordinates.extend_from_slice(artifact.sha256.as_bytes());
            coordinates.push(0);
            coordinates.extend_from_slice(&artifact.size_bytes.to_be_bytes());
        }
        general_compute_runtime::sha256_digest(&coordinates)
    };

    let result = GeneralComputeResult {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        status,
        exit_code: None,
        error_code: Some(error_code.into()),
        stdout: String::new(),
        stderr: stderr.into(),
        output_artifacts: vec![],
        usage: general_compute_runtime::UsageClaim::default(),
        runtime_version: request.runtime_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        input_sha256,
        determinism: request.determinism.clone(),
        capability_summary: vec![],
        gpu_selection: None,
        output_manifest_root: general_compute_runtime::canonical_artifact_root(&[]),
        evidence: general_compute_runtime::EvidenceEnvelope::default(),
    };
    Ok(Some(serde_json::to_vec(&result)?))
}

fn billable_amount_cpt(task: &Task) -> i64 {
    if matches!(
        task.runtime.as_deref(),
        Some("managed-function-v0") | Some("production_sandboxed_dsl")
    ) && task.managed_receipt_json.is_some()
    {
        managed_receipt_amount_cpt(task).min(task.max_cpt).max(0)
    } else {
        task.max_cpt.max(0)
    }
}

async fn increment_worker_success(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    worker_id: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO worker_reputation (worker_id, successful_tasks, score, last_attested_at, updated_at)
         VALUES ($1, 1, 101, NOW(), NOW())
         ON CONFLICT (worker_id) DO UPDATE SET
            successful_tasks = worker_reputation.successful_tasks + 1,
            score = LEAST(1000, worker_reputation.score + 1),
            last_attested_at = NOW(),
            updated_at = NOW()",
    )
    .bind(worker_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn increment_worker_failure(pool: &PgPool, worker_id: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO worker_reputation (worker_id, failed_tasks, score, updated_at)
         VALUES ($1, 1, 95, NOW())
         ON CONFLICT (worker_id) DO UPDATE SET
            failed_tasks = worker_reputation.failed_tasks + 1,
            score = GREATEST(0, worker_reputation.score - 5),
            updated_at = NOW()",
    )
    .bind(worker_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_task_attestation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: &str,
    worker_id: &str,
    verdict: &str,
    confidence: i32,
    details: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO task_attestations (task_id, worker_id, verdict, confidence, details)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(task_id)
    .bind(worker_id)
    .bind(verdict)
    .bind(confidence)
    .bind(details)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn checksum_proof_details(result_ref: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(result_ref.as_bytes());
    format!(
        "result_ref_sha1={:x};result_ref={}",
        hasher.finalize(),
        result_ref
    )
}

fn rotate_general_compute_attempt(task: &Task) -> Result<Option<Vec<u8>>> {
    if task.runtime.as_deref() != Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION) {
        return Ok(task.general_compute_manifest_json.clone());
    }
    let Some(manifest) = task.general_compute_manifest_json.as_deref() else {
        anyhow::bail!("general-compute task is missing its request manifest");
    };
    let mut request: GeneralComputeRequest = serde_json::from_slice(manifest)
        .map_err(|_| anyhow::anyhow!("general-compute request manifest is malformed"))?;
    request.attempt_id = uuid::Uuid::new_v4().to_string();
    request.request_digest = request.canonical_request_digest();
    Ok(Some(serde_json::to_vec(&request)?))
}

async fn insert_task_attestation_pool(
    pool: &PgPool,
    task_id: &str,
    worker_id: &str,
    verdict: &str,
    confidence: i32,
    details: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO task_attestations (task_id, worker_id, verdict, confidence, details)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(task_id)
    .bind(worker_id)
    .bind(verdict)
    .bind(confidence)
    .bind(details)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
mod tests {
    use super::*;
    use chrono::Utc;
    use general_compute_runtime::{
        canonical_artifact_root, ArtifactManifest, ArtifactRole, DeterminismPolicy,
        EvidenceEnvelope, ExecutionPolicy, GeneralComputeRequest, GeneralComputeResult,
        ResultStatus, UsageClaim, GENERAL_COMPUTE_RUNTIME_VERSION,
    };
    use hivemind_database::postgres::IsolatedTestPool;

    // Ledger row shape: (kind, payer_user, provider_worker_id, provider_user, amount_cpt, status)
    type LedgerRow = (String, String, Option<String>, Option<String>, i64, String);

    async fn pool(test_name: &str) -> Option<(PgPool, IsolatedTestPool)> {
        let fixture = hivemind_database::postgres::create_isolated_test_pool(test_name)
            .await
            .ok()?;
        if hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .is_err()
        {
            fixture.cleanup().await.ok();
            return None;
        }
        Some((fixture.pool.clone(), fixture))
    }

    #[tokio::test]
    async fn task_repository_pool_uses_isolated_schema() {
        let (p, fixture) = match pool("task_repository_pool_uses_isolated_schema").await {
            Some(parts) => parts,
            None => return,
        };

        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&p)
            .await
            .unwrap();

        assert!(
            schema.starts_with("hm_test_"),
            "task repository DB tests must use an isolated schema, got {schema}"
        );
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_proof_authorization_duplicate_is_idempotent_and_conflict_safe() {
        let (p, fixture) = match pool("managed_proof_authorization_duplicate").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("managed-auth-task-{unique}");
        let owner = format!("managed-auth-owner-{unique}");
        let worker_id = format!("managed-auth-worker-{unique}");
        let mut task = make_task(&task_id, &owner);
        task.runtime = Some("managed-function-v0".into());
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.20")
            .await
            .unwrap();

        let now = Utc::now().timestamp();
        let record = ManagedProofAuthorizationRecord {
            task_id: task_id.clone(),
            protocol_version: 1,
            proof_task_id: task_id.clone(),
            owner: owner.clone(),
            worker_id: worker_id.clone(),
            execution_id: "managed-execution-1".into(),
            attempt_id: "managed-attempt-1".into(),
            idempotency_key: "managed-idempotency-1".into(),
            request_digest: format!("sha256:{}", "a".repeat(64)),
            lease_generation: 1,
            runtime: "managed-function-v0".into(),
            backend_id: String::new(),
            semantics_manifest_sha256: String::new(),
            proof_scheme: "risc0-zkvm-3.0.6".into(),
            image_id_json: "[1,1,1,1,1,1,1,1]".into(),
            deadline_unix_ms: (now + 300) * 1_000,
            token_jti: "jti-original".into(),
            token_iat: now,
            token_exp: now + 600,
            token_sha256: format!("sha256:{}", "b".repeat(64)),
        };
        let first = repo
            .record_managed_proof_authorization(&record)
            .await
            .unwrap();

        let mut retry = record.clone();
        retry.token_jti = "jti-retry-must-not-win".into();
        retry.token_iat = now + 1;
        retry.token_exp = now + 601;
        retry.token_sha256 = format!("sha256:{}", "c".repeat(64));
        let second = repo
            .record_managed_proof_authorization(&retry)
            .await
            .unwrap();
        assert_eq!(first, second);

        let row_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM managed_proof_authorizations WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(row_count, 1);

        let mut conflict = record.clone();
        conflict.backend_id = "different-backend".into();
        assert!(repo
            .record_managed_proof_authorization(&conflict)
            .await
            .is_err());
        let persisted: (String, String, i64, i64, String) = sqlx::query_as(
            "SELECT token_jti, token_sha256, token_iat, token_exp, backend_id
             FROM managed_proof_authorizations WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(persisted.0, "jti-original");
        assert_eq!(persisted.1, format!("sha256:{}", "b".repeat(64)));
        assert_eq!(persisted.2, now);
        assert_eq!(persisted.3, now + 600);
        assert_eq!(persisted.4, "");

        cleanup_task_case(&repo.pool, &task_id, &owner, None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_proof_authorization_duplicate_race_converges_to_one_row() {
        let (p, fixture) = match pool("managed_proof_authorization_duplicate_race").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("managed-auth-race-task-{unique}");
        let owner = format!("managed-auth-race-owner-{unique}");
        let worker_id = format!("managed-auth-race-worker-{unique}");
        let mut task = make_task(&task_id, &owner);
        task.runtime = Some("managed-function-v0".into());
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.21")
            .await
            .unwrap();

        let now = Utc::now().timestamp();
        let record = ManagedProofAuthorizationRecord {
            task_id: task_id.clone(),
            protocol_version: 1,
            proof_task_id: task_id.clone(),
            owner: owner.clone(),
            worker_id: worker_id.clone(),
            execution_id: "managed-execution-race".into(),
            attempt_id: "managed-attempt-race".into(),
            idempotency_key: "managed-idempotency-race".into(),
            request_digest: format!("sha256:{}", "d".repeat(64)),
            lease_generation: 1,
            runtime: "managed-function-v0".into(),
            backend_id: String::new(),
            semantics_manifest_sha256: String::new(),
            proof_scheme: "risc0-zkvm-3.0.6".into(),
            image_id_json: "[2,2,2,2,2,2,2,2]".into(),
            deadline_unix_ms: (now + 300) * 1_000,
            token_jti: "jti-race".into(),
            token_iat: now,
            token_exp: now + 600,
            token_sha256: format!("sha256:{}", "e".repeat(64)),
        };
        let left_repo = TaskRepository::new(repo.pool.clone());
        let right_repo = TaskRepository::new(repo.pool.clone());
        let (left, right) = tokio::join!(
            left_repo.record_managed_proof_authorization(&record),
            right_repo.record_managed_proof_authorization(&record)
        );
        assert_eq!(left.unwrap(), right.unwrap());

        let row_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM managed_proof_authorizations WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(row_count, 1);

        cleanup_task_case(&repo.pool, &task_id, &format!("unused-{unique}"), None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_proof_authorization_state_transitions_are_idempotent_and_generation_bound() {
        let (p, fixture) = match pool("managed_proof_authorization_state").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("managed-state-task-{unique}");
        let owner = format!("managed-state-owner-{unique}");
        let worker_id = format!("managed-state-worker-{unique}");
        let mut task = make_task(&task_id, &owner);
        task.runtime = Some("managed-function-v0".into());
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.22")
            .await
            .unwrap();
        let now = Utc::now().timestamp();
        let record = ManagedProofAuthorizationRecord {
            task_id: task_id.clone(),
            protocol_version: 1,
            proof_task_id: task_id.clone(),
            owner: owner.clone(),
            worker_id: worker_id.clone(),
            execution_id: "managed-execution-state".into(),
            attempt_id: "managed-attempt-state".into(),
            idempotency_key: "managed-idempotency-state".into(),
            request_digest: format!("sha256:{}", "f".repeat(64)),
            lease_generation: 1,
            runtime: "managed-function-v0".into(),
            backend_id: String::new(),
            semantics_manifest_sha256: String::new(),
            proof_scheme: "risc0-zkvm-3.0.6".into(),
            image_id_json: "[3,3,3,3,3,3,3,3]".into(),
            deadline_unix_ms: (now + 300) * 1_000,
            token_jti: "jti-state".into(),
            token_iat: now,
            token_exp: now + 600,
            token_sha256: format!("sha256:{}", "1".repeat(64)),
        };
        repo.record_managed_proof_authorization(&record)
            .await
            .unwrap();
        for state in ["issued", "submitted", "running", "observed_verified"] {
            let update = ManagedProofAuthorizationStateUpdate {
                task_id: &task_id,
                lease_generation: 1,
                attempt_id: "managed-attempt-state",
                worker_id: &worker_id,
                execution_id: "managed-execution-state",
                idempotency_key: "managed-idempotency-state",
                request_digest: &format!("sha256:{}", "f".repeat(64)),
                state,
            };
            repo.update_managed_proof_authorization_state(&update)
                .await
                .unwrap();
            repo.update_managed_proof_authorization_state(&update)
                .await
                .unwrap();
        }
        let stale_update = ManagedProofAuthorizationStateUpdate {
            task_id: &task_id,
            lease_generation: 2,
            attempt_id: "managed-attempt-state",
            worker_id: &worker_id,
            execution_id: "managed-execution-state",
            idempotency_key: "managed-idempotency-state",
            request_digest: &format!("sha256:{}", "f".repeat(64)),
            state: "failed",
        };
        assert!(repo
            .update_managed_proof_authorization_state(&stale_update)
            .await
            .is_err());
        let persisted: String =
            sqlx::query_scalar("SELECT state FROM managed_proof_authorizations WHERE task_id = $1")
                .bind(&task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        assert_eq!(persisted, "observed_verified");

        cleanup_task_case(&repo.pool, &task_id, &format!("unused-{unique}"), None).await;
        fixture.cleanup().await.ok();
    }
    #[test]
    fn managed_v0_billing_formula_matches_frozen_manifest() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../executor-rs/crates/managed-function-runtime/managed-function-v0-semantics.json"
        ))
        .unwrap();
        let mut task = make_task("managed-v0-billing-contract", "owner");
        task.runtime = Some("managed-function-v0".into());
        task.managed_receipt_json = Some("{}".into());
        task.managed_executed_ops = 17;
        task.max_cpt = 100;

        assert_eq!(
            manifest["billing"]["base_invocation_cpt"],
            MANAGED_BASE_INVOCATION_CPT
        );
        assert_eq!(manifest["billing"]["usage_unit_cpt"], 1);
        assert_eq!(
            manifest["billing"]["formula"],
            "min(max_cpt, base_invocation_cpt + usage_units)"
        );
        assert_eq!(managed_receipt_amount_cpt(&task), 18);
        assert_eq!(billable_amount_cpt(&task), 18);

        task.max_cpt = 10;
        assert!(manifest["billing"]["max_cpt_cap_applied"]
            .as_bool()
            .unwrap());
        assert_eq!(billable_amount_cpt(&task), 10);
    }

    #[test]
    fn general_compute_settlement_uses_fixed_reservation_and_keeps_usage_unverified() {
        let mut request = GeneralComputeRequest {
            execution_id: "settlement-execution".into(),
            attempt_id: "settlement-attempt".into(),
            idempotency_key: "settlement-idempotency".into(),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "settlement-backend".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let usage = UsageClaim {
            cpu_time_ms: 17,
            wall_time_ms: 23,
            output_bytes: 5,
            ..UsageClaim::default()
        };
        let result = GeneralComputeResult {
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            status: ResultStatus::Completed,
            exit_code: Some(0),
            error_code: None,
            stdout: "ok".into(),
            stderr: String::new(),
            output_artifacts: vec![],
            usage: usage.clone(),
            runtime_version: request.runtime_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            input_sha256: general_compute_runtime::canonical_input_digest(b"source", &[]),
            determinism: request.determinism.clone(),
            capability_summary: vec![],
            gpu_selection: None,
            output_manifest_root: canonical_artifact_root(&[]),
            evidence: EvidenceEnvelope::default(),
        };

        let settlement =
            trusted_general_compute_settlement(&request, &result, "settlement-worker", 42)
                .expect("a validated general-compute result should produce a settlement record");

        assert_eq!(settlement.amount_cpt, 42);
        assert_eq!(settlement.basis, "fixed_reservation");
        assert_eq!(settlement.evidence_level, "unverified");
        assert_eq!(settlement.worker_id, "settlement-worker");
        assert_eq!(
            serde_json::from_slice::<UsageClaim>(&settlement.usage_claim_json).unwrap(),
            usage
        );

        let mut forged_request = request.clone();
        forged_request.cost_model_version = "worker-selected-cost".into();
        forged_request.request_digest = forged_request.canonical_request_digest();
        assert!(trusted_general_compute_settlement(
            &forged_request,
            &result,
            "settlement-worker",
            42,
        )
        .is_err());
    }

    #[tokio::test]
    async fn general_compute_completion_persists_nodepool_settlement_provenance() {
        let (p, fixture) = match pool("task_repository_general_compute_settlement").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("settlement-owner-{unique}");
        let worker_id = format!("settlement-worker-{unique}");
        let task_id = format!("settlement-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &format!("provider-{unique}")).await;

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "settlement-backend".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let result = GeneralComputeResult {
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            status: ResultStatus::Completed,
            exit_code: Some(0),
            error_code: None,
            stdout: "settled".into(),
            stderr: String::new(),
            output_artifacts: vec![],
            usage: UsageClaim {
                cpu_time_ms: 7,
                wall_time_ms: 9,
                ..UsageClaim::default()
            },
            runtime_version: request.runtime_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            input_sha256: general_compute_runtime::canonical_input_digest(b"source", &[]),
            determinism: request.determinism.clone(),
            capability_summary: vec![],
            gpu_selection: None,
            output_manifest_root: canonical_artifact_root(&[]),
            evidence: EvidenceEnvelope::default(),
        };
        let manifest = serde_json::to_vec(&request).unwrap();
        let result_json = serde_json::to_vec(&result).unwrap();

        let mut task = make_task(&task_id, &username);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.max_cpt = 42;
        task.general_compute_manifest_json = Some(manifest.clone());
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.77")
            .await
            .unwrap();

        let completed = repo
            .complete_general_compute_for_worker(
                &task_id,
                &worker_id,
                &manifest,
                &result_json,
                Some("settled"),
            )
            .await
            .unwrap();
        assert!(completed.billing_settled);
        assert_eq!(completed.billed_amount, 42);

        let row: (
            String,
            String,
            String,
            String,
            String,
            String,
            i64,
            String,
            String,
            String,
            Vec<u8>,
        ) = sqlx::query_as(
            "SELECT worker_id, execution_id, attempt_id, idempotency_key,
                        request_digest, billing_version, amount_cpt,
                        cost_model_version, settlement_basis, evidence_level,
                        usage_claim_json
                 FROM general_compute_settlements WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(row.0, worker_id);
        assert_eq!(row.1, request.execution_id);
        assert_eq!(row.2, request.attempt_id);
        assert_eq!(row.3, request.idempotency_key);
        assert_eq!(row.4, request.request_digest);
        assert_eq!(row.5, "billing-v1");
        assert_eq!(row.6, 42);
        assert_eq!(row.7, "cost-v1");
        assert_eq!(row.8, "fixed_reservation");
        assert_eq!(row.9, "unverified");
        assert_eq!(
            serde_json::from_slice::<UsageClaim>(&row.10).unwrap(),
            result.usage
        );

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&row.0)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_failure_persists_typed_result_without_settlement() {
        let (p, fixture) = match pool("task_repository_general_compute_failure").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("failure-owner-{unique}");
        let worker_id = format!("failure-worker-{unique}");
        let task_id = format!("failure-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &format!("provider-{unique}")).await;

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "failure-backend".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let result = GeneralComputeResult {
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            status: ResultStatus::BackendUnavailable,
            exit_code: None,
            error_code: Some("backend_unavailable".into()),
            stdout: String::new(),
            stderr: "network denied".into(),
            output_artifacts: vec![],
            usage: UsageClaim::default(),
            runtime_version: request.runtime_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            input_sha256: general_compute_runtime::canonical_input_digest(b"source", &[]),
            determinism: request.determinism.clone(),
            capability_summary: vec![],
            gpu_selection: None,
            output_manifest_root: canonical_artifact_root(&[]),
            evidence: EvidenceEnvelope::default(),
        };
        let manifest = serde_json::to_vec(&request).unwrap();
        let result_json = serde_json::to_vec(&result).unwrap();

        let mut task = make_task(&task_id, &username);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.max_cpt = 42;
        task.general_compute_manifest_json = Some(manifest.clone());
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.78")
            .await
            .unwrap();

        let failed = repo
            .fail_general_compute_for_worker(
                &task_id,
                &worker_id,
                &manifest,
                &result_json,
                "backend_unavailable",
            )
            .await
            .unwrap();
        assert_eq!(failed.status, TaskStatus::Failed);
        assert!(!failed.billing_settled);
        let lease_state: String = sqlx::query_scalar(
            "SELECT state FROM general_compute_transfer_leases WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(lease_state, "revoked");

        let persisted: Vec<u8> = sqlx::query_scalar(
            "SELECT result_json FROM general_compute_results WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        let persisted_result: GeneralComputeResult = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(persisted_result, result);

        let settlement_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_settlements WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(settlement_count, 0);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_cancel_persists_nodepool_typed_result_without_settlement() {
        let (p, fixture) = match pool("task_repository_general_compute_cancel").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("cancel-owner-{unique}");
        let worker_id = format!("cancel-worker-{unique}");
        let task_id = format!("cancel-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &format!("provider-{unique}")).await;

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "cancel-backend".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let manifest = serde_json::to_vec(&request).unwrap();

        let mut task = make_task(&task_id, &username);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(manifest);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.79")
            .await
            .unwrap();

        let cancelled = repo.cancel(&task_id).await.unwrap();
        assert_eq!(cancelled.status, TaskStatus::Cancelled);

        let persisted: Vec<u8> = sqlx::query_scalar(
            "SELECT result_json FROM general_compute_results WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        let result: GeneralComputeResult = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(result.status, ResultStatus::Cancelled);
        assert_eq!(result.error_code.as_deref(), Some("task_cancelled"));
        assert_eq!(result.execution_id, request.execution_id);
        assert_eq!(result.attempt_id, request.attempt_id);
        assert_eq!(result.request_digest, request.request_digest);
        assert_eq!(
            result.input_sha256,
            general_compute_runtime::canonical_input_digest(b"source", &[])
        );
        assert_eq!(result.output_manifest_root, canonical_artifact_root(&[]));

        let settlement_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_settlements WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(settlement_count, 0);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_stale_running_persists_nodepool_typed_timeout_without_settlement() {
        let (p, fixture) = match pool("task_repository_general_compute_stale_timeout").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("timeout-owner-{unique}");
        let worker_id = format!("timeout-worker-{unique}");
        let task_id = format!("timeout-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &format!("provider-{unique}")).await;

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "timeout-backend".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let manifest = serde_json::to_vec(&request).unwrap();

        let mut task = make_task(&task_id, &username);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(manifest);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.80")
            .await
            .unwrap();
        sqlx::query(
            "UPDATE tasks
             SET status = 'RUNNING', last_update = NOW() - INTERVAL '121 seconds'
             WHERE task_id = $1",
        )
        .bind(&task_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        assert_eq!(repo.mark_stale_running().await.unwrap(), 1);
        let timed_out = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(timed_out.status, TaskStatus::TimedOut);
        assert_eq!(
            timed_out.status_message.as_deref(),
            Some("Worker heartbeat lost")
        );

        let persisted: Vec<u8> = sqlx::query_scalar(
            "SELECT result_json FROM general_compute_results WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        let result: GeneralComputeResult = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(result.status, ResultStatus::TimedOut);
        assert_eq!(result.error_code.as_deref(), Some("worker_heartbeat_lost"));
        assert_eq!(result.execution_id, request.execution_id);
        assert_eq!(result.attempt_id, request.attempt_id);
        assert_eq!(result.request_digest, request.request_digest);
        assert_eq!(
            result.input_sha256,
            general_compute_runtime::canonical_input_digest(b"source", &[])
        );
        assert_eq!(result.output_manifest_root, canonical_artifact_root(&[]));

        let settlement_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_settlements WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(settlement_count, 0);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_fail_for_worker_persists_nodepool_typed_failure_without_settlement() {
        let (p, fixture) = match pool("task_repository_general_compute_nodepool_failure").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("nodepool-fail-owner-{unique}");
        let worker_id = format!("nodepool-fail-worker-{unique}");
        let task_id = format!("nodepool-fail-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &format!("provider-{unique}")).await;

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "nodepool-fail-backend".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let manifest = serde_json::to_vec(&request).unwrap();

        let mut task = make_task(&task_id, &username);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(manifest);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.82")
            .await
            .unwrap();

        let reason = "Max redispatch attempts exceeded";
        let failed = repo
            .fail_for_worker(&task_id, &worker_id, reason)
            .await
            .unwrap();
        assert_eq!(failed.status, TaskStatus::Failed);
        assert_eq!(failed.status_message.as_deref(), Some(reason));
        let lease_state: String = sqlx::query_scalar(
            "SELECT state FROM general_compute_transfer_leases WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(lease_state, "revoked");

        let persisted: Vec<u8> = sqlx::query_scalar(
            "SELECT result_json FROM general_compute_results WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        let result: GeneralComputeResult = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(result.status, ResultStatus::Failed);
        assert_eq!(result.error_code.as_deref(), Some("nodepool_task_failed"));
        assert_eq!(result.stderr, reason);
        assert_eq!(result.execution_id, request.execution_id);
        assert_eq!(result.attempt_id, request.attempt_id);
        assert_eq!(result.request_digest, request.request_digest);
        assert_eq!(
            result.input_sha256,
            general_compute_runtime::canonical_input_digest(b"source", &[])
        );
        assert_eq!(result.output_manifest_root, canonical_artifact_root(&[]));

        let settlement_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_settlements WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(settlement_count, 0);

        let reputation: (i64, i32) = sqlx::query_as(
            "SELECT failed_tasks, score FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(reputation, (1, 95));
        let attestation_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations
             WHERE task_id = $1 AND worker_id = $2 AND verdict = 'rejected'",
        )
        .bind(&task_id)
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(attestation_count, 1);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_fail_persists_nodepool_typed_failure_without_settlement() {
        let (p, fixture) = match pool("task_repository_general_compute_fail").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("generic-fail-owner-{unique}");
        let task_id = format!("generic-fail-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "generic-fail-backend".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let manifest = serde_json::to_vec(&request).unwrap();

        let mut task = make_task(&task_id, &username);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(manifest);
        repo.create(&task).await.unwrap();

        let reason = "Nodepool rejected the task";
        let failed = repo.fail(&task_id, reason).await.unwrap();
        assert_eq!(failed.status, TaskStatus::Failed);
        assert_eq!(failed.status_message.as_deref(), Some(reason));

        let persisted: (String, Vec<u8>) = sqlx::query_as(
            "SELECT worker_id, result_json FROM general_compute_results WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(persisted.0, "nodepool");
        let result: GeneralComputeResult = serde_json::from_slice(&persisted.1).unwrap();
        assert_eq!(result.status, ResultStatus::Failed);
        assert_eq!(result.error_code.as_deref(), Some("nodepool_task_failed"));
        assert_eq!(result.stderr, reason);
        assert_eq!(result.execution_id, request.execution_id);
        assert_eq!(result.attempt_id, request.attempt_id);
        assert_eq!(result.request_digest, request.request_digest);
        assert_eq!(
            result.input_sha256,
            general_compute_runtime::canonical_input_digest(b"source", &[])
        );
        assert_eq!(result.output_manifest_root, canonical_artifact_root(&[]));

        let settlement_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_settlements WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(settlement_count, 0);

        cleanup_task_case(&repo.pool, &task_id, &username, None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_fail_does_not_overwrite_completed_task() {
        let (p, fixture) = match pool("task_repository_fail_completed_guard").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("fail-guard-owner-{unique}");
        let task_id = format!("fail-guard-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();

        let mut task = make_task(&task_id, &username);
        task.max_cpt = 0;
        repo.create(&task).await.unwrap();
        let completed = repo.complete(&task_id, None, Some("done")).await.unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);

        let late_fail = repo.fail(&task_id, "late failure").await;
        assert!(late_fail.is_err());
        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Completed);
        assert_eq!(stored.output.as_deref(), Some("done"));

        cleanup_task_case(&repo.pool, &task_id, &username, None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_create_and_find_task() {
        let (p, fixture) = match pool("task_repository_create_and_find_task").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);

        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: "example-task-create-1".into(),
            owner: "example-user".into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: Some("test task".into()),
            output: None,
            result_torrent: None,
            torrent_source: Some("example-btih".into()),
            runtime: None,
            task_source: None,
            general_compute_manifest_json: None,
            managed_dsl_backend_id: None,
            managed_dsl_semantics_manifest_sha256: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 100,
            req_gpu_score: 0,
            req_memory_gb: 8,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
            retry_count: 0,
            max_retries: 3,
            deadline: None,
            deterministic: false,
            side_effects: false,
            priority: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            download_bytes: 0,
            cache_hits: 0,
            created_at: Utc::now(),
            last_update: Utc::now(),
            completed_at: None,
        };

        let created = repo.create(&task).await.unwrap();
        assert_eq!(created.task_id, "example-task-create-1");
        assert_eq!(created.status, TaskStatus::Pending);
        assert_eq!(created.req_storage_gb, 10);

        let found = repo.find_by_task_id("example-task-create-1").await.unwrap();
        assert!(found.is_some());

        sqlx::query("DELETE FROM tasks WHERE task_id = 'example-task-create-1'")
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_inline_artifacts_are_persisted_as_a_verified_source() {
        let (p, fixture) = match pool("task_repository_general_compute_artifact_source").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-artifact-source-{unique}");
        let bytes = b"trusted source bytes".to_vec();
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, &bytes),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();

        let mut task = make_task(&task_id, "artifact-source-owner");
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        let stored = repo
            .general_compute_artifact_bytes(
                &task_id,
                "source",
                &request.source_artifact.sha256,
                request.source_artifact.size_bytes,
            )
            .await
            .unwrap();
        assert_eq!(stored, Some(bytes.clone()));

        let tampered = vec![b'x'; request.source_artifact.size_bytes as usize];
        sqlx::query(
            "UPDATE general_compute_artifact_sources
             SET content = $1
             WHERE task_id = $2 AND artifact_id = $3",
        )
        .bind(&tampered)
        .bind(&task_id)
        .bind("source")
        .execute(&repo.pool)
        .await
        .unwrap();
        assert_eq!(
            repo.general_compute_artifact_bytes(
                &task_id,
                "source",
                &request.source_artifact.sha256,
                request.source_artifact.size_bytes,
            )
            .await
            .unwrap(),
            None,
            "content must be rehashed before it becomes a trusted source"
        );

        sqlx::query(
            "UPDATE general_compute_artifact_sources
             SET sha256 = $1, content = $2
             WHERE task_id = $3 AND artifact_id = 'source'",
        )
        .bind(general_compute_runtime::sha256_digest(b"different bytes"))
        .bind(&bytes)
        .bind(&task_id)
        .execute(&repo.pool)
        .await
        .unwrap();
        assert_eq!(
            repo.general_compute_artifact_bytes(
                &task_id,
                "source",
                &request.source_artifact.sha256,
                request.source_artifact.size_bytes,
            )
            .await
            .unwrap(),
            None,
            "a drifted source row must not be mistaken for a missing row"
        );

        sqlx::query("DELETE FROM general_compute_artifact_sources WHERE task_id = $1")
            .bind(&task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        let restored = repo
            .general_compute_artifact_bytes(
                &task_id,
                "source",
                &request.source_artifact.sha256,
                request.source_artifact.size_bytes,
            )
            .await
            .unwrap();
        assert_eq!(restored, Some(bytes));

        cleanup_task_case(&repo.pool, &task_id, "artifact-source-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_assignment_creates_and_rotates_transfer_lease() {
        let (p, fixture) = match pool("task_repository_general_compute_transfer_lease").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-transfer-lease-{unique}");
        let bytes = b"trusted transfer source";
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, bytes),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();

        let mut task = make_task(&task_id, "transfer-lease-owner");
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        repo.assign_to_worker(&task_id, "worker-a", "10.0.0.41")
            .await
            .unwrap();
        let first = repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .expect("assignment must create an active transfer lease");
        assert_eq!(first.worker_id, "worker-a");
        assert_eq!(first.execution_id, request.execution_id);
        assert_eq!(first.attempt_id, request.attempt_id);
        assert_eq!(first.generation, 1);
        assert_eq!(first.state, "active");

        repo.reset_to_pending_for_worker(&task_id, "worker-a")
            .await
            .unwrap();
        assert!(repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .is_none());

        repo.assign_to_worker(&task_id, "worker-b", "10.0.0.42")
            .await
            .unwrap();
        let second = repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .expect("reassignment must create a replacement lease");
        assert_eq!(second.worker_id, "worker-b");
        assert_eq!(second.generation, 2);
        assert_ne!(second.attempt_id, first.attempt_id);

        let revoked_state: String = sqlx::query_scalar(
            "SELECT state FROM general_compute_transfer_leases
             WHERE task_id = $1 AND generation = $2",
        )
        .bind(&task_id)
        .bind(first.generation)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(revoked_state, "revoked");

        cleanup_task_case(&repo.pool, &task_id, "transfer-lease-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_chunk_sources_are_idempotent_and_conflict_safe() {
        let (p, fixture) = match pool("task_repository_general_compute_chunk_source").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-chunk-source-{unique}");
        let mut task = make_task(&task_id, "chunk-source-owner");
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        let bytes = b"abcd";
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, bytes),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.source_artifact.chunks = vec![general_compute_runtime::ArtifactChunk {
            offset: 0,
            size_bytes: bytes.len() as u64,
            sha256: general_compute_runtime::sha256_digest(bytes),
        }];
        request.source_artifact.inline_bytes = None;
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        let digest = general_compute_runtime::sha256_digest(bytes);
        repo.put_general_compute_artifact_chunk(
            &task_id,
            "source",
            0,
            bytes.len() as u64,
            &digest,
            bytes,
        )
        .await
        .unwrap();
        repo.put_general_compute_artifact_chunk(
            &task_id,
            "source",
            0,
            bytes.len() as u64,
            &digest,
            bytes,
        )
        .await
        .unwrap();
        assert_eq!(
            repo.general_compute_artifact_chunks(&task_id, "source")
                .await
                .unwrap()
                .len(),
            1
        );

        let conflict = repo
            .put_general_compute_artifact_chunk(
                &task_id,
                "source",
                0,
                bytes.len() as u64,
                &general_compute_runtime::sha256_digest(b"wxyz"),
                b"wxyz",
            )
            .await;
        assert!(conflict.is_err());

        cleanup_task_case(&repo.pool, &task_id, "chunk-source-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_chunk_and_completion_state_are_atomic() {
        let (p, fixture) = match pool("task_repository_general_compute_chunk_atomicity").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-chunk-atomicity-{unique}");
        let bytes = b"atomic chunk";
        let request = chunked_general_compute_request(&unique, bytes);
        let task = task_for_general_compute_request(&task_id, "chunk-atomicity-owner", &request);
        repo.create(&task).await.unwrap();

        sqlx::query(
            "ALTER TABLE general_compute_artifacts
             ADD CONSTRAINT reject_complete_for_atomicity_test CHECK (complete = false)",
        )
        .execute(&repo.pool)
        .await
        .unwrap();

        let chunk = &request.source_artifact.chunks[0];
        repo.put_general_compute_artifact_chunk(
            &task_id,
            "source",
            chunk.offset,
            chunk.size_bytes,
            &chunk.sha256,
            bytes,
        )
        .await
        .expect_err("completion-state failure must abort the chunk write");

        let stored_chunks: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_artifact_chunks
             WHERE task_id = $1 AND artifact_id = 'source'",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(
            stored_chunks, 0,
            "chunk and completion state must commit atomically"
        );

        cleanup_task_case(&repo.pool, &task_id, "chunk-atomicity-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_chunk_source_rejects_an_unbound_manifest_coordinate() {
        let (p, fixture) = match pool("task_repository_general_compute_chunk_binding").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-chunk-binding-{unique}");
        let mut task = make_task(&task_id, "chunk-binding-owner");
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        let bytes = b"abcd";
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest {
                artifact_id: "source".into(),
                role: ArtifactRole::Source,
                size_bytes: bytes.len() as u64,
                mime_type: "text/plain".into(),
                sha256: general_compute_runtime::sha256_digest(bytes),
                chunks: vec![general_compute_runtime::ArtifactChunk {
                    offset: 0,
                    size_bytes: bytes.len() as u64,
                    sha256: general_compute_runtime::sha256_digest(bytes),
                }],
                inline_bytes: None,
            },
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        let error = repo
            .put_general_compute_artifact_chunk(
                &task_id,
                "not-in-manifest",
                0,
                bytes.len() as u64,
                &general_compute_runtime::sha256_digest(bytes),
                bytes,
            )
            .await
            .expect_err("chunk coordinates must be bound to the persisted manifest");
        assert!(error.to_string().contains("manifest"));

        cleanup_task_case(&repo.pool, &task_id, "chunk-binding-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_chunk_source_survives_attempt_rotation() {
        let (p, fixture) = match pool("task_repository_general_compute_chunk_retry").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-chunk-retry-{unique}");
        let bytes = b"retryable source";
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest {
                artifact_id: "source".into(),
                role: ArtifactRole::Source,
                size_bytes: bytes.len() as u64,
                mime_type: "text/plain".into(),
                sha256: general_compute_runtime::sha256_digest(bytes),
                chunks: vec![general_compute_runtime::ArtifactChunk {
                    offset: 0,
                    size_bytes: bytes.len() as u64,
                    sha256: general_compute_runtime::sha256_digest(bytes),
                }],
                inline_bytes: None,
            },
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let mut task = make_task(&task_id, "chunk-retry-owner");
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();
        let digest = general_compute_runtime::sha256_digest(bytes);
        repo.put_general_compute_artifact_chunk(
            &task_id,
            "source",
            0,
            bytes.len() as u64,
            &digest,
            bytes,
        )
        .await
        .unwrap();
        let worker_id = "chunk-retry-worker";
        insert_worker(&repo.pool, worker_id, "chunk-retry-provider").await;
        repo.assign_to_worker(&task_id, worker_id, "10.0.0.70")
            .await
            .unwrap();
        let reset = repo
            .reset_to_pending_for_worker(&task_id, worker_id)
            .await
            .unwrap();
        let rotated: GeneralComputeRequest =
            serde_json::from_slice(reset.general_compute_manifest_json.as_deref().unwrap())
                .unwrap();
        assert_ne!(rotated.attempt_id, request.attempt_id);
        assert_eq!(
            rotated.source_artifact.sha256,
            request.source_artifact.sha256
        );
        assert_eq!(
            repo.general_compute_artifact_bytes(
                &task_id,
                "source",
                &rotated.source_artifact.sha256,
                rotated.source_artifact.size_bytes,
            )
            .await
            .unwrap(),
            Some(bytes.to_vec())
        );

        cleanup_task_case(&repo.pool, &task_id, "chunk-retry-owner", Some(worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_artifact_lifecycle_persists_identity_completeness_and_expiry() {
        let (p, fixture) = match pool("task_repository_general_compute_artifact_lifecycle").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-artifact-lifecycle-{unique}");
        let bytes = b"durable lifecycle";
        let digest = general_compute_runtime::sha256_digest(bytes);
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest {
                artifact_id: "source".into(),
                role: ArtifactRole::Source,
                size_bytes: bytes.len() as u64,
                mime_type: "text/plain".into(),
                sha256: digest.clone(),
                chunks: vec![general_compute_runtime::ArtifactChunk {
                    offset: 0,
                    size_bytes: bytes.len() as u64,
                    sha256: digest.clone(),
                }],
                inline_bytes: None,
            },
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let mut task = make_task(&task_id, "artifact-lifecycle-owner");
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        let pending = repo
            .general_compute_artifact_state(&task_id, "source")
            .await
            .unwrap()
            .expect("task creation must persist immutable artifact identity");
        assert_eq!(pending.sha256, digest);
        assert_eq!(pending.size_bytes, bytes.len() as u64);
        assert_eq!(pending.availability_status, "pending");
        assert!(!pending.complete);

        repo.put_general_compute_artifact_chunk(
            &task_id,
            "source",
            0,
            bytes.len() as u64,
            &general_compute_runtime::sha256_digest(bytes),
            bytes,
        )
        .await
        .unwrap();

        let ready = repo
            .general_compute_artifact_state(&task_id, "source")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ready.availability_status, "available");
        assert!(ready.complete);
        assert_eq!(
            repo.general_compute_artifact_bytes(
                &task_id,
                "source",
                &ready.sha256,
                ready.size_bytes
            )
            .await
            .unwrap(),
            Some(bytes.to_vec())
        );

        sqlx::query(
            "UPDATE general_compute_artifacts
             SET expires_at = NOW() - INTERVAL '1 second'
             WHERE task_id = $1 AND artifact_id = 'source'",
        )
        .bind(&task_id)
        .execute(&repo.pool)
        .await
        .unwrap();
        let expired = repo
            .general_compute_artifact_state(&task_id, "source")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(expired.availability_status, "expired");
        assert!(!expired.complete);
        assert_eq!(
            repo.general_compute_artifact_bytes(
                &task_id,
                "source",
                &expired.sha256,
                expired.size_bytes,
            )
            .await
            .unwrap(),
            None,
            "expired bytes must not be a trusted scheduler source"
        );
        let rejected = repo
            .put_general_compute_artifact_chunk(
                &task_id,
                "source",
                0,
                bytes.len() as u64,
                &general_compute_runtime::sha256_digest(bytes),
                bytes,
            )
            .await
            .expect_err("expired artifacts must reject new source uploads");
        assert!(rejected.to_string().contains("expired"));

        cleanup_task_case(&repo.pool, &task_id, "artifact-lifecycle-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_artifact_coordinates_are_checked_against_immutable_identity() {
        let (p, fixture) = match pool("task_repository_general_compute_artifact_coordinates").await
        {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-artifact-coordinates-{unique}");
        let bytes = b"coordinate binding";
        let digest = general_compute_runtime::sha256_digest(bytes);
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest {
                artifact_id: "source".into(),
                role: ArtifactRole::Source,
                size_bytes: bytes.len() as u64,
                mime_type: "text/plain".into(),
                sha256: digest.clone(),
                chunks: vec![general_compute_runtime::ArtifactChunk {
                    offset: 0,
                    size_bytes: bytes.len() as u64,
                    sha256: digest.clone(),
                }],
                inline_bytes: None,
            },
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let mut task = make_task(&task_id, "artifact-coordinate-owner");
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        assert!(repo
            .general_compute_artifact_coordinates_match(
                &task_id,
                "source",
                request.source_artifact.size_bytes,
                &request.source_artifact.sha256,
                &request.source_artifact.chunks,
            )
            .await
            .unwrap());
        let mut changed = request.source_artifact.chunks.clone();
        changed[0].sha256 = general_compute_runtime::sha256_digest(b"different");
        assert!(!repo
            .general_compute_artifact_coordinates_match(
                &task_id,
                "source",
                request.source_artifact.size_bytes,
                &request.source_artifact.sha256,
                &changed,
            )
            .await
            .unwrap());

        cleanup_task_case(&repo.pool, &task_id, "artifact-coordinate-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn legacy_general_compute_source_backfill_preserves_verified_existing_bytes() {
        let (p, fixture) = match pool("task_repository_general_compute_artifact_backfill").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("general-compute-artifact-backfill-{unique}");
        let bytes = b"legacy trusted source".to_vec();
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, &bytes),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let mut task = make_task(&task_id, "artifact-backfill-owner");
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();
        sqlx::query("DELETE FROM general_compute_artifact_manifest_chunks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM general_compute_artifacts WHERE task_id = $1")
            .bind(&task_id)
            .execute(&repo.pool)
            .await
            .unwrap();

        assert!(repo
            .general_compute_artifact_coordinates_match(
                &task_id,
                "source",
                request.source_artifact.size_bytes,
                &request.source_artifact.sha256,
                &request.source_artifact.chunks,
            )
            .await
            .unwrap());
        assert_eq!(
            repo.general_compute_artifact_bytes(
                &task_id,
                "source",
                &request.source_artifact.sha256,
                request.source_artifact.size_bytes,
            )
            .await
            .unwrap(),
            Some(bytes)
        );

        cleanup_task_case(&repo.pool, &task_id, "artifact-backfill-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_settles_billing_when_balance_is_sufficient() {
        let (p, fixture) = match pool("task_repository_complete_settles_billing").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("billing-ok-user-{unique}");
        let provider = format!("billing-provider-{unique}");
        let worker_id = format!("billing-worker-{unique}");
        let task_id = format!("billing-ok-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.max_cpt = 25;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.10")
            .await
            .unwrap();

        let completed = repo
            .complete(&task_id, Some("result-btih"), Some("done"))
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
        assert!(completed.billing_settled);
        assert_eq!(completed.billed_amount, 25);

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 75);

        let rows: Vec<LedgerRow> = sqlx::query_as(
            "SELECT kind, payer_user, provider_worker_id, provider_user, amount_cpt, status
             FROM ledger_entries WHERE task_id = $1 ORDER BY kind",
        )
        .bind(&task_id)
        .fetch_all(&repo.pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "payer_debit".to_string(),
                    username.clone(),
                    Some(worker_id.clone()),
                    Some(provider.clone()),
                    25,
                    "settled".to_string(),
                ),
                (
                    "platform_fee".to_string(),
                    username.clone(),
                    Some(worker_id.clone()),
                    Some(provider.clone()),
                    2,
                    "settled".to_string(),
                ),
                (
                    "provider_credit".to_string(),
                    username.clone(),
                    Some(worker_id.clone()),
                    Some(provider.clone()),
                    23,
                    "settled".to_string(),
                ),
            ]
        );

        let reputation: (i64, i64, i32) = sqlx::query_as(
            "SELECT successful_tasks, failed_tasks, score FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(reputation, (1, 0, 101));

        let attestation_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations WHERE task_id = $1 AND worker_id = $2",
        )
        .bind(&task_id)
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(attestation_count, 1);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_assign_to_worker_does_not_overwrite_existing_assignment() {
        let (p, fixture) = match pool("task_repository_assign_no_overwrite").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("assign-owner-{unique}");
        let first_worker = format!("assign-worker-a-{unique}");
        let second_worker = format!("assign-worker-b-{unique}");
        let task_id = format!("assign-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &first_worker, "assign-provider-a").await;
        insert_worker(&repo.pool, &second_worker, "assign-provider-b").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();

        let assigned = repo
            .assign_to_worker(&task_id, &first_worker, "10.0.0.21")
            .await
            .unwrap();
        assert_eq!(assigned.worker_id.as_deref(), Some(first_worker.as_str()));

        let second = repo
            .assign_to_worker(&task_id, &second_worker, "10.0.0.22")
            .await;
        assert!(second.is_err(), "second assignment should not overwrite");

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.worker_id.as_deref(), Some(first_worker.as_str()));
        assert_eq!(stored.worker_ip.as_deref(), Some("10.0.0.21"));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&first_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&second_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_does_not_overlap_between_repositories() {
        let fixture = match hivemind_database::postgres::create_isolated_test_pool(
            "task_repository_claim_overlap",
        )
        .await
        {
            Ok(fixture) => fixture,
            Err(_) => return,
        };
        let p = fixture.pool.clone();
        hivemind_database::postgres::run_migrations(&p).await.ok();
        sqlx::query("DELETE FROM tasks WHERE task_id LIKE 'claim-task-%'")
            .execute(&p)
            .await
            .ok();
        sqlx::query(
            "DELETE FROM worker_reputation
             WHERE worker_id LIKE 'claim-worker-a-%' OR worker_id LIKE 'claim-worker-b-%'",
        )
        .execute(&p)
        .await
        .ok();
        sqlx::query(
            "DELETE FROM worker_nodes
             WHERE worker_id LIKE 'claim-worker-a-%' OR worker_id LIKE 'claim-worker-b-%'",
        )
        .execute(&p)
        .await
        .ok();
        sqlx::query("DELETE FROM users WHERE username LIKE 'claim-owner-%'")
            .execute(&p)
            .await
            .ok();
        let repo_a = TaskRepository::new(p.clone());
        let repo_b = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("claim-owner-{unique}");
        let worker_a = format!("claim-worker-a-{unique}");
        let worker_b = format!("claim-worker-b-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&p)
        .await
        .unwrap();
        insert_worker(&p, &worker_a, "claim-provider-a").await;
        insert_worker(&p, &worker_b, "claim-provider-b").await;
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 10, 0, 100, false), ($2, 10, 0, 100, false)",
        )
        .bind(&worker_a)
        .bind(&worker_b)
        .execute(&p)
        .await
        .unwrap();

        let mut task_ids = Vec::new();
        for index in 0..4 {
            let task_id = format!("claim-task-{index}-{unique}");
            task_ids.push(task_id.clone());
            let mut task = make_task(&task_id, &username);
            task.priority = 10_000 - index;
            repo_a.create(&task).await.unwrap();
        }

        let (claimed_a, claimed_b) = tokio::join!(
            repo_a.claim_pending_for_worker(&worker_a, "10.0.0.31", 2),
            repo_b.claim_pending_for_worker(&worker_b, "10.0.0.32", 2),
        );
        let claimed_a = claimed_a.unwrap();
        let claimed_b = claimed_b.unwrap();

        let claimed_a_ids: std::collections::HashSet<_> =
            claimed_a.iter().map(|task| task.task_id.as_str()).collect();
        let claimed_b_ids: std::collections::HashSet<_> =
            claimed_b.iter().map(|task| task.task_id.as_str()).collect();

        assert!(!claimed_a_ids.is_empty());
        assert!(!claimed_b_ids.is_empty());
        assert!(
            claimed_a_ids.is_disjoint(&claimed_b_ids),
            "claimed task sets must not overlap"
        );
        assert_eq!(claimed_a.len() + claimed_b.len(), 4);

        let assigned_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE task_id = ANY($1) AND status = 'ASSIGNED'",
        )
        .bind(&task_ids)
        .fetch_one(&p)
        .await
        .unwrap();
        assert_eq!(assigned_count, 4);

        for task_id in task_ids {
            sqlx::query("DELETE FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .execute(&p)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
            .bind(&worker_a)
            .bind(&worker_b)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
            .bind(&worker_a)
            .bind(&worker_b)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&p)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_blocks_banned_worker() {
        let (p, fixture) = match pool("task_repository_claim_blocks_banned_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("claim-ban-owner-{unique}");
        let worker_id = format!("claim-ban-worker-{unique}");
        let task_id = format!("claim-ban-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&p)
        .await
        .unwrap();
        insert_worker(&p, &worker_id, "claim-ban-provider").await;

        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 10, 0, 200, true)",
        )
        .bind(&worker_id)
        .execute(&p)
        .await
        .unwrap();

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();

        let claimed = repo
            .claim_pending_for_worker(&worker_id, "10.0.0.31", 5)
            .await
            .unwrap();
        assert!(claimed.is_empty());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&p)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_blocks_low_score_worker() {
        let (p, fixture) = match pool("task_repository_claim_blocks_low_score_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("claim-score-owner-{unique}");
        let worker_id = format!("claim-score-worker-{unique}");
        let task_id = format!("claim-score-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&p)
        .await
        .unwrap();
        insert_worker(&p, &worker_id, "claim-score-provider").await;

        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 0, 5, 10, false)",
        )
        .bind(&worker_id)
        .execute(&p)
        .await
        .unwrap();

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();

        let claimed = repo
            .claim_pending_for_worker(&worker_id, "10.0.0.32", 5)
            .await
            .unwrap();
        assert!(claimed.is_empty());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&p)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_trusted_workers_excludes_missing_reputation_rows() {
        let (p, fixture) = match pool("task_repository_trusted_workers_missing_reputation").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let trusted_worker = format!("trust-present-worker-{unique}");
        let missing_worker = format!("trust-missing-worker-{unique}");

        insert_worker(&p, &trusted_worker, "trust-present-provider").await;
        insert_worker(&p, &missing_worker, "trust-missing-provider").await;
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 10, 0, 100, false)",
        )
        .bind(&trusted_worker)
        .execute(&p)
        .await
        .unwrap();

        let workers = vec![
            make_worker_node(&trusted_worker, "10.0.0.41"),
            make_worker_node(&missing_worker, "10.0.0.42"),
        ];
        let trusted = repo.trusted_workers(&workers).await.unwrap();

        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].worker_id, trusted_worker);

        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
            .bind(&trusted_worker)
            .bind(&missing_worker)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
            .bind(&trusted_worker)
            .bind(&missing_worker)
            .execute(&p)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_blocks_missing_reputation_row() {
        let (p, fixture) = match pool("task_repository_claim_blocks_missing_reputation").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("claim-missing-rep-owner-{unique}");
        let worker_id = format!("claim-missing-rep-worker-{unique}");
        let task_id = format!("claim-missing-rep-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&p)
        .await
        .unwrap();
        insert_worker(&p, &worker_id, "claim-missing-rep-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();

        let claimed = repo
            .claim_pending_for_worker(&worker_id, "10.0.0.43", 5)
            .await
            .unwrap();
        assert!(claimed.is_empty());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());

        cleanup_task_case(&p, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_complete_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("stale-complete-owner-{unique}");
        let stale_worker = format!("stale-complete-old-{unique}");
        let current_worker = format!("stale-complete-current-{unique}");
        let task_id = format!("stale-complete-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "stale-provider-old").await;
        insert_worker(&repo.pool, &current_worker, "stale-provider-current").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.41")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.42")
            .await
            .unwrap();

        let stale_complete = repo
            .complete_for_worker(
                &task_id,
                &stale_worker,
                Some("old-result"),
                Some("old output"),
            )
            .await;
        assert!(stale_complete.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));
        assert_eq!(stored.result_torrent, None);
        assert_eq!(stored.output, None);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_for_worker_does_not_overwrite_cancelled_task() {
        let (p, fixture) = match pool("task_repository_complete_does_not_overwrite_cancelled").await
        {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("cancel-complete-owner-{unique}");
        let worker_id = format!("cancel-complete-worker-{unique}");
        let task_id = format!("cancel-complete-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "cancel-complete-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.44")
            .await
            .unwrap();
        repo.cancel(&task_id).await.unwrap();

        let late_complete = repo
            .complete_for_worker(
                &task_id,
                &worker_id,
                Some("late-result"),
                Some("late output"),
            )
            .await;
        assert!(late_complete.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Cancelled);
        assert_eq!(stored.result_torrent, None);
        assert_eq!(stored.output, None);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_general_compute_for_worker_rejects_old_manifest() {
        let (p, fixture) = match pool("task_repository_general_compute_manifest_guard").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("manifest-guard-owner-{unique}");
        let worker_id = format!("manifest-guard-worker-{unique}");
        let task_id = format!("manifest-guard-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "manifest-guard-provider").await;

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: "current-attempt".into(),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let current_manifest = serde_json::to_vec(&request).unwrap();
        let typed_result_json = serde_json::to_vec(&GeneralComputeResult {
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            status: ResultStatus::Completed,
            exit_code: Some(0),
            error_code: None,
            stdout: "current output".into(),
            stderr: String::new(),
            output_artifacts: vec![],
            usage: UsageClaim {
                wall_time_ms: 1,
                ..UsageClaim::default()
            },
            runtime_version: request.runtime_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            input_sha256: general_compute_runtime::canonical_input_digest(b"source", &[]),
            determinism: request.determinism.clone(),
            capability_summary: vec![],
            gpu_selection: None,
            output_manifest_root: canonical_artifact_root(&[]),
            evidence: EvidenceEnvelope::default(),
        })
        .unwrap();
        let stale_result_json = br#"{"status":"stale"}"#;

        let mut task = make_task(&task_id, &username);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(current_manifest.clone());
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.56")
            .await
            .unwrap();

        let mut stale_request = request.clone();
        stale_request.attempt_id = "old-attempt".into();
        stale_request.request_digest = stale_request.canonical_request_digest();
        let stale_manifest = serde_json::to_vec(&stale_request).unwrap();
        let stale_result = repo
            .complete_general_compute_for_worker(
                &task_id,
                &worker_id,
                &stale_manifest,
                stale_result_json,
                Some("stale output"),
            )
            .await;
        assert!(stale_result.is_err());

        let persisted_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM general_compute_results WHERE task_id = $1")
                .bind(&task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        assert_eq!(persisted_count, 0);

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert!(stored.output.is_none());
        assert!(!stored.billing_settled);

        let completed = repo
            .complete_general_compute_for_worker(
                &task_id,
                &worker_id,
                &current_manifest,
                &typed_result_json,
                Some("current output"),
            )
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
        assert_eq!(completed.output.as_deref(), Some("current output"));

        let persisted: (String, Vec<u8>) = sqlx::query_as(
            "SELECT worker_id, result_json
             FROM general_compute_results
             WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(persisted.0, worker_id);
        assert_eq!(persisted.1, typed_result_json);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_report_output_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_report_output_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("report-output-owner-{unique}");
        let stale_worker = format!("report-output-old-{unique}");
        let current_worker = format!("report-output-current-{unique}");
        let task_id = format!("report-output-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "report-output-provider-old").await;
        insert_worker(
            &repo.pool,
            &current_worker,
            "report-output-provider-current",
        )
        .await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.47")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.48")
            .await
            .unwrap();

        let stale_output = repo
            .record_output_for_worker(&task_id, &stale_worker, "old worker output")
            .await;
        assert!(stale_output.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));
        assert_eq!(stored.output, None);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_update_resource_usage_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_usage_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("report-usage-owner-{unique}");
        let stale_worker = format!("report-usage-old-{unique}");
        let current_worker = format!("report-usage-current-{unique}");
        let task_id = format!("report-usage-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "report-usage-provider-old").await;
        insert_worker(&repo.pool, &current_worker, "report-usage-provider-current").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.49")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.50")
            .await
            .unwrap();

        let stale_usage = repo
            .update_resource_usage_for_worker(&task_id, &stale_worker, 11.0, 22.0, 33.0, 44.0)
            .await;
        assert!(stale_usage.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));
        assert_eq!(stored.cpu_usage, 0.0);
        assert_eq!(stored.memory_usage, 0.0);
        assert_eq!(stored.gpu_usage, 0.0);
        assert_eq!(stored.gpu_memory_usage, 0.0);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_result_for_worker_preserves_reported_output() {
        let (p, fixture) = match pool("task_repository_result_preserves_output").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("report-result-owner-{unique}");
        let worker_id = format!("report-result-worker-{unique}");
        let task_id = format!("report-result-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "report-result-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.51")
            .await
            .unwrap();
        repo.record_output_for_worker(&task_id, &worker_id, "stdout before result")
            .await
            .unwrap();

        let completed = repo
            .complete_result_for_worker(&task_id, &worker_id, "btih:reported-result")
            .await
            .unwrap();

        assert_eq!(completed.status, TaskStatus::Completed);
        assert_eq!(completed.output.as_deref(), Some("stdout before result"));
        assert_eq!(
            completed.result_torrent.as_deref(),
            Some("btih:reported-result")
        );

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_record_batch_report_for_worker_rejects_wrong_worker() {
        let (p, fixture) = match pool("task_repository_batch_report_rejects_wrong_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("batch-report-owner-{unique}");
        let worker_id = format!("batch-report-worker-{unique}");
        let wrong_worker = format!("batch-report-wrong-worker-{unique}");
        let task_id = format!("batch-report-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "batch-report-provider").await;
        insert_worker(&repo.pool, &wrong_worker, "batch-report-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.52")
            .await
            .unwrap();
        repo.complete_for_worker(&task_id, &worker_id, Some("result"), None)
            .await
            .unwrap();

        let wrong_report = repo
            .record_batch_report_for_worker(
                &task_id,
                &wrong_worker,
                BatchTaskReport {
                    output: Some("wrong worker log"),
                    cpu_time_ms: 10,
                    wall_time_ms: 20,
                    peak_memory_mb: 30,
                    download_bytes: 40,
                    cache_hits: 50,
                },
            )
            .await;
        assert!(wrong_report.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.worker_id.as_deref(), Some(worker_id.as_str()));
        assert_eq!(stored.output, None);
        assert_eq!(stored.cpu_time_ms, 0);
        assert_eq!(stored.cache_hits, 0);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&wrong_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_cancel_does_not_overwrite_completed_task() {
        let (p, fixture) = match pool("task_repository_cancel_does_not_overwrite_completed").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("cancel-completed-owner-{unique}");
        let worker_id = format!("cancel-completed-worker-{unique}");
        let task_id = format!("cancel-completed-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "cancel-completed-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.45")
            .await
            .unwrap();
        repo.complete_for_worker(&task_id, &worker_id, Some("result"), Some("output"))
            .await
            .unwrap();

        let late_cancel = repo.cancel(&task_id).await;
        assert!(late_cancel.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Completed);
        assert_eq!(stored.result_torrent.as_deref(), Some("result"));
        assert_eq!(stored.output.as_deref(), Some("output"));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_fail_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_fail_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("stale-fail-owner-{unique}");
        let stale_worker = format!("stale-fail-old-{unique}");
        let current_worker = format!("stale-fail-current-{unique}");
        let task_id = format!("stale-fail-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "stale-fail-provider-old").await;
        insert_worker(&repo.pool, &current_worker, "stale-fail-provider-current").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.43")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.44")
            .await
            .unwrap();

        let stale_fail = repo
            .fail_for_worker(&task_id, &stale_worker, "old failure")
            .await;
        assert!(stale_fail.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));
        assert_ne!(stored.status_message.as_deref(), Some("old failure"));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_reset_to_pending_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_reset_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("stale-reset-owner-{unique}");
        let stale_worker = format!("stale-reset-old-{unique}");
        let current_worker = format!("stale-reset-current-{unique}");
        let task_id = format!("stale-reset-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "stale-reset-provider-old").await;
        insert_worker(&repo.pool, &current_worker, "stale-reset-provider-current").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.45")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.46")
            .await
            .unwrap();

        let stale_reset = repo
            .reset_to_pending_for_worker(&task_id, &stale_worker)
            .await;
        assert!(stale_reset.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_reset_to_pending_for_worker_rotates_general_compute_attempt_identity() {
        let (p, fixture) = match pool("task_repository_reset_rotates_general_compute_attempt").await
        {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("attempt-rotate-owner-{unique}");
        let worker_id = format!("attempt-rotate-worker-{unique}");
        let task_id = format!("attempt-rotate-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "attempt-rotate-provider").await;

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: "attempt-1".into(),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();

        let mut task = make_task(&task_id, &username);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.55")
            .await
            .unwrap();

        let reset = repo
            .reset_to_pending_for_worker(&task_id, &worker_id)
            .await
            .unwrap();
        let updated: GeneralComputeRequest =
            serde_json::from_slice(reset.general_compute_manifest_json.as_deref().unwrap())
                .unwrap();

        assert_eq!(updated.execution_id, request.execution_id);
        assert_ne!(updated.attempt_id, request.attempt_id);
        assert_eq!(updated.idempotency_key, request.idempotency_key);
        assert_eq!(updated.request_digest, updated.canonical_request_digest());
        assert_eq!(reset.retry_count, 1);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_is_idempotent_for_settled_billing_and_ledger() {
        let (p, fixture) = match pool("task_repository_complete_idempotent_billing").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("billing-repeat-user-{unique}");
        let provider = format!("billing-repeat-provider-{unique}");
        let worker_id = format!("billing-repeat-worker-{unique}");
        let task_id = format!("billing-repeat-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.max_cpt = 25;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.11")
            .await
            .unwrap();

        repo.complete(&task_id, Some("result-btih"), Some("done"))
            .await
            .unwrap();
        let completed_again = repo
            .complete(&task_id, Some("result-btih-2"), Some("done again"))
            .await
            .unwrap();

        assert_eq!(completed_again.status, TaskStatus::Completed);
        assert!(completed_again.billing_settled);
        assert_eq!(completed_again.billed_amount, 25);

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 75);

        let ledger_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ledger_entries WHERE task_id = $1")
                .bind(&task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        assert_eq!(ledger_count, 3);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_does_not_fail_task_when_billing_balance_is_insufficient() {
        let (p, fixture) = match pool("task_repository_complete_insufficient_balance").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("billing-zero-user-{unique}");
        let provider = format!("billing-zero-provider-{unique}");
        let worker_id = format!("billing-zero-worker-{unique}");
        let task_id = format!("billing-zero-task-{unique}");

        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 0)")
            .bind(&username)
            .execute(&repo.pool)
            .await
            .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.max_cpt = 25;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.12")
            .await
            .unwrap();

        let completed = repo
            .complete(&task_id, Some("result-btih"), Some("done"))
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
        assert!(!completed.billing_settled);
        assert_eq!(completed.billed_amount, 0);
        assert_ne!(
            completed.status_message.as_deref(),
            Some("insufficient balance")
        );

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 0);

        let ledger_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ledger_entries WHERE task_id = $1")
                .bind(&task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        assert_eq!(ledger_count, 0);

        let failure_rep: (i64, i64, i32) = sqlx::query_as(
            "SELECT successful_tasks, failed_tasks, score FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap_or((0, 0, 100));
        assert_eq!(failure_rep, (1, 0, 101));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_managed_complete_settles_billing_from_receipt() {
        let (p, fixture) = match pool("task_repository_managed_receipt_billing").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("managed-billing-user-{unique}");
        let provider = format!("managed-billing-provider-{unique}");
        let worker_id = format!("managed-billing-worker-{unique}");
        let task_id = format!("managed-billing-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.runtime = Some("managed-function-v0".into());
        task.max_cpt = 25;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.15")
            .await
            .unwrap();

        let completed = repo
            .complete_for_worker_with_managed_receipt(
                &task_id,
                &worker_id,
                Some("7"),
                2_500,
                2_049,
                "{\"usage_units\":2500,\"output_bytes\":2049}",
            )
            .await
            .unwrap();

        assert!(completed.billing_settled);
        assert_eq!(completed.billed_amount, 25);
        assert_eq!(completed.managed_executed_ops, 2_500);
        assert_eq!(completed.managed_output_bytes, 2_049);
        assert_eq!(
            completed.managed_receipt_json.as_deref(),
            Some("{\"usage_units\":2500,\"output_bytes\":2049}")
        );

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 75);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_managed_receipt_billing_is_capped_by_max_cpt() {
        let (p, fixture) = match pool("task_repository_managed_receipt_billing_cap").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("managed-cap-user-{unique}");
        let provider = format!("managed-cap-provider-{unique}");
        let worker_id = format!("managed-cap-worker-{unique}");
        let task_id = format!("managed-cap-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.runtime = Some("managed-function-v0".into());
        task.max_cpt = 5;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.16")
            .await
            .unwrap();

        let completed = repo
            .complete_for_worker_with_managed_receipt(
                &task_id,
                &worker_id,
                Some("large"),
                10_000,
                8_192,
                "{\"executed_ops\":10000,\"output_bytes\":8192}",
            )
            .await
            .unwrap();

        assert!(completed.billing_settled);
        assert_eq!(completed.billed_amount, 5);

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 95);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_deterministic_complete_requires_result_reference() {
        let (p, fixture) = match pool("task_repository_deterministic_requires_result").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("verify-missing-owner-{unique}");
        let worker_id = format!("verify-missing-worker-{unique}");
        let task_id = format!("verify-missing-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "verify-provider").await;

        let mut task = make_task(&task_id, &username);
        task.deterministic = true;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.13")
            .await
            .unwrap();

        let result = repo
            .complete_for_worker(&task_id, &worker_id, None, Some("done"))
            .await;
        assert!(result.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert!(stored.result_torrent.is_none());

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_deterministic_complete_records_checksum_proof() {
        let (p, fixture) = match pool("task_repository_deterministic_checksum_proof").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("verify-proof-owner-{unique}");
        let worker_id = format!("verify-proof-worker-{unique}");
        let task_id = format!("verify-proof-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "verify-provider").await;

        let mut task = make_task(&task_id, &username);
        task.deterministic = true;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.14")
            .await
            .unwrap();

        let completed = repo
            .complete_for_worker(
                &task_id,
                &worker_id,
                Some("sha1:result-reference"),
                Some("done"),
            )
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);

        let proof_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations
             WHERE task_id = $1 AND worker_id = $2 AND verdict = 'checksum_proof'",
        )
        .bind(&task_id)
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(proof_count, 1);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    async fn insert_worker(pool: &PgPool, worker_id: &str, username: &str) {
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '10.0.0.2', 4, 16)",
        )
        .bind(worker_id)
        .bind(username)
        .execute(pool)
        .await
        .unwrap();
    }

    fn make_worker_node(worker_id: &str, ip: &str) -> WorkerNode {
        WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: worker_id.into(),
            username: "test".into(),
            ip: ip.into(),
            virtual_ip: None,
            hostname: None,
            cpu_cores: 4,
            memory_gb: 16,
            cpu_score: 400,
            gpu_score: 0,
            gpu_memory_gb: 0,
            gpu_name: None,
            vram_mb: 0,
            storage_total_gb: 500,
            storage_available_gb: 200,
            provider_enabled: true,
            cpu_cores_limit: 0,
            memory_gb_limit: 0,
            gpu_memory_gb_limit: 0,
            storage_gb_limit: 0,
            min_cpt_per_hour: 0,
            location: "local".into(),
            status: hivemind_models::WorkerStatus::Idle,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: 16,
            queue_capacity: 4,
            general_compute_capabilities_json: None,
            managed_dsl_capabilities_json: None,
            admission_mode: hivemind_models::PRIVATE_STATIC_ADMISSION_MODE.into(),
            dynamic_capabilities_json: None,
            dynamic_capabilities_digest: None,
            dynamic_admission_ready: false,
            dynamic_readiness_reason: None,
            dynamic_observed_at: None,
            last_heartbeat: Utc::now(),
            registered_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn general_compute_claim_creates_attempt_bound_transfer_lease() {
        let (p, fixture) = match pool("task_repository_general_compute_lease_claim").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let worker_id = format!("lease-claim-worker-{unique}");
        sqlx::query(
            "INSERT INTO worker_reputation
                (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 1, 0, 100, false)",
        )
        .bind(&worker_id)
        .execute(&repo.pool)
        .await
        .unwrap();
        let request = inline_general_compute_request(&unique, b"claim lease source");
        let task_id = format!("lease-claim-{unique}");
        let mut task = task_for_general_compute_request(&task_id, "lease-claim-owner", &request);
        task.max_cpt = 0;
        repo.create(&task).await.unwrap();

        let claimed = repo
            .claim_pending_for_worker(&worker_id, "10.0.0.9", 1)
            .await
            .unwrap();
        assert_eq!(claimed.len(), 1);
        let lease = repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .expect("claim and lease creation must commit together");
        assert!(lease.matches_assignment(
            &task_id,
            &request.execution_id,
            &request.attempt_id,
            &worker_id,
        ));

        cleanup_task_case(&repo.pool, &task_id, "lease-claim-owner", None).await;
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_transfer_lease_is_bound_to_the_assignment() {
        let (p, fixture) = match pool("task_repository_general_compute_lease_binding").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let (task_id, request) =
            create_assigned_general_compute_task(&repo, "lease-binding", "worker-a").await;

        let lease = repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .expect("general-compute assignment must create a lease");
        assert_eq!(lease.generation, 1);
        assert!(lease.matches_assignment(
            &task_id,
            &request.execution_id,
            &request.attempt_id,
            "worker-a",
        ));
        assert!(!lease.matches_assignment(
            &task_id,
            &request.execution_id,
            &request.attempt_id,
            "worker-b",
        ));

        let legacy_task_id = format!("legacy-lease-{}", uuid::Uuid::new_v4());
        let legacy = make_task(&legacy_task_id, "legacy-lease-owner");
        repo.create(&legacy).await.unwrap();
        repo.assign_to_worker(&legacy_task_id, "worker-a", "10.0.0.1")
            .await
            .unwrap();
        assert!(repo
            .general_compute_transfer_lease(&legacy_task_id)
            .await
            .unwrap()
            .is_none());

        cleanup_task_case(&repo.pool, &task_id, "lease-binding-owner", None).await;
        cleanup_task_case(&repo.pool, &legacy_task_id, "legacy-lease-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_redispatch_revokes_and_rotates_transfer_lease() {
        let (p, fixture) = match pool("task_repository_general_compute_lease_rotation").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let (task_id, request) =
            create_assigned_general_compute_task(&repo, "lease-rotation", "worker-a").await;
        let first = repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .unwrap();

        let pending = repo
            .reset_to_pending_for_worker(&task_id, "worker-a")
            .await
            .unwrap();
        assert!(repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .is_none());
        let rotated: GeneralComputeRequest =
            serde_json::from_slice(pending.general_compute_manifest_json.as_deref().unwrap())
                .unwrap();
        assert_ne!(rotated.attempt_id, request.attempt_id);

        repo.assign_to_worker(&task_id, "worker-b", "10.0.0.2")
            .await
            .unwrap();
        let second = repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.generation, first.generation + 1);
        assert!(second.matches_assignment(
            &task_id,
            &rotated.execution_id,
            &rotated.attempt_id,
            "worker-b",
        ));

        let first_state: String = sqlx::query_scalar(
            "SELECT state FROM general_compute_transfer_leases
             WHERE task_id = $1 AND generation = $2",
        )
        .bind(&task_id)
        .bind(first.generation)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(first_state, "revoked");

        cleanup_task_case(&repo.pool, &task_id, "lease-rotation-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_transfer_lease_expiry_is_materialized_fail_closed() {
        let (p, fixture) = match pool("task_repository_general_compute_lease_expiry").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let (task_id, _) =
            create_assigned_general_compute_task(&repo, "lease-expiry", "worker-a").await;
        sqlx::query(
            "UPDATE general_compute_transfer_leases
             SET expires_at = NOW() - INTERVAL '1 second'
             WHERE task_id = $1 AND state = 'active'",
        )
        .bind(&task_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        assert!(repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .is_none());
        let state: String = sqlx::query_scalar(
            "SELECT state FROM general_compute_transfer_leases WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(state, "expired");

        cleanup_task_case(&repo.pool, &task_id, "lease-expiry-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_fail_without_worker_penalty_is_typed_and_non_settling() {
        let (p, fixture) = match pool("task_repository_general_compute_no_penalty_failure").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let worker_id = format!("no-penalty-worker-{}", uuid::Uuid::new_v4());
        let (task_id, request) =
            create_assigned_general_compute_task(&repo, "no-penalty-failure", &worker_id).await;
        sqlx::query(
            "INSERT INTO worker_reputation
             (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 0, 0, 100, false)",
        )
        .bind(&worker_id)
        .execute(&repo.pool)
        .await
        .unwrap();
        let reason = "Nodepool has no trusted artifact source";

        let failed = repo
            .fail_for_worker_without_penalty(&task_id, &worker_id, reason)
            .await
            .unwrap();

        assert_eq!(failed.status, TaskStatus::Failed);
        assert_eq!(failed.status_message.as_deref(), Some(reason));
        assert!(repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .is_none());
        let result_json: Vec<u8> = sqlx::query_scalar(
            "SELECT result_json FROM general_compute_results WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        let result: GeneralComputeResult = serde_json::from_slice(&result_json).unwrap();
        assert_eq!(result.status, ResultStatus::Failed);
        assert_eq!(result.error_code.as_deref(), Some("nodepool_task_failed"));
        assert_eq!(result.stderr, reason);
        assert_eq!(result.execution_id, request.execution_id);
        assert_eq!(result.attempt_id, request.attempt_id);
        assert_eq!(result.request_digest, request.request_digest);
        let settlement_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_settlements WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(settlement_count, 0);
        let reputation: (i64, i32) = sqlx::query_as(
            "SELECT failed_tasks, score FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(reputation, (0, 100));
        let attestation_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM task_attestations WHERE task_id = $1")
                .bind(&task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        assert_eq!(attestation_count, 0);

        cleanup_task_case(
            &repo.pool,
            &task_id,
            "no-penalty-failure-owner",
            Some(&worker_id),
        )
        .await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn general_compute_terminal_transitions_revoke_transfer_leases() {
        let (p, fixture) = match pool("task_repository_general_compute_lease_terminal").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);

        for transition in [
            "complete",
            "cancel",
            "fail",
            "fail_without_penalty",
            "timeout",
        ] {
            let worker_id = format!("worker-{transition}");
            let (task_id, _) = create_assigned_general_compute_task(
                &repo,
                &format!("lease-{transition}"),
                &worker_id,
            )
            .await;
            match transition {
                "complete" => {
                    repo.complete_for_worker(&task_id, &worker_id, None, Some("done"))
                        .await
                        .unwrap();
                }
                "cancel" => {
                    repo.cancel(&task_id).await.unwrap();
                }
                "fail" => {
                    repo.fail(&task_id, "terminal lease test").await.unwrap();
                }
                "fail_without_penalty" => {
                    repo.fail_for_worker_without_penalty(
                        &task_id,
                        &worker_id,
                        "operator admission failed",
                    )
                    .await
                    .unwrap();
                }
                "timeout" => {
                    sqlx::query(
                        "UPDATE tasks
                         SET status = 'RUNNING', last_update = NOW() - INTERVAL '121 seconds'
                         WHERE task_id = $1",
                    )
                    .bind(&task_id)
                    .execute(&repo.pool)
                    .await
                    .unwrap();
                    assert_eq!(repo.mark_stale_running().await.unwrap(), 1);
                }
                _ => unreachable!(),
            }

            let state: String = sqlx::query_scalar(
                "SELECT state FROM general_compute_transfer_leases WHERE task_id = $1",
            )
            .bind(&task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
            assert_eq!(
                state, "revoked",
                "{transition} must revoke transfer authority"
            );
            assert!(repo
                .general_compute_transfer_lease(&task_id)
                .await
                .unwrap()
                .is_none());

            cleanup_task_case(
                &repo.pool,
                &task_id,
                &format!("lease-{transition}-owner"),
                None,
            )
            .await;
        }

        fixture.cleanup().await.ok();
    }

    async fn create_assigned_general_compute_task(
        repo: &TaskRepository,
        label: &str,
        worker_id: &str,
    ) -> (String, GeneralComputeRequest) {
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("{label}-{unique}");
        let request = inline_general_compute_request(&unique, b"lease source");
        let mut task =
            task_for_general_compute_request(&task_id, &format!("{label}-owner"), &request);
        task.max_cpt = 0;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, worker_id, "10.0.0.1")
            .await
            .unwrap();
        (task_id, request)
    }

    fn inline_general_compute_request(unique: &str, bytes: &[u8]) -> GeneralComputeRequest {
        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, bytes),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        request
    }

    fn chunked_general_compute_request(unique: &str, bytes: &[u8]) -> GeneralComputeRequest {
        let mut request = inline_general_compute_request(unique, bytes);
        request.source_artifact.mime_type = "text/plain".into();
        request.source_artifact.chunks = vec![general_compute_runtime::ArtifactChunk {
            offset: 0,
            size_bytes: bytes.len() as u64,
            sha256: general_compute_runtime::sha256_digest(bytes),
        }];
        request.source_artifact.inline_bytes = None;
        request.request_digest = request.canonical_request_digest();
        request
    }

    fn task_for_general_compute_request(
        task_id: &str,
        owner: &str,
        request: &GeneralComputeRequest,
    ) -> Task {
        let mut task = make_task(task_id, owner);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(request).unwrap());
        task
    }

    async fn cleanup_task_case(
        pool: &PgPool,
        task_id: &str,
        username: &str,
        worker_id: Option<&str>,
    ) {
        sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
            .bind(task_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM task_attestations WHERE task_id = $1")
            .bind(task_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .execute(pool)
            .await
            .ok();
        if let Some(worker_id) = worker_id {
            sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
                .bind(worker_id)
                .execute(pool)
                .await
                .ok();
            sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
                .bind(worker_id)
                .execute(pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(username)
            .execute(pool)
            .await
            .ok();
    }

    fn make_task(task_id: &str, owner: &str) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            task_id: task_id.into(),
            owner: owner.into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: Some("test task".into()),
            output: None,
            result_torrent: None,
            torrent_source: Some("example-btih".into()),
            runtime: None,
            task_source: None,
            general_compute_manifest_json: None,
            managed_dsl_backend_id: None,
            managed_dsl_semantics_manifest_sha256: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 100,
            req_gpu_score: 0,
            req_memory_gb: 8,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
            retry_count: 0,
            max_retries: 3,
            deadline: None,
            deterministic: false,
            side_effects: false,
            priority: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            download_bytes: 0,
            cache_hits: 0,
            created_at: Utc::now(),
            last_update: Utc::now(),
            completed_at: None,
        }
    }
}

use anyhow::Result;
use chrono::{DateTime, Utc};
use general_compute_runtime::{
    managed_gpu::{
        ManagedGpuCapability, ManagedGpuEvidence, ManagedGpuRequest, ManagedGpuResult,
        ManagedGpuStatus, ManagedGpuUsage, MANAGED_GPU_BILLING_VERSION,
        MANAGED_GPU_COST_MODEL_VERSION, MANAGED_GPU_RUNTIME_VERSION, MANAGED_GPU_SETTLEMENT_BASIS,
    },
    GeneralComputeRequest, GeneralComputeResult, ResultStatus, TrustedWorkerCapabilityRegistration,
};
use hivemind_models::{
    Task, TaskStatus, WorkerNode, PRIVATE_STATIC_ADMISSION_MODE, PUBLIC_DYNAMIC_ADMISSION_MODE,
};
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

/// Immutable Nodepool-owned capability binding for one managed GPU attempt.
/// The registration and selected device are captured together at assignment;
/// mutable Worker registration changes cannot rewrite this attempt's trust
/// boundary or strand its terminal result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedGpuAttemptBinding {
    pub capability_snapshot_json: String,
    pub selected_gpu: ManagedGpuCapability,
}

/// Who owns a managed GPU terminal failure. Worker-owned typed results affect
/// reputation and attestations; Nodepool-owned synthetic results do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedGpuFailureAttribution {
    Worker,
    Nodepool,
}

#[derive(Debug, thiserror::Error)]
enum ManagedGpuAttemptBindingError {
    #[error("managed GPU capability binding is not UTF-8: {0}")]
    InvalidUtf8(String),
    #[error("managed GPU capability binding is malformed: {0}")]
    MalformedSnapshot(String),
    #[error("managed GPU selected device is malformed: {0}")]
    MalformedSelectedGpu(String),
    #[error("managed GPU selected device is invalid: {0}")]
    InvalidSelectedGpu(String),
}

fn managed_gpu_task_status(status: ManagedGpuStatus) -> &'static str {
    match status {
        ManagedGpuStatus::Cancelled => "CANCELLED",
        ManagedGpuStatus::TimedOut => "TIMED_OUT",
        ManagedGpuStatus::Failed
        | ManagedGpuStatus::ResourceExhausted
        | ManagedGpuStatus::BackendUnavailable => "FAILED",
        ManagedGpuStatus::Completed => unreachable!("completed status has no failure task status"),
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

/// Nodepool-owned settlement evidence for the independent managed GPU route.
/// Worker usage remains an unverified claim; the amount is the task's fixed
/// reservation and never a worker-selected variable price.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedGpuSettlement {
    worker_id: String,
    execution_id: String,
    attempt_id: String,
    idempotency_key: String,
    request_digest: String,
    attempt_generation: i64,
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
        let runtime = task
            .runtime
            .as_deref()
            .map(str::trim)
            .filter(|runtime| !runtime.is_empty());
        let general_compute_manifest = task
            .general_compute_manifest_json
            .as_deref()
            .filter(|manifest| !manifest.is_empty());
        let managed_gpu_manifest = task
            .managed_gpu_manifest_json
            .as_deref()
            .filter(|manifest| !manifest.is_empty());
        if general_compute_manifest.is_some() && managed_gpu_manifest.is_some() {
            anyhow::bail!("general-compute and managed GPU manifests cannot be combined");
        }
        if general_compute_manifest.is_some()
            && runtime != Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
        {
            anyhow::bail!(
                "general-compute request manifest requires runtime general-compute-v1alpha1"
            );
        }
        if managed_gpu_manifest.is_some() && runtime != Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!("managed GPU request manifest requires runtime managed-function-gpu-v1");
        }
        if runtime == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
            && general_compute_manifest.is_none()
        {
            anyhow::bail!("general-compute request manifest is missing");
        }
        if runtime == Some(MANAGED_GPU_RUNTIME_VERSION) {
            let manifest = managed_gpu_manifest
                .ok_or_else(|| anyhow::anyhow!("managed GPU request manifest is missing"))?;
            if manifest.len() > hivemind_proto::MANAGED_GPU_MANIFEST_MAX_BYTES {
                anyhow::bail!("managed GPU request manifest exceeds the byte limit");
            }
            let request: ManagedGpuRequest = serde_json::from_slice(manifest)
                .map_err(|error| anyhow::anyhow!("managed GPU request is malformed: {error}"))?;
            request
                .validate()
                .map_err(|error| anyhow::anyhow!("managed GPU request is invalid: {error:?}"))?;
            if u64::try_from(task.max_cpt).ok() != Some(request.reservation_cpt) {
                anyhow::bail!("managed GPU task max_cpt must equal reservation_cpt");
            }
            if task
                .task_source
                .as_deref()
                .is_some_and(|source| !source.trim().is_empty())
                || task
                    .torrent_source
                    .as_deref()
                    .is_some_and(|source| !source.trim().is_empty())
                || task
                    .expected_btih
                    .as_deref()
                    .is_some_and(|btih| !btih.trim().is_empty())
            {
                anyhow::bail!("managed GPU tasks cannot carry legacy source fields");
            }
            if task
                .managed_dsl_backend_id
                .as_deref()
                .is_some_and(|backend_id| !backend_id.trim().is_empty())
                || task
                    .managed_dsl_semantics_manifest_sha256
                    .as_deref()
                    .is_some_and(|digest| !digest.trim().is_empty())
                || task
                    .managed_receipt_json
                    .as_deref()
                    .is_some_and(|receipt| !receipt.trim().is_empty())
            {
                anyhow::bail!("managed GPU tasks cannot carry managed DSL or proof fields");
            }
        }
        if runtime != Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
            && general_compute_manifest.is_some()
        {
            anyhow::bail!("general-compute manifest is not valid for this runtime");
        }
        if task.retry_count != 0 {
            anyhow::bail!("new tasks must start with retry_count zero");
        }
        if task.max_retries < 0 {
            anyhow::bail!("max_retries must not be negative");
        }

        let mut tx = self.pool.begin().await?;
        if runtime == Some("production_sandboxed_dsl")
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
            "INSERT INTO tasks (task_id, owner, status, status_message, torrent_source, runtime, task_source, general_compute_manifest_json, managed_gpu_manifest_json, managed_dsl_backend_id, managed_dsl_semantics_manifest_sha256, expected_btih,
             req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
             host_count, max_cpt, max_retries, deadline,
             deterministic, side_effects, priority, created_at, last_update)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,NOW(),NOW()) RETURNING *",
        )
        .bind(&task.task_id).bind(&task.owner)
        .bind(task.status.as_str()).bind(&task.status_message)
        .bind(&task.torrent_source).bind(runtime).bind(&task.task_source)
        .bind(&task.general_compute_manifest_json)
        .bind(&task.managed_gpu_manifest_json)
        .bind(&task.managed_dsl_backend_id)
        .bind(&task.managed_dsl_semantics_manifest_sha256)
        .bind(&task.expected_btih)
        .bind(task.req_cpu_score).bind(task.req_gpu_score)
        .bind(task.req_memory_gb).bind(task.req_gpu_memory_gb)
        .bind(task.req_storage_gb)
        .bind(task.host_count).bind(task.max_cpt).bind(task.max_retries)
        .bind(task.deadline).bind(task.deterministic).bind(task.side_effects).bind(task.priority)
        .fetch_one(&mut *tx).await?;

        if runtime == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION) {
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

    /// Return the Nodepool-validated managed GPU result for the task's current
    /// attempt. The task row and typed result are read in one transaction so a
    /// public result read cannot combine a terminal task with a stale attempt.
    /// No settlement or other state transition occurs on this path.
    pub async fn managed_gpu_result_for_task(&self, task_id: &str) -> Result<Option<Vec<u8>>> {
        let mut tx = self.pool.begin().await?;
        let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE task_id = $1 FOR SHARE")
            .bind(task_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(task) = task else {
            tx.commit().await?;
            return Ok(None);
        };
        if task.runtime.as_deref().map(str::trim) != Some(MANAGED_GPU_RUNTIME_VERSION) {
            tx.commit().await?;
            return Ok(None);
        }
        let manifest = task
            .managed_gpu_manifest_json
            .as_deref()
            .filter(|manifest| !manifest.is_empty())
            .ok_or_else(|| anyhow::anyhow!("managed GPU task has no persisted request manifest"))?;
        if manifest.len() > hivemind_proto::MANAGED_GPU_MANIFEST_MAX_BYTES {
            anyhow::bail!("managed GPU request manifest exceeds the byte limit");
        }
        let request = serde_json::from_slice::<ManagedGpuRequest>(manifest)
            .map_err(|error| anyhow::anyhow!("managed GPU request is malformed: {error}"))?;
        request
            .validate()
            .map_err(|error| anyhow::anyhow!("managed GPU request is invalid: {error:?}"))?;
        let attempt_generation = i64::from(task.retry_count)
            .checked_add(1)
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("managed GPU attempt generation is invalid"))?;
        let Some(worker_id) = task.worker_id.as_deref().filter(|id| !id.trim().is_empty()) else {
            tx.commit().await?;
            return Ok(None);
        };
        let row: Option<(String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT worker_id, attempt_id, result_json
             FROM managed_gpu_results
             WHERE task_id = $1 AND attempt_generation = $2
             FOR SHARE",
        )
        .bind(task_id)
        .bind(attempt_generation)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((persisted_worker_id, persisted_attempt_id, result_json)) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        if persisted_worker_id != worker_id || persisted_attempt_id != request.attempt_id {
            anyhow::bail!("managed GPU result does not match the current task assignment");
        }
        if result_json.is_empty()
            || result_json.len() > hivemind_proto::MANAGED_GPU_RESULT_MAX_BYTES
        {
            anyhow::bail!("managed GPU result is outside the byte limit");
        }
        let result = serde_json::from_slice::<ManagedGpuResult>(&result_json).map_err(|error| {
            anyhow::anyhow!("persisted managed GPU result is malformed: {error}")
        })?;
        let binding =
            managed_gpu_attempt_binding_tx(&mut tx, task_id, worker_id, attempt_generation)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("managed GPU result assignment binding is missing")
                })?;
        let registration = serde_json::from_str::<TrustedWorkerCapabilityRegistration>(
            &binding.capability_snapshot_json,
        )
        .map_err(|error| {
            anyhow::anyhow!("managed GPU capability snapshot is malformed: {error}")
        })?;
        result
            .validate_against(&request, &registration)
            .map_err(|error| {
                anyhow::anyhow!("persisted managed GPU result is not trusted: {error:?}")
            })?;
        if result.selected_gpu != binding.selected_gpu {
            anyhow::bail!("persisted managed GPU result does not match the assignment device");
        }
        let reservation_cpt = i64::try_from(request.reservation_cpt)
            .map_err(|_| anyhow::anyhow!("managed GPU reservation exceeds database range"))?;
        match result.status {
            ManagedGpuStatus::Completed => {
                if task.status != TaskStatus::Completed
                    || !task.billing_settled
                    || task.billed_amount != reservation_cpt
                {
                    anyhow::bail!("completed managed GPU result is not atomically settled");
                }
            }
            ManagedGpuStatus::Failed
            | ManagedGpuStatus::Cancelled
            | ManagedGpuStatus::TimedOut
            | ManagedGpuStatus::ResourceExhausted
            | ManagedGpuStatus::BackendUnavailable => {
                if task.status == TaskStatus::Completed
                    || task.billing_settled
                    || task.billed_amount != 0
                {
                    anyhow::bail!("non-success managed GPU result has unexpected settlement state");
                }
            }
        }
        let canonical_result = serde_json::to_vec(&result)?;
        tx.commit().await?;
        Ok(Some(canonical_result))
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

    /// Load the immutable capability binding captured for one managed GPU
    /// attempt. This deliberately does not consult the Worker registration,
    /// which may have changed or disappeared since assignment.
    pub async fn managed_gpu_attempt_binding(
        &self,
        task_id: &str,
        worker_id: &str,
        attempt_generation: i64,
    ) -> Result<Option<ManagedGpuAttemptBinding>> {
        let row: Option<(Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT capability_snapshot_json, selected_gpu_json
             FROM managed_gpu_attempt_bindings
             WHERE task_id = $1 AND attempt_generation = $2 AND worker_id = $3",
        )
        .bind(task_id)
        .bind(attempt_generation)
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((snapshot_json, selected_gpu_json)) = row else {
            return Ok(None);
        };
        decode_managed_gpu_attempt_binding(snapshot_json, selected_gpu_json)
            .map(Some)
            .map_err(Into::into)
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
            "SELECT * FROM tasks
             WHERE status IN ('PENDING', 'QUEUED')
               AND retry_count >= 0
               AND max_retries >= 0
               AND retry_count <= max_retries
             ORDER BY priority DESC, created_at ASC LIMIT 100",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
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
        self.assign_to_worker_with_retry_limit(task_id, worker_id, worker_ip, i32::MAX)
            .await
    }

    /// Assign a pending task while enforcing both the task retry budget and
    /// the Dispatcher safety cap. The row is locked before any capability
    /// lookup so an exhausted task cannot be revived by a concurrent caller.
    pub async fn assign_to_worker_with_retry_limit(
        &self,
        task_id: &str,
        worker_id: &str,
        worker_ip: &str,
        retry_limit: i32,
    ) -> Result<Task> {
        if retry_limit < 0 {
            anyhow::bail!("retry limit must not be negative");
        }
        let mut tx = self.pool.begin().await?;
        let current =
            sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE task_id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?;
        if !matches!(current.status, TaskStatus::Pending | TaskStatus::Queued) {
            anyhow::bail!("task is not pending or queued");
        }
        let effective_limit = retry_limit.min(current.max_retries.max(0));
        if current.max_retries < 0
            || current.retry_count < 0
            || current.retry_count > effective_limit
        {
            let terminal = self
                .terminalize_retry_exhausted_locked(
                    &mut tx,
                    &current,
                    "Retry limit exceeded before assignment",
                )
                .await?;
            tx.commit().await?;
            anyhow::bail!(
                "task {} is retry-exhausted and was terminalized as {}",
                terminal.task_id,
                terminal.status.as_str()
            );
        }

        let managed_gpu_binding = if current.runtime.as_deref().map(str::trim)
            == Some(MANAGED_GPU_RUNTIME_VERSION)
        {
            let manifest = current
                .managed_gpu_manifest_json
                .as_deref()
                .ok_or_else(|| {
                    anyhow::anyhow!("managed GPU task is missing its request manifest")
                })?;
            let request = serde_json::from_slice::<ManagedGpuRequest>(manifest)
                .map_err(|error| anyhow::anyhow!("managed GPU request is malformed: {error}"))?;
            request
                .validate()
                .map_err(|error| anyhow::anyhow!("managed GPU request is invalid: {error:?}"))?;
            let (capability_snapshot_json, registration) =
                trusted_managed_gpu_registration_with_snapshot(&mut tx, worker_id).await?;
            let selected_gpu = registration
                .select_managed_gpu_for_request(&request)
                .map_err(|error| {
                    anyhow::anyhow!("managed GPU selection is unavailable: {error:?}")
                })?;
            Some((capability_snapshot_json, selected_gpu))
        } else {
            None
        };

        let assigned = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET worker_id = $1, worker_ip = $2, status = 'ASSIGNED', last_update = NOW()
             WHERE task_id = $3
               AND status IN ('PENDING', 'QUEUED')
               AND retry_count >= 0
               AND retry_count <= max_retries
             RETURNING *",
        )
        .bind(worker_id)
        .bind(worker_ip)
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
        if let Some((capability_snapshot_json, selected_gpu)) = managed_gpu_binding {
            let attempt_generation = i64::from(assigned.retry_count)
                .checked_add(1)
                .filter(|generation| *generation > 0)
                .ok_or_else(|| anyhow::anyhow!("managed GPU attempt generation is invalid"))?;
            let selected_gpu_json = serde_json::to_vec(&selected_gpu)?;
            sqlx::query(
                "INSERT INTO managed_gpu_attempt_bindings (
                    task_id, attempt_generation, worker_id,
                    capability_snapshot_json, selected_gpu_json
                 ) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(task_id)
            .bind(attempt_generation)
            .bind(worker_id)
            .bind(capability_snapshot_json.as_bytes())
            .bind(selected_gpu_json)
            .execute(&mut *tx)
            .await?;
        }
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
        let current_generation: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(generation), 0)
             FROM general_compute_transfer_leases
             WHERE task_id = $1",
        )
        .bind(&task.task_id)
        .fetch_one(&mut **tx)
        .await?;
        let generation = current_generation
            .checked_add(1)
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("general-compute transfer generation exhausted"))?;
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

    pub async fn mark_worker_execution_running_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
    ) -> Result<Option<Task>> {
        let mut tx = self.pool.begin().await?;
        if lock_worker_attempt_snapshot(&mut tx, expected, worker_id)
            .await?
            .is_none()
        {
            tx.commit().await?;
            return Ok(None);
        }
        let updated = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'RUNNING', last_update = NOW()
             WHERE id = $1 AND task_id = $2 AND worker_id = $3
               AND status IN ('ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(expected.id)
        .bind(&expected.task_id)
        .bind(worker_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(updated)
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

    pub async fn refresh_worker_endpoint_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        worker_ip: &str,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        if lock_worker_attempt_snapshot(&mut tx, expected, worker_id)
            .await?
            .is_none()
        {
            tx.commit().await?;
            return Ok(false);
        }
        let updated = sqlx::query(
            "UPDATE tasks
             SET worker_ip = $1, last_update = NOW()
             WHERE id = $2 AND task_id = $3 AND worker_id = $4
               AND status IN ('ASSIGNED', 'RUNNING')",
        )
        .bind(worker_ip)
        .bind(expected.id)
        .bind(&expected.task_id)
        .bind(worker_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn claim_pending_for_worker(
        &self,
        worker_id: &str,
        worker_ip: &str,
        limit: i64,
    ) -> Result<Vec<Task>> {
        self.terminalize_exhausted_pending().await?;
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
                  AND retry_count >= 0
                  AND retry_count <= max_retries
                  -- PullBatch is the legacy completion surface. Modern
                  -- runtimes must be assigned by the capability-aware
                  -- dispatcher so they cannot bypass typed admission,
                  -- proof verification, or settlement gates.
                  AND (runtime IS NULL OR BTRIM(runtime) = '')
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
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before completion"))
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
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before completion"))
    }

    pub async fn complete_for_worker_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        result_torrent: Option<&str>,
        output: Option<&str>,
    ) -> Result<Option<Task>> {
        self.complete_guarded(
            &expected.task_id,
            Some(worker_id),
            result_torrent,
            output,
            None,
            None,
            None,
            ManagedCompletionEvidence::Untrusted,
            Some(expected),
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
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before completion"))
    }

    pub async fn complete_for_worker_legacy_managed_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        output: Option<&str>,
    ) -> Result<Option<Task>> {
        self.complete_guarded(
            &expected.task_id,
            Some(worker_id),
            None,
            output,
            None,
            None,
            None,
            ManagedCompletionEvidence::LegacyFallback,
            Some(expected),
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
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before completion"))
    }

    pub async fn complete_for_worker_observed_verified_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        output: Option<&str>,
    ) -> Result<Option<Task>> {
        self.complete_guarded(
            &expected.task_id,
            Some(worker_id),
            None,
            output,
            None,
            None,
            None,
            ManagedCompletionEvidence::ObservedVerified,
            Some(expected),
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
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before completion"))
    }

    pub async fn complete_general_compute_for_worker_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        output: Option<&str>,
    ) -> Result<Option<Task>> {
        self.complete_guarded(
            &expected.task_id,
            Some(worker_id),
            None,
            output,
            None,
            Some(expected_manifest),
            Some(result_json),
            ManagedCompletionEvidence::Untrusted,
            Some(expected),
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
        self.fail_general_compute_for_worker_inner(
            task_id,
            worker_id,
            expected_manifest,
            result_json,
            reason,
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before failure recording"))
    }

    pub async fn fail_general_compute_for_worker_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        reason: &str,
    ) -> Result<Option<Task>> {
        self.fail_general_compute_for_worker_inner(
            &expected.task_id,
            worker_id,
            expected_manifest,
            result_json,
            reason,
            Some(expected),
        )
        .await
    }

    async fn fail_general_compute_for_worker_inner(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        reason: &str,
        expected_snapshot: Option<&Task>,
    ) -> Result<Option<Task>> {
        let result = serde_json::from_slice::<GeneralComputeResult>(result_json)
            .map_err(|error| anyhow::anyhow!("general-compute result is malformed: {error}"))?;
        if result.status == ResultStatus::Completed {
            anyhow::bail!("completed general-compute result cannot use the failure path");
        }

        let mut tx = self.pool.begin().await?;
        if let Some(expected) = expected_snapshot {
            if lock_worker_attempt_snapshot(&mut tx, expected, worker_id)
                .await?
                .is_none()
            {
                tx.commit().await?;
                return Ok(None);
            }
        }
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
        Ok(Some(failed))
    }

    /// Complete one managed-function-gpu-v1 attempt using only the canonical
    /// request manifest stored on the task and the private operator-owned
    /// capability snapshot. Billing and the typed result transition share one
    /// database transaction so a successful GPU result cannot become an
    /// unsettled COMPLETED task.
    pub async fn complete_managed_gpu_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
    ) -> Result<Task> {
        self.complete_managed_gpu_for_worker_inner(
            task_id,
            worker_id,
            expected_manifest,
            result_json,
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before managed GPU completion"))
    }

    pub async fn complete_managed_gpu_for_worker_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
    ) -> Result<Option<Task>> {
        self.complete_managed_gpu_for_worker_inner(
            &expected.task_id,
            worker_id,
            expected_manifest,
            result_json,
            Some(expected),
        )
        .await
    }

    async fn complete_managed_gpu_for_worker_inner(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        expected_snapshot: Option<&Task>,
    ) -> Result<Option<Task>> {
        if expected_manifest.is_empty()
            || expected_manifest.len() > hivemind_proto::MANAGED_GPU_MANIFEST_MAX_BYTES
        {
            anyhow::bail!("managed GPU request manifest is outside the byte limit");
        }
        if result_json.is_empty()
            || result_json.len() > hivemind_proto::MANAGED_GPU_RESULT_MAX_BYTES
        {
            anyhow::bail!("managed GPU result is outside the byte limit");
        }
        let request = serde_json::from_slice::<ManagedGpuRequest>(expected_manifest)
            .map_err(|error| anyhow::anyhow!("managed GPU request is malformed: {error}"))?;
        request
            .validate()
            .map_err(|error| anyhow::anyhow!("managed GPU request is invalid: {error:?}"))?;
        let result = serde_json::from_slice::<ManagedGpuResult>(result_json)
            .map_err(|error| anyhow::anyhow!("managed GPU result is malformed: {error}"))?;
        if result.status != ManagedGpuStatus::Completed {
            anyhow::bail!("only a completed managed GPU result can use the success path");
        }
        let reservation_cpt = i64::try_from(request.reservation_cpt)
            .map_err(|_| anyhow::anyhow!("managed GPU reservation exceeds database range"))?;

        let mut tx = self.pool.begin().await?;
        let current = if let Some(expected) = expected_snapshot {
            let Some(current) = lock_worker_attempt_snapshot(&mut tx, expected, worker_id).await?
            else {
                tx.commit().await?;
                return Ok(None);
            };
            current
        } else {
            sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE task_id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?
        };
        if current.runtime.as_deref().map(str::trim) != Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!("task is not a managed-function-gpu-v1 task");
        }
        if current.managed_gpu_manifest_json.as_deref() != Some(expected_manifest) {
            anyhow::bail!("managed GPU result does not match the persisted request manifest");
        }
        if current.max_cpt != reservation_cpt {
            anyhow::bail!("managed GPU task reservation does not match its request manifest");
        }
        let current_attempt_generation = i64::from(current.retry_count)
            .checked_add(1)
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("managed GPU attempt generation is invalid"))?;
        let canonical_result_json = serde_json::to_vec(&result)?;
        let existing_result: Option<(String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT worker_id, attempt_id, result_json
             FROM managed_gpu_results
             WHERE task_id = $1 AND attempt_generation = $2
             FOR UPDATE",
        )
        .bind(task_id)
        .bind(current_attempt_generation)
        .fetch_optional(&mut *tx)
        .await?;
        let exact_result = existing_result.as_ref().is_some_and(
            |(persisted_worker, _persisted_attempt, persisted_result)| {
                persisted_worker == worker_id
                    && canonical_managed_gpu_result_bytes_equal(
                        persisted_result,
                        &canonical_result_json,
                    )
            },
        );
        if !matches!(current.status.as_str(), "ASSIGNED" | "RUNNING") {
            if current.status.as_str() == "COMPLETED"
                && exact_result
                && current.billing_settled
                && current.billed_amount == reservation_cpt
            {
                let settled: bool = sqlx::query_scalar(
                    "SELECT EXISTS(
                         SELECT 1
                         FROM managed_gpu_settlements
                         WHERE task_id = $1
                           AND worker_id = $2
                           AND execution_id = $3
                           AND attempt_id = $4
                           AND idempotency_key = $5
                           AND request_digest = $6
                           AND attempt_generation = $7
                           AND billing_version = $8
                           AND cost_model_version = $9
                           AND settlement_basis = $10
                           AND amount_cpt = $11
                     )",
                )
                .bind(task_id)
                .bind(worker_id)
                .bind(&request.execution_id)
                .bind(&request.attempt_id)
                .bind(&request.idempotency_key)
                .bind(&request.request_digest)
                .bind(current_attempt_generation)
                .bind(MANAGED_GPU_BILLING_VERSION)
                .bind(MANAGED_GPU_COST_MODEL_VERSION)
                .bind(MANAGED_GPU_SETTLEMENT_BASIS)
                .bind(reservation_cpt)
                .fetch_one(&mut *tx)
                .await?;
                if settled {
                    tx.commit().await?;
                    return Ok(Some(current));
                }
            }
            anyhow::bail!("managed GPU task is no longer active for completion");
        }
        if current.worker_id.as_deref() != Some(worker_id) {
            anyhow::bail!("managed GPU completion does not match the current Worker assignment");
        }
        if current.billing_settled || current.billed_amount != 0 {
            anyhow::bail!("managed GPU task has unexpected prior billing state");
        }
        if existing_result.is_some() {
            anyhow::bail!("managed GPU task already has a conflicting typed result");
        }

        let binding =
            managed_gpu_attempt_binding_tx(&mut tx, task_id, worker_id, current_attempt_generation)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("managed GPU assignment capability binding is missing")
                })?;
        let registration = serde_json::from_str::<TrustedWorkerCapabilityRegistration>(
            &binding.capability_snapshot_json,
        )
        .map_err(|error| anyhow::anyhow!("trusted capability snapshot is malformed: {error}"))?;
        result
            .validate_against(&request, &registration)
            .map_err(|error| anyhow::anyhow!("managed GPU result is not trusted: {error:?}"))?;
        if result.selected_gpu != binding.selected_gpu {
            anyhow::bail!("managed GPU result does not match the assignment device");
        }
        let settlement =
            managed_gpu_settlement(&request, &result, worker_id, current_attempt_generation)?;
        let output_bytes = i64::try_from(result.output.len())
            .map_err(|_| anyhow::anyhow!("managed GPU output exceeds database range"))?;
        let wall_time_ms = i64::try_from(result.usage.wall_time_ms)
            .map_err(|_| anyhow::anyhow!("managed GPU wall time exceeds database range"))?;

        sqlx::query(
            "INSERT INTO managed_gpu_results (
                task_id, attempt_id, attempt_generation, worker_id, result_json
             ) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(task_id)
        .bind(&request.attempt_id)
        .bind(current_attempt_generation)
        .bind(worker_id)
        .bind(result_json)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO managed_gpu_settlements (
                task_id, worker_id, execution_id, attempt_id, idempotency_key,
                request_digest, attempt_generation, billing_version, cost_model_version,
                usage_claim_json, evidence_level, settlement_basis, amount_cpt
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(task_id)
        .bind(&settlement.worker_id)
        .bind(&settlement.execution_id)
        .bind(&settlement.attempt_id)
        .bind(&settlement.idempotency_key)
        .bind(&settlement.request_digest)
        .bind(settlement.attempt_generation)
        .bind(&settlement.billing_version)
        .bind(&settlement.cost_model_version)
        .bind(&settlement.usage_claim_json)
        .bind(&settlement.evidence_level)
        .bind(&settlement.basis)
        .bind(settlement.amount_cpt)
        .execute(&mut *tx)
        .await?;

        let provider_user: String =
            sqlx::query_scalar("SELECT username FROM worker_nodes WHERE worker_id = $1")
                .bind(worker_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| anyhow::anyhow!("managed GPU provider account is missing"))?;
        if provider_user.trim().is_empty() {
            anyhow::bail!("managed GPU provider account is invalid");
        }
        let platform_fee_cpt = reservation_cpt
            .checked_mul(PLATFORM_FEE_BPS)
            .ok_or_else(|| anyhow::anyhow!("managed GPU platform fee overflowed"))?
            / 10_000;
        let provider_credit_cpt = reservation_cpt
            .checked_sub(platform_fee_cpt)
            .ok_or_else(|| anyhow::anyhow!("managed GPU provider credit underflowed"))?;
        let provider_balance_limit = i64::MAX
            .checked_sub(provider_credit_cpt)
            .ok_or_else(|| anyhow::anyhow!("managed GPU provider credit exceeds database range"))?;

        // Lock payer and provider accounts in a stable order. This prevents
        // opposite-direction settlements from deadlocking while they debit
        // one account and credit the other.
        let mut account_users = vec![current.owner.clone(), provider_user.clone()];
        account_users.sort_unstable();
        account_users.dedup();
        for username in account_users {
            let locked: Option<String> =
                sqlx::query_scalar("SELECT username FROM users WHERE username = $1 FOR UPDATE")
                    .bind(&username)
                    .fetch_optional(&mut *tx)
                    .await?;
            if locked.is_none() {
                anyhow::bail!("managed GPU provider account is missing: {username}");
            }
        }

        let charged = sqlx::query(
            "UPDATE users
             SET balance = balance - $1, updated_at = NOW()
             WHERE username = $2 AND balance >= $1",
        )
        .bind(reservation_cpt)
        .bind(&current.owner)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !charged {
            anyhow::bail!("payer has insufficient balance for managed GPU settlement");
        }

        let credited = sqlx::query(
            "UPDATE users
             SET balance = balance + $1, updated_at = NOW()
             WHERE username = $2 AND is_active = true AND balance <= $3",
        )
        .bind(provider_credit_cpt)
        .bind(&provider_user)
        .bind(provider_balance_limit)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !credited {
            anyhow::bail!("managed GPU provider account balance exceeds database range");
        }

        insert_ledger_entry(
            &mut tx,
            task_id,
            &current.owner,
            Some(worker_id),
            Some(&provider_user),
            "payer_debit",
            reservation_cpt,
        )
        .await?;
        insert_ledger_entry(
            &mut tx,
            task_id,
            &current.owner,
            Some(worker_id),
            Some(&provider_user),
            "provider_credit",
            provider_credit_cpt,
        )
        .await?;
        insert_ledger_entry(
            &mut tx,
            task_id,
            &current.owner,
            Some(worker_id),
            Some(&provider_user),
            "platform_fee",
            platform_fee_cpt,
        )
        .await?;

        let settled = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'COMPLETED',
                 status_message = 'managed GPU execution settled',
                 output = $1,
                 result_torrent = NULL,
                 billing_settled = true,
                 billed_amount = $2,
                 managed_output_bytes = $3,
                 wall_time_ms = $4,
                 last_update = NOW(),
                 completed_at = NOW()
             WHERE task_id = $5
               AND worker_id = $6
               AND status IN ('ASSIGNED', 'RUNNING')
               AND managed_gpu_manifest_json = $7
               AND billing_settled = false
             RETURNING *",
        )
        .bind(&result.output)
        .bind(reservation_cpt)
        .bind(output_bytes)
        .bind(wall_time_ms)
        .bind(task_id)
        .bind(worker_id)
        .bind(expected_manifest)
        .fetch_one(&mut *tx)
        .await?;
        increment_worker_success(&mut tx, worker_id).await?;
        insert_task_attestation(
            &mut tx,
            task_id,
            worker_id,
            "accepted",
            100,
            "managed GPU result settled",
        )
        .await?;
        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;
        tx.commit().await?;
        Ok(Some(settled))
    }

    /// Construct a Nodepool-owned non-success result when a Worker never
    /// returned a typed envelope. The selected device is taken from the
    /// private capability snapshot so the normal typed failure path can still
    /// enforce the same request, attempt, and capability bindings.
    pub async fn fail_managed_gpu_without_worker_result(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        status: ManagedGpuStatus,
        error_code: &str,
        reason: &str,
    ) -> Result<Task> {
        self.fail_managed_gpu_without_worker_result_inner(
            task_id,
            worker_id,
            expected_manifest,
            status,
            error_code,
            reason,
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before managed GPU failure recording"))
    }

    pub async fn fail_managed_gpu_without_worker_result_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        expected_manifest: &[u8],
        status: ManagedGpuStatus,
        error_code: &str,
        reason: &str,
    ) -> Result<Option<Task>> {
        self.fail_managed_gpu_without_worker_result_inner(
            &expected.task_id,
            worker_id,
            expected_manifest,
            status,
            error_code,
            reason,
            Some(expected),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn fail_managed_gpu_without_worker_result_inner(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        status: ManagedGpuStatus,
        error_code: &str,
        reason: &str,
        expected_snapshot: Option<&Task>,
    ) -> Result<Option<Task>> {
        if status == ManagedGpuStatus::Completed {
            anyhow::bail!("managed GPU scheduler failure cannot use completed status");
        }
        if error_code.trim().is_empty() {
            anyhow::bail!("managed GPU scheduler failure code must not be empty");
        }
        let request = match serde_json::from_slice::<ManagedGpuRequest>(expected_manifest) {
            Ok(request) => request,
            Err(error) => {
                tracing::error!(
                    task_id,
                    worker_id,
                    error = %error,
                    "managed GPU request manifest is malformed; quarantining without a typed result"
                );
                return match expected_snapshot {
                    Some(expected) => {
                        self.quarantine_managed_gpu_without_typed_result_snapshot(
                            expected,
                            worker_id,
                            Some(expected_manifest),
                            managed_gpu_task_status(status),
                            "managed GPU request manifest is malformed",
                        )
                        .await
                    }
                    None => self
                        .quarantine_managed_gpu_without_typed_result(
                            task_id,
                            worker_id,
                            Some(expected_manifest),
                            managed_gpu_task_status(status),
                            "managed GPU request manifest is malformed",
                        )
                        .await
                        .map(Some),
                };
            }
        };
        if let Err(error) = request.validate() {
            tracing::error!(
                task_id,
                worker_id,
                error = ?error,
                "managed GPU request manifest is invalid; quarantining without a typed result"
            );
            return match expected_snapshot {
                Some(expected) => {
                    self.quarantine_managed_gpu_without_typed_result_snapshot(
                        expected,
                        worker_id,
                        Some(expected_manifest),
                        managed_gpu_task_status(status),
                        "managed GPU request manifest is invalid",
                    )
                    .await
                }
                None => self
                    .quarantine_managed_gpu_without_typed_result(
                        task_id,
                        worker_id,
                        Some(expected_manifest),
                        managed_gpu_task_status(status),
                        "managed GPU request manifest is invalid",
                    )
                    .await
                    .map(Some),
            };
        }
        let current = if let Some(expected) = expected_snapshot {
            expected.clone()
        } else {
            self.find_by_task_id(task_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("managed GPU task does not exist"))?
        };
        let attempt_generation = i64::from(current.retry_count)
            .checked_add(1)
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("managed GPU attempt generation is invalid"))?;
        let binding = match self
            .managed_gpu_attempt_binding(task_id, worker_id, attempt_generation)
            .await
        {
            Ok(Some(binding)) => binding,
            Ok(None) => {
                tracing::error!(
                    task_id,
                    worker_id,
                    attempt_generation,
                    "managed GPU assignment has no immutable capability binding; quarantining without a typed result"
                );
                return match expected_snapshot {
                    Some(expected) => {
                        self.quarantine_managed_gpu_without_typed_result_snapshot(
                            expected,
                            worker_id,
                            Some(expected_manifest),
                            managed_gpu_task_status(status),
                            reason,
                        )
                        .await
                    }
                    None => self
                        .quarantine_managed_gpu_without_typed_result(
                            task_id,
                            worker_id,
                            Some(expected_manifest),
                            managed_gpu_task_status(status),
                            reason,
                        )
                        .await
                        .map(Some),
                };
            }
            Err(error) if is_managed_gpu_binding_integrity_error(&error) => {
                tracing::error!(
                    task_id,
                    worker_id,
                    attempt_generation,
                    error = %error,
                    "managed GPU assignment capability binding is corrupt; quarantining without a typed result"
                );
                return match expected_snapshot {
                    Some(expected) => {
                        self.quarantine_managed_gpu_without_typed_result_snapshot(
                            expected,
                            worker_id,
                            Some(expected_manifest),
                            managed_gpu_task_status(status),
                            reason,
                        )
                        .await
                    }
                    None => self
                        .quarantine_managed_gpu_without_typed_result(
                            task_id,
                            worker_id,
                            Some(expected_manifest),
                            managed_gpu_task_status(status),
                            reason,
                        )
                        .await
                        .map(Some),
                };
            }
            Err(error) => return Err(error),
        };
        let result = ManagedGpuResult {
            protocol_version:
                general_compute_runtime::managed_gpu::MANAGED_GPU_RESULT_PROTOCOL_VERSION.into(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            runtime_version: request.runtime_version.clone(),
            semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
            operation_registry_version: request.operation_registry_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            source_sha256: request.source_sha256(),
            input_sha256: request.input_sha256(),
            reservation_cpt: request.reservation_cpt,
            status,
            exit_code: (status == ManagedGpuStatus::Failed).then_some(1),
            error_code: Some(error_code.to_owned()),
            output: String::new(),
            output_sha256: general_compute_runtime::sha256_digest(b""),
            selected_gpu: binding.selected_gpu,
            usage: ManagedGpuUsage {
                source_bytes: request.source.len() as u64,
                input_bytes: request.input_json.len() as u64,
                ..ManagedGpuUsage::default()
            },
            evidence: ManagedGpuEvidence::default(),
        };
        let result_json = serde_json::to_vec(&result)?;
        self.fail_managed_gpu_for_worker_with_attribution(
            task_id,
            worker_id,
            expected_manifest,
            &result_json,
            reason,
            ManagedGpuFailureAttribution::Nodepool,
            expected_snapshot,
        )
        .await
    }

    /// Terminalize a legacy or corrupted managed GPU assignment when the
    /// immutable binding needed for a typed result is unavailable. This is a
    /// Nodepool-owned quarantine: it never charges, rewards, penalizes, or
    /// fabricates a GPU identity, and it prevents the task from remaining
    /// active indefinitely.
    pub async fn quarantine_managed_gpu_without_typed_result(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: Option<&[u8]>,
        terminal_status: &str,
        reason: &str,
    ) -> Result<Task> {
        self.quarantine_managed_gpu_without_typed_result_inner(
            task_id,
            worker_id,
            expected_manifest,
            terminal_status,
            reason,
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before managed GPU quarantine"))
    }

    pub async fn quarantine_managed_gpu_without_typed_result_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        expected_manifest: Option<&[u8]>,
        terminal_status: &str,
        reason: &str,
    ) -> Result<Option<Task>> {
        self.quarantine_managed_gpu_without_typed_result_inner(
            &expected.task_id,
            worker_id,
            expected_manifest,
            terminal_status,
            reason,
            Some(expected),
        )
        .await
    }

    async fn quarantine_managed_gpu_without_typed_result_inner(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: Option<&[u8]>,
        terminal_status: &str,
        reason: &str,
        expected_snapshot: Option<&Task>,
    ) -> Result<Option<Task>> {
        if !matches!(terminal_status, "FAILED" | "CANCELLED" | "TIMED_OUT") {
            anyhow::bail!("managed GPU quarantine status is invalid");
        }
        let status_message = if reason.trim().is_empty() {
            "managed GPU attempt quarantined because its immutable assignment binding is unavailable"
        } else {
            reason
        };
        let mut tx = self.pool.begin().await?;
        let current = if let Some(expected) = expected_snapshot {
            let Some(current) = lock_worker_attempt_snapshot(&mut tx, expected, worker_id).await?
            else {
                tx.commit().await?;
                return Ok(None);
            };
            current
        } else {
            sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE task_id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?
        };
        if current.runtime.as_deref().map(str::trim) != Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!("task is not a managed-function-gpu-v1 task");
        }
        if let Some(expected_manifest) = expected_manifest {
            if current.managed_gpu_manifest_json.as_deref() != Some(expected_manifest) {
                anyhow::bail!("managed GPU task manifest changed before quarantine");
            }
        }
        if current.worker_id.as_deref() != Some(worker_id) {
            anyhow::bail!("managed GPU quarantine does not match the current Worker assignment");
        }
        if !matches!(current.status.as_str(), "ASSIGNED" | "RUNNING") {
            if current.status.as_str() == terminal_status
                && !current.billing_settled
                && current.billed_amount == 0
            {
                tx.commit().await?;
                return Ok(Some(current));
            }
            anyhow::bail!("managed GPU task is no longer active for quarantine");
        }
        let terminal = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = $1,
                 status_message = $2,
                 output = NULL,
                 result_torrent = NULL,
                 billing_settled = false,
                 billed_amount = 0,
                 managed_output_bytes = 0,
                 wall_time_ms = 0,
                 managed_receipt_json = NULL,
                 last_update = NOW(),
                 completed_at = NOW()
             WHERE task_id = $3
               AND worker_id = $4
               AND status IN ('ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(terminal_status)
        .bind(status_message)
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&mut *tx)
        .await?;
        let proof_state = match terminal_status {
            "CANCELLED" => "cancelled",
            "TIMED_OUT" => "expired",
            "FAILED" => "failed",
            _ => unreachable!("quarantine status was validated above"),
        };
        update_active_managed_proof_state(
            &mut tx,
            task_id,
            managed_proof_attempt_id(&terminal).as_deref(),
            proof_state,
        )
        .await?;
        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;
        tx.commit().await?;
        Ok(Some(terminal))
    }

    /// Persist a validated managed-function-gpu-v1 failure with Worker-owned
    /// evidence and the corresponding reputation/attestation updates.
    pub async fn fail_managed_gpu_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        reason: &str,
    ) -> Result<Task> {
        self.fail_managed_gpu_for_worker_with_attribution(
            task_id,
            worker_id,
            expected_manifest,
            result_json,
            reason,
            ManagedGpuFailureAttribution::Worker,
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before managed GPU failure recording"))
    }

    /// Persist a validated managed-function-gpu-v1 failure for the exact
    /// attempt snapshot observed by the dispatcher. A stale same-Worker
    /// response returns `Ok(None)` before any result, attestation, or lease
    /// mutation is made.
    pub async fn fail_managed_gpu_for_worker_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        reason: &str,
    ) -> Result<Option<Task>> {
        self.fail_managed_gpu_for_worker_with_attribution(
            &expected.task_id,
            worker_id,
            expected_manifest,
            result_json,
            reason,
            ManagedGpuFailureAttribution::Worker,
            Some(expected),
        )
        .await
    }

    /// Persist a validated managed-function-gpu-v1 failure without charging
    /// either account. The typed result remains the only Worker result accepted
    /// for this route, including cancellation and backend-unavailable states.
    #[allow(clippy::too_many_arguments)]
    async fn fail_managed_gpu_for_worker_with_attribution(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_manifest: &[u8],
        result_json: &[u8],
        reason: &str,
        attribution: ManagedGpuFailureAttribution,
        expected_snapshot: Option<&Task>,
    ) -> Result<Option<Task>> {
        if expected_manifest.is_empty()
            || expected_manifest.len() > hivemind_proto::MANAGED_GPU_MANIFEST_MAX_BYTES
        {
            anyhow::bail!("managed GPU request manifest is outside the byte limit");
        }
        if result_json.is_empty()
            || result_json.len() > hivemind_proto::MANAGED_GPU_RESULT_MAX_BYTES
        {
            anyhow::bail!("managed GPU result is outside the byte limit");
        }
        let request = serde_json::from_slice::<ManagedGpuRequest>(expected_manifest)
            .map_err(|error| anyhow::anyhow!("managed GPU request is malformed: {error}"))?;
        request
            .validate()
            .map_err(|error| anyhow::anyhow!("managed GPU request is invalid: {error:?}"))?;
        let result = serde_json::from_slice::<ManagedGpuResult>(result_json)
            .map_err(|error| anyhow::anyhow!("managed GPU result is malformed: {error}"))?;
        if result.status == ManagedGpuStatus::Completed {
            anyhow::bail!("completed managed GPU result cannot use the failure path");
        }
        let terminal_task_status = managed_gpu_task_status(result.status);
        let reservation_cpt = i64::try_from(request.reservation_cpt)
            .map_err(|_| anyhow::anyhow!("managed GPU reservation exceeds database range"))?;

        let mut tx = self.pool.begin().await?;
        let current = if let Some(expected) = expected_snapshot {
            let Some(current) = lock_worker_attempt_snapshot(&mut tx, expected, worker_id).await?
            else {
                tx.commit().await?;
                return Ok(None);
            };
            current
        } else {
            sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE task_id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?
        };
        if current.runtime.as_deref().map(str::trim) != Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!("task is not a managed-function-gpu-v1 task");
        }
        if current.managed_gpu_manifest_json.as_deref() != Some(expected_manifest) {
            anyhow::bail!("managed GPU result does not match the persisted request manifest");
        }
        if current.max_cpt != reservation_cpt {
            anyhow::bail!("managed GPU task reservation does not match its request manifest");
        }
        let current_attempt_generation = i64::from(current.retry_count)
            .checked_add(1)
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("managed GPU attempt generation is invalid"))?;
        let canonical_result_json = serde_json::to_vec(&result)?;
        let existing_result: Option<(String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT worker_id, attempt_id, result_json
             FROM managed_gpu_results
             WHERE task_id = $1 AND attempt_generation = $2
             FOR UPDATE",
        )
        .bind(task_id)
        .bind(current_attempt_generation)
        .fetch_optional(&mut *tx)
        .await?;
        let exact_result = existing_result.as_ref().is_some_and(
            |(persisted_worker, _persisted_attempt, persisted_result)| {
                persisted_worker == worker_id
                    && canonical_managed_gpu_result_bytes_equal(
                        persisted_result,
                        &canonical_result_json,
                    )
            },
        );
        if !matches!(current.status.as_str(), "ASSIGNED" | "RUNNING") {
            if current.status.as_str() == terminal_task_status
                && exact_result
                && !current.billing_settled
                && current.billed_amount == 0
            {
                let settled: bool = sqlx::query_scalar(
                    "SELECT EXISTS(
                         SELECT 1 FROM managed_gpu_settlements WHERE task_id = $1
                     )",
                )
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?;
                if !settled {
                    tx.commit().await?;
                    return Ok(Some(current));
                }
            }
            anyhow::bail!("managed GPU task is no longer active for failure recording");
        }
        if current.worker_id.as_deref() != Some(worker_id) {
            anyhow::bail!("managed GPU failure does not match the current Worker assignment");
        }
        if current.billing_settled || current.billed_amount != 0 {
            anyhow::bail!("managed GPU task has unexpected prior billing state");
        }
        if existing_result.is_some() {
            anyhow::bail!("managed GPU task already has a conflicting typed result");
        }

        let binding =
            managed_gpu_attempt_binding_tx(&mut tx, task_id, worker_id, current_attempt_generation)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("managed GPU assignment capability binding is missing")
                })?;
        let registration = serde_json::from_str::<TrustedWorkerCapabilityRegistration>(
            &binding.capability_snapshot_json,
        )
        .map_err(|error| anyhow::anyhow!("trusted capability snapshot is malformed: {error}"))?;
        result
            .validate_against(&request, &registration)
            .map_err(|error| anyhow::anyhow!("managed GPU result is not trusted: {error:?}"))?;
        if result.selected_gpu != binding.selected_gpu {
            anyhow::bail!("managed GPU result does not match the assignment device");
        }
        let output_bytes = i64::try_from(result.output.len())
            .map_err(|_| anyhow::anyhow!("managed GPU output exceeds database range"))?;
        let wall_time_ms = i64::try_from(result.usage.wall_time_ms)
            .map_err(|_| anyhow::anyhow!("managed GPU wall time exceeds database range"))?;
        let failure_reason = if reason.trim().is_empty() {
            result
                .error_code
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("managed GPU execution failed")
        } else {
            reason
        };

        sqlx::query(
            "INSERT INTO managed_gpu_results (
                task_id, attempt_id, attempt_generation, worker_id, result_json
             ) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(task_id)
        .bind(&request.attempt_id)
        .bind(current_attempt_generation)
        .bind(worker_id)
        .bind(result_json)
        .execute(&mut *tx)
        .await?;
        let failed = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = $1,
                 status_message = $2,
                 output = NULL,
                 result_torrent = NULL,
                 billing_settled = false,
                 billed_amount = 0,
                 managed_output_bytes = $3,
                 wall_time_ms = $4,
                 last_update = NOW(),
                 completed_at = NOW()
             WHERE task_id = $5
               AND worker_id = $6
               AND status IN ('ASSIGNED', 'RUNNING')
               AND managed_gpu_manifest_json = $7
               AND billing_settled = false
             RETURNING *",
        )
        .bind(terminal_task_status)
        .bind(failure_reason)
        .bind(output_bytes)
        .bind(wall_time_ms)
        .bind(task_id)
        .bind(worker_id)
        .bind(expected_manifest)
        .fetch_one(&mut *tx)
        .await?;
        if attribution == ManagedGpuFailureAttribution::Worker {
            increment_worker_failure_tx(&mut tx, worker_id).await?;
            insert_task_attestation(&mut tx, task_id, worker_id, "rejected", 100, failure_reason)
                .await?;
        }
        self.revoke_general_compute_transfer_lease(&mut tx, task_id)
            .await?;
        tx.commit().await?;
        Ok(Some(failed))
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
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("task changed before completion"))
    }

    pub async fn complete_for_worker_with_managed_receipt_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        output: Option<&str>,
        executed_ops: i64,
        output_bytes: i64,
        receipt_json: &str,
    ) -> Result<Option<Task>> {
        self.complete_guarded(
            &expected.task_id,
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
            Some(expected),
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

    /// Complete a legacy task only when the Worker echoes the retry generation
    /// that was returned with its lease. The initial generation is zero, so an
    /// old client remains compatible with first-attempt tasks while delayed
    /// reports from an earlier retry fail closed.
    pub async fn complete_result_for_worker_attempt(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
        result_torrent: &str,
    ) -> Result<Option<Task>> {
        let Some(expected) = self
            .find_worker_attempt_snapshot(task_id, worker_id, retry_count)
            .await?
        else {
            return Ok(None);
        };
        self.complete_for_worker_snapshot(&expected, worker_id, Some(result_torrent), None)
            .await
    }

    pub async fn record_output_for_worker_attempt(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
        output: &str,
    ) -> Result<Option<Task>> {
        let Some(expected) = self
            .find_output_worker_attempt_snapshot(task_id, worker_id, retry_count)
            .await?
        else {
            return Ok(None);
        };
        if expected.runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!(
                "managed GPU tasks accept output only through the validated typed result path"
            );
        }
        let mut tx = self.pool.begin().await?;
        if lock_output_worker_attempt_snapshot(&mut tx, &expected, worker_id)
            .await?
            .is_none()
        {
            tx.commit().await?;
            return Ok(None);
        }
        let updated = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET output = $1, last_update = NOW()
             WHERE id = $2 AND task_id = $3 AND worker_id = $4
               AND retry_count = $5
               AND (
                   status IN ('ASSIGNED', 'RUNNING')
                   OR (status = 'COMPLETED' AND output IS NULL)
               )
             RETURNING *",
        )
        .bind(output)
        .bind(expected.id)
        .bind(&expected.task_id)
        .bind(worker_id)
        .bind(retry_count)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(updated)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_resource_usage_for_worker_attempt(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
        cpu: f64,
        memory: f64,
        gpu: f64,
        gpu_mem: f64,
    ) -> Result<bool> {
        let Some(expected) = self
            .find_usage_worker_attempt_snapshot(task_id, worker_id, retry_count)
            .await?
        else {
            return Ok(false);
        };
        if expected.runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!("managed GPU usage requires the validated typed result path");
        }
        let mut tx = self.pool.begin().await?;
        if lock_usage_worker_attempt_snapshot(&mut tx, &expected, worker_id)
            .await?
            .is_none()
        {
            tx.commit().await?;
            return Ok(false);
        }
        let updated = sqlx::query(
            "UPDATE tasks
             SET cpu_usage = $1, memory_usage = $2, gpu_usage = $3, gpu_memory_usage = $4,
                 last_update = NOW()
             WHERE id = $5 AND task_id = $6 AND worker_id = $7
               AND retry_count = $8
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
        .bind(expected.id)
        .bind(&expected.task_id)
        .bind(worker_id)
        .bind(retry_count)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn record_batch_report_for_worker_attempt(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
        report: BatchTaskReport<'_>,
    ) -> Result<Option<Task>> {
        let Some(expected) = self
            .find_terminal_worker_attempt_snapshot(task_id, worker_id, retry_count)
            .await?
        else {
            return Ok(None);
        };
        if expected.runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!(
                "managed GPU tasks accept output only through the validated typed result path"
            );
        }
        let mut tx = self.pool.begin().await?;
        if lock_terminal_worker_attempt_snapshot(&mut tx, &expected, worker_id)
            .await?
            .is_none()
        {
            tx.commit().await?;
            return Ok(None);
        }
        let updated = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET output = COALESCE($1, output),
                 cpu_time_ms = $2,
                 wall_time_ms = $3,
                 peak_memory_mb = $4,
                 download_bytes = $5,
                 cache_hits = $6,
                 last_update = NOW()
             WHERE id = $7 AND task_id = $8 AND worker_id = $9
               AND retry_count = $10
               AND status IN ('COMPLETED', 'FAILED')
             RETURNING *",
        )
        .bind(report.output)
        .bind(report.cpu_time_ms)
        .bind(report.wall_time_ms)
        .bind(report.peak_memory_mb)
        .bind(report.download_bytes)
        .bind(report.cache_hits)
        .bind(expected.id)
        .bind(&expected.task_id)
        .bind(worker_id)
        .bind(retry_count)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn record_output_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        output: &str,
    ) -> Result<Task> {
        let runtime: Option<String> =
            sqlx::query_scalar("SELECT runtime FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .fetch_one(&self.pool)
                .await?;
        if runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!(
                "managed GPU tasks accept output only through the validated typed result path"
            );
        }
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
        expected_snapshot: Option<&Task>,
    ) -> Result<Option<Task>> {
        let mut tx = self.pool.begin().await?;
        let snapshot = if let Some(expected) = expected_snapshot {
            let Some(worker_id) = worker_id else {
                tx.commit().await?;
                return Ok(None);
            };
            lock_worker_attempt_snapshot(&mut tx, expected, worker_id).await?
        } else {
            None
        };
        if expected_snapshot.is_some() && snapshot.is_none() {
            tx.commit().await?;
            return Ok(None);
        }
        let runtime: Option<String> = if let Some(snapshot) = snapshot.as_ref() {
            snapshot.runtime.clone()
        } else {
            sqlx::query_scalar("SELECT runtime FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?
        };
        let managed_runtime = matches!(
            runtime.as_deref(),
            Some("managed-function-v0") | Some("production_sandboxed_dsl")
        );
        let general_compute_runtime =
            runtime.as_deref() == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION);
        if runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!("managed GPU tasks require the dedicated typed settlement path");
        }
        if general_compute_runtime && general_compute_result.is_none() {
            anyhow::bail!(
                "general-compute task completion requires a validated typed result envelope"
            );
        }
        if managed_runtime && managed_evidence == ManagedCompletionEvidence::Untrusted {
            anyhow::bail!(
                "managed task completion requires a Nodepool-verified proof or an explicit rollout compatibility path"
            );
        }
        let deterministic = if let Some(snapshot) = snapshot.as_ref() {
            snapshot.deterministic
        } else {
            sqlx::query_scalar("SELECT deterministic FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?
        };
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
            return Ok(Some(completed));
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
            Ok(Some(settled))
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
            Ok(Some(completed))
        }
    }

    pub async fn fail(&self, task_id: &str, reason: &str) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let runtime: Option<String> =
            sqlx::query_scalar("SELECT runtime FROM tasks WHERE task_id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?;
        if runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!("managed GPU tasks require a typed failure result or owner cancellation");
        }
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
        self.fail_for_worker_inner(task_id, worker_id, reason, true, None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task changed before failure recording"))
    }

    pub async fn fail_for_worker_attempt(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
        reason: &str,
    ) -> Result<Option<Task>> {
        let Some(expected) = self
            .find_worker_attempt_snapshot(task_id, worker_id, retry_count)
            .await?
        else {
            return Ok(None);
        };
        self.fail_for_worker_snapshot(&expected, worker_id, reason)
            .await
    }

    pub async fn fail_for_worker_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        reason: &str,
    ) -> Result<Option<Task>> {
        self.fail_for_worker_inner(&expected.task_id, worker_id, reason, true, Some(expected))
            .await
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
        self.fail_for_worker_inner(task_id, worker_id, reason, false, None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task changed before failure recording"))
    }

    pub async fn fail_for_worker_without_penalty_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        reason: &str,
    ) -> Result<Option<Task>> {
        self.fail_for_worker_inner(&expected.task_id, worker_id, reason, false, Some(expected))
            .await
    }

    async fn fail_for_worker_inner(
        &self,
        task_id: &str,
        worker_id: &str,
        reason: &str,
        penalize_worker: bool,
        expected_snapshot: Option<&Task>,
    ) -> Result<Option<Task>> {
        let mut tx = self.pool.begin().await?;
        let snapshot = if let Some(expected) = expected_snapshot {
            lock_worker_attempt_snapshot(&mut tx, expected, worker_id).await?
        } else {
            None
        };
        if expected_snapshot.is_some() && snapshot.is_none() {
            tx.commit().await?;
            return Ok(None);
        }
        let runtime: Option<String> = if let Some(snapshot) = snapshot.as_ref() {
            snapshot.runtime.clone()
        } else {
            sqlx::query_scalar("SELECT runtime FROM tasks WHERE task_id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?
        };
        if runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!("managed GPU tasks require a typed failure result or owner cancellation");
        }
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
        if penalize_worker {
            increment_worker_failure(&self.pool, worker_id).await?;
            insert_task_attestation_pool(&self.pool, task_id, worker_id, "rejected", 100, reason)
                .await?;
        }
        Ok(Some(failed))
    }

    pub async fn cancel(&self, task_id: &str) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let mut cancelled = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'CANCELLED', last_update = NOW(), completed_at = NOW()
             WHERE task_id = $1 AND status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;

        let gpu_cancellation = cancelled.runtime.as_deref().map(str::trim)
            == Some(MANAGED_GPU_RUNTIME_VERSION)
            && cancelled.worker_id.is_some();
        if gpu_cancellation {
            let worker_id = cancelled
                .worker_id
                .as_deref()
                .expect("managed GPU cancellation worker was checked above");
            let mut typed_gpu_cancellation = false;
            if let Some(manifest) = cancelled.managed_gpu_manifest_json.as_deref() {
                let request = match serde_json::from_slice::<ManagedGpuRequest>(manifest) {
                    Ok(request) => Some(request),
                    Err(error) => {
                        tracing::error!(
                            task_id,
                            error = %error,
                            "managed GPU cancellation manifest is malformed; quarantining without a typed result"
                        );
                        None
                    }
                };
                if let Some(request) = request {
                    if let Err(error) = request.validate() {
                        tracing::error!(
                            task_id,
                            error = ?error,
                            "managed GPU cancellation manifest is invalid; quarantining without a typed result"
                        );
                    } else {
                        let attempt_generation = i64::from(cancelled.retry_count)
                            .checked_add(1)
                            .filter(|generation| *generation > 0);
                        if let Some(attempt_generation) = attempt_generation {
                            let binding = match managed_gpu_attempt_binding_tx(
                                &mut tx,
                                task_id,
                                worker_id,
                                attempt_generation,
                            )
                            .await
                            {
                                Ok(binding) => binding,
                                Err(error) if is_managed_gpu_binding_integrity_error(&error) => {
                                    tracing::error!(
                                        task_id,
                                        worker_id,
                                        error = %error,
                                        "managed GPU cancellation binding is corrupt; quarantining without a typed result"
                                    );
                                    None
                                }
                                Err(error) => return Err(error),
                            };
                            if let Some(binding) = binding {
                                match serde_json::from_str::<TrustedWorkerCapabilityRegistration>(
                                    &binding.capability_snapshot_json,
                                ) {
                                    Ok(registration) => {
                                        let result = nodepool_managed_gpu_terminal_result(
                                            &request,
                                            binding.selected_gpu,
                                            ManagedGpuStatus::Cancelled,
                                            "task_cancelled",
                                        );
                                        match result.validate_against(&request, &registration) {
                                            Ok(()) => {
                                                let result_json = serde_json::to_vec(&result)?;
                                                sqlx::query(
                                                    "INSERT INTO managed_gpu_results (
                                                        task_id, attempt_id, attempt_generation, worker_id, result_json
                                                     ) VALUES ($1, $2, $3, $4, $5)",
                                                )
                                                .bind(task_id)
                                                .bind(&request.attempt_id)
                                                .bind(attempt_generation)
                                                .bind(worker_id)
                                                .bind(result_json)
                                                .execute(&mut *tx)
                                                .await?;
                                                typed_gpu_cancellation = true;
                                            }
                                            Err(error) => tracing::error!(
                                                task_id,
                                                error = ?error,
                                                "managed GPU cancellation result failed trust validation; quarantining without a typed result"
                                            ),
                                        }
                                    }
                                    Err(error) => tracing::error!(
                                        task_id,
                                        error = %error,
                                        "managed GPU cancellation snapshot is malformed; quarantining without a typed result"
                                    ),
                                }
                            }
                        } else {
                            tracing::error!(
                                task_id,
                                "managed GPU cancellation attempt generation is invalid; quarantining without a typed result"
                            );
                        }
                    }
                }
            } else {
                tracing::error!(
                    task_id,
                    "managed GPU cancellation request manifest is missing; quarantining without a typed result"
                );
            }
            cancelled = sqlx::query_as::<_, Task>(
                "UPDATE tasks
                 SET status_message = $1,
                     output = NULL,
                     result_torrent = NULL,
                     billing_settled = false,
                     billed_amount = 0,
                     managed_output_bytes = 0,
                     wall_time_ms = 0,
                     managed_receipt_json = NULL
                 WHERE task_id = $2
                 RETURNING *",
            )
            .bind(if typed_gpu_cancellation {
                "task cancelled by owner"
            } else {
                "managed GPU task cancelled without a trusted typed result"
            })
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
        }

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

    pub async fn mark_stale_managed_gpu_running(&self) -> Result<u64> {
        let stale = sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
             WHERE status = 'RUNNING'
               AND runtime = $1
               AND worker_id IS NOT NULL
               AND last_update < NOW() - INTERVAL '120 seconds'
             ORDER BY priority DESC, created_at ASC",
        )
        .bind(MANAGED_GPU_RUNTIME_VERSION)
        .fetch_all(&self.pool)
        .await?;
        let mut timed_out = 0u64;
        for task in stale {
            let Some(worker_id) = task.worker_id.as_deref() else {
                continue;
            };
            let Some(manifest) = task.managed_gpu_manifest_json.as_deref() else {
                match self
                    .quarantine_managed_gpu_without_typed_result(
                        &task.task_id,
                        worker_id,
                        None,
                        "TIMED_OUT",
                        "Worker heartbeat lost; managed GPU request manifest is unavailable",
                    )
                    .await
                {
                    Ok(_) => timed_out += 1,
                    Err(error) => tracing::warn!(
                        task_id = %task.task_id,
                        worker_id,
                        error = %error,
                        "could not quarantine stale managed GPU task without its request manifest"
                    ),
                }
                continue;
            };
            match self
                .fail_managed_gpu_without_worker_result(
                    &task.task_id,
                    worker_id,
                    manifest,
                    ManagedGpuStatus::TimedOut,
                    "worker_heartbeat_lost",
                    "Worker heartbeat lost",
                )
                .await
            {
                Ok(_) => timed_out += 1,
                Err(error) => tracing::warn!(
                    task_id = %task.task_id,
                    worker_id,
                    error = %error,
                    "could not persist a typed managed GPU timeout; leaving task running"
                ),
            }
        }
        Ok(timed_out)
    }

    pub async fn mark_stale_running(&self) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let timed_out = sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = 'TIMED_OUT', status_message = 'Worker heartbeat lost', completed_at = NOW()
             WHERE status = 'RUNNING'
               AND runtime IS DISTINCT FROM $1
               AND last_update < NOW() - INTERVAL '120 seconds'
             RETURNING *",
        )
        .bind(MANAGED_GPU_RUNTIME_VERSION)
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

    /// Terminalize pending rows that are already beyond their persisted retry
    /// budget. This prevents a malformed or legacy row from remaining pending
    /// forever after assignment queries correctly refuse it.
    pub async fn terminalize_exhausted_pending(&self) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let exhausted = sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
             WHERE status IN ('PENDING', 'QUEUED')
               AND (retry_count < 0 OR max_retries < 0 OR retry_count > max_retries)
             FOR UPDATE SKIP LOCKED",
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut terminalized = 0u64;
        for task in &exhausted {
            self.terminalize_retry_exhausted_locked(
                &mut tx,
                task,
                "Retry limit exceeded before assignment",
            )
            .await?;
            terminalized += 1;
        }
        tx.commit().await?;
        Ok(terminalized)
    }

    /// An ASSIGNED task without a Worker identity cannot be safely retried or
    /// attributed. Terminalize it without Worker penalty and revoke all task
    /// transfer/proof state in the same transaction.
    pub async fn terminalize_stale_assignment_without_worker(
        &self,
        expected: &Task,
        reason: &str,
    ) -> Result<Option<Task>> {
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
             WHERE id = $1
               AND task_id = $2
               AND worker_id IS NULL
               AND status = 'ASSIGNED'
             FOR UPDATE",
        )
        .bind(expected.id)
        .bind(&expected.task_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(current) = current else {
            tx.commit().await?;
            return Ok(None);
        };
        let terminal = self
            .terminalize_retry_exhausted_locked(&mut tx, &current, reason)
            .await?;
        tx.commit().await?;
        Ok(Some(terminal))
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

    /// Terminalize an active attempt when another retry would exceed the
    /// effective limit. This helper runs inside the caller's transaction so
    /// the state change and lease/proof cleanup are atomic.
    async fn terminalize_retry_exhausted_locked(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        current: &Task,
        reason: &str,
    ) -> Result<Task> {
        if !matches!(
            current.status,
            TaskStatus::Assigned | TaskStatus::Running | TaskStatus::Pending | TaskStatus::Queued
        ) {
            anyhow::bail!("task is no longer active for retry terminalization");
        }
        let failed = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'FAILED',
                 status_message = $1,
                 output = NULL,
                 result_torrent = NULL,
                 billing_settled = false,
                 billed_amount = 0,
                 managed_output_bytes = 0,
                 wall_time_ms = 0,
                 managed_receipt_json = NULL,
                 last_update = NOW(),
                 completed_at = NOW()
             WHERE id = $2
               AND task_id = $3
               AND status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(reason)
        .bind(current.id)
        .bind(&current.task_id)
        .fetch_one(&mut **tx)
        .await?;

        update_active_managed_proof_state(
            tx,
            &current.task_id,
            managed_proof_attempt_id(&failed).as_deref(),
            "failed",
        )
        .await?;
        self.revoke_general_compute_transfer_lease(tx, &current.task_id)
            .await?;

        if failed.runtime.as_deref().map(str::trim) != Some(MANAGED_GPU_RUNTIME_VERSION) {
            if let Some(result_json) = nodepool_general_compute_terminal_result(
                &failed,
                ResultStatus::Failed,
                "nodepool_task_failed",
                reason,
                b"general-compute-retry-limit-input-v1",
            )? {
                sqlx::query(
                    "INSERT INTO general_compute_results (task_id, worker_id, result_json)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (task_id) DO NOTHING",
                )
                .bind(&failed.task_id)
                .bind(failed.worker_id.as_deref().unwrap_or("nodepool"))
                .bind(result_json)
                .execute(&mut **tx)
                .await?;
            }
        }
        Ok(failed)
    }

    /// Atomically reset the exact attempt or terminalize it when the caller's
    /// effective retry limit has been reached. A returned task is either the
    /// new pending attempt or the terminal failure.
    pub async fn retry_to_pending_for_worker_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
        retry_limit: i32,
        reason: &str,
    ) -> Result<Option<Task>> {
        let mut tx = self.pool.begin().await?;
        let current = lock_worker_attempt_snapshot(&mut tx, expected, worker_id).await?;
        let Some(current) = current else {
            tx.commit().await?;
            return Ok(None);
        };
        let updated = self
            .reset_to_pending_locked(
                &mut tx,
                &expected.task_id,
                &current,
                Some(retry_limit),
                reason,
            )
            .await?;
        tx.commit().await?;
        Ok(Some(updated))
    }

    /// Atomically reset the exact attempt observed by the dispatcher, using
    /// the attempt generation encoded in the expected manifest.
    pub async fn retry_to_pending_for_worker_attempt(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_retry_count: i32,
        expected_manifest: &[u8],
        retry_limit: i32,
        reason: &str,
    ) -> Result<Option<Task>> {
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
             WHERE task_id = $1
               AND worker_id = $2
               AND status IN ('ASSIGNED', 'RUNNING')
               AND retry_count = $3
               AND managed_gpu_manifest_json = $4
             FOR UPDATE",
        )
        .bind(task_id)
        .bind(worker_id)
        .bind(expected_retry_count)
        .bind(expected_manifest)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(current) = current else {
            tx.commit().await?;
            return Ok(None);
        };
        let updated = self
            .reset_to_pending_locked(&mut tx, task_id, &current, Some(retry_limit), reason)
            .await?;
        tx.commit().await?;
        Ok(Some(updated))
    }

    /// the snapshot observed by the dispatcher. A late response from an older
    /// attempt must never redispatch a newer attempt assigned to the same
    /// Worker, even when both attempts use the same Worker.
    pub async fn reset_to_pending_for_worker_snapshot(
        &self,
        expected: &Task,
        worker_id: &str,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
             WHERE id = $1
               AND task_id = $2
               AND worker_id = $3
               AND status IN ('ASSIGNED', 'RUNNING')
               AND retry_count = $4
               AND runtime IS NOT DISTINCT FROM $5
               AND general_compute_manifest_json IS NOT DISTINCT FROM $6
               AND managed_gpu_manifest_json IS NOT DISTINCT FROM $7
             FOR UPDATE",
        )
        .bind(expected.id)
        .bind(&expected.task_id)
        .bind(worker_id)
        .bind(expected.retry_count)
        .bind(expected.runtime.as_deref())
        .bind(expected.general_compute_manifest_json.as_deref())
        .bind(expected.managed_gpu_manifest_json.as_deref())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(current) = current else {
            tx.commit().await?;
            return Ok(false);
        };
        self.reset_to_pending_locked(
            &mut tx,
            &expected.task_id,
            &current,
            None,
            "Retry limit exceeded",
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Reset a GPU attempt only if the task still contains the exact attempt
    /// manifest observed by the dispatcher. A late response from an older
    /// attempt must never redispatch a newer attempt assigned to the same
    /// Worker.
    pub async fn reset_to_pending_for_worker_attempt(
        &self,
        task_id: &str,
        worker_id: &str,
        expected_retry_count: i32,
        expected_manifest: &[u8],
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
             WHERE task_id = $1
               AND worker_id = $2
               AND status IN ('ASSIGNED', 'RUNNING')
               AND retry_count = $3
               AND managed_gpu_manifest_json = $4
             FOR UPDATE",
        )
        .bind(task_id)
        .bind(worker_id)
        .bind(expected_retry_count)
        .bind(expected_manifest)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(current) = current else {
            tx.commit().await?;
            return Ok(false);
        };
        self.reset_to_pending_locked(&mut tx, task_id, &current, None, "Retry limit exceeded")
            .await?;
        tx.commit().await?;
        Ok(true)
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

        let updated = self
            .reset_to_pending_locked(&mut tx, task_id, &current, None, "Retry limit exceeded")
            .await?;
        tx.commit().await?;
        Ok(updated)
    }

    async fn reset_to_pending_locked(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        task_id: &str,
        current: &Task,
        retry_limit: Option<i32>,
        reason: &str,
    ) -> Result<Task> {
        if !matches!(current.status, TaskStatus::Assigned | TaskStatus::Running) {
            anyhow::bail!("task is not active and cannot be reset");
        }
        let effective_limit = retry_limit
            .unwrap_or(i32::MAX)
            .max(0)
            .min(current.max_retries.max(0));
        if current.retry_count < 0 || current.retry_count >= effective_limit {
            return self
                .terminalize_retry_exhausted_locked(tx, current, reason)
                .await;
        }
        let next_retry_count = current
            .retry_count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("task retry count exhausted"))?;
        let rotated_general_compute_manifest = rotate_general_compute_attempt(current)?;
        let rotated_managed_gpu_manifest = rotate_managed_gpu_attempt(current)?;
        self.revoke_general_compute_transfer_lease(tx, task_id)
            .await?;
        update_active_managed_proof_state(
            tx,
            task_id,
            managed_proof_attempt_id(current).as_deref(),
            "revoked",
        )
        .await?;
        let updated = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'PENDING', status_message = 'Redispatched', worker_id = NULL, worker_ip = NULL,
                 general_compute_manifest_json = $1,
                 managed_gpu_manifest_json = $2,
                 retry_count = $3, last_update = NOW()
             WHERE id = $4
               AND task_id = $5
               AND status IN ('ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(rotated_general_compute_manifest)
        .bind(rotated_managed_gpu_manifest)
        .bind(next_retry_count)
        .bind(current.id)
        .bind(task_id)
        .fetch_one(&mut **tx)
        .await?;
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
        let runtime: Option<String> =
            sqlx::query_scalar("SELECT runtime FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .fetch_one(&self.pool)
                .await?;
        if runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
            anyhow::bail!(
                "managed GPU tasks accept output only through the validated typed result path"
            );
        }
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

impl TaskRepository {
    pub(crate) async fn find_worker_attempt_snapshot(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
    ) -> Result<Option<Task>> {
        if retry_count < 0 {
            return Ok(None);
        }
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
         WHERE task_id = $1 AND worker_id = $2 AND retry_count = $3
           AND status IN ('ASSIGNED', 'RUNNING')",
        )
        .bind(task_id)
        .bind(worker_id)
        .bind(retry_count)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn find_output_worker_attempt_snapshot(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
    ) -> Result<Option<Task>> {
        if retry_count < 0 {
            return Ok(None);
        }
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
             WHERE task_id = $1 AND worker_id = $2 AND retry_count = $3
               AND (
                   status IN ('ASSIGNED', 'RUNNING')
                   OR (status = 'COMPLETED' AND output IS NULL)
               )",
        )
        .bind(task_id)
        .bind(worker_id)
        .bind(retry_count)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn find_usage_worker_attempt_snapshot(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
    ) -> Result<Option<Task>> {
        if retry_count < 0 {
            return Ok(None);
        }
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
             WHERE task_id = $1 AND worker_id = $2 AND retry_count = $3
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
        .bind(task_id)
        .bind(worker_id)
        .bind(retry_count)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn find_terminal_worker_attempt_snapshot(
        &self,
        task_id: &str,
        worker_id: &str,
        retry_count: i32,
    ) -> Result<Option<Task>> {
        if retry_count < 0 {
            return Ok(None);
        }
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks
         WHERE task_id = $1 AND worker_id = $2 AND retry_count = $3
           AND status IN ('COMPLETED', 'FAILED')",
        )
        .bind(task_id)
        .bind(worker_id)
        .bind(retry_count)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }
}

async fn lock_terminal_worker_attempt_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    expected: &Task,
    worker_id: &str,
) -> Result<Option<Task>> {
    if expected.worker_id.as_deref() != Some(worker_id)
        || !matches!(expected.status, TaskStatus::Completed | TaskStatus::Failed)
    {
        return Ok(None);
    }
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks
         WHERE id = $1
           AND task_id = $2
           AND worker_id = $3
           AND status IN ('COMPLETED', 'FAILED')
           AND retry_count = $4
           AND runtime IS NOT DISTINCT FROM $5
           AND general_compute_manifest_json IS NOT DISTINCT FROM $6
           AND managed_gpu_manifest_json IS NOT DISTINCT FROM $7
         FOR UPDATE",
    )
    .bind(expected.id)
    .bind(&expected.task_id)
    .bind(worker_id)
    .bind(expected.retry_count)
    .bind(expected.runtime.as_deref())
    .bind(expected.general_compute_manifest_json.as_deref())
    .bind(expected.managed_gpu_manifest_json.as_deref())
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn lock_worker_attempt_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    expected: &Task,
    worker_id: &str,
) -> Result<Option<Task>> {
    if expected.worker_id.as_deref() != Some(worker_id)
        || !matches!(expected.status, TaskStatus::Assigned | TaskStatus::Running)
    {
        return Ok(None);
    }
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks
         WHERE id = $1
           AND task_id = $2
           AND worker_id = $3
           AND status IN ('ASSIGNED', 'RUNNING')
           AND retry_count = $4
           AND runtime IS NOT DISTINCT FROM $5
           AND general_compute_manifest_json IS NOT DISTINCT FROM $6
           AND managed_gpu_manifest_json IS NOT DISTINCT FROM $7
         FOR UPDATE",
    )
    .bind(expected.id)
    .bind(&expected.task_id)
    .bind(worker_id)
    .bind(expected.retry_count)
    .bind(expected.runtime.as_deref())
    .bind(expected.general_compute_manifest_json.as_deref())
    .bind(expected.managed_gpu_manifest_json.as_deref())
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn lock_output_worker_attempt_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    expected: &Task,
    worker_id: &str,
) -> Result<Option<Task>> {
    if expected.worker_id.as_deref() != Some(worker_id)
        || !(matches!(expected.status, TaskStatus::Assigned | TaskStatus::Running)
            || (expected.status == TaskStatus::Completed && expected.output.is_none()))
    {
        return Ok(None);
    }
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks
         WHERE id = $1
           AND task_id = $2
           AND worker_id = $3
           AND retry_count = $4
           AND runtime IS NOT DISTINCT FROM $5
           AND general_compute_manifest_json IS NOT DISTINCT FROM $6
           AND managed_gpu_manifest_json IS NOT DISTINCT FROM $7
           AND (
               status IN ('ASSIGNED', 'RUNNING')
               OR (status = 'COMPLETED' AND output IS NULL)
           )
         FOR UPDATE",
    )
    .bind(expected.id)
    .bind(&expected.task_id)
    .bind(worker_id)
    .bind(expected.retry_count)
    .bind(expected.runtime.as_deref())
    .bind(expected.general_compute_manifest_json.as_deref())
    .bind(expected.managed_gpu_manifest_json.as_deref())
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn lock_usage_worker_attempt_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    expected: &Task,
    worker_id: &str,
) -> Result<Option<Task>> {
    if expected.worker_id.as_deref() != Some(worker_id)
        || !(matches!(expected.status, TaskStatus::Assigned | TaskStatus::Running)
            || (expected.status == TaskStatus::Completed
                && expected.cpu_usage == 0.0
                && expected.memory_usage == 0.0
                && expected.gpu_usage == 0.0
                && expected.gpu_memory_usage == 0.0))
    {
        return Ok(None);
    }
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks
         WHERE id = $1
           AND task_id = $2
           AND worker_id = $3
           AND retry_count = $4
           AND runtime IS NOT DISTINCT FROM $5
           AND general_compute_manifest_json IS NOT DISTINCT FROM $6
           AND managed_gpu_manifest_json IS NOT DISTINCT FROM $7
           AND (
               status IN ('ASSIGNED', 'RUNNING')
               OR (
                   status = 'COMPLETED'
                   AND cpu_usage = 0
                   AND memory_usage = 0
                   AND gpu_usage = 0
                   AND gpu_memory_usage = 0
               )
           )
         FOR UPDATE",
    )
    .bind(expected.id)
    .bind(&expected.task_id)
    .bind(worker_id)
    .bind(expected.retry_count)
    .bind(expected.runtime.as_deref())
    .bind(expected.general_compute_manifest_json.as_deref())
    .bind(expected.managed_gpu_manifest_json.as_deref())
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
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
        "UPDATE managed_proof_authorizations AS proof_auth
         SET state = $1, updated_at = NOW()
         FROM tasks AS task
         WHERE proof_auth.task_id = $2
           AND task.task_id = proof_auth.task_id
           AND proof_auth.attempt_id = $3
           AND proof_auth.state IN ('issued', 'submitted', 'running')
           AND (
               (
                   proof_auth.runtime = 'general-compute-v1alpha1'
                   AND EXISTS (
                       SELECT 1
                       FROM general_compute_transfer_leases lease
                       WHERE lease.task_id = proof_auth.task_id
                         AND lease.attempt_id = proof_auth.attempt_id
                         AND lease.generation = proof_auth.lease_generation
                         AND lease.state = 'active'
                         AND (lease.expires_at IS NULL OR lease.expires_at > NOW())
                   )
               )
               OR (
                   proof_auth.runtime <> 'general-compute-v1alpha1'
                   AND proof_auth.lease_generation = task.retry_count::BIGINT + 1
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
         VALUES ($1, $2, $3, $4, $5, $6, 'CPT', 'settled', $7)",
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

async fn trusted_managed_gpu_registration_with_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    worker_id: &str,
) -> Result<(String, TrustedWorkerCapabilityRegistration)> {
    if worker_id.trim().is_empty() {
        anyhow::bail!("managed GPU settlement requires a worker identity");
    }
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT general_compute_capabilities_json
         FROM worker_nodes
         WHERE worker_id = $1 AND admission_mode = $2
         FOR SHARE",
    )
    .bind(worker_id)
    .bind(PRIVATE_STATIC_ADMISSION_MODE)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((Some(snapshot),)) = row else {
        anyhow::bail!("managed GPU requires a private trusted capability snapshot");
    };
    if snapshot.trim().is_empty() {
        anyhow::bail!("managed GPU capability snapshot is empty");
    }
    let registration = serde_json::from_str(&snapshot).map_err(|error| {
        anyhow::anyhow!("managed GPU capability snapshot is malformed: {error}")
    })?;
    Ok((snapshot, registration))
}

async fn managed_gpu_attempt_binding_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: &str,
    worker_id: &str,
    attempt_generation: i64,
) -> Result<Option<ManagedGpuAttemptBinding>> {
    let row: Option<(Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT capability_snapshot_json, selected_gpu_json
         FROM managed_gpu_attempt_bindings
         WHERE task_id = $1 AND attempt_generation = $2 AND worker_id = $3
         FOR SHARE",
    )
    .bind(task_id)
    .bind(attempt_generation)
    .bind(worker_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((snapshot_json, selected_gpu_json)) = row else {
        return Ok(None);
    };
    decode_managed_gpu_attempt_binding(snapshot_json, selected_gpu_json)
        .map(Some)
        .map_err(Into::into)
}

fn decode_managed_gpu_attempt_binding(
    snapshot_json: Vec<u8>,
    selected_gpu_json: Vec<u8>,
) -> std::result::Result<ManagedGpuAttemptBinding, ManagedGpuAttemptBindingError> {
    let capability_snapshot_json = String::from_utf8(snapshot_json)
        .map_err(|error| ManagedGpuAttemptBindingError::InvalidUtf8(error.to_string()))?;
    serde_json::from_str::<TrustedWorkerCapabilityRegistration>(&capability_snapshot_json)
        .map_err(|error| ManagedGpuAttemptBindingError::MalformedSnapshot(error.to_string()))?;
    let selected_gpu = serde_json::from_slice::<ManagedGpuCapability>(&selected_gpu_json)
        .map_err(|error| ManagedGpuAttemptBindingError::MalformedSelectedGpu(error.to_string()))?;
    selected_gpu
        .validate()
        .map_err(|error| ManagedGpuAttemptBindingError::InvalidSelectedGpu(error.to_string()))?;
    Ok(ManagedGpuAttemptBinding {
        capability_snapshot_json,
        selected_gpu,
    })
}

pub(crate) fn is_managed_gpu_binding_integrity_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ManagedGpuAttemptBindingError>()
        .is_some()
}

fn nodepool_managed_gpu_terminal_result(
    request: &ManagedGpuRequest,
    selected_gpu: ManagedGpuCapability,
    status: ManagedGpuStatus,
    error_code: &str,
) -> ManagedGpuResult {
    ManagedGpuResult {
        protocol_version: general_compute_runtime::managed_gpu::MANAGED_GPU_RESULT_PROTOCOL_VERSION
            .into(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        runtime_version: request.runtime_version.clone(),
        semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
        operation_registry_version: request.operation_registry_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        source_sha256: request.source_sha256(),
        input_sha256: request.input_sha256(),
        reservation_cpt: request.reservation_cpt,
        status,
        exit_code: (status == ManagedGpuStatus::Failed).then_some(1),
        error_code: Some(error_code.to_owned()),
        output: String::new(),
        output_sha256: general_compute_runtime::sha256_digest(b""),
        selected_gpu,
        usage: ManagedGpuUsage {
            source_bytes: request.source.len() as u64,
            input_bytes: request.input_json.len() as u64,
            ..ManagedGpuUsage::default()
        },
        evidence: ManagedGpuEvidence::default(),
    }
}

fn managed_gpu_settlement(
    request: &ManagedGpuRequest,
    result: &ManagedGpuResult,
    worker_id: &str,
    attempt_generation: i64,
) -> Result<ManagedGpuSettlement> {
    if worker_id.trim().is_empty() {
        anyhow::bail!("managed GPU settlement requires a worker identity");
    }
    if attempt_generation <= 0 {
        anyhow::bail!("managed GPU settlement attempt generation must be positive");
    }
    if result.status != ManagedGpuStatus::Completed {
        anyhow::bail!("only completed managed GPU results can settle");
    }
    if request.billing_version != MANAGED_GPU_BILLING_VERSION
        || request.cost_model_version != MANAGED_GPU_COST_MODEL_VERSION
        || request.settlement_basis != MANAGED_GPU_SETTLEMENT_BASIS
    {
        anyhow::bail!("managed GPU billing or settlement identity is not Nodepool-approved");
    }
    let amount_cpt = i64::try_from(request.reservation_cpt)
        .map_err(|_| anyhow::anyhow!("managed GPU reservation exceeds database range"))?;
    if amount_cpt <= 0 {
        anyhow::bail!("managed GPU settlement reservation must be positive");
    }
    Ok(ManagedGpuSettlement {
        worker_id: worker_id.to_owned(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        attempt_generation,
        billing_version: request.billing_version.clone(),
        cost_model_version: request.cost_model_version.clone(),
        usage_claim_json: serde_json::to_vec(&result.usage)?,
        evidence_level: "unverified".into(),
        basis: MANAGED_GPU_SETTLEMENT_BASIS.into(),
        amount_cpt,
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

async fn increment_worker_failure_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    worker_id: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO worker_reputation (worker_id, failed_tasks, score, updated_at)
         VALUES ($1, 1, 95, NOW())
         ON CONFLICT (worker_id) DO UPDATE SET
            failed_tasks = worker_reputation.failed_tasks + 1,
            score = GREATEST(0, worker_reputation.score - 5),
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

fn rotate_managed_gpu_attempt(task: &Task) -> Result<Option<Vec<u8>>> {
    if task.runtime.as_deref().map(str::trim) != Some(MANAGED_GPU_RUNTIME_VERSION) {
        return Ok(task.managed_gpu_manifest_json.clone());
    }
    let Some(manifest) = task.managed_gpu_manifest_json.as_deref() else {
        anyhow::bail!("managed GPU task is missing its request manifest");
    };
    let mut request: ManagedGpuRequest = serde_json::from_slice(manifest)
        .map_err(|_| anyhow::anyhow!("managed GPU request manifest is malformed"))?;
    request.attempt_id = uuid::Uuid::new_v4().to_string();
    request.request_digest = request.canonical_request_digest();
    request
        .validate()
        .map_err(|error| anyhow::anyhow!("rotated managed GPU request is invalid: {error:?}"))?;
    Ok(Some(serde_json::to_vec(&request)?))
}

fn canonical_managed_gpu_result_bytes_equal(persisted: &[u8], expected_canonical: &[u8]) -> bool {
    let Ok(parsed) = serde_json::from_slice::<ManagedGpuResult>(persisted) else {
        return false;
    };
    match serde_json::to_vec(&parsed) {
        Ok(canonical) => canonical == expected_canonical,
        Err(_) => false,
    }
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
        canonical_artifact_root,
        managed_gpu::{
            ManagedGpuBackendRegistration, ManagedGpuCapability, ManagedGpuEvidence,
            ManagedGpuEvidenceLevel, ManagedGpuLimits, ManagedGpuProofPolicy, ManagedGpuRequest,
            ManagedGpuRequirement, ManagedGpuResult, ManagedGpuStatus, ManagedGpuUsage,
            MANAGED_GPU_BILLING_VERSION, MANAGED_GPU_COST_MODEL_VERSION,
            MANAGED_GPU_OPERATION_REGISTRY_VERSION, MANAGED_GPU_REQUEST_PROTOCOL_VERSION,
            MANAGED_GPU_RESULT_PROTOCOL_VERSION, MANAGED_GPU_RUNTIME_VERSION,
            MANAGED_GPU_SEMANTICS_MANIFEST_SHA256, MANAGED_GPU_SETTLEMENT_BASIS,
        },
        ArtifactManifest, ArtifactRole, DeterminismPolicy, EvidenceEnvelope, ExecutionPolicy,
        GeneralComputeRequest, GeneralComputeResult, ResultStatus,
        TrustedWorkerCapabilityRegistration, UsageClaim, WorkerCapabilities,
        GENERAL_COMPUTE_RUNTIME_VERSION,
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
    async fn managed_gpu_stale_running_persists_typed_timeout_without_settlement() {
        let (p, fixture) = match pool("task_repository_managed_gpu_stale_timeout").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let reputation_before = seed_managed_gpu_reputation(&repo.pool, &case.worker_id).await;
        sqlx::query(
            "UPDATE tasks
             SET status = 'RUNNING', last_update = NOW() - INTERVAL '121 seconds'
             WHERE task_id = $1",
        )
        .bind(&case.task_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        assert_eq!(repo.mark_stale_managed_gpu_running().await.unwrap(), 1);
        let timed_out = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(timed_out.status, TaskStatus::TimedOut);
        assert_eq!(
            timed_out.status_message.as_deref(),
            Some("Worker heartbeat lost")
        );
        let persisted: Vec<u8> =
            sqlx::query_scalar("SELECT result_json FROM managed_gpu_results WHERE task_id = $1")
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        let result: ManagedGpuResult = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(result.status, ManagedGpuStatus::TimedOut);
        assert_eq!(result.error_code.as_deref(), Some("worker_heartbeat_lost"));
        assert_eq!(result.execution_id, case.request.execution_id);
        assert_eq!(result.attempt_id, case.request.attempt_id);
        assert_eq!(result.idempotency_key, case.request.idempotency_key);
        assert_eq!(result.request_digest, case.request.request_digest);
        assert!(result.output.is_empty());
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "Nodepool-owned timeout must not mutate Worker reputation"
        );
        assert_eq!(repo.mark_stale_managed_gpu_running().await.unwrap(), 0);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn refresh_worker_endpoint_updates_managed_gpu_liveness_timestamp() {
        let (p, fixture) = match pool("task_repository_managed_gpu_heartbeat_refresh").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        sqlx::query(
            "UPDATE tasks
             SET status = 'RUNNING', last_update = NOW() - INTERVAL '121 seconds'
             WHERE task_id = $1",
        )
        .bind(&case.task_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        repo.refresh_worker_endpoint(&case.task_id, &case.worker_id, "10.0.0.99")
            .await
            .unwrap();
        let refreshed = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(refreshed.worker_ip.as_deref(), Some("10.0.0.99"));
        assert!(
            refreshed.last_update > Utc::now() - chrono::Duration::seconds(5),
            "worker heartbeat must refresh the stale-running timestamp"
        );
        assert_eq!(repo.mark_stale_managed_gpu_running().await.unwrap(), 0);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
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
            managed_gpu_manifest_json: None,
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
    async fn managed_gpu_assignment_binds_exact_device_across_registration_changes() {
        let (p, fixture) = match pool("task_repository_managed_gpu_binding").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let original_registration = managed_gpu_registration(&case.request, &case.capability);
        let binding = repo
            .managed_gpu_attempt_binding(&case.task_id, &case.worker_id, 1)
            .await
            .unwrap()
            .expect("assignment must persist an immutable GPU binding");
        assert_eq!(binding.selected_gpu, case.capability);
        assert_eq!(
            binding.capability_snapshot_json,
            serde_json::to_string(&original_registration).unwrap()
        );

        let changed_capability = ManagedGpuCapability::new(
            "cuda-test-mutated-0",
            case.request.gpu_requirement.compute_capability.clone(),
            case.request.gpu_requirement.runtime_version.clone(),
            case.request.gpu_requirement.driver_abi.clone(),
            16 * 1024 * 1024 * 1024,
            32,
            case.request.guest_image_digest.clone(),
            1,
            "GPU-fedcba9876543210",
        )
        .unwrap();
        let changed_registration = managed_gpu_registration(&case.request, &changed_capability);
        sqlx::query(
            "UPDATE worker_nodes
             SET general_compute_capabilities_json = $1
             WHERE worker_id = $2",
        )
        .bind(serde_json::to_string(&changed_registration).unwrap())
        .bind(&case.worker_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        let unchanged = repo
            .managed_gpu_attempt_binding(&case.task_id, &case.worker_id, 1)
            .await
            .unwrap()
            .expect("the immutable GPU binding must survive registration updates");
        assert_eq!(unchanged, binding);

        let result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[4,6]]",
        );
        let completed = repo
            .complete_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &serde_json::to_vec(&result).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
        assert!(completed.billing_settled);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_success_settles_once_and_replays_exactly() {
        let (p, fixture) = match pool("task_repository_managed_gpu_success").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[4,6]]",
        );
        let result_json = serde_json::to_vec(&result).unwrap();

        let completed = repo
            .complete_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &result_json,
            )
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
        assert!(completed.billing_settled);
        assert_eq!(completed.billed_amount, case.request.reservation_cpt as i64);
        assert_eq!(completed.output.as_deref(), Some("[[4,6]]"));
        assert!(completed.result_torrent.is_none());
        assert_eq!(completed.managed_output_bytes, result.output.len() as i64);
        assert_eq!(completed.wall_time_ms, 1);

        assert_eq!(user_balance(&repo.pool, &case.owner).await, 75);
        assert_eq!(user_balance(&repo.pool, &case.provider).await, 23);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            1
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 3);

        let settlement: (
            String,
            String,
            String,
            String,
            i64,
            String,
            String,
            String,
            String,
            i64,
        ) = sqlx::query_as(
            "SELECT worker_id, execution_id, attempt_id, idempotency_key,
                        attempt_generation, billing_version, cost_model_version,
                        settlement_basis, evidence_level, amount_cpt
                 FROM managed_gpu_settlements WHERE task_id = $1",
        )
        .bind(&case.task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(settlement.0, case.worker_id);
        assert_eq!(settlement.1, case.request.execution_id);
        assert_eq!(settlement.2, case.request.attempt_id);
        assert_eq!(settlement.3, case.request.idempotency_key);
        assert_eq!(settlement.4, 1);
        assert_eq!(settlement.5, MANAGED_GPU_BILLING_VERSION);
        assert_eq!(settlement.6, MANAGED_GPU_COST_MODEL_VERSION);
        assert_eq!(settlement.7, MANAGED_GPU_SETTLEMENT_BASIS);
        assert_eq!(settlement.8, "unverified");
        assert_eq!(settlement.9, 25);

        let usage_json: Vec<u8> = sqlx::query_scalar(
            "SELECT usage_claim_json FROM managed_gpu_settlements WHERE task_id = $1",
        )
        .bind(&case.task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        let usage: ManagedGpuUsage = serde_json::from_slice(&usage_json).unwrap();
        assert_eq!(usage.executed_operations, 1);
        assert_eq!(usage.operation_cost_units, 10);
        assert_eq!(usage.gpu_time_ms, 1);

        let reputation: (i64, i64, i32) = sqlx::query_as(
            "SELECT successful_tasks, failed_tasks, score
             FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&case.worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(reputation, (1, 0, 101));
        let accepted_attestations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations
             WHERE task_id = $1 AND worker_id = $2 AND verdict = 'accepted'",
        )
        .bind(&case.task_id)
        .bind(&case.worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(accepted_attestations, 1);

        let replay = repo
            .complete_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &result_json,
            )
            .await
            .unwrap();
        assert_eq!(replay.status, TaskStatus::Completed);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            1
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 3);
        assert_eq!(user_balance(&repo.pool, &case.owner).await, 75);
        assert_eq!(user_balance(&repo.pool, &case.provider).await, 23);

        let mut conflicting = result.clone();
        conflicting.output = "[[9,9]]".into();
        conflicting.output_sha256 =
            general_compute_runtime::sha256_digest(conflicting.output.as_bytes());
        conflicting.usage.output_bytes = conflicting.output.len() as u64;
        let conflicting_json = serde_json::to_vec(&conflicting).unwrap();
        let conflict = repo
            .complete_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &conflicting_json,
            )
            .await;
        assert!(
            conflict.is_err(),
            "conflicting terminal replay must be rejected"
        );
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            1
        );

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_result_getter_returns_canonical_result_without_mutating_settlement() {
        let (p, fixture) = match pool("task_repository_managed_gpu_result_getter").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[4,6]]",
        );
        let result_json = serde_json::to_vec(&result).unwrap();

        repo.complete_managed_gpu_for_worker(
            &case.task_id,
            &case.worker_id,
            &case.manifest,
            &result_json,
        )
        .await
        .unwrap();
        let before = (
            user_balance(&repo.pool, &case.owner).await,
            user_balance(&repo.pool, &case.provider).await,
            managed_gpu_result_count(&repo.pool, &case.task_id).await,
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            task_ledger_count(&repo.pool, &case.task_id).await,
        );

        let exposed = repo
            .managed_gpu_result_for_task(&case.task_id)
            .await
            .unwrap()
            .expect("a settled current GPU attempt must be readable");
        let decoded: ManagedGpuResult = serde_json::from_slice(&exposed).unwrap();
        assert_eq!(decoded, result);
        assert_eq!(decoded.status, ManagedGpuStatus::Completed);

        let after = (
            user_balance(&repo.pool, &case.owner).await,
            user_balance(&repo.pool, &case.provider).await,
            managed_gpu_result_count(&repo.pool, &case.task_id).await,
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            task_ledger_count(&repo.pool, &case.task_id).await,
        );
        assert_eq!(after, before, "GET result must not settle or mutate state");
        let task = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.billing_settled);
        assert_eq!(task.billed_amount, case.request.reservation_cpt as i64);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_result_getter_is_current_attempt_and_assignment_bound() {
        let (p, fixture) = match pool("task_repository_managed_gpu_result_attempt").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let stale_result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[stale]]",
        );
        let stale_result_json = serde_json::to_vec(&stale_result).unwrap();

        let pending = repo
            .reset_to_pending_for_worker(&case.task_id, &case.worker_id)
            .await
            .unwrap();
        let rotated_manifest = pending.managed_gpu_manifest_json.clone().unwrap();
        let rotated_request: ManagedGpuRequest = serde_json::from_slice(&rotated_manifest).unwrap();
        repo.assign_to_worker(&case.task_id, &case.worker_id, "10.0.0.81")
            .await
            .unwrap();

        // An old result may remain for audit, but it must not satisfy a newer
        // attempt while the current generation has no result yet.
        sqlx::query(
            "INSERT INTO managed_gpu_results
                (task_id, attempt_id, attempt_generation, worker_id, result_json)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&case.task_id)
        .bind(&case.request.attempt_id)
        .bind(1_i64)
        .bind(&case.worker_id)
        .bind(&stale_result_json)
        .execute(&repo.pool)
        .await
        .unwrap();
        assert!(repo
            .managed_gpu_result_for_task(&case.task_id)
            .await
            .unwrap()
            .is_none());

        let current_result = managed_gpu_result(
            &rotated_request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[current]]",
        );
        let current_result_json = serde_json::to_vec(&current_result).unwrap();
        repo.complete_managed_gpu_for_worker(
            &case.task_id,
            &case.worker_id,
            &rotated_manifest,
            &current_result_json,
        )
        .await
        .unwrap();
        let exposed = repo
            .managed_gpu_result_for_task(&case.task_id)
            .await
            .unwrap()
            .expect("the current attempt result must be exposed");
        let decoded: ManagedGpuResult = serde_json::from_slice(&exposed).unwrap();
        assert_eq!(decoded.attempt_id, rotated_request.attempt_id);
        assert_eq!(decoded.output, "[[current]]");

        // Changing the task's assigned Worker invalidates the public read even
        // though the stored result itself was valid for the original Worker.
        sqlx::query("UPDATE tasks SET worker_id = $1 WHERE task_id = $2")
            .bind(format!("wrong-worker-{unique}"))
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        let error = repo
            .managed_gpu_result_for_task(&case.task_id)
            .await
            .expect_err("a result for another Worker must fail closed");
        assert!(error
            .to_string()
            .contains("does not match the current task assignment"));

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_result_getter_rejects_malformed_persisted_result() {
        let (p, fixture) = match pool("task_repository_managed_gpu_result_malformed").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[4,6]]",
        );
        repo.complete_managed_gpu_for_worker(
            &case.task_id,
            &case.worker_id,
            &case.manifest,
            &serde_json::to_vec(&result).unwrap(),
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE managed_gpu_results
             SET result_json = $1
             WHERE task_id = $2 AND attempt_generation = 1",
        )
        .bind(b"not-json".as_slice())
        .bind(&case.task_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        let error = repo
            .managed_gpu_result_for_task(&case.task_id)
            .await
            .expect_err("malformed persisted bytes must not reach clients");
        assert!(error
            .to_string()
            .contains("persisted managed GPU result is malformed"));

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_failure_is_unbilled_and_replays_exactly() {
        let (p, fixture) = match pool("task_repository_managed_gpu_failure").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Failed,
            "",
        );
        let result_json = serde_json::to_vec(&result).unwrap();

        let failed = repo
            .fail_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &result_json,
                "GPU backend failed",
            )
            .await
            .unwrap();
        assert_eq!(failed.status, TaskStatus::Failed);
        assert!(!failed.billing_settled);
        assert_eq!(failed.billed_amount, 0);
        assert!(failed.output.is_none());
        assert!(failed.result_torrent.is_none());
        assert_eq!(failed.status_message.as_deref(), Some("GPU backend failed"));
        assert_eq!(user_balance(&repo.pool, &case.owner).await, 100);
        assert_eq!(user_balance(&repo.pool, &case.provider).await, 0);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);

        let reputation: (i64, i64, i32) = sqlx::query_as(
            "SELECT successful_tasks, failed_tasks, score
             FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&case.worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(reputation, (0, 1, 95));
        let rejected_attestations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations
             WHERE task_id = $1 AND worker_id = $2 AND verdict = 'rejected'",
        )
        .bind(&case.task_id)
        .bind(&case.worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(rejected_attestations, 1);

        let replay = repo
            .fail_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &result_json,
                "different replay reason",
            )
            .await
            .unwrap();
        assert_eq!(replay.status, TaskStatus::Failed);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(user_balance(&repo.pool, &case.owner).await, 100);

        let mut conflicting = result.clone();
        conflicting.error_code = Some("different_gpu_error".into());
        let conflicting_json = serde_json::to_vec(&conflicting).unwrap();
        let conflict = repo
            .fail_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &conflicting_json,
                "conflict",
            )
            .await;
        assert!(
            conflict.is_err(),
            "conflicting failure replay must be rejected"
        );
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(rejected_attestations, 1);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_nodepool_failure_is_typed_without_worker_penalty() {
        let (p, fixture) = match pool("task_repository_managed_gpu_nodepool_failure").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let reputation_before = seed_managed_gpu_reputation(&repo.pool, &case.worker_id).await;

        let failed = repo
            .fail_managed_gpu_without_worker_result(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                ManagedGpuStatus::BackendUnavailable,
                "backend_unavailable",
                "GPU backend is unavailable",
            )
            .await
            .unwrap();
        assert_eq!(failed.status, TaskStatus::Failed);
        assert_eq!(
            failed.status_message.as_deref(),
            Some("GPU backend is unavailable")
        );
        assert!(!failed.billing_settled);
        assert_eq!(failed.billed_amount, 0);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);

        let persisted: Vec<u8> =
            sqlx::query_scalar("SELECT result_json FROM managed_gpu_results WHERE task_id = $1")
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        let result: ManagedGpuResult = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(result.status, ManagedGpuStatus::BackendUnavailable);
        assert_eq!(result.error_code.as_deref(), Some("backend_unavailable"));
        assert_eq!(result.selected_gpu, case.capability);

        assert_eq!(
            managed_gpu_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "Nodepool-owned GPU failure must not mutate Worker reputation"
        );
        let rejected_attestations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations
             WHERE task_id = $1 AND worker_id = $2 AND verdict = 'rejected'",
        )
        .bind(&case.task_id)
        .bind(&case.worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(rejected_attestations, 0);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_malformed_manifest_is_quarantined_without_stranding_task() {
        let (p, fixture) = match pool("task_repository_managed_gpu_malformed_manifest").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let reputation_before = seed_managed_gpu_reputation(&repo.pool, &case.worker_id).await;
        let malformed_manifest = b"not-json".to_vec();
        sqlx::query("UPDATE tasks SET managed_gpu_manifest_json = $1 WHERE task_id = $2")
            .bind(&malformed_manifest)
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();

        let timed_out = repo
            .fail_managed_gpu_without_worker_result(
                &case.task_id,
                &case.worker_id,
                &malformed_manifest,
                ManagedGpuStatus::TimedOut,
                "worker_heartbeat_lost",
                "Worker heartbeat lost",
            )
            .await
            .unwrap();
        assert_eq!(timed_out.status, TaskStatus::TimedOut);
        assert_eq!(
            timed_out.status_message.as_deref(),
            Some("managed GPU request manifest is malformed")
        );
        assert!(!timed_out.billing_settled);
        assert_eq!(timed_out.billed_amount, 0);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "manifest quarantine must not mutate Worker reputation"
        );

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_missing_binding_is_quarantined_without_stranding_task() {
        let (p, fixture) = match pool("task_repository_managed_gpu_missing_binding").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let reputation_before = seed_managed_gpu_reputation(&repo.pool, &case.worker_id).await;
        sqlx::query("DELETE FROM managed_gpu_attempt_bindings WHERE task_id = $1")
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();

        let timed_out = repo
            .fail_managed_gpu_without_worker_result(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                ManagedGpuStatus::TimedOut,
                "worker_heartbeat_lost",
                "Worker heartbeat lost",
            )
            .await
            .unwrap();
        assert_eq!(timed_out.status, TaskStatus::TimedOut);
        assert_eq!(
            timed_out.status_message.as_deref(),
            Some("Worker heartbeat lost")
        );
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "missing-binding quarantine must not mutate Worker reputation"
        );

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_cancel_without_binding_is_durable() {
        let (p, fixture) = match pool("task_repository_managed_gpu_cancel_missing_binding").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let reputation_before = seed_managed_gpu_reputation(&repo.pool, &case.worker_id).await;
        sqlx::query("DELETE FROM managed_gpu_attempt_bindings WHERE task_id = $1")
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();

        let cancelled = repo.cancel(&case.task_id).await.unwrap();
        assert_eq!(cancelled.status, TaskStatus::Cancelled);
        assert_eq!(
            cancelled.status_message.as_deref(),
            Some("managed GPU task cancelled without a trusted typed result")
        );
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "cancellation quarantine must not mutate Worker reputation"
        );

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_cancel_persists_typed_result_without_worker_penalty() {
        let (p, fixture) = match pool("task_repository_managed_gpu_cancel").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let reputation_before = seed_managed_gpu_reputation(&repo.pool, &case.worker_id).await;

        let cancelled = repo.cancel(&case.task_id).await.unwrap();
        assert_eq!(cancelled.status, TaskStatus::Cancelled);
        assert!(!cancelled.billing_settled);
        assert_eq!(cancelled.billed_amount, 0);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);

        let persisted: Vec<u8> =
            sqlx::query_scalar("SELECT result_json FROM managed_gpu_results WHERE task_id = $1")
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        let result: ManagedGpuResult = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(result.status, ManagedGpuStatus::Cancelled);
        assert_eq!(result.error_code.as_deref(), Some("task_cancelled"));
        assert_eq!(result.selected_gpu, case.capability);

        assert_eq!(
            managed_gpu_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "owner cancellation must not mutate Worker reputation"
        );
        let attestation_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations
             WHERE task_id = $1 AND worker_id = $2",
        )
        .bind(&case.task_id)
        .bind(&case.worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(attestation_count, 0);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_retry_rotates_manifest_and_rejects_stale_result() {
        let (p, fixture) = match pool("task_repository_managed_gpu_retry").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, true, true).await;
        let stale_result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[4,6]]",
        );
        let stale_result_json = serde_json::to_vec(&stale_result).unwrap();

        let pending = repo
            .reset_to_pending_for_worker(&case.task_id, &case.worker_id)
            .await
            .unwrap();
        assert_eq!(pending.status, TaskStatus::Pending);
        assert_eq!(pending.retry_count, 1);
        let rotated_manifest = pending.managed_gpu_manifest_json.clone().unwrap();
        let rotated_request: ManagedGpuRequest = serde_json::from_slice(&rotated_manifest).unwrap();
        assert_ne!(rotated_request.attempt_id, case.request.attempt_id);
        assert_ne!(rotated_request.request_digest, case.request.request_digest);
        assert_eq!(rotated_request.execution_id, case.request.execution_id);
        assert_eq!(
            rotated_request.idempotency_key,
            case.request.idempotency_key
        );

        let stale = repo
            .complete_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &stale_result_json,
            )
            .await;
        assert!(
            stale.is_err(),
            "the old attempt must not settle after rotation"
        );
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(user_balance(&repo.pool, &case.owner).await, 100);

        let changed_capability = ManagedGpuCapability::new(
            "cuda-test-retry-1",
            case.request.gpu_requirement.compute_capability.clone(),
            case.request.gpu_requirement.runtime_version.clone(),
            case.request.gpu_requirement.driver_abi.clone(),
            16 * 1024 * 1024 * 1024,
            32,
            case.request.guest_image_digest.clone(),
            1,
            "GPU-fedcba9876543210",
        )
        .unwrap();
        let changed_registration = managed_gpu_registration(&case.request, &changed_capability);
        sqlx::query(
            "UPDATE worker_nodes
             SET general_compute_capabilities_json = $1
             WHERE worker_id = $2",
        )
        .bind(serde_json::to_string(&changed_registration).unwrap())
        .bind(&case.worker_id)
        .execute(&repo.pool)
        .await
        .unwrap();
        repo.assign_to_worker(&case.task_id, &case.worker_id, "10.0.0.81")
            .await
            .unwrap();
        let rotated_binding = repo
            .managed_gpu_attempt_binding(&case.task_id, &case.worker_id, 2)
            .await
            .unwrap()
            .expect("retry assignment must persist a new immutable GPU binding");
        assert_eq!(rotated_binding.selected_gpu, changed_capability);
        assert_ne!(rotated_binding.selected_gpu, case.capability);
        let rotated_result = managed_gpu_result(
            &rotated_request,
            &changed_capability,
            ManagedGpuStatus::Completed,
            "[[4,6]]",
        );
        let rotated_result_json = serde_json::to_vec(&rotated_result).unwrap();
        let completed = repo
            .complete_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &rotated_manifest,
                &rotated_result_json,
            )
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
        assert_eq!(completed.retry_count, 1);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 1);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            1
        );
        let generation: i64 = sqlx::query_scalar(
            "SELECT attempt_generation FROM managed_gpu_settlements WHERE task_id = $1",
        )
        .bind(&case.task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(generation, 2);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_insufficient_balance_rolls_back_all_settlement_state() {
        let (p, fixture) = match pool("task_repository_managed_gpu_insufficient_balance").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 10, true, true).await;
        let reputation_before = seed_managed_gpu_reputation(&repo.pool, &case.worker_id).await;
        let result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[4,6]]",
        );
        let result_json = serde_json::to_vec(&result).unwrap();

        let error = repo
            .complete_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &result_json,
            )
            .await
            .expect_err("insufficient balance must reject settlement");
        assert!(error.to_string().contains("insufficient balance"));
        let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert!(!stored.billing_settled);
        assert_eq!(stored.billed_amount, 0);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(user_balance(&repo.pool, &case.owner).await, 10);
        assert_eq!(user_balance(&repo.pool, &case.provider).await, 0);
        assert_eq!(
            managed_gpu_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "failed settlement rollback must not mutate Worker reputation"
        );

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_missing_provider_account_rolls_back_debit_and_result() {
        let (p, fixture) = match pool("task_repository_managed_gpu_missing_provider").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_case(&repo, &unique, 100, false, true).await;
        let result = managed_gpu_result(
            &case.request,
            &case.capability,
            ManagedGpuStatus::Completed,
            "[[4,6]]",
        );
        let result_json = serde_json::to_vec(&result).unwrap();

        let error = repo
            .complete_managed_gpu_for_worker(
                &case.task_id,
                &case.worker_id,
                &case.manifest,
                &result_json,
            )
            .await
            .expect_err("missing provider account must reject settlement");
        assert!(error.to_string().contains("provider account"));
        let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert!(!stored.billing_settled);
        assert_eq!(managed_gpu_result_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(
            managed_gpu_settlement_count(&repo.pool, &case.task_id).await,
            0
        );
        assert_eq!(task_ledger_count(&repo.pool, &case.task_id).await, 0);
        assert_eq!(user_balance(&repo.pool, &case.owner).await, 100);

        cleanup_managed_gpu_case(&repo, fixture, &case).await;
    }

    #[tokio::test]
    async fn managed_gpu_requires_private_operator_capability_snapshot() {
        let (p, fixture) = match pool("task_repository_managed_gpu_snapshot").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let owner = format!("gpu-snapshot-owner-{unique}");
        let provider = format!("gpu-snapshot-provider-{unique}");
        let worker_id = format!("gpu-snapshot-worker-{unique}");
        let task_id = format!("gpu-snapshot-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&owner)
        .execute(&repo.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 0)")
            .bind(&provider)
            .execute(&repo.pool)
            .await
            .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let request = managed_gpu_request(&unique);
        let manifest = serde_json::to_vec(&request).unwrap();
        let mut task = make_task(&task_id, &owner);
        task.runtime = Some(MANAGED_GPU_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.managed_gpu_manifest_json = Some(manifest);
        task.max_cpt = request.reservation_cpt as i64;
        repo.create(&task).await.unwrap();

        let missing = repo
            .assign_to_worker(&task_id, &worker_id, "10.0.0.80")
            .await
            .expect_err("GPU assignment must require a private trusted snapshot");
        assert!(missing
            .to_string()
            .contains("private trusted capability snapshot"));
        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());
        let binding_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM managed_gpu_attempt_bindings WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(binding_count, 0);

        cleanup_task_case(&repo.pool, &task_id, &owner, Some(&worker_id)).await;
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&provider)
            .execute(&repo.pool)
            .await
            .ok();
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
        let modern_task_id = format!("claim-modern-task-{unique}");
        let modern_request =
            inline_general_compute_request(&format!("modern-{unique}"), b"modern claim source");
        let mut modern_task =
            task_for_general_compute_request(&modern_task_id, &username, &modern_request);
        modern_task.priority = 20_000;
        repo_a.create(&modern_task).await.unwrap();

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

        let modern = repo_a
            .find_by_task_id(&modern_task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(modern.status, TaskStatus::Pending);
        assert!(modern.worker_id.is_none());

        for task_id in task_ids {
            sqlx::query("DELETE FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .execute(&p)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&modern_task_id)
            .execute(&p)
            .await
            .ok();
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
    async fn test_legacy_result_completion_rejects_general_compute_tasks() {
        let (p, fixture) = match pool("task_repository_legacy_result_rejected").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("legacy-result-owner-{unique}");
        let worker_id = format!("legacy-result-worker-{unique}");
        let task_id = format!("legacy-result-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "legacy-result-provider").await;

        let mut request = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: format!("attempt-{unique}"),
            idempotency_key: format!("idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "legacy-result-backend".into(),
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
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.78")
            .await
            .unwrap();

        let error = repo
            .complete_result_for_worker(&task_id, &worker_id, "btih:forged-result")
            .await
            .expect_err("legacy result upload must not complete general-compute tasks");
        assert!(error
            .to_string()
            .contains("validated typed result envelope"));

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.result_torrent, None);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
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

        let assigned = repo
            .assign_to_worker(&task_id, &worker_id, "10.0.0.9")
            .await
            .unwrap();
        assert_eq!(assigned.status, TaskStatus::Assigned);
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
            let (task_id, request) = if transition == "complete" {
                create_assigned_general_compute_task_with_max_cpt(
                    &repo,
                    &format!("lease-{transition}"),
                    &worker_id,
                    25,
                )
                .await
            } else {
                create_assigned_general_compute_task(
                    &repo,
                    &format!("lease-{transition}"),
                    &worker_id,
                )
                .await
            };
            match transition {
                "complete" => {
                    let result_json = serde_json::to_vec(&GeneralComputeResult {
                        execution_id: request.execution_id.clone(),
                        attempt_id: request.attempt_id.clone(),
                        idempotency_key: request.idempotency_key.clone(),
                        request_digest: request.request_digest.clone(),
                        status: ResultStatus::Completed,
                        exit_code: Some(0),
                        error_code: None,
                        stdout: "lease completion output".into(),
                        stderr: String::new(),
                        output_artifacts: vec![],
                        usage: UsageClaim {
                            wall_time_ms: 1,
                            ..UsageClaim::default()
                        },
                        runtime_version: request.runtime_version.clone(),
                        backend_id: request.backend_id.clone(),
                        guest_image_digest: request.guest_image_digest.clone(),
                        input_sha256: general_compute_runtime::canonical_input_digest(
                            request
                                .source_artifact
                                .inline_bytes
                                .as_deref()
                                .unwrap_or_default(),
                            &[],
                        ),
                        determinism: request.determinism.clone(),
                        capability_summary: vec![],
                        gpu_selection: None,
                        output_manifest_root: canonical_artifact_root(&[]),
                        evidence: EvidenceEnvelope::default(),
                    })
                    .unwrap();
                    let manifest = serde_json::to_vec(&request).unwrap();
                    repo.complete_general_compute_for_worker(
                        &task_id,
                        &worker_id,
                        &manifest,
                        &result_json,
                        Some("lease completion output"),
                    )
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

    #[tokio::test]
    async fn retry_limit_terminalizes_general_compute_and_revokes_lease() {
        let (p, fixture) =
            match pool("task_repository_retry_limit_terminalizes_general_compute").await {
                Some(parts) => parts,
                None => return,
            };
        let repo = TaskRepository::new(p);
        let (task_id, _) =
            create_assigned_general_compute_task(&repo, "retry-limit-terminal", "worker-a").await;
        sqlx::query("UPDATE tasks SET max_retries = 0 WHERE task_id = $1")
            .bind(&task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        let assigned = repo.find_by_task_id(&task_id).await.unwrap().unwrap();

        let terminal = repo
            .retry_to_pending_for_worker_snapshot(
                &assigned,
                "worker-a",
                4,
                "retry budget exhausted",
            )
            .await
            .unwrap()
            .expect("the current attempt must be terminalized");

        assert_eq!(terminal.status, TaskStatus::Failed);
        assert!(terminal.completed_at.is_some());
        assert!(!terminal.billing_settled);
        assert_eq!(terminal.billed_amount, 0);
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

        cleanup_task_case(&repo.pool, &task_id, "retry-limit-terminal-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn retry_limit_uses_dispatcher_cap_and_rotates_only_once() {
        let (p, fixture) = match pool("task_repository_retry_limit_dispatcher_cap").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let (task_id, _) =
            create_assigned_general_compute_task(&repo, "retry-limit-cap", "worker-a").await;
        let first = repo.find_by_task_id(&task_id).await.unwrap().unwrap();

        let pending = repo
            .retry_to_pending_for_worker_snapshot(&first, "worker-a", 1, "dispatcher retry cap")
            .await
            .unwrap()
            .expect("the first retry must be pending");
        assert_eq!(pending.status, TaskStatus::Pending);
        assert_eq!(pending.retry_count, 1);
        assert!(repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .is_none());

        repo.assign_to_worker(&task_id, "worker-a", "10.0.0.1")
            .await
            .unwrap();
        let second = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        let terminal = repo
            .retry_to_pending_for_worker_snapshot(&second, "worker-a", 1, "dispatcher retry cap")
            .await
            .unwrap()
            .expect("the second failure must terminalize the task");
        assert_eq!(terminal.status, TaskStatus::Failed);
        assert!(repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .is_none());

        cleanup_task_case(&repo.pool, &task_id, "retry-limit-cap-owner", None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn assignment_terminalizes_pending_task_already_over_retry_ceiling() {
        let (p, fixture) = match pool("task_repository_assignment_retry_ceiling").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("assignment-retry-ceiling-{unique}");
        let owner = format!("assignment-retry-ceiling-owner-{unique}");
        let task = make_task(&task_id, &owner);
        repo.create(&task).await.unwrap();
        sqlx::query("UPDATE tasks SET retry_count = 1, max_retries = 0 WHERE task_id = $1")
            .bind(&task_id)
            .execute(&repo.pool)
            .await
            .unwrap();

        let assignment = repo
            .assign_to_worker_with_retry_limit(&task_id, "worker-ceiling", "10.0.0.1", 4)
            .await;
        assert!(assignment.is_err());
        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert!(stored.completed_at.is_some());

        cleanup_task_case(&repo.pool, &task_id, &owner, None).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn dispatcher_terminalizes_workerless_stale_assignment_even_before_retry_limit() {
        let (p, fixture) = match pool("dispatcher_workerless_stale_assignment").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("workerless-stale-{unique}");
        let owner = format!("workerless-stale-owner-{unique}");
        let task = make_task(&task_id, &owner);
        repo.create(&task).await.unwrap();
        sqlx::query(
            "UPDATE tasks
             SET status = 'ASSIGNED', worker_id = NULL, worker_ip = NULL,
                 last_update = NOW() - INTERVAL '1 hour'
             WHERE task_id = $1",
        )
        .bind(&task_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        let dispatcher = crate::dispatcher::Dispatcher::new(
            hivemind_database::DatabaseManager { pool: p },
            1,
            3,
        );
        let (redispatched, failed) = dispatcher.process_timeouts().await.unwrap();
        assert_eq!(redispatched, 0);
        assert_eq!(failed, 1);
        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert!(stored.completed_at.is_some());

        cleanup_task_case(&repo.pool, &task_id, &owner, None).await;
        fixture.cleanup().await.ok();
    }

    async fn create_assigned_general_compute_task(
        repo: &TaskRepository,
        label: &str,
        worker_id: &str,
    ) -> (String, GeneralComputeRequest) {
        create_assigned_general_compute_task_with_max_cpt(repo, label, worker_id, 0).await
    }

    async fn create_assigned_general_compute_task_with_max_cpt(
        repo: &TaskRepository,
        label: &str,
        worker_id: &str,
        max_cpt: i64,
    ) -> (String, GeneralComputeRequest) {
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("{label}-{unique}");
        let request = inline_general_compute_request(&unique, b"lease source");
        let mut task =
            task_for_general_compute_request(&task_id, &format!("{label}-owner"), &request);
        task.max_cpt = max_cpt;
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

    struct ManagedGpuTestCase {
        task_id: String,
        owner: String,
        provider: String,
        worker_id: String,
        request: ManagedGpuRequest,
        capability: ManagedGpuCapability,
        manifest: Vec<u8>,
    }

    async fn seed_managed_gpu_reputation(pool: &PgPool, worker_id: &str) -> (i64, i64, i32, bool) {
        let baseline = (7, 3, 83, false);
        sqlx::query(
            "INSERT INTO worker_reputation
             (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(worker_id)
        .bind(baseline.0)
        .bind(baseline.1)
        .bind(baseline.2)
        .bind(baseline.3)
        .execute(pool)
        .await
        .unwrap();
        baseline
    }

    async fn managed_gpu_reputation(pool: &PgPool, worker_id: &str) -> (i64, i64, i32, bool) {
        sqlx::query_as(
            "SELECT successful_tasks, failed_tasks, score, banned
             FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(worker_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn managed_gpu_request(unique: &str) -> ManagedGpuRequest {
        let image_digest = format!("sha256:{}", "a".repeat(64));
        let gpu_requirement = ManagedGpuRequirement::new(
            "8.9",
            "12.4",
            "550",
            8 * 1024 * 1024 * 1024,
            1,
            image_digest.clone(),
        )
        .unwrap();
        let mut request = ManagedGpuRequest {
            protocol_version: MANAGED_GPU_REQUEST_PROTOCOL_VERSION.into(),
            execution_id: format!("gpu-execution-{unique}"),
            attempt_id: format!("gpu-attempt-{unique}"),
            idempotency_key: format!("gpu-idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
            semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
            operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
            backend_id: "managed-cuda-test".into(),
            guest_image_digest: image_digest,
            source: "gpu_add_f32([[1, 2]], [[3, 4]])".into(),
            input_json: r#"{"lhs":[[1,2]],"rhs":[[3,4]]}"#.into(),
            gpu_requirement,
            limits: ManagedGpuLimits::default(),
            reservation_cpt: 25,
            billing_version: MANAGED_GPU_BILLING_VERSION.into(),
            cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
            settlement_basis: MANAGED_GPU_SETTLEMENT_BASIS.into(),
            proof_policy: ManagedGpuProofPolicy::None,
        };
        request.request_digest = request.canonical_request_digest();
        request.validate().unwrap();
        request
    }

    fn managed_gpu_capability(request: &ManagedGpuRequest) -> ManagedGpuCapability {
        ManagedGpuCapability::new(
            "cuda-test-0",
            request.gpu_requirement.compute_capability.clone(),
            request.gpu_requirement.runtime_version.clone(),
            request.gpu_requirement.driver_abi.clone(),
            16 * 1024 * 1024 * 1024,
            32,
            request.guest_image_digest.clone(),
            0,
            "GPU-0123456789abcdef",
        )
        .unwrap()
    }

    fn managed_gpu_registration(
        request: &ManagedGpuRequest,
        capability: &ManagedGpuCapability,
    ) -> TrustedWorkerCapabilityRegistration {
        TrustedWorkerCapabilityRegistration {
            worker: WorkerCapabilities {
                guest_image_digests: vec![request.guest_image_digest.clone()],
                capabilities: vec!["cuda".into()],
                max_threads: 4,
                gpu_available: true,
            },
            gpu_capabilities: vec![],
            managed_gpu_backends: vec![ManagedGpuBackendRegistration {
                backend_id: request.backend_id.clone(),
                runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
                semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
                operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
                guest_image_digest: request.guest_image_digest.clone(),
                billing_version: MANAGED_GPU_BILLING_VERSION.into(),
                cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
                reservation_cpt: request.reservation_cpt,
                max_source_bytes: 256 * 1024,
                max_input_bytes: 16 * 1024 * 1024,
                max_output_bytes: 16 * 1024 * 1024,
                max_operations: 1_000_000,
                max_gpu_time_ms: 120_000,
                capabilities: vec![capability.clone()],
            }],
            backends: vec![],
        }
    }

    fn managed_gpu_result(
        request: &ManagedGpuRequest,
        capability: &ManagedGpuCapability,
        status: ManagedGpuStatus,
        output: &str,
    ) -> ManagedGpuResult {
        let completed = status == ManagedGpuStatus::Completed;
        let executed_operations = if completed { 1 } else { 0 };
        ManagedGpuResult {
            protocol_version: MANAGED_GPU_RESULT_PROTOCOL_VERSION.into(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            runtime_version: request.runtime_version.clone(),
            semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
            operation_registry_version: request.operation_registry_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            source_sha256: request.source_sha256(),
            input_sha256: request.input_sha256(),
            reservation_cpt: request.reservation_cpt,
            status,
            exit_code: completed
                .then_some(0)
                .or_else(|| (status == ManagedGpuStatus::Failed).then_some(1)),
            error_code: (!completed).then(|| "gpu_execution_error".into()),
            output: output.into(),
            output_sha256: general_compute_runtime::sha256_digest(output.as_bytes()),
            selected_gpu: capability.clone(),
            usage: ManagedGpuUsage {
                source_bytes: request.source.len() as u64,
                input_bytes: request.input_json.len() as u64,
                output_bytes: output.len() as u64,
                executed_operations,
                operation_cost_units: executed_operations * 10,
                wall_time_ms: 1,
                gpu_time_ms: 1,
                gpu_memory_bytes: 1024,
            },
            evidence: ManagedGpuEvidence {
                level: ManagedGpuEvidenceLevel::Unverified,
                payload_sha256: None,
            },
        }
    }

    async fn setup_managed_gpu_case(
        repo: &TaskRepository,
        unique: &str,
        owner_balance: i64,
        create_provider_user: bool,
        install_snapshot: bool,
    ) -> ManagedGpuTestCase {
        let owner = format!("gpu-owner-{unique}");
        let provider = format!("gpu-provider-{unique}");
        let worker_id = format!("gpu-worker-{unique}");
        let task_id = format!("gpu-task-{unique}");
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', $2)")
            .bind(&owner)
            .bind(owner_balance)
            .execute(&repo.pool)
            .await
            .unwrap();
        if create_provider_user {
            sqlx::query(
                "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 0)",
            )
            .bind(&provider)
            .execute(&repo.pool)
            .await
            .unwrap();
        }
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let request = managed_gpu_request(unique);
        let capability = managed_gpu_capability(&request);
        if install_snapshot {
            let registration = managed_gpu_registration(&request, &capability);
            let snapshot = serde_json::to_string(&registration).unwrap();
            sqlx::query(
                "UPDATE worker_nodes
                 SET general_compute_capabilities_json = $1,
                     admission_mode = $2
                 WHERE worker_id = $3",
            )
            .bind(snapshot)
            .bind(PRIVATE_STATIC_ADMISSION_MODE)
            .bind(&worker_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        }

        let manifest = serde_json::to_vec(&request).unwrap();
        let mut task = make_task(&task_id, &owner);
        task.runtime = Some(MANAGED_GPU_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.managed_gpu_manifest_json = Some(manifest.clone());
        task.max_cpt = request.reservation_cpt as i64;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.80")
            .await
            .unwrap();

        ManagedGpuTestCase {
            task_id,
            owner,
            provider,
            worker_id,
            request,
            capability,
            manifest,
        }
    }

    async fn cleanup_managed_gpu_case(
        repo: &TaskRepository,
        fixture: IsolatedTestPool,
        case: &ManagedGpuTestCase,
    ) {
        cleanup_task_case(
            &repo.pool,
            &case.task_id,
            &case.owner,
            Some(&case.worker_id),
        )
        .await;
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&case.provider)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    async fn managed_gpu_result_count(pool: &PgPool, task_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn managed_gpu_settlement_count(pool: &PgPool, task_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM managed_gpu_settlements WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn task_ledger_count(pool: &PgPool, task_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM ledger_entries WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn user_balance(pool: &PgPool, username: &str) -> i64 {
        sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap()
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
            managed_gpu_manifest_json: None,
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

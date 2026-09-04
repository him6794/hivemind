#![allow(clippy::result_large_err)]

use crate::managed_proof_verifier::{verify_managed_proof, ManagedProofVerifierError};
use anyhow::Result;
use chrono::{Duration as ChronoDuration, Utc};
use general_compute_runtime::managed_gpu::{
    ManagedGpuCapability, ManagedGpuRequest, ManagedGpuResult, ManagedGpuStatus,
    MANAGED_GPU_RUNTIME_VERSION,
};
use hivemind_auth::managed_proof::{
    claims_with_issuance, new_claims, ManagedProofAuthorizationSigner,
};
use hivemind_auth::worker_execution::{WorkerExecutionIdentity, WorkerExecutionSigner};
use hivemind_client_core::{SessionError, SessionTask, SharedSessionRegistry};
use hivemind_config::ManagedProofRolloutMode;
use hivemind_database::DatabaseManager;
use hivemind_managed_proof::{
    dsl_proof_task_id, ClaimError, ExecutionClaim, RISC0_MANAGED_GUEST_ID, RISC0_PROOF_SCHEME,
};
use hivemind_managed_prover_protocol::{
    ManagedProverRequest, RemoteManagedProofRequest, MANAGED_PROVER_PROTOCOL_VERSION,
    REMOTE_MANAGED_PROOF_PROTOCOL_VERSION,
};
use hivemind_models::{Claims, Task, TaskStatus, WorkerNode};
use hivemind_proto::{
    general_compute_chunk_service_client::GeneralComputeChunkServiceClient,
    validate_general_compute_chunk_resume_request, validate_general_compute_chunk_upload,
    validate_general_compute_prepare_request, worker_node_service_client::WorkerNodeServiceClient,
    ExecuteTaskRequest, ExecuteTaskResponse, GeneralComputeChunkResumeRequest,
    GeneralComputeChunkUpload, GeneralComputePrepareRequest, ManagedProofEnvelope,
    ResourceSpec as ProtoResourceSpec, GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES,
    GENERAL_COMPUTE_RESULT_MAX_BYTES, LEGACY_MANAGED_RECEIPT_MAX_BYTES,
    MANAGED_GPU_RESULT_MAX_BYTES, WORKER_RPC_MESSAGE_MAX_BYTES, WORKER_STATUS_MESSAGE_MAX_BYTES,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::managed_proof_metrics::{self, ManagedProofMetricEvent};
use crate::scheduler;
use crate::task_repository::{
    is_managed_gpu_binding_integrity_error, ManagedProofAuthorizationRecord,
    ManagedProofAuthorizationStateUpdate, TaskRepository,
};

pub struct Dispatcher {
    repo: Arc<TaskRepository>,
    db: DatabaseManager,
    task_timeout_secs: u64,
    max_redispatch: i32,
    worker_execution_private_key_pem: String,
    managed_proof_authorization_private_key_pem: String,
    managed_proof_provider_configured: bool,
    managed_proof_rollout_mode: ManagedProofRolloutMode,
    session_registry: Option<SharedSessionRegistry>,
}

struct WorkerExecutionOptions {
    worker_execution_private_key_pem: String,
    managed_proof_authorization_private_key_pem: String,
    managed_proof_provider_configured: bool,
    managed_proof_rollout_mode: ManagedProofRolloutMode,
    max_redispatch: i32,
}

impl Dispatcher {
    pub fn new(db: DatabaseManager, task_timeout_secs: u64, max_redispatch: i32) -> Self {
        assert!(max_redispatch >= 0, "max_redispatch must not be negative");
        Self {
            repo: Arc::new(TaskRepository::new(db.pool.clone())),
            db,
            task_timeout_secs,
            max_redispatch,
            worker_execution_private_key_pem: std::env::var("WORKER_EXECUTION_PRIVATE_KEY_PEM")
                .unwrap_or_default(),
            managed_proof_authorization_private_key_pem: std::env::var(
                "MANAGED_PROOF_AUTH_PRIVATE_KEY_PEM",
            )
            .unwrap_or_default(),
            managed_proof_provider_configured: false,
            managed_proof_rollout_mode: ManagedProofRolloutMode::Enforce,
            session_registry: None,
        }
    }

    pub fn with_worker_execution_private_key(mut self, private_key_pem: String) -> Self {
        self.worker_execution_private_key_pem = private_key_pem;
        self
    }

    pub fn with_managed_proof_authorization_private_key(mut self, private_key_pem: String) -> Self {
        self.managed_proof_authorization_private_key_pem = private_key_pem;
        self
    }

    pub fn with_managed_proof_provider_configured(mut self, configured: bool) -> Self {
        self.managed_proof_provider_configured = configured;
        self
    }

    pub fn with_managed_proof_rollout_mode(
        mut self,
        rollout_mode: ManagedProofRolloutMode,
    ) -> Self {
        self.managed_proof_rollout_mode = rollout_mode;
        self
    }

    pub fn with_session_registry(mut self, session_registry: SharedSessionRegistry) -> Self {
        self.session_registry = Some(session_registry);
        self
    }

    pub async fn dispatch_one(
        &self,
        task: &Task,
        workers: &[WorkerNode],
    ) -> Option<(String, String)> {
        let ranked_workers = match self.rank_workers_by_cache_affinity(task, workers).await {
            Ok(ranked) => ranked,
            Err(e) => {
                warn!(
                    "Failed to compute cache affinity for task {}: {}",
                    task.task_id, e
                );
                workers.to_vec()
            }
        };
        let worker = scheduler::find_best_worker(task, &ranked_workers).await?;
        let wid = worker.worker_id.clone();
        let wip = worker.ip.clone();
        match self
            .repo
            .assign_to_worker_with_retry_limit(&task.task_id, &wid, &wip, self.max_redispatch)
            .await
        {
            Ok(_) => {
                info!(
                    "Dispatched task {} to worker {} ({})",
                    task.task_id, wid, wip
                );
                Some((wid, wip))
            }
            Err(e) => {
                error!("Failed to dispatch task {}: {}", task.task_id, e);
                None
            }
        }
    }

    pub async fn handle_worker_session_ack(
        &self,
        worker_id: &str,
        owner: &str,
        task_id: &str,
        attempt_id: &str,
        idempotency_key: &str,
        request_digest: &str,
    ) -> Result<()> {
        let Some(task) = self.repo.find_by_task_id(task_id).await? else {
            anyhow::bail!("Worker session ACK references an unknown task");
        };
        if task.worker_id.as_deref() != Some(worker_id) || task.owner != owner {
            anyhow::bail!("Worker session ACK does not match the current assignment");
        }
        if !matches!(task.status, TaskStatus::Assigned | TaskStatus::Running) {
            anyhow::bail!("Worker session ACK references a non-running task");
        }
        let expected_identity = legacy_execution_identity(&task);
        if attempt_id != expected_identity.1
            || idempotency_key != expected_identity.2
            || request_digest != expected_identity.3
        {
            anyhow::bail!("Worker session ACK belongs to a stale task attempt");
        }
        if task.status == TaskStatus::Assigned
            && self
                .repo
                .mark_worker_execution_running_snapshot(&task, worker_id)
                .await?
                .is_none()
        {
            anyhow::bail!("Worker session ACK could not transition the task to RUNNING");
        }
        Ok(())
    }

    pub async fn handle_worker_session_result(
        &self,
        worker_id: &str,
        task_id: &str,
        request: ExecuteTaskRequest,
        response: ExecuteTaskResponse,
    ) -> Result<()> {
        let Some(task) = self.repo.find_by_task_id(task_id).await? else {
            return Ok(());
        };
        if task.worker_id.as_deref() != Some(worker_id)
            || !matches!(task.status, TaskStatus::Assigned | TaskStatus::Running)
        {
            return Ok(());
        }
        if request.task_id != task_id {
            anyhow::bail!("Worker session request task identity does not match delivery");
        }
        if request.token.trim().is_empty() {
            anyhow::bail!("Worker session request is missing its execution token");
        }
        if response.execution_id != request.execution_id
            || response.attempt_id != request.attempt_id
            || response.idempotency_key != request.idempotency_key
            || response.request_digest != request.request_digest
        {
            anyhow::bail!("Worker session result identity does not match the delivered request");
        }
        validate_worker_response_sizes(&response)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if is_managed_runtime(task.runtime.as_deref())
            || task.runtime.as_deref().map(str::trim)
                == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
            || task.runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION)
        {
            anyhow::bail!("session result requires the authoritative execution path");
        }
        let expected_identity = legacy_execution_identity(&task);
        if (
            request.execution_id.as_str(),
            request.attempt_id.as_str(),
            request.idempotency_key.as_str(),
            request.request_digest.as_str(),
        ) != (
            expected_identity.0.as_str(),
            expected_identity.1.as_str(),
            expected_identity.2.as_str(),
            expected_identity.3.as_str(),
        ) {
            anyhow::bail!("Worker session result belongs to a stale task attempt");
        }
        if response.success {
            if self
                .repo
                .complete_for_worker_snapshot(
                    &task,
                    worker_id,
                    None,
                    Some(&response.status_message),
                )
                .await?
                .is_none()
            {
                warn!(
                    task_id,
                    worker_id, "session completion arrived after the active attempt changed"
                );
            }
        } else if self
            .repo
            .fail_for_worker_snapshot(&task, worker_id, &response.status_message)
            .await?
            .is_none()
        {
            warn!(
                task_id,
                worker_id, "session failure arrived after the active attempt changed"
            );
        }
        Ok(())
    }

    async fn enqueue_worker_session_task(&self, snapshot: &Task, worker_id: &str) -> Result<bool> {
        let Some(task) = self.repo.find_by_task_id(&snapshot.task_id).await? else {
            return Ok(false);
        };
        if task.worker_id.as_deref() != Some(worker_id)
            || !matches!(task.status, TaskStatus::Assigned | TaskStatus::Running)
        {
            return Ok(false);
        }
        if !session_compatible_runtime(task.runtime.as_deref()) {
            return Ok(false);
        }
        let Some(registry) = self.session_registry.as_ref() else {
            return Ok(false);
        };
        let Some(identity) = registry
            .lock()
            .map_err(|_| anyhow::anyhow!("Worker session registry is unavailable"))?
            .active_identity_for_worker(worker_id)
        else {
            return Ok(false);
        };
        if identity.owner != task.owner {
            warn!(
                task_id = %task.task_id,
                worker_id,
                "Ignoring an active Worker session owned by another user"
            );
            return Ok(false);
        }
        let token = worker_session_execution_token_with_lifetime(
            &self.worker_execution_private_key_pem,
            &task,
            worker_id,
            SESSION_WORKER_EXECUTION_TOKEN_TTL_SECS,
        )?;
        let request = build_execute_task_request_with_credentials(&task, token, None);
        let delivery = SessionTask {
            task_id: task.task_id.clone(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            retry_count: task.retry_count,
            payload: prost::Message::encode_to_vec(&request),
        };
        let outcome = match registry
            .lock()
            .map_err(|_| anyhow::anyhow!("Worker session registry is unavailable"))?
            .enqueue(&identity, delivery)
        {
            Ok(outcome) => outcome,
            Err(SessionError::QueueFull) => {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    "Worker session delivery queue is full; retaining assignment for timeout retry"
                );
                return Ok(true);
            }
            Err(SessionError::InactiveSession | SessionError::InvalidResumeToken) => {
                return Ok(false);
            }
            Err(other) => return Err(anyhow::anyhow!(other.to_string())),
        };
        info!(
            task_id = %task.task_id,
            worker_id,
            delivery_sequence = outcome.delivery.delivery_sequence,
            duplicate = outcome.duplicate,
            "Queued task on outbound Worker session"
        );
        Ok(true)
    }

    pub fn cancel_session_delivery(&self, task: &Task) {
        if !session_compatible_runtime(task.runtime.as_deref()) {
            return;
        }
        let Some(registry) = self.session_registry.as_ref() else {
            return;
        };
        let Some(worker_id) = task.worker_id.as_deref() else {
            return;
        };
        let attempt_id = legacy_execution_identity(task).1;
        if let Ok(mut registry) = registry.lock() {
            let removed = registry.cancel_task_attempt(&task.task_id, &attempt_id);
            if removed > 0 {
                info!(
                    task_id = %task.task_id,
                    worker_id,
                    "Invalidated outbound session delivery during task reset"
                );
            }
        }
    }

    pub async fn dispatch_pending(&self, workers: &[WorkerNode]) -> Result<u64> {
        let terminalized = self.repo.terminalize_exhausted_pending().await?;
        if terminalized > 0 {
            warn!(
                "Terminalized {} pending tasks that exceeded their retry limit",
                terminalized
            );
        }
        let mut available_workers = self.repo.trusted_workers(workers).await?;
        let pending = self.repo.find_pending().await?;
        let mut dispatched = 0u64;
        for task in &pending {
            if let Some((worker_id, _)) = self.dispatch_one(task, &available_workers).await {
                reserve_worker_for_batch(&mut available_workers, &worker_id);
                dispatched += 1;
            }
        }
        if dispatched > 0 {
            info!("Dispatched {} pending tasks", dispatched);
        }
        Ok(dispatched)
    }

    pub async fn registered_workers(&self) -> Result<Vec<WorkerNode>> {
        sqlx::query_as::<_, WorkerNode>(
            "SELECT * FROM worker_nodes WHERE status IN ('ACTIVE', 'IDLE', 'BUSY')",
        )
        .fetch_all(&self.db.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn dispatch_pending_from_registered_workers(&self) -> Result<u64> {
        let workers = self.registered_workers().await?;
        self.dispatch_pending(&workers).await
    }

    pub async fn dispatch_pending_from_registered_workers_and_execute(&self) -> Result<u64> {
        let terminalized = self.repo.terminalize_exhausted_pending().await?;
        if terminalized > 0 {
            warn!(
                "Terminalized {} pending tasks that exceeded their retry limit",
                terminalized
            );
        }
        let workers = self.registered_workers().await?;
        let mut available_workers = self.repo.trusted_workers(&workers).await?;
        let pending = self.repo.find_pending().await?;
        let mut dispatched = 0u64;
        for task in &pending {
            if let Some((worker_id, worker_addr)) =
                self.dispatch_one(task, &available_workers).await
            {
                reserve_worker_for_batch(&mut available_workers, &worker_id);
                dispatched += 1;
                let session_enqueued =
                    match self.enqueue_worker_session_task(task, &worker_id).await {
                        Ok(enqueued) => enqueued,
                        Err(error) => {
                            warn!(
                                task_id = %task.task_id,
                                worker_id = %worker_id,
                                error = %error,
                                "Outbound Worker session enqueue failed; using unary fallback"
                            );
                            false
                        }
                    };
                if session_enqueued {
                    continue;
                }
                let repo = self.repo.clone();
                let task = task.clone();
                let execution_options = WorkerExecutionOptions {
                    worker_execution_private_key_pem: self.worker_execution_private_key_pem.clone(),
                    managed_proof_authorization_private_key_pem: self
                        .managed_proof_authorization_private_key_pem
                        .clone(),
                    managed_proof_provider_configured: self.managed_proof_provider_configured,
                    managed_proof_rollout_mode: self.managed_proof_rollout_mode,
                    max_redispatch: self.max_redispatch,
                };
                tokio::spawn(async move {
                    if let Err(e) = execute_on_worker_with_managed_proof_key(
                        repo,
                        task,
                        worker_id,
                        worker_addr,
                        execution_options,
                    )
                    .await
                    {
                        warn!("Worker execution failed: {}", e);
                    }
                });
            }
        }
        if dispatched > 0 {
            info!("Dispatched {} pending tasks", dispatched);
        }
        Ok(dispatched)
    }

    async fn rank_workers_by_cache_affinity(
        &self,
        task: &Task,
        workers: &[WorkerNode],
    ) -> Result<Vec<WorkerNode>> {
        if workers.len() <= 1 {
            return Ok(workers.to_vec());
        }
        let source = match task.torrent_source.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(workers.to_vec()),
        };
        let worker_ids: Vec<String> = workers.iter().map(|w| w.worker_id.clone()).collect();
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT
                worker_id,
                (
                    COALESCE(SUM(
                        CASE
                            WHEN completed_at IS NOT NULL
                                 AND completed_at >= NOW() - INTERVAL '7 days'
                            THEN 3
                            ELSE 1
                        END
                    ), 0)
                    + COALESCE(SUM(cache_hits), 0)
                )::BIGINT AS hit_score
             FROM tasks
             WHERE status = 'COMPLETED'
               AND torrent_source = $1
               AND worker_id = ANY($2)
             GROUP BY worker_id",
        )
        .bind(source)
        .bind(&worker_ids)
        .fetch_all(&self.db.pool)
        .await?;

        let score_map: HashMap<String, i64> = rows.into_iter().collect();
        let original_index: HashMap<String, usize> = workers
            .iter()
            .enumerate()
            .map(|(idx, w)| (w.worker_id.clone(), idx))
            .collect();

        let mut ranked = workers.to_vec();
        ranked.sort_by(|a, b| {
            let a_score = *score_map.get(&a.worker_id).unwrap_or(&0);
            let b_score = *score_map.get(&b.worker_id).unwrap_or(&0);
            b_score.cmp(&a_score).then_with(|| {
                let a_idx = original_index
                    .get(&a.worker_id)
                    .copied()
                    .unwrap_or(usize::MAX);
                let b_idx = original_index
                    .get(&b.worker_id)
                    .copied()
                    .unwrap_or(usize::MAX);
                a_idx.cmp(&b_idx)
            })
        });
        Ok(ranked)
    }

    pub async fn process_timeouts(&self) -> Result<(u64, u64)> {
        let stale = self
            .repo
            .find_stale_dispatched(self.task_timeout_secs)
            .await?;
        let mut redispatched = 0u64;
        let mut failed = 0u64;
        for task in &stale {
            let retry_limit = effective_retry_limit(task, self.max_redispatch);
            if task.worker_id.is_none() || task.retry_count >= retry_limit {
                if let Some(worker_id) = task.worker_id.as_deref() {
                    self.cancel_session_delivery(task);
                    let result = if task.runtime.as_deref().map(str::trim)
                        == Some(MANAGED_GPU_RUNTIME_VERSION)
                    {
                        if let Some(manifest) = task.managed_gpu_manifest_json.as_deref() {
                            self.repo
                                .fail_managed_gpu_without_worker_result_snapshot(
                                    task,
                                    worker_id,
                                    manifest,
                                    ManagedGpuStatus::TimedOut,
                                    "retry_limit_exceeded",
                                    "Max redispatch attempts exceeded",
                                )
                                .await
                        } else {
                            self.repo
                                .quarantine_managed_gpu_without_typed_result_snapshot(
                                    task,
                                    worker_id,
                                    None,
                                    "TIMED_OUT",
                                    "Max redispatch attempts exceeded; managed GPU request manifest is missing",
                                )
                                .await
                        }
                    } else {
                        self.repo
                            .fail_for_worker_snapshot(
                                task,
                                worker_id,
                                "Max redispatch attempts exceeded",
                            )
                            .await
                    };
                    match result {
                        Ok(Some(_)) => {
                            warn!(
                                "Task {} failed after {} retries",
                                task.task_id, task.retry_count
                            );
                            failed += 1;
                        }
                        Ok(None) => warn!(
                            "Task {} changed before its retry-limit failure could be recorded",
                            task.task_id
                        ),
                        Err(e) => error!("Failed to mark task {} as failed: {}", task.task_id, e),
                    }
                } else {
                    match self
                        .repo
                        .terminalize_stale_assignment_without_worker(
                            task,
                            "Stale assignment has no Worker identity",
                        )
                        .await
                    {
                        Ok(Some(_)) => {
                            warn!(
                                "Task {} failed because its stale assignment has no Worker identity",
                                task.task_id
                            );
                            failed += 1;
                        }
                        Ok(None) => warn!(
                            "Task {} changed before its worker-less stale assignment could be terminalized",
                            task.task_id
                        ),
                        Err(e) => error!(
                            "Failed to terminalize worker-less stale task {}: {}",
                            task.task_id,
                            e
                        ),
                    }
                }
            } else if let Some(worker_id) = task.worker_id.as_deref() {
                self.cancel_session_delivery(task);
                let result = if task.runtime.as_deref().map(str::trim)
                    == Some(MANAGED_GPU_RUNTIME_VERSION)
                {
                    if let Some(manifest) = task.managed_gpu_manifest_json.as_deref() {
                        if !managed_gpu_manifest_is_valid(manifest) {
                            self.repo
                                .quarantine_managed_gpu_without_typed_result_snapshot(
                                    task,
                                    worker_id,
                                    Some(manifest),
                                    "FAILED",
                                    "managed GPU request manifest is malformed or invalid",
                                )
                                .await
                                .map(|quarantined| quarantined.map(|_| ()))
                        } else {
                            self.repo
                                .retry_to_pending_for_worker_attempt(
                                    &task.task_id,
                                    worker_id,
                                    task.retry_count,
                                    manifest,
                                    retry_limit,
                                    "Retry limit exceeded while resetting stale managed GPU attempt",
                                )
                                .await
                                .map(|updated| {
                                    updated.and_then(|task| {
                                        (task.status == TaskStatus::Pending).then_some(())
                                    })
                                })
                        }
                    } else {
                        self.repo
                            .quarantine_managed_gpu_without_typed_result_snapshot(
                                task,
                                worker_id,
                                None,
                                "FAILED",
                                "managed GPU request manifest is missing",
                            )
                            .await
                            .map(|quarantined| quarantined.map(|_| ()))
                    }
                } else {
                    self.repo
                        .retry_to_pending_for_worker_snapshot(
                            task,
                            worker_id,
                            retry_limit,
                            "Retry limit exceeded while resetting stale assignment",
                        )
                        .await
                        .map(|updated| {
                            updated
                                .and_then(|task| (task.status == TaskStatus::Pending).then_some(()))
                        })
                };
                match result {
                    Ok(Some(())) => {
                        info!(
                            "Task {} reset to pending (retry {}/{})",
                            task.task_id,
                            task.retry_count + 1,
                            retry_limit
                        );
                        redispatched += 1;
                    }
                    Ok(None) => {
                        warn!(
                            "Task {} was changed before its stale assignment could be reset",
                            task.task_id
                        );
                    }
                    Err(e) => error!("Failed to reset task {}: {}", task.task_id, e),
                }
            } else {
                match self
                    .repo
                    .terminalize_stale_assignment_without_worker(
                        task,
                        "Stale assignment has no Worker identity",
                    )
                    .await
                {
                    Ok(Some(_)) => {
                        warn!(
                            "Task {} failed because its stale assignment has no Worker identity",
                            task.task_id
                        );
                        failed += 1;
                    }
                    Ok(None) => warn!(
                        "Task {} changed before its worker-less stale assignment could be terminalized",
                        task.task_id
                    ),
                    Err(e) => error!(
                        "Failed to terminalize worker-less stale task {}: {}",
                        task.task_id,
                        e
                    ),
                }
            }
        }
        let managed_gpu_timed_out = self.repo.mark_stale_managed_gpu_running().await?;
        let timed_out = self.repo.mark_stale_running().await? + managed_gpu_timed_out;
        if timed_out > 0 {
            warn!("Marked {} running tasks as timed out", timed_out);
        }
        Ok((redispatched, failed))
    }

    pub fn start_registered_dispatch_loop(
        self: Arc<Self>,
        interval: std::time::Duration,
    ) -> watch::Sender<bool> {
        let (tx, mut rx) = watch::channel(false);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        if let Err(e) = self.dispatch_pending_from_registered_workers_and_execute().await { error!("Dispatch loop error: {}", e); }
                    }
                    _ = rx.changed() => { if *rx.borrow() { info!("Dispatch loop shutting down"); break; } }
                }
            }
        });
        tx
    }

    pub fn start_timeout_loop(
        self: Arc<Self>,
        interval: std::time::Duration,
    ) -> watch::Sender<bool> {
        let (tx, mut rx) = watch::channel(false);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        if let Err(e) = self.process_timeouts().await { error!("Timeout loop error: {}", e); }
                    }
                    _ = rx.changed() => { if *rx.borrow() { info!("Timeout loop shutting down"); break; } }
                }
            }
        });
        tx
    }
}

pub fn worker_endpoint(addr: &str) -> Result<String> {
    let addr = addr.trim();
    if addr.starts_with("http://") || addr.starts_with("https://") {
        Ok(addr.to_string())
    } else if addr.strip_prefix("0.0.0.0:").is_some() || addr.strip_prefix("[::]:").is_some() {
        anyhow::bail!(
            "worker address {addr} is a bind address, not a routable endpoint; set WORKER_ADVERTISE_ADDR on the worker"
        );
    } else {
        Ok(format!("http://{addr}"))
    }
}

#[derive(Clone)]
struct ManagedProofDispatch {
    token: String,
    execution_id: String,
    attempt_id: String,
    idempotency_key: String,
    request_digest: String,
    lease_generation: i64,
    deadline_unix_ms: i64,
}

pub fn build_execute_task_request(task: &Task) -> ExecuteTaskRequest {
    build_execute_task_request_with_token(task, String::new())
}

fn build_execute_task_request_with_token(task: &Task, token: String) -> ExecuteTaskRequest {
    build_execute_task_request_with_credentials(task, token, None)
}

fn build_execute_task_request_with_credentials(
    task: &Task,
    token: String,
    managed_proof: Option<&ManagedProofDispatch>,
) -> ExecuteTaskRequest {
    let runtime = task.runtime.as_deref().map(str::trim);
    let is_managed_gpu = runtime == Some(MANAGED_GPU_RUNTIME_VERSION);
    let is_general_compute =
        runtime == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION);
    let identity = if is_general_compute {
        general_compute_identity(task)
    } else {
        None
    };
    let managed_identity = if is_managed_runtime(task.runtime.as_deref()) {
        managed_proof_attempt_identity(task).ok()
    } else {
        None
    };
    let managed_gpu_identity = if is_managed_gpu {
        managed_gpu_identity(task).ok()
    } else {
        None
    };
    let legacy_identity = legacy_execution_identity(task);
    ExecuteTaskRequest {
        task_id: task.task_id.clone(),
        torrent: if is_managed_gpu || is_general_compute {
            String::new()
        } else {
            task.torrent_source.clone().unwrap_or_default()
        },
        resource_limits: Some(ProtoResourceSpec {
            cpu_cores: 0,
            memory_mb: task.req_memory_gb as i64 * 1024,
            gpu_count: 0,
            gpu_name: String::new(),
            vram_mb: task.req_gpu_memory_gb as i64 * 1024,
            cpu_score: task.req_cpu_score,
            gpu_score: task.req_gpu_score,
            storage_total_gb: task.req_storage_gb,
            storage_available_gb: task.req_storage_gb,
        }),
        runtime: runtime.unwrap_or_default().to_owned(),
        task_source: if is_managed_gpu || is_general_compute {
            String::new()
        } else {
            task.task_source.clone().unwrap_or_default()
        },
        token,
        managed_budget_units: if matches!(
            runtime,
            Some("managed-function-v0") | Some("production_sandboxed_dsl")
        ) {
            task.max_cpt.max(0)
        } else {
            0
        },
        general_compute_manifest_json: if is_general_compute {
            task.general_compute_manifest_json
                .clone()
                .unwrap_or_default()
        } else {
            Vec::new()
        },
        managed_gpu_manifest_json: if is_managed_gpu {
            task.managed_gpu_manifest_json.clone().unwrap_or_default()
        } else {
            Vec::new()
        },
        execution_id: if is_managed_gpu {
            managed_gpu_identity
                .as_ref()
                .map(|identity| identity.0.clone())
                .unwrap_or_default()
        } else {
            managed_proof
                .map(|proof| proof.execution_id.clone())
                .or_else(|| managed_identity.as_ref().map(|identity| identity.0.clone()))
                .or_else(|| identity.as_ref().map(|identity| identity.0.clone()))
                .or_else(|| Some(legacy_identity.0.clone()))
                .unwrap_or_default()
        },
        attempt_id: if is_managed_gpu {
            managed_gpu_identity
                .as_ref()
                .map(|identity| identity.1.clone())
                .unwrap_or_default()
        } else {
            managed_proof
                .map(|proof| proof.attempt_id.clone())
                .or_else(|| managed_identity.as_ref().map(|identity| identity.1.clone()))
                .or_else(|| identity.as_ref().map(|identity| identity.1.clone()))
                .or_else(|| Some(legacy_identity.1.clone()))
                .unwrap_or_default()
        },
        idempotency_key: if is_managed_gpu {
            managed_gpu_identity
                .as_ref()
                .map(|identity| identity.2.clone())
                .unwrap_or_default()
        } else {
            managed_proof
                .map(|proof| proof.idempotency_key.clone())
                .or_else(|| managed_identity.as_ref().map(|identity| identity.2.clone()))
                .or_else(|| identity.as_ref().map(|identity| identity.2.clone()))
                .or_else(|| Some(legacy_identity.2.clone()))
                .unwrap_or_default()
        },
        request_digest: if is_managed_gpu {
            managed_gpu_identity
                .as_ref()
                .map(|identity| identity.3.clone())
                .unwrap_or_default()
        } else {
            managed_proof
                .map(|proof| proof.request_digest.clone())
                .or_else(|| identity.as_ref().map(|identity| identity.3.clone()))
                .or_else(|| Some(legacy_identity.3.clone()))
                .unwrap_or_default()
        },
        managed_dsl_backend_id: if runtime == Some("production_sandboxed_dsl") {
            task.managed_dsl_backend_id.clone().unwrap_or_default()
        } else {
            String::new()
        },
        managed_dsl_semantics_manifest_sha256: if runtime == Some("production_sandboxed_dsl") {
            task.managed_dsl_semantics_manifest_sha256
                .clone()
                .unwrap_or_default()
        } else {
            String::new()
        },
        managed_proof_authorization_token: managed_proof
            .map(|proof| proof.token.clone())
            .unwrap_or_default(),
        managed_proof_lease_generation: managed_proof
            .map(|proof| proof.lease_generation)
            .unwrap_or_default(),
        managed_proof_deadline_unix_ms: managed_proof
            .map(|proof| proof.deadline_unix_ms)
            .unwrap_or_default(),
    }
}

fn legacy_execution_identity(task: &Task) -> (String, String, String, String) {
    let stable_task = task.id.simple().to_string();
    let execution_id = format!("legacy-execution-v1:{stable_task}");
    let attempt_id = format!("legacy-attempt-v1:{stable_task}:{}", task.retry_count);
    let idempotency_key = format!("legacy-result-v1:{stable_task}:{}", task.retry_count);
    let request_digest =
        sha256_prefixed(format!("{execution_id}\n{attempt_id}\n{idempotency_key}").as_bytes());
    (execution_id, attempt_id, idempotency_key, request_digest)
}

fn session_compatible_runtime(runtime: Option<&str>) -> bool {
    !is_managed_runtime(runtime)
        && runtime.map(str::trim) != Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
        && runtime.map(str::trim) != Some(MANAGED_GPU_RUNTIME_VERSION)
}

fn general_compute_identity(task: &Task) -> Option<(String, String, String, String)> {
    let manifest = task.general_compute_manifest_json.as_deref()?;
    let request =
        serde_json::from_slice::<general_compute_runtime::GeneralComputeRequest>(manifest).ok()?;
    request.validate().ok()?;
    Some((
        request.execution_id,
        request.attempt_id,
        request.idempotency_key,
        request.request_digest,
    ))
}

fn managed_gpu_identity(task: &Task) -> Result<(String, String, String, String)> {
    let manifest = task
        .managed_gpu_manifest_json
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("managed-function-gpu-v1 request manifest is missing"))?;
    let request = serde_json::from_slice::<ManagedGpuRequest>(manifest)
        .map_err(|error| anyhow::anyhow!("managed GPU request is malformed: {error}"))?;
    request
        .validate()
        .map_err(|error| anyhow::anyhow!("managed GPU request is invalid: {error:?}"))?;
    if u64::try_from(task.max_cpt).ok() != Some(request.reservation_cpt) {
        anyhow::bail!("managed GPU task reservation does not match its request manifest");
    }
    Ok((
        request.execution_id,
        request.attempt_id,
        request.idempotency_key,
        request.request_digest,
    ))
}

fn validate_managed_gpu_response_identity(
    task: &Task,
    response: &ExecuteTaskResponse,
) -> std::result::Result<(), &'static str> {
    if task.runtime.as_deref().map(str::trim) != Some(MANAGED_GPU_RUNTIME_VERSION) {
        return Ok(());
    }
    let Some(manifest) = task.managed_gpu_manifest_json.as_deref() else {
        return Err("managed-function-gpu-v1 request manifest is missing");
    };
    let request = serde_json::from_slice::<ManagedGpuRequest>(manifest)
        .map_err(|_| "managed-function-gpu-v1 request manifest is malformed")?;
    request
        .validate()
        .map_err(|_| "managed-function-gpu-v1 request manifest is invalid")?;
    if response.execution_id != request.execution_id
        || response.attempt_id != request.attempt_id
        || response.idempotency_key != request.idempotency_key
        || response.request_digest != request.request_digest
    {
        return Err("managed GPU response identity does not match the persisted request");
    }
    if response.managed_gpu_result_json.is_empty() {
        return Err("managed GPU response is missing its typed result envelope");
    }
    Ok(())
}

fn validate_general_compute_response_identity(
    task: &Task,
    response: &ExecuteTaskResponse,
) -> std::result::Result<(), &'static str> {
    if task.runtime.as_deref() != Some("general-compute-v1alpha1") {
        return Ok(());
    }
    let Some((execution_id, attempt_id, idempotency_key, request_digest)) =
        general_compute_identity(task)
    else {
        return Err("general-compute request identity is missing or malformed");
    };
    if response.execution_id != execution_id
        || response.attempt_id != attempt_id
        || response.idempotency_key != idempotency_key
        || response.request_digest != request_digest
    {
        return Err("general-compute response identity does not match the persisted request");
    }
    Ok(())
}

fn validate_managed_proof_response_identity(
    task: &Task,
    response: &ExecuteTaskResponse,
    dispatch: Option<&ManagedProofDispatch>,
) -> std::result::Result<(), &'static str> {
    if !is_managed_runtime(task.runtime.as_deref()) {
        return Ok(());
    }
    let Ok((execution_id, attempt_id, idempotency_key)) = managed_proof_attempt_identity(task)
    else {
        return Err("managed proof attempt identity is invalid");
    };
    if response.execution_id != execution_id
        || response.attempt_id != attempt_id
        || response.idempotency_key != idempotency_key
    {
        return Err("managed proof response identity does not match the current task attempt");
    }
    match dispatch {
        Some(dispatch)
            if response.request_digest != dispatch.request_digest
                || response.execution_id != dispatch.execution_id
                || response.attempt_id != dispatch.attempt_id
                || response.idempotency_key != dispatch.idempotency_key =>
        {
            Err("managed proof response identity does not match the dispatched attempt")
        }
        Some(_) => Ok(()),
        None if response.request_digest.is_empty() => Ok(()),
        None => Err("local managed proof response unexpectedly contains a request digest"),
    }
}

fn managed_gpu_manifest_is_valid(manifest: &[u8]) -> bool {
    manifest.len() <= hivemind_proto::MANAGED_GPU_MANIFEST_MAX_BYTES
        && serde_json::from_slice::<ManagedGpuRequest>(manifest)
            .ok()
            .is_some_and(|request| request.validate().is_ok())
}

/// The task-level limit may be lower than the dispatcher safety cap. A corrupt
/// negative task limit is treated as zero so it can only fail closed, never
/// grant an unbounded retry budget.
fn effective_retry_limit(task: &Task, dispatcher_limit: i32) -> i32 {
    dispatcher_limit.max(0).min(task.max_retries.max(0))
}

async fn retry_or_terminalize_without_worker_penalty(
    repo: &TaskRepository,
    task: &Task,
    worker_id: &str,
    dispatcher_limit: i32,
    reason: &str,
) -> Result<()> {
    if task.runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
        return reset_managed_gpu_attempt(repo, task, worker_id, dispatcher_limit, reason).await;
    }

    let retry_limit = effective_retry_limit(task, dispatcher_limit);
    let updated = repo
        .retry_to_pending_for_worker_snapshot(task, worker_id, retry_limit, reason)
        .await?;
    match updated {
        Some(updated) if updated.status == TaskStatus::Pending => {}
        Some(updated) => warn!(
            task_id = %task.task_id,
            worker_id,
            status = updated.status.as_str(),
            "retryable failure exhausted the retry limit and terminalized the task"
        ),
        None => warn!(
            task_id = %task.task_id,
            worker_id,
            "retryable failure arrived after the active attempt changed; leaving current task untouched"
        ),
    }
    Ok(())
}

async fn reset_managed_gpu_attempt(
    repo: &TaskRepository,
    task: &Task,
    worker_id: &str,
    max_redispatch: i32,
    reason: &str,
) -> Result<()> {
    let Some(manifest) = task.managed_gpu_manifest_json.as_deref() else {
        let quarantined = repo
            .quarantine_managed_gpu_without_typed_result_snapshot(
                task, worker_id, None, "FAILED", reason,
            )
            .await?;
        if quarantined.is_some() {
            warn!(
                task_id = %task.task_id,
                worker_id,
                "managed GPU attempt was quarantined because its request manifest is missing"
            );
        } else {
            warn!(
                task_id = %task.task_id,
                worker_id,
                "managed GPU quarantine skipped because the active attempt changed"
            );
        }
        return Ok(());
    };
    if !managed_gpu_manifest_is_valid(manifest) {
        let quarantined = repo
            .quarantine_managed_gpu_without_typed_result_snapshot(
                task,
                worker_id,
                Some(manifest),
                "FAILED",
                "managed GPU request manifest is malformed or invalid",
            )
            .await?;
        if quarantined.is_some() {
            warn!(
                task_id = %task.task_id,
                worker_id,
                "managed GPU attempt was quarantined because its request manifest is malformed or invalid"
            );
        } else {
            warn!(
                task_id = %task.task_id,
                worker_id,
                "managed GPU quarantine skipped because the active attempt changed"
            );
        }
        return Ok(());
    }
    let retry_limit = effective_retry_limit(task, max_redispatch);
    if task.retry_count >= retry_limit {
        let failed = repo
            .fail_managed_gpu_without_worker_result_snapshot(
                task,
                worker_id,
                manifest,
                ManagedGpuStatus::Failed,
                "retry_limit_exceeded",
                reason,
            )
            .await?;
        if failed.is_some() {
            warn!(
                task_id = %task.task_id,
                worker_id,
                retry_count = task.retry_count,
                "managed GPU attempt reached the redispatch limit and was terminally failed"
            );
        } else {
            warn!(
                task_id = %task.task_id,
                worker_id,
                "managed GPU response arrived after the active attempt changed; leaving current task untouched"
            );
        }
        return Ok(());
    }
    let updated = repo
        .retry_to_pending_for_worker_snapshot(task, worker_id, retry_limit, reason)
        .await?;
    match updated {
        Some(updated) if updated.status == TaskStatus::Pending => {}
        Some(updated) => warn!(
            task_id = %task.task_id,
            worker_id,
            status = updated.status.as_str(),
            "managed GPU retry exhausted during reset"
        ),
        None => warn!(
            task_id = %task.task_id,
            worker_id,
            "managed GPU response arrived after the active attempt changed; leaving current task untouched"
        ),
    }
    Ok(())
}

fn same_active_task_attempt(expected: &Task, current: &Task, worker_id: &str) -> bool {
    expected.id == current.id
        && expected.retry_count == current.retry_count
        && current.worker_id.as_deref() == Some(worker_id)
        && matches!(current.status, TaskStatus::Assigned | TaskStatus::Running)
}

const SESSION_WORKER_EXECUTION_TOKEN_TTL_SECS: i64 = 15 * 60;

fn worker_execution_token(
    private_key_pem: &str,
    task: &Task,
    worker_id: &str,
    transfer_generation: Option<i64>,
) -> anyhow::Result<String> {
    worker_execution_token_with_lifetime(
        private_key_pem,
        task,
        worker_id,
        transfer_generation,
        5 * 60,
    )
}

fn worker_execution_token_with_lifetime(
    private_key_pem: &str,
    task: &Task,
    worker_id: &str,
    transfer_generation: Option<i64>,
    lifetime_seconds: i64,
) -> anyhow::Result<String> {
    if private_key_pem.trim().is_empty() {
        anyhow::bail!("WORKER_EXECUTION_PRIVATE_KEY_PEM is required for worker execution dispatch")
    }
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: task.owner.clone(),
        user_id: task.owner.clone(),
        role: Some("worker-execution".into()),
        task_id: Some(task.task_id.clone()),
        worker_id: Some(worker_id.to_string()),
        exp: (now + lifetime_seconds) as usize,
        iat: now as usize,
    };
    let signer = WorkerExecutionSigner::from_pem(private_key_pem)?;
    let runtime = task.runtime.as_deref().map(str::trim);
    if runtime == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION) {
        let (execution_id, attempt_id, idempotency_key, request_digest) =
            general_compute_identity(task)
                .ok_or_else(|| anyhow::anyhow!("general-compute request identity is missing"))?;
        let transfer_generation = transfer_generation
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("general-compute transfer lease is missing"))?;
        signer.encode_execution_claims(
            &claims,
            &WorkerExecutionIdentity {
                execution_id,
                attempt_id,
                idempotency_key,
                request_digest,
                transfer_generation,
            },
        )
    } else if runtime == Some(MANAGED_GPU_RUNTIME_VERSION) {
        let (execution_id, attempt_id, idempotency_key, request_digest) =
            managed_gpu_identity(task)?;
        let attempt_generation = i64::from(task.retry_count)
            .checked_add(1)
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("managed GPU attempt generation is invalid"))?;
        signer.encode_execution_claims(
            &claims,
            &WorkerExecutionIdentity {
                execution_id,
                attempt_id,
                idempotency_key,
                request_digest,
                // GPU-v1 has no general-compute transfer lease. The positive
                // attempt generation only satisfies the shared token schema;
                // Worker admission does not interpret it as a transfer lease.
                transfer_generation: attempt_generation,
            },
        )
    } else {
        signer.encode_claims(&claims)
    }
}

fn worker_session_execution_token_with_lifetime(
    private_key_pem: &str,
    task: &Task,
    worker_id: &str,
    lifetime_seconds: i64,
) -> anyhow::Result<String> {
    if private_key_pem.trim().is_empty() {
        anyhow::bail!("WORKER_EXECUTION_PRIVATE_KEY_PEM is required for worker execution dispatch")
    }
    let (_, attempt_id, _, _) = legacy_execution_identity(task);
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: task.owner.clone(),
        user_id: task.owner.clone(),
        role: Some("worker-execution".into()),
        task_id: Some(task.task_id.clone()),
        worker_id: Some(worker_id.to_string()),
        exp: (now + lifetime_seconds) as usize,
        iat: now as usize,
    };
    WorkerExecutionSigner::from_pem(private_key_pem)?.encode_attempt_claims(&claims, &attempt_id)
}

async fn mint_managed_proof_dispatch(
    repo: &TaskRepository,
    task: &Task,
    worker_id: &str,
    transfer_generation: Option<i64>,
    authorization_private_key_pem: &str,
) -> Result<Option<ManagedProofDispatch>> {
    if !is_managed_runtime(task.runtime.as_deref()) {
        return Ok(None);
    }
    if authorization_private_key_pem.trim().is_empty() {
        anyhow::bail!("managed proof authorization private key is required");
    }

    let runtime = task.runtime.as_deref().unwrap_or_default();
    let capability_snapshot = repo.managed_dsl_capability_snapshot(worker_id).await?;
    if !scheduler::worker_supports_managed_dsl_request(
        capability_snapshot.as_deref(),
        runtime,
        task.managed_dsl_backend_id.as_deref(),
        task.managed_dsl_semantics_manifest_sha256.as_deref(),
        task.max_cpt,
    ) {
        anyhow::bail!("assigned Worker lacks the operator-approved managed DSL capability");
    }

    let lease_generation = if task.runtime.as_deref()
        == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
    {
        transfer_generation
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("managed proof transfer lease is missing"))?
    } else {
        i64::from(task.retry_count)
            .checked_add(1)
            .filter(|generation| *generation > 0)
            .ok_or_else(|| anyhow::anyhow!("managed proof attempt generation is invalid"))?
    };

    let (execution_id, attempt_id, idempotency_key) = managed_proof_attempt_identity(task)?;
    let mut deadline_unix_ms = match repo
        .managed_proof_authorization_deadline(&task.task_id, lease_generation, &attempt_id)
        .await?
    {
        Some(deadline) => deadline,
        None => managed_proof_deadline(task)?.0,
    };
    if deadline_unix_ms <= Utc::now().timestamp_millis() {
        anyhow::bail!("managed proof deadline has expired");
    }

    let signer = ManagedProofAuthorizationSigner::from_pem(authorization_private_key_pem)?;
    // A concurrent first mint can win the task-row lock between the lookup and
    // insert. If that happens, retry once using the persisted deadline; the
    // deadline participates in the canonical request digest and must not be
    // regenerated for the same attempt.
    let mut retried_with_persisted_deadline = false;
    let (request, persisted) = loop {
        let lifetime = managed_proof_lifetime(deadline_unix_ms)?;
        let request = managed_proof_request(
            task,
            worker_id,
            &execution_id,
            &attempt_id,
            &idempotency_key,
            lease_generation,
            deadline_unix_ms,
        )?;
        let candidate_claims = new_claims(
            &request,
            uuid::Uuid::new_v4().to_string(),
            Utc::now(),
            lifetime,
        )?;
        let candidate_token = signer.encode(&candidate_claims)?;
        let image_id_json = serde_json::to_string(&request.image_id)?;
        let result = repo
            .record_managed_proof_authorization(&ManagedProofAuthorizationRecord {
                task_id: request.task_id.clone(),
                protocol_version: request.protocol_version,
                proof_task_id: request.proof_task_id.clone(),
                owner: request.owner.clone(),
                worker_id: request.worker_id.clone(),
                execution_id: request.execution_id.clone(),
                attempt_id: request.attempt_id.clone(),
                idempotency_key: request.idempotency_key.clone(),
                request_digest: request.request_digest.clone(),
                lease_generation: request.lease_generation,
                runtime: request.runtime.clone(),
                backend_id: request.backend_id.clone(),
                semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
                proof_scheme: request.proof_scheme.clone(),
                image_id_json,
                deadline_unix_ms: request.deadline_unix_ms,
                token_jti: candidate_claims.jti.clone(),
                token_iat: i64::try_from(candidate_claims.iat)
                    .map_err(|_| anyhow::anyhow!("managed proof issue time is out of range"))?,
                token_exp: i64::try_from(candidate_claims.exp)
                    .map_err(|_| anyhow::anyhow!("managed proof expiry is out of range"))?,
                token_sha256: sha256_prefixed(candidate_token.as_bytes()),
            })
            .await;
        match result {
            Ok(persisted) => break (request, persisted),
            Err(error) if !retried_with_persisted_deadline => {
                let Some(persisted_deadline) = repo
                    .managed_proof_authorization_deadline(
                        &task.task_id,
                        lease_generation,
                        &attempt_id,
                    )
                    .await?
                else {
                    return Err(error);
                };
                if persisted_deadline == deadline_unix_ms {
                    return Err(error);
                }
                if persisted_deadline <= Utc::now().timestamp_millis() {
                    anyhow::bail!("persisted managed proof deadline has expired");
                }
                deadline_unix_ms = persisted_deadline;
                retried_with_persisted_deadline = true;
            }
            Err(error) => return Err(error),
        }
    };

    let claims = claims_with_issuance(
        &request,
        persisted.token_jti.clone(),
        persisted.token_iat,
        persisted.token_exp,
    )?;
    let token = signer.encode(&claims)?;
    let token_sha256 = sha256_prefixed(token.as_bytes());
    if token_sha256 != persisted.token_sha256 {
        anyhow::bail!(
            "managed proof authorization cannot be regenerated with the configured signing key"
        );
    }

    Ok(Some(ManagedProofDispatch {
        token,
        execution_id,
        attempt_id,
        idempotency_key,
        request_digest: request.request_digest,
        lease_generation,
        deadline_unix_ms: request.deadline_unix_ms,
    }))
}

fn is_managed_runtime(runtime: Option<&str>) -> bool {
    matches!(
        runtime,
        Some("managed-function-v0") | Some("production_sandboxed_dsl")
    )
}

fn managed_proof_attempt_identity(task: &Task) -> Result<(String, String, String)> {
    let attempt_number = u32::try_from(task.retry_count)
        .map_err(|_| anyhow::anyhow!("managed proof retry count is invalid"))?;
    let stable_task = task.id.simple().to_string();
    Ok((
        format!("managed-execution-v1:{stable_task}"),
        format!("managed-attempt-v1:{stable_task}:{attempt_number}"),
        format!("managed-proof-v1:{stable_task}:{attempt_number}"),
    ))
}

fn managed_proof_deadline(task: &Task) -> Result<(i64, ChronoDuration)> {
    let now = Utc::now();
    let rpc_deadline = now
        .checked_add_signed(
            ChronoDuration::from_std(WORKER_EXECUTE_RPC_TIMEOUT)
                .map_err(|_| anyhow::anyhow!("worker execute timeout is invalid"))?,
        )
        .ok_or_else(|| anyhow::anyhow!("managed proof deadline overflowed"))?;
    let deadline = task.deadline.map_or(rpc_deadline, |deadline| {
        if deadline < rpc_deadline {
            deadline
        } else {
            rpc_deadline
        }
    });
    let deadline_unix_ms = deadline.timestamp_millis();
    Ok((deadline_unix_ms, managed_proof_lifetime(deadline_unix_ms)?))
}

fn managed_proof_lifetime(deadline_unix_ms: i64) -> Result<ChronoDuration> {
    let remaining_ms = deadline_unix_ms
        .checked_sub(Utc::now().timestamp_millis())
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| anyhow::anyhow!("managed proof deadline has expired"))?;
    let lifetime_ms = remaining_ms
        .checked_add(999)
        .ok_or_else(|| anyhow::anyhow!("managed proof lifetime overflowed"))?;
    let lifetime_seconds = lifetime_ms / 1_000 + 5;
    Ok(ChronoDuration::seconds(lifetime_seconds))
}

fn managed_proof_request(
    task: &Task,
    worker_id: &str,
    execution_id: &str,
    attempt_id: &str,
    idempotency_key: &str,
    lease_generation: i64,
    deadline_unix_ms: i64,
) -> Result<RemoteManagedProofRequest> {
    let runtime = task
        .runtime
        .as_deref()
        .filter(|runtime| is_managed_runtime(Some(runtime)))
        .ok_or_else(|| anyhow::anyhow!("managed proof runtime is unsupported"))?;
    let proof_task_id = if runtime == "production_sandboxed_dsl" {
        let backend_id = task
            .managed_dsl_backend_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("managed DSL backend identity is missing"))?;
        let semantics_digest = task
            .managed_dsl_semantics_manifest_sha256
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("managed DSL semantics identity is missing"))?;
        dsl_proof_task_id(task.task_id.as_str(), runtime, backend_id, semantics_digest)
    } else {
        task.task_id.clone()
    };
    let sidecar_request = ManagedProverRequest {
        protocol_version: MANAGED_PROVER_PROTOCOL_VERSION,
        task_id: proof_task_id.clone(),
        source: task.task_source.clone().unwrap_or_default(),
        input: task.torrent_source.clone().unwrap_or_default(),
        max_usage_units: u64::try_from(task.max_cpt)
            .map_err(|_| anyhow::anyhow!("managed proof budget is invalid"))?,
    };
    sidecar_request
        .validate()
        .map_err(|error| anyhow::anyhow!("managed proof request is invalid: {error}"))?;

    let request = RemoteManagedProofRequest {
        protocol_version: REMOTE_MANAGED_PROOF_PROTOCOL_VERSION,
        task_id: task.task_id.clone(),
        proof_task_id,
        owner: task.owner.clone(),
        worker_id: worker_id.to_string(),
        execution_id: execution_id.to_string(),
        attempt_id: attempt_id.to_string(),
        idempotency_key: idempotency_key.to_string(),
        request_digest: String::new(),
        lease_generation,
        runtime: runtime.to_string(),
        backend_id: if runtime == "production_sandboxed_dsl" {
            task.managed_dsl_backend_id.clone().unwrap_or_default()
        } else {
            String::new()
        },
        semantics_manifest_sha256: if runtime == "production_sandboxed_dsl" {
            task.managed_dsl_semantics_manifest_sha256
                .clone()
                .unwrap_or_default()
        } else {
            String::new()
        },
        source: sidecar_request.source,
        input: sidecar_request.input,
        max_usage_units: sidecar_request.max_usage_units,
        proof_scheme: RISC0_PROOF_SCHEME.to_string(),
        image_id: RISC0_MANAGED_GUEST_ID,
        deadline_unix_ms,
    };
    request
        .with_computed_digest()
        .map_err(|error| anyhow::anyhow!("managed proof request binding is invalid: {error}"))
}

fn sha256_prefixed(value: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value)))
}

#[derive(Debug, thiserror::Error)]
#[error(
    "Nodepool has no trusted source bytes for artifact {artifact_id} (including CAS-only artifacts)"
)]
struct CasOnlyArtifactUnavailable {
    artifact_id: String,
}

/// Build a test source map from inline fixture bytes. Production dispatch never
/// treats mutable manifest bytes as the source authority.
#[cfg(test)]
fn inline_general_compute_chunk_plan(
    task: &Task,
    token: &str,
) -> anyhow::Result<Vec<GeneralComputeChunkUpload>> {
    let manifest = task
        .general_compute_manifest_json
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("general-compute request manifest is missing"))?;
    let request: general_compute_runtime::GeneralComputeRequest = serde_json::from_slice(manifest)
        .map_err(|error| anyhow::anyhow!("general-compute request is malformed: {error}"))?;
    let source_bytes = std::iter::once(&request.source_artifact)
        .chain(request.input_artifacts.iter())
        .filter_map(|artifact| {
            artifact
                .inline_bytes
                .clone()
                .map(|bytes| (artifact.artifact_id.clone(), bytes))
        })
        .collect();
    general_compute_chunk_plan(task, token, 1, &source_bytes)
}

/// Build authenticated uploads exclusively from Nodepool-owned artifact
/// sources. The source map is never supplied by a Worker or Master; it is
/// loaded from the task-bound persistence layer before dispatch.
fn general_compute_chunk_plan(
    task: &Task,
    token: &str,
    transfer_generation: i64,
    source_bytes: &HashMap<String, Vec<u8>>,
) -> anyhow::Result<Vec<GeneralComputeChunkUpload>> {
    let Some(manifest) = task.general_compute_manifest_json.as_deref() else {
        anyhow::bail!("general-compute request manifest is missing")
    };
    let request: general_compute_runtime::GeneralComputeRequest = serde_json::from_slice(manifest)
        .map_err(|error| anyhow::anyhow!("general-compute request is malformed: {error}"))?;
    request
        .validate()
        .map_err(|error| anyhow::anyhow!("general-compute request is invalid: {error:?}"))?;
    if transfer_generation <= 0 {
        anyhow::bail!("general-compute transfer lease generation must be positive");
    }
    let artifacts = std::iter::once(&request.source_artifact).chain(request.input_artifacts.iter());
    let mut uploads = Vec::new();
    for artifact in artifacts {
        if artifact.chunks.is_empty() {
            continue;
        }
        let bytes = source_bytes.get(&artifact.artifact_id).ok_or_else(|| {
            anyhow::Error::new(CasOnlyArtifactUnavailable {
                artifact_id: artifact.artifact_id.clone(),
            })
        })?;
        if bytes.len() as u64 != artifact.size_bytes
            || general_compute_runtime::sha256_digest(bytes) != artifact.sha256
        {
            anyhow::bail!(
                "Nodepool artifact source does not match manifest for {}",
                artifact.artifact_id
            );
        }
        for chunk in &artifact.chunks {
            let start = usize::try_from(chunk.offset)
                .map_err(|_| anyhow::anyhow!("artifact chunk offset is too large"))?;
            let size = usize::try_from(chunk.size_bytes)
                .map_err(|_| anyhow::anyhow!("artifact chunk size is too large"))?;
            let end = start
                .checked_add(size)
                .ok_or_else(|| anyhow::anyhow!("artifact chunk range overflows"))?;
            let payload = bytes
                .get(start..end)
                .ok_or_else(|| anyhow::anyhow!("artifact chunk range exceeds source bytes"))?;
            let upload = GeneralComputeChunkUpload {
                token: token.to_owned(),
                execution_id: request.execution_id.clone(),
                attempt_id: request.attempt_id.clone(),
                idempotency_key: request.idempotency_key.clone(),
                request_digest: request.request_digest.clone(),
                artifact_id: artifact.artifact_id.clone(),
                offset: chunk.offset as i64,
                size_bytes: chunk.size_bytes as i64,
                sha256: chunk.sha256.clone(),
                bytes: payload.to_vec(),
                transfer_generation,
            };
            validate_general_compute_chunk_upload(&upload)
                .map_err(|message| anyhow::anyhow!(message))?;
            uploads.push(upload);
        }
    }
    Ok(uploads)
}

/// Load the only trusted raw-byte source available to the scheduler for a
/// general-compute attempt. Every artifact, including one originally submitted
/// inline, must be read from the task-bound Nodepool persistence row. The
/// mutable attempt manifest supplies coordinates only; it is never a raw-byte
/// authority after task creation.
async fn load_general_compute_artifact_sources(
    repo: &TaskRepository,
    task: &Task,
) -> anyhow::Result<HashMap<String, Vec<u8>>> {
    let Some(manifest) = task.general_compute_manifest_json.as_deref() else {
        anyhow::bail!("general-compute request manifest is missing")
    };
    let request: general_compute_runtime::GeneralComputeRequest = serde_json::from_slice(manifest)
        .map_err(|error| anyhow::anyhow!("general-compute request is malformed: {error}"))?;
    request
        .validate()
        .map_err(|error| anyhow::anyhow!("general-compute request is invalid: {error:?}"))?;

    let mut sources = HashMap::new();
    for artifact in std::iter::once(&request.source_artifact).chain(request.input_artifacts.iter())
    {
        if !repo
            .general_compute_artifact_coordinates_match(
                &task.task_id,
                &artifact.artifact_id,
                artifact.size_bytes,
                &artifact.sha256,
                &artifact.chunks,
            )
            .await?
        {
            anyhow::bail!(
                "Nodepool artifact coordinates do not match immutable identity for {}",
                artifact.artifact_id
            );
        }
        let bytes = repo
            .general_compute_artifact_bytes(
                &task.task_id,
                &artifact.artifact_id,
                &artifact.sha256,
                artifact.size_bytes,
            )
            .await?
            .ok_or_else(|| {
                anyhow::Error::new(CasOnlyArtifactUnavailable {
                    artifact_id: artifact.artifact_id.clone(),
                })
            })?;
        if bytes.len() as u64 != artifact.size_bytes
            || general_compute_runtime::sha256_digest(&bytes) != artifact.sha256
        {
            anyhow::bail!(
                "Nodepool artifact source does not match manifest for {}",
                artifact.artifact_id
            );
        }
        if sources
            .insert(artifact.artifact_id.clone(), bytes)
            .is_some()
        {
            anyhow::bail!("general-compute manifest contains duplicate artifact id");
        }
    }
    Ok(sources)
}

#[cfg(test)]
async fn prepare_general_compute_on_worker(
    channel: tonic::transport::Channel,
    task: &Task,
    token: &str,
) -> anyhow::Result<()> {
    let manifest = task
        .general_compute_manifest_json
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("general-compute request manifest is missing"))?;
    let request: general_compute_runtime::GeneralComputeRequest = serde_json::from_slice(manifest)
        .map_err(|error| anyhow::anyhow!("general-compute request is malformed: {error}"))?;
    let source_bytes = std::iter::once(&request.source_artifact)
        .chain(request.input_artifacts.iter())
        .filter_map(|artifact| {
            artifact
                .inline_bytes
                .clone()
                .map(|bytes| (artifact.artifact_id.clone(), bytes))
        })
        .collect();
    prepare_general_compute_on_worker_with_sources(channel, task, token, 1, &source_bytes).await
}

async fn prepare_general_compute_on_worker_with_sources(
    channel: tonic::transport::Channel,
    task: &Task,
    token: &str,
    transfer_generation: i64,
    source_bytes: &HashMap<String, Vec<u8>>,
) -> anyhow::Result<()> {
    let uploads = general_compute_chunk_plan(task, token, transfer_generation, source_bytes)?;
    let identity = general_compute_identity(task)
        .ok_or_else(|| anyhow::anyhow!("general-compute request identity is missing"))?;
    let manifest = task
        .general_compute_manifest_json
        .clone()
        .ok_or_else(|| anyhow::anyhow!("general-compute request manifest is missing"))?;
    let mut client = GeneralComputeChunkServiceClient::new(channel)
        .max_encoding_message_size(GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES)
        .max_decoding_message_size(GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES);
    let prepare = GeneralComputePrepareRequest {
        task_id: task.task_id.clone(),
        token: token.to_owned(),
        runtime: general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        general_compute_manifest_json: manifest,
        execution_id: identity.0.clone(),
        attempt_id: identity.1.clone(),
        idempotency_key: identity.2.clone(),
        request_digest: identity.3.clone(),
        transfer_generation,
    };
    validate_general_compute_prepare_request(&prepare)
        .map_err(|message| anyhow::anyhow!(message))?;
    let prepared = client
        .prepare_general_compute(tonic::Request::new(prepare))
        .await?
        .into_inner();
    if !prepared.success
        || prepared.execution_id != identity.0
        || prepared.attempt_id != identity.1
        || prepared.idempotency_key != identity.2
        || prepared.request_digest != identity.3
        || prepared.transfer_generation != transfer_generation
    {
        anyhow::bail!("worker general-compute prepare response did not match the request")
    }
    let mut artifact_ids = Vec::new();
    for upload in &uploads {
        if !artifact_ids.iter().any(|id| id == &upload.artifact_id) {
            artifact_ids.push(upload.artifact_id.clone());
        }
    }
    for artifact_id in artifact_ids {
        let resume = GeneralComputeChunkResumeRequest {
            token: token.to_owned(),
            execution_id: identity.0.clone(),
            attempt_id: identity.1.clone(),
            idempotency_key: identity.2.clone(),
            request_digest: identity.3.clone(),
            artifact_id: artifact_id.clone(),
            transfer_generation,
            completed_sha256: Vec::new(),
        };
        validate_general_compute_chunk_resume_request(&resume)
            .map_err(|message| anyhow::anyhow!(message))?;
        let resume = client
            .resume_chunks(tonic::Request::new(resume))
            .await?
            .into_inner();
        if !resume.success {
            anyhow::bail!("worker general-compute resume response was unsuccessful")
        }
        let artifact_uploads: Vec<_> = uploads
            .iter()
            .filter(|upload| upload.artifact_id == artifact_id)
            .cloned()
            .collect();
        for upload in
            select_missing_general_compute_chunks(&artifact_uploads, &resume.missing_chunks)?
        {
            let response = client
                .upload_chunk(tonic::Request::new(upload))
                .await?
                .into_inner();
            if !response.success || response.accepted_chunks != 1 {
                anyhow::bail!("worker rejected a general-compute artifact chunk")
            }
        }
    }
    Ok(())
}

fn select_missing_general_compute_chunks(
    uploads: &[GeneralComputeChunkUpload],
    missing: &[hivemind_proto::GeneralComputeChunkDescriptor],
) -> anyhow::Result<Vec<GeneralComputeChunkUpload>> {
    let mut selected = Vec::with_capacity(missing.len());
    for descriptor in missing {
        let upload = uploads
            .iter()
            .find(|upload| {
                upload.offset == descriptor.offset
                    && upload.size_bytes == descriptor.size_bytes
                    && upload.sha256 == descriptor.sha256
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "worker requested a chunk that is not present in the Nodepool manifest"
                )
            })?;
        selected.push(upload.clone());
    }
    Ok(selected)
}

#[cfg(test)]
async fn execute_on_worker(
    repo: Arc<TaskRepository>,
    task: Task,
    worker_id: String,
    worker_addr: String,
    worker_execution_private_key_pem: &str,
    managed_proof_rollout_mode: ManagedProofRolloutMode,
) -> Result<()> {
    execute_on_worker_with_managed_proof_key(
        repo,
        task,
        worker_id,
        worker_addr,
        WorkerExecutionOptions {
            worker_execution_private_key_pem: worker_execution_private_key_pem.to_owned(),
            managed_proof_authorization_private_key_pem: String::new(),
            managed_proof_provider_configured: false,
            managed_proof_rollout_mode,
            max_redispatch: i32::MAX,
        },
    )
    .await
}

async fn execute_on_worker_with_managed_proof_key(
    repo: Arc<TaskRepository>,
    task: Task,
    worker_id: String,
    worker_addr: String,
    options: WorkerExecutionOptions,
) -> Result<()> {
    let WorkerExecutionOptions {
        worker_execution_private_key_pem,
        managed_proof_authorization_private_key_pem,
        managed_proof_provider_configured,
        managed_proof_rollout_mode,
        max_redispatch,
    } = options;
    let Some(current_task) = repo.find_by_task_id(&task.task_id).await? else {
        warn!("Task {} disappeared before worker execution", task.task_id);
        return Ok(());
    };
    if current_task.worker_id.as_deref() != Some(worker_id.as_str())
        || !matches!(
            current_task.status,
            TaskStatus::Assigned | TaskStatus::Running
        )
    {
        info!(
            "Skipping worker execution for task {} because it is no longer assigned to worker {}",
            task.task_id, worker_id
        );
        return Ok(());
    }

    let Some(current_task) = repo
        .mark_worker_execution_running_snapshot(&task, &worker_id)
        .await?
    else {
        info!(
            "Skipping worker execution for task {} because its assignment changed before execution started",
            task.task_id
        );
        return Ok(());
    };

    let endpoint = match worker_transport_endpoint(&worker_addr) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            reset_after_worker_rpc_failure(
                repo.as_ref(),
                &current_task,
                &worker_id,
                max_redispatch,
                WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
                &error,
            )
            .await?;
            return Ok(());
        }
    };
    let channel = match endpoint.connect().await {
        Ok(channel) => channel,
        Err(error) => {
            reset_after_worker_rpc_failure(
                repo.as_ref(),
                &current_task,
                &worker_id,
                max_redispatch,
                worker_transport_failure_disposition(&error),
                &error,
            )
            .await?;
            return Ok(());
        }
    };
    let mut client = WorkerNodeServiceClient::new(channel.clone())
        .max_encoding_message_size(WORKER_RPC_MESSAGE_MAX_BYTES)
        .max_decoding_message_size(WORKER_RPC_MESSAGE_MAX_BYTES);
    if !repo
        .refresh_worker_endpoint_snapshot(&current_task, &worker_id, &worker_addr)
        .await?
    {
        info!(
            "Skipping worker execution for task {} because its assignment changed before transport setup",
            task.task_id
        );
        return Ok(());
    }
    let transfer_lease = if current_task.runtime.as_deref()
        == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
    {
        let lease = match repo.general_compute_transfer_lease(&task.task_id).await? {
            Some(lease) => lease,
            None => {
                let error =
                    anyhow::anyhow!("general-compute transfer lease is missing or inactive");
                reset_after_worker_rpc_failure(
                    repo.as_ref(),
                    &current_task,
                    &worker_id,
                    max_redispatch,
                    WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
                    &error,
                )
                .await?;
                return Ok(());
            }
        };
        let (execution_id, attempt_id, _, _) = general_compute_identity(&current_task)
            .ok_or_else(|| anyhow::anyhow!("general-compute request identity is missing"))?;
        if !lease.matches_assignment(&task.task_id, &execution_id, &attempt_id, &worker_id) {
            let error = anyhow::anyhow!(
                "general-compute transfer lease does not match the assigned Worker attempt"
            );
            reset_after_worker_rpc_failure(
                repo.as_ref(),
                &current_task,
                &worker_id,
                max_redispatch,
                WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
                &error,
            )
            .await?;
            return Ok(());
        }
        Some(lease)
    } else {
        None
    };
    let token = match worker_execution_token(
        &worker_execution_private_key_pem,
        &current_task,
        &worker_id,
        transfer_lease.as_ref().map(|lease| lease.generation),
    ) {
        Ok(token) => token,
        Err(error) => {
            let reason = error.to_string();
            // A signing-key/configuration failure is a Nodepool control-plane
            // fault, not evidence against the assigned Worker. Managed GPU
            // attempts use the same bounded retry/typed-terminal path as other
            // Nodepool-owned dispatch failures.
            if current_task.runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
                reset_after_worker_rpc_failure(
                    repo.as_ref(),
                    &current_task,
                    &worker_id,
                    max_redispatch,
                    WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
                    &error,
                )
                .await?;
            } else {
                if repo
                    .fail_for_worker_without_penalty_snapshot(&current_task, &worker_id, &reason)
                    .await?
                    .is_none()
                {
                    warn!(
                        "Task {} token failure arrived after the active attempt changed; leaving current task untouched",
                        task.task_id
                    );
                }
            }
            warn!(
                "Task {} could not create a worker execution token; handling without worker penalty: {}",
                task.task_id, reason
            );
            return Ok(());
        }
    };
    let managed_proof = if managed_proof_provider_configured
        && managed_proof_rollout_mode != ManagedProofRolloutMode::Off
    {
        match mint_managed_proof_dispatch(
            repo.as_ref(),
            &current_task,
            &worker_id,
            transfer_lease.as_ref().map(|lease| lease.generation),
            &managed_proof_authorization_private_key_pem,
        )
        .await
        {
            Ok(dispatch) => dispatch,
            Err(error) => {
                let reason = error.to_string();
                if is_managed_runtime(current_task.runtime.as_deref()) {
                    if managed_proof_dispatch_should_redispatch(&error) {
                        reset_after_worker_rpc_failure(
                            repo.as_ref(),
                            &current_task,
                            &worker_id,
                            max_redispatch,
                            WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
                            &error,
                        )
                        .await?;
                        warn!(
                            "Task {} no longer matches worker {} managed capability; resetting for redispatch: {}",
                            task.task_id, worker_id, reason
                        );
                        return Ok(());
                    }
                    if repo
                        .fail_for_worker_without_penalty_snapshot(
                            &current_task,
                            &worker_id,
                            &reason,
                        )
                        .await?
                        .is_none()
                    {
                        warn!(
                            "Task {} proof authorization failure arrived after the active attempt changed; leaving current task untouched",
                            task.task_id
                        );
                    }
                    warn!(
                        "Task {} could not create managed proof authorization; failing without worker penalty: {}",
                        task.task_id, reason
                    );
                    return Ok(());
                }
                return Err(error);
            }
        }
    } else {
        None
    };
    let general_compute_sources = if current_task.runtime.as_deref()
        == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
    {
        let source_bytes = match load_general_compute_artifact_sources(repo.as_ref(), &current_task)
            .await
        {
            Ok(source_bytes) => source_bytes,
            Err(error) => {
                match general_compute_prepare_failure_disposition(&error) {
                    GeneralComputePrepareFailureDisposition::FailTaskWithoutWorkerPenalty => {
                        let reason = error.to_string();
                        if repo
                            .fail_for_worker_without_penalty_snapshot(
                                &current_task,
                                &worker_id,
                                &reason,
                            )
                            .await?
                            .is_none()
                        {
                            warn!(
                                "Task {} artifact failure arrived after the active attempt changed; leaving current task untouched",
                                task.task_id
                            );
                        }
                        warn!(
                            "Task {} cannot load Nodepool-owned general-compute artifacts for worker {}; failing without worker penalty: {}",
                            task.task_id, worker_id, reason
                        );
                    }
                    GeneralComputePrepareFailureDisposition::RetryWithoutWorkerPenalty => {
                        reset_after_worker_rpc_failure(
                            repo.as_ref(),
                            &current_task,
                            &worker_id,
                            max_redispatch,
                            WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
                            &error,
                        )
                        .await?;
                    }
                }
                return Ok(());
            }
        };
        let generation = transfer_lease
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("general-compute transfer lease is missing"))?
            .generation;
        if let Err(error) = prepare_general_compute_on_worker_with_sources(
            channel.clone(),
            &current_task,
            &token,
            generation,
            &source_bytes,
        )
        .await
        {
            match general_compute_prepare_failure_disposition(&error) {
                GeneralComputePrepareFailureDisposition::FailTaskWithoutWorkerPenalty => {
                    let reason = error.to_string();
                    if repo
                        .fail_for_worker_without_penalty_snapshot(
                            &current_task,
                            &worker_id,
                            &reason,
                        )
                        .await?
                        .is_none()
                    {
                        warn!(
                            "Task {} preparation failure arrived after the active attempt changed; leaving current task untouched",
                            task.task_id
                        );
                    }
                    warn!(
                        "Task {} cannot be prepared for general-compute on worker {}; failing without worker penalty: {}",
                        task.task_id, worker_id, reason
                    );
                }
                GeneralComputePrepareFailureDisposition::RetryWithoutWorkerPenalty => {
                    reset_after_worker_rpc_failure(
                        repo.as_ref(),
                        &current_task,
                        &worker_id,
                        max_redispatch,
                        WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
                        &error,
                    )
                    .await?;
                }
            }
            return Ok(());
        }
        Some(source_bytes)
    } else {
        None
    };
    if let Err(error) = update_managed_proof_dispatch_state(
        repo.as_ref(),
        &task.task_id,
        &worker_id,
        managed_proof.as_ref(),
        "submitted",
    )
    .await
    {
        warn!(
            task_id = %task.task_id,
            error = %error,
            "managed proof authorization was no longer active before submission"
        );
        return Ok(());
    }
    if let Err(error) = update_managed_proof_dispatch_state(
        repo.as_ref(),
        &task.task_id,
        &worker_id,
        managed_proof.as_ref(),
        "running",
    )
    .await
    {
        warn!(
            task_id = %task.task_id,
            error = %error,
            "managed proof authorization was no longer active before worker execution"
        );
        return Ok(());
    }
    let mut request = tonic::Request::new(build_execute_task_request_with_credentials(
        &current_task,
        token,
        managed_proof.as_ref(),
    ));
    request.set_timeout(WORKER_EXECUTE_RPC_TIMEOUT);
    let response = {
        let execute = client.execute_task(request);
        tokio::pin!(execute);
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(20));
        loop {
            tokio::select! {
                response = &mut execute => break response,
                _ = heartbeat.tick() => {
                    match repo
                        .refresh_worker_endpoint_snapshot(&current_task, &worker_id, &worker_addr)
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            warn!(
                                task_id = %task.task_id,
                                "stopping Worker execution because its assignment changed"
                            );
                            return Ok(());
                        }
                        Err(error) => {
                            warn!(
                                task_id = %task.task_id,
                                error = %error,
                                "failed to refresh the running Worker execution heartbeat"
                            );
                        }
                    }
                }
            }
        }
    };
    match response {
        Ok(response) => {
            let response = response.into_inner();
            let Some(response_task) = repo.find_by_task_id(&task.task_id).await? else {
                warn!(
                    "Ignoring a response for task {} because the task no longer exists",
                    task.task_id
                );
                return Ok(());
            };
            if !same_active_task_attempt(&current_task, &response_task, &worker_id) {
                warn!(
                    "Ignoring a stale response for task {} from worker {} because the active attempt changed",
                    task.task_id, worker_id
                );
                return Ok(());
            }
            let current_task = response_task;
            if let Err(reason) = validate_managed_proof_response_identity(
                &current_task,
                &response,
                managed_proof.as_ref(),
            ) {
                retry_or_terminalize_without_worker_penalty(
                    repo.as_ref(),
                    &current_task,
                    &worker_id,
                    max_redispatch,
                    reason,
                )
                .await?;
                warn!(
                    "Task {} returned a managed proof attempt identity mismatch from worker {}; redispatching or terminalizing: {}",
                    task.task_id, worker_id, reason
                );
                return Ok(());
            }
            if let Err(reason) = validate_managed_gpu_response_identity(&current_task, &response) {
                reset_managed_gpu_attempt(
                    repo.as_ref(),
                    &current_task,
                    &worker_id,
                    max_redispatch,
                    "invalid_response_identity",
                )
                .await?;
                warn!(
                    "Task {} returned an invalid managed GPU response from worker {}; redispatching: {}",
                    task.task_id, worker_id, reason
                );
                return Ok(());
            }
            if let Err(reason) =
                validate_general_compute_response_identity(&current_task, &response)
            {
                retry_or_terminalize_without_worker_penalty(
                    repo.as_ref(),
                    &current_task,
                    &worker_id,
                    max_redispatch,
                    reason,
                )
                .await?;
                warn!(
                    "Task {} returned an attempt identity mismatch from worker {}; redispatching or terminalizing: {}",
                    task.task_id, worker_id, reason
                );
                return Ok(());
            }
            if let Err(reason) = validate_worker_response_sizes(&response) {
                let reason = reason.to_string();
                if matches!(
                    current_task.runtime.as_deref().map(str::trim),
                    Some(MANAGED_GPU_RUNTIME_VERSION)
                        | Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
                ) {
                    if current_task.runtime.as_deref().map(str::trim)
                        == Some(MANAGED_GPU_RUNTIME_VERSION)
                    {
                        reset_managed_gpu_attempt(
                            repo.as_ref(),
                            &current_task,
                            &worker_id,
                            max_redispatch,
                            "response_too_large",
                        )
                        .await?;
                    } else {
                        retry_or_terminalize_without_worker_penalty(
                            repo.as_ref(),
                            &current_task,
                            &worker_id,
                            max_redispatch,
                            &reason,
                        )
                        .await?;
                    }
                } else if repo
                    .fail_for_worker_snapshot(&current_task, &worker_id, &reason)
                    .await?
                    .is_none()
                {
                    warn!(
                        "Task {} oversized response arrived after the active attempt changed; leaving current task untouched",
                        task.task_id
                    );
                    return Ok(());
                }
                warn!(
                    "Task {} returned an oversized response from worker {}: {}",
                    task.task_id, worker_id, reason
                );
                return Ok(());
            }
            let managed_gpu_result = if current_task.runtime.as_deref().map(str::trim)
                == Some(MANAGED_GPU_RUNTIME_VERSION)
            {
                let attempt_generation = i64::from(current_task.retry_count)
                    .checked_add(1)
                    .filter(|generation| *generation > 0)
                    .ok_or_else(|| anyhow::anyhow!("managed GPU attempt generation is invalid"))?;
                let binding = match repo
                    .managed_gpu_attempt_binding(
                        &current_task.task_id,
                        &worker_id,
                        attempt_generation,
                    )
                    .await
                {
                    Ok(binding) => binding,
                    Err(error) if is_managed_gpu_binding_integrity_error(&error) => {
                        let reason =
                            format!("managed GPU attempt capability binding is corrupt: {error}");
                        let quarantined = repo
                            .quarantine_managed_gpu_without_typed_result_snapshot(
                                &current_task,
                                &worker_id,
                                current_task.managed_gpu_manifest_json.as_deref(),
                                "FAILED",
                                &reason,
                            )
                            .await?;
                        if quarantined.is_some() {
                            warn!(
                                task_id = %current_task.task_id,
                                worker_id = %worker_id,
                                error = %reason,
                                "managed GPU task was quarantined because its immutable assignment capability binding is corrupt"
                            );
                        } else {
                            warn!(
                                task_id = %current_task.task_id,
                                worker_id = %worker_id,
                                "managed GPU quarantine skipped because the active attempt changed"
                            );
                        }
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
                let Some(binding) = binding else {
                    reset_managed_gpu_attempt(
                        repo.as_ref(),
                        &current_task,
                        &worker_id,
                        max_redispatch,
                        "trusted_capability_missing",
                    )
                    .await?;
                    warn!(
                        "Task {} returned a managed GPU result without an immutable assignment capability binding; redispatching",
                        task.task_id
                    );
                    return Ok(());
                };
                match decode_and_validate_managed_gpu_result(
                    &current_task,
                    &response,
                    &binding.capability_snapshot_json,
                    &binding.selected_gpu,
                ) {
                    Ok(result) => Some(result),
                    Err(reason) => {
                        reset_managed_gpu_attempt(
                            repo.as_ref(),
                            &current_task,
                            &worker_id,
                            max_redispatch,
                            "invalid_typed_result",
                        )
                        .await?;
                        warn!(
                            "Task {} returned an invalid typed managed GPU result from worker {}; redispatching: {}",
                            task.task_id, worker_id, reason
                        );
                        return Ok(());
                    }
                }
            } else {
                None
            };
            let general_compute_result = if current_task.runtime.as_deref()
                == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
            {
                let capability_snapshot =
                    repo.general_compute_capability_snapshot(&worker_id).await?;
                let Some(capability_snapshot) = capability_snapshot else {
                    retry_or_terminalize_without_worker_penalty(
                        repo.as_ref(),
                        &current_task,
                        &worker_id,
                        max_redispatch,
                        "general-compute trusted capability snapshot is missing",
                    )
                    .await?;
                    warn!(
                        "Task {} returned a general-compute result from worker {} without a trusted capability snapshot; redispatching or terminalizing",
                        task.task_id, worker_id
                    );
                    return Ok(());
                };
                match decode_and_validate_general_compute_result(
                    &current_task,
                    &response,
                    &capability_snapshot,
                ) {
                    Ok(result) => {
                        let request = serde_json::from_slice::<
                            general_compute_runtime::GeneralComputeRequest,
                        >(
                            current_task
                                .general_compute_manifest_json
                                .as_deref()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("general-compute request manifest is missing")
                                })?,
                        )
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "general-compute request manifest is malformed: {error}"
                            )
                        })?;
                        let registration = serde_json::from_str::<
                            general_compute_runtime::TrustedWorkerCapabilityRegistration,
                        >(&capability_snapshot)
                        .map_err(|error| {
                            anyhow::anyhow!("trusted capability snapshot is malformed: {error}")
                        })?;
                        let matrix =
                            general_compute_runtime::CapabilityMatrix::new(registration.backends);
                        if let Some(sources) = general_compute_sources.as_ref() {
                            if let Err(reason) = validate_production_input_digest(
                                &request, &result, &matrix, sources,
                            ) {
                                retry_or_terminalize_without_worker_penalty(
                                    repo.as_ref(),
                                    &current_task,
                                    &worker_id,
                                    max_redispatch,
                                    &reason,
                                )
                                .await?;
                                warn!(
                                    "Task {} returned an invalid production input digest from worker {}; redispatching or terminalizing: {}",
                                    task.task_id, worker_id, reason
                                );
                                return Ok(());
                            }
                        }
                        Some(result)
                    }
                    Err(reason) => {
                        retry_or_terminalize_without_worker_penalty(
                            repo.as_ref(),
                            &current_task,
                            &worker_id,
                            max_redispatch,
                            &reason,
                        )
                        .await?;
                        warn!(
                            "Task {} returned an invalid typed general-compute result from worker {}; redispatching or terminalizing: {}",
                            task.task_id, worker_id, reason
                        );
                        return Ok(());
                    }
                }
            } else {
                None
            };
            if response.success {
                if current_task.runtime.as_deref().map(str::trim)
                    == Some(MANAGED_GPU_RUNTIME_VERSION)
                {
                    let _result = managed_gpu_result.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("managed GPU typed result is missing after validation")
                    })?;
                    if repo
                        .complete_managed_gpu_for_worker_snapshot(
                            &current_task,
                            &worker_id,
                            current_task
                                .managed_gpu_manifest_json
                                .as_deref()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("managed GPU request manifest is missing")
                                })?,
                            &response.managed_gpu_result_json,
                        )
                        .await?
                        .is_none()
                    {
                        warn!(
                            task_id = %task.task_id,
                            worker_id = %worker_id,
                            "managed GPU completion arrived after the active attempt changed; leaving current task untouched"
                        );
                        return Ok(());
                    }
                    info!(
                        "Task {} completed managed GPU execution on worker {}",
                        task.task_id, worker_id
                    );
                    return Ok(());
                }
                let managed_proof = match managed_proof_for_completion_with_mode(
                    managed_proof_rollout_mode,
                    &current_task,
                    &response,
                ) {
                    Ok(proof) => proof,
                    Err(reason) => {
                        managed_proof_metrics::record(ManagedProofMetricEvent::Rejected);
                        record_managed_proof_audit(
                            repo.as_ref(),
                            &task.task_id,
                            &worker_id,
                            managed_proof_rollout_mode,
                            "rejected",
                            Some(reason),
                        )
                        .await;
                        if repo
                            .fail_for_worker_snapshot(&current_task, &worker_id, reason)
                            .await?
                            .is_none()
                        {
                            warn!(
                                "Task {} managed proof failure arrived after the active attempt changed; leaving current task untouched",
                                task.task_id
                            );
                            return Ok(());
                        }
                        warn!(
                            "Task {} failed managed proof gate on worker {}: {}",
                            task.task_id, worker_id, reason
                        );
                        return Ok(());
                    }
                };
                let mut observed_verified = false;
                let managed_completion = if let Some(proof) = managed_proof {
                    match resolve_verified_managed_completion(
                        &current_task,
                        &response,
                        verify_managed_proof(proof),
                    )
                    .await
                    {
                        Ok(completion) => {
                            if managed_proof_rollout_mode == ManagedProofRolloutMode::Observe {
                                observed_verified = true;
                                Some(completion)
                            } else {
                                Some(completion)
                            }
                        }
                        Err(error) => {
                            if managed_proof_rollout_mode == ManagedProofRolloutMode::Observe {
                                managed_proof_metrics::record(ManagedProofMetricEvent::Rejected);
                                managed_proof_metrics::record(
                                    ManagedProofMetricEvent::ObserveFallback,
                                );
                                let reason = error.to_string();
                                record_managed_proof_audit(
                                    repo.as_ref(),
                                    &task.task_id,
                                    &worker_id,
                                    managed_proof_rollout_mode,
                                    "observed_rejected",
                                    Some(&reason),
                                )
                                .await;
                                warn!(
                                    task_id = %task.task_id,
                                    error = %error,
                                    "Managed proof observation failed; retaining legacy settlement"
                                );
                                return complete_legacy_worker_result(
                                    repo.as_ref(),
                                    &task,
                                    &worker_id,
                                    &response,
                                )
                                .await;
                            }
                            let disposition = managed_proof_failure_disposition(&error);
                            // Local verifier saturation is the nodepool's own
                            // backpressure, not worker misbehaviour, so it is
                            // counted and audited as a retry rather than a
                            // rejection.
                            let (metric, audit_event) = match disposition {
                                ManagedProofFailureDisposition::RetryWithoutWorkerPenalty => {
                                    (ManagedProofMetricEvent::QueueRetry, "queue_retry")
                                }
                                ManagedProofFailureDisposition::FailWorkerResult => {
                                    (ManagedProofMetricEvent::Rejected, "rejected")
                                }
                            };
                            managed_proof_metrics::record(metric);
                            let reason = error.to_string();
                            record_managed_proof_audit(
                                repo.as_ref(),
                                &task.task_id,
                                &worker_id,
                                managed_proof_rollout_mode,
                                audit_event,
                                Some(&reason),
                            )
                            .await;
                            match disposition {
                                ManagedProofFailureDisposition::RetryWithoutWorkerPenalty => {
                                    retry_or_terminalize_without_worker_penalty(
                                        repo.as_ref(),
                                        &current_task,
                                        &worker_id,
                                        max_redispatch,
                                        &reason,
                                    )
                                    .await?;
                                    warn!(
                                        "Task {} deferred or terminalized because the local managed proof verifier is busy: {}",
                                        task.task_id, reason
                                    );
                                }
                                ManagedProofFailureDisposition::FailWorkerResult => {
                                    if repo
                                        .fail_for_worker_snapshot(
                                            &current_task,
                                            &worker_id,
                                            &reason,
                                        )
                                        .await?
                                        .is_none()
                                    {
                                        warn!(
                                            "Task {} managed proof failure arrived after the active attempt changed; leaving current task untouched",
                                            task.task_id
                                        );
                                        return Ok(());
                                    }
                                    warn!(
                                        "Task {} failed managed proof verification on worker {}: {}",
                                        task.task_id, worker_id, reason
                                    );
                                }
                            }
                            return Ok(());
                        }
                    }
                } else {
                    None
                };
                if let Some(completion) = managed_completion {
                    if observed_verified {
                        if repo
                            .complete_for_worker_observed_verified_snapshot(
                                &current_task,
                                &worker_id,
                                Some(&response.status_message),
                            )
                            .await?
                            .is_none()
                        {
                            warn!(
                                "Task {} completion arrived after the active attempt changed; leaving current task untouched",
                                task.task_id
                            );
                            return Ok(());
                        }
                        managed_proof_metrics::record(ManagedProofMetricEvent::Verified);
                        managed_proof_metrics::record(ManagedProofMetricEvent::ObserveFallback);
                        record_managed_proof_audit(
                            repo.as_ref(),
                            &task.task_id,
                            &worker_id,
                            managed_proof_rollout_mode,
                            "observed_verified",
                            None,
                        )
                        .await;
                    } else {
                        if repo
                            .complete_for_worker_with_managed_receipt_snapshot(
                                &current_task,
                                &worker_id,
                                Some(&response.status_message),
                                completion.usage_units,
                                completion.output_bytes,
                                &completion.claim_json,
                            )
                            .await?
                            .is_none()
                        {
                            warn!(
                                "Task {} completion arrived after the active attempt changed; leaving current task untouched",
                                task.task_id
                            );
                            return Ok(());
                        }
                        managed_proof_metrics::record(ManagedProofMetricEvent::Verified);
                        record_managed_proof_audit(
                            repo.as_ref(),
                            &task.task_id,
                            &worker_id,
                            managed_proof_rollout_mode,
                            "verified",
                            None,
                        )
                        .await;
                    }
                } else if let Some(result) = general_compute_result.as_ref() {
                    if repo
                        .complete_general_compute_for_worker_snapshot(
                            &current_task,
                            &worker_id,
                            current_task
                                .general_compute_manifest_json
                                .as_deref()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("general-compute request manifest is missing")
                                })?,
                            &response.general_compute_result_json,
                            (!result.stdout.is_empty()).then_some(result.stdout.as_str()),
                        )
                        .await?
                        .is_none()
                    {
                        warn!(
                            "Task {} general-compute completion arrived after the active attempt changed; leaving current task untouched",
                            task.task_id
                        );
                        return Ok(());
                    }
                } else if is_managed_runtime(current_task.runtime.as_deref()) {
                    if repo
                        .complete_for_worker_legacy_managed_snapshot(
                            &current_task,
                            &worker_id,
                            Some(&response.status_message),
                        )
                        .await?
                        .is_none()
                    {
                        warn!(
                            "Task {} completion arrived after the active attempt changed; leaving current task untouched",
                            task.task_id
                        );
                        return Ok(());
                    }
                    if managed_proof_rollout_mode == ManagedProofRolloutMode::Observe
                        && response.managed_proof.is_none()
                    {
                        managed_proof_metrics::record(ManagedProofMetricEvent::ObserveFallback);
                        record_managed_proof_audit(
                            repo.as_ref(),
                            &task.task_id,
                            &worker_id,
                            managed_proof_rollout_mode,
                            "observed_missing",
                            Some("Managed proof was not returned by the worker"),
                        )
                        .await;
                    }
                    if managed_proof_rollout_mode != ManagedProofRolloutMode::Enforce {
                        managed_proof_metrics::record(ManagedProofMetricEvent::LegacySettlement);
                        record_managed_proof_audit(
                            repo.as_ref(),
                            &task.task_id,
                            &worker_id,
                            managed_proof_rollout_mode,
                            "legacy_settlement",
                            None,
                        )
                        .await;
                    }
                } else if repo
                    .complete_for_worker_snapshot(
                        &current_task,
                        &worker_id,
                        None,
                        Some(&response.status_message),
                    )
                    .await?
                    .is_none()
                {
                    warn!(
                        "Task {} completion arrived after the active attempt changed; leaving current task untouched",
                        task.task_id
                    );
                    return Ok(());
                }
                info!("Task {} completed by worker {}", task.task_id, worker_id);
            } else {
                if current_task.runtime.as_deref().map(str::trim)
                    == Some(MANAGED_GPU_RUNTIME_VERSION)
                {
                    let result = managed_gpu_result.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("managed GPU typed result is missing after validation")
                    })?;
                    let reason = result
                        .error_code
                        .as_deref()
                        .filter(|message| !message.trim().is_empty())
                        .unwrap_or("managed GPU execution failed");
                    if repo
                        .fail_managed_gpu_for_worker_snapshot(
                            &current_task,
                            &worker_id,
                            current_task
                                .managed_gpu_manifest_json
                                .as_deref()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("managed GPU request manifest is missing")
                                })?,
                            &response.managed_gpu_result_json,
                            reason,
                        )
                        .await?
                        .is_none()
                    {
                        warn!(
                            task_id = %task.task_id,
                            worker_id = %worker_id,
                            "managed GPU failure arrived after the active attempt changed; leaving current task untouched"
                        );
                        return Ok(());
                    }
                    warn!(
                        "Task {} failed typed managed GPU execution on worker {}: {}",
                        task.task_id, worker_id, reason
                    );
                    return Ok(());
                }
                if let Some(result) = general_compute_result.as_ref() {
                    let reason = result
                        .error_code
                        .as_deref()
                        .filter(|message| !message.trim().is_empty())
                        .unwrap_or("general-compute execution failed");
                    if repo
                        .fail_general_compute_for_worker_snapshot(
                            &current_task,
                            &worker_id,
                            current_task
                                .general_compute_manifest_json
                                .as_deref()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("general-compute request manifest is missing")
                                })?,
                            &response.general_compute_result_json,
                            reason,
                        )
                        .await?
                        .is_none()
                    {
                        warn!(
                            "Task {} general-compute failure arrived after the active attempt changed; leaving current task untouched",
                            task.task_id
                        );
                        return Ok(());
                    }
                    warn!(
                        "Task {} failed typed general-compute execution on worker {}: {}",
                        task.task_id, worker_id, reason
                    );
                    return Ok(());
                }
                if repo
                    .fail_for_worker_snapshot(&current_task, &worker_id, &response.status_message)
                    .await?
                    .is_none()
                {
                    warn!(
                        "Task {} failure arrived after the active attempt changed; leaving current task untouched",
                        task.task_id
                    );
                    return Ok(());
                }
                warn!(
                    "Task {} failed on worker {}: {}",
                    task.task_id, worker_id, response.status_message
                );
            }
        }
        Err(e) => {
            let disposition = worker_rpc_failure_disposition(&e);
            reset_after_worker_rpc_failure(
                repo.as_ref(),
                &current_task,
                &worker_id,
                max_redispatch,
                disposition,
                &e,
            )
            .await?;
        }
    }
    Ok(())
}

async fn complete_legacy_worker_result(
    repo: &TaskRepository,
    task: &Task,
    worker_id: &str,
    response: &ExecuteTaskResponse,
) -> Result<()> {
    if repo
        .complete_for_worker_legacy_managed_snapshot(
            task,
            worker_id,
            Some(&response.status_message),
        )
        .await?
        .is_none()
    {
        warn!(
            task_id = %task.task_id,
            worker_id = %worker_id,
            "legacy managed completion arrived after the active attempt changed; leaving current task untouched"
        );
        return Ok(());
    }
    managed_proof_metrics::record(ManagedProofMetricEvent::LegacySettlement);
    record_managed_proof_audit(
        repo,
        &task.task_id,
        worker_id,
        ManagedProofRolloutMode::Observe,
        "legacy_settlement",
        None,
    )
    .await;
    info!(
        task_id = %task.task_id,
        worker_id = %worker_id,
        "Managed proof observation retained legacy settlement"
    );
    Ok(())
}

async fn update_managed_proof_dispatch_state(
    repo: &TaskRepository,
    task_id: &str,
    worker_id: &str,
    dispatch: Option<&ManagedProofDispatch>,
    state: &str,
) -> Result<()> {
    let Some(dispatch) = dispatch else {
        return Ok(());
    };
    let update = ManagedProofAuthorizationStateUpdate {
        task_id,
        lease_generation: dispatch.lease_generation,
        attempt_id: &dispatch.attempt_id,
        worker_id,
        execution_id: &dispatch.execution_id,
        idempotency_key: &dispatch.idempotency_key,
        request_digest: &dispatch.request_digest,
        state,
    };
    repo.update_managed_proof_authorization_state(&update).await
}

async fn record_managed_proof_audit(
    repo: &TaskRepository,
    task_id: &str,
    worker_id: &str,
    rollout_mode: ManagedProofRolloutMode,
    event: &str,
    reason: Option<&str>,
) {
    let detail = serde_json::json!({
        "event": event,
        "worker_id": worker_id,
        "rollout_mode": rollout_mode.as_str(),
        "reason": reason,
    });
    tracing::info!(
        target: "hivemind::proof_audit",
        task_id,
        worker_id,
        rollout_mode = rollout_mode.as_str(),
        event,
        reason = reason.unwrap_or_default(),
        "managed proof audit event"
    );
    if let Err(error) = sqlx::query(
        "INSERT INTO admin_audit_logs (admin_user, action, target_type, target_id, detail)
         VALUES ('system', 'managed_proof_verification', 'task', $1, $2)",
    )
    .bind(task_id)
    .bind(sqlx::types::Json(detail))
    .execute(&repo.pool)
    .await
    {
        tracing::warn!(
            target: "hivemind::proof_audit",
            task_id,
            error = %error,
            "failed to persist managed proof audit event"
        );
    }
}

const WORKER_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const WORKER_EXECUTE_RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);

fn worker_transport_endpoint(worker_addr: &str) -> Result<tonic::transport::Endpoint> {
    Ok(
        tonic::transport::Endpoint::from_shared(worker_endpoint(worker_addr)?)?
            .connect_timeout(WORKER_CONNECT_TIMEOUT)
            .timeout(WORKER_EXECUTE_RPC_TIMEOUT),
    )
}

#[cfg(test)]
fn managed_proof_for_completion<'a>(
    task: &Task,
    response: &'a ExecuteTaskResponse,
) -> std::result::Result<Option<&'a ManagedProofEnvelope>, &'static str> {
    managed_proof_for_completion_with_mode(
        hivemind_config::ManagedProofRolloutMode::Enforce,
        task,
        response,
    )
}

fn managed_proof_for_completion_with_mode<'a>(
    rollout_mode: hivemind_config::ManagedProofRolloutMode,
    task: &Task,
    response: &'a ExecuteTaskResponse,
) -> std::result::Result<Option<&'a ManagedProofEnvelope>, &'static str> {
    if !matches!(
        task.runtime.as_deref(),
        Some("managed-function-v0") | Some("production_sandboxed_dsl")
    ) {
        return Ok(None);
    }

    match rollout_mode {
        hivemind_config::ManagedProofRolloutMode::Off => Ok(None),
        hivemind_config::ManagedProofRolloutMode::Observe => Ok(response.managed_proof.as_ref()),
        hivemind_config::ManagedProofRolloutMode::Enforce => response
            .managed_proof
            .as_ref()
            .map(Some)
            .ok_or("Managed proof is required"),
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[allow(clippy::enum_variant_names)]
enum WorkerResponseSizeError {
    #[error("Worker status message exceeds the application limit")]
    StatusMessageTooLarge,
    #[error("Worker legacy managed receipt exceeds the application limit")]
    LegacyReceiptTooLarge,
    #[error("Worker typed general-compute result exceeds the application limit")]
    GeneralComputeResultTooLarge,
    #[error("Worker typed managed GPU result exceeds the application limit")]
    ManagedGpuResultTooLarge,
}

fn validate_worker_response_sizes(
    response: &ExecuteTaskResponse,
) -> std::result::Result<(), WorkerResponseSizeError> {
    if response.status_message.len() > WORKER_STATUS_MESSAGE_MAX_BYTES {
        return Err(WorkerResponseSizeError::StatusMessageTooLarge);
    }
    if response.managed_receipt_json.len() > LEGACY_MANAGED_RECEIPT_MAX_BYTES {
        return Err(WorkerResponseSizeError::LegacyReceiptTooLarge);
    }
    if response.general_compute_result_json.len() > GENERAL_COMPUTE_RESULT_MAX_BYTES {
        return Err(WorkerResponseSizeError::GeneralComputeResultTooLarge);
    }
    if response.managed_gpu_result_json.len() > MANAGED_GPU_RESULT_MAX_BYTES {
        return Err(WorkerResponseSizeError::ManagedGpuResultTooLarge);
    }
    Ok(())
}

fn decode_and_validate_general_compute_result(
    task: &Task,
    response: &ExecuteTaskResponse,
    capability_snapshot_json: &str,
) -> std::result::Result<general_compute_runtime::GeneralComputeResult, String> {
    if response.general_compute_result_json.is_empty() {
        return Err("general-compute typed result is missing".into());
    }
    let request = task
        .general_compute_manifest_json
        .as_deref()
        .ok_or_else(|| "general-compute request manifest is missing".to_string())
        .and_then(|manifest| {
            serde_json::from_slice::<general_compute_runtime::GeneralComputeRequest>(manifest)
                .map_err(|error| format!("general-compute request is malformed: {error}"))
        })?;
    let registration = serde_json::from_str::<
        general_compute_runtime::TrustedWorkerCapabilityRegistration,
    >(capability_snapshot_json)
    .map_err(|error| format!("trusted capability snapshot is malformed: {error}"))?;
    let trusted_gpu_selection = registration
        .select_gpu_for_request(&request)
        .map_err(|error| format!("trusted GPU selection failed: {error:?}"))?;
    let matrix = general_compute_runtime::CapabilityMatrix::new(registration.backends);
    matrix
        .validate_request(&request, &registration.worker)
        .map_err(|error| {
            format!("request no longer matches trusted capability snapshot: {error:?}")
        })?;
    let result = serde_json::from_slice::<general_compute_runtime::GeneralComputeResult>(
        &response.general_compute_result_json,
    )
    .map_err(|error| format!("general-compute typed result is malformed: {error}"))?;
    result
        .validate_against(&request, &matrix)
        .map_err(|error| format!("general-compute typed result failed validation: {error:?}"))?;
    if result.gpu_selection != trusted_gpu_selection {
        return Err(
            "general-compute typed result GPU selection does not match trusted GPU selection"
                .into(),
        );
    }
    let expected_success = result.status == general_compute_runtime::ResultStatus::Completed;
    if response.success != expected_success {
        return Err("worker success flag does not match typed result status".into());
    }
    Ok(result)
}

fn decode_and_validate_managed_gpu_result(
    task: &Task,
    response: &ExecuteTaskResponse,
    capability_snapshot_json: &str,
    assignment_gpu: &ManagedGpuCapability,
) -> std::result::Result<ManagedGpuResult, String> {
    if response.managed_gpu_result_json.is_empty() {
        return Err("managed GPU typed result is missing".into());
    }
    let request = task
        .managed_gpu_manifest_json
        .as_deref()
        .ok_or_else(|| "managed GPU request manifest is missing".to_string())
        .and_then(|manifest| {
            serde_json::from_slice::<ManagedGpuRequest>(manifest)
                .map_err(|error| format!("managed GPU request is malformed: {error}"))
        })?;
    request
        .validate()
        .map_err(|error| format!("managed GPU request is invalid: {error:?}"))?;
    if u64::try_from(task.max_cpt).ok() != Some(request.reservation_cpt) {
        return Err("managed GPU task reservation does not match its request manifest".into());
    }
    let registration = serde_json::from_str::<
        general_compute_runtime::TrustedWorkerCapabilityRegistration,
    >(capability_snapshot_json)
    .map_err(|error| format!("trusted capability snapshot is malformed: {error}"))?;
    let trusted_gpu = registration
        .select_managed_gpu_for_request(&request)
        .map_err(|error| format!("trusted managed GPU selection failed: {error:?}"))?;
    if trusted_gpu != *assignment_gpu {
        return Err("managed GPU assignment device no longer matches its trusted snapshot".into());
    }
    let result = serde_json::from_slice::<ManagedGpuResult>(&response.managed_gpu_result_json)
        .map_err(|error| format!("managed GPU typed result is malformed: {error}"))?;
    result
        .validate_against(&request, &registration)
        .map_err(|error| format!("managed GPU typed result failed validation: {error:?}"))?;
    if result.selected_gpu != *assignment_gpu {
        return Err("managed GPU typed result selected a different assignment device".into());
    }
    let expected_success = result.status == ManagedGpuStatus::Completed;
    if response.success != expected_success {
        return Err("worker success flag does not match managed GPU typed result status".into());
    }
    Ok(result)
}

/// is tied to the exact raw bytes that Nodepool loaded from its immutable
/// artifact source rows. Reference-direct results retain their legacy digest
/// semantics because that adapter predates the production runner protocol.
fn validate_production_input_digest(
    request: &general_compute_runtime::GeneralComputeRequest,
    result: &general_compute_runtime::GeneralComputeResult,
    capabilities: &general_compute_runtime::CapabilityMatrix,
    sources: &HashMap<String, Vec<u8>>,
) -> Result<(), String> {
    let backend = capabilities
        .backends
        .iter()
        .find(|backend| backend.backend_id == request.backend_id)
        .ok_or_else(|| "production input digest backend is not registered".to_string())?;
    if backend.execution_mode
        != general_compute_runtime::sandbox::BackendExecutionMode::ProductionSandboxedOci
        || result.status != general_compute_runtime::ResultStatus::Completed
    {
        return Ok(());
    }

    let source = sources
        .get(&request.source_artifact.artifact_id)
        .ok_or_else(|| "production input digest source bytes are unavailable".to_string())?;
    if source.len() as u64 != request.source_artifact.size_bytes
        || general_compute_runtime::sha256_digest(source) != request.source_artifact.sha256
    {
        return Err("production input digest source bytes do not match the manifest".into());
    }
    let mut inputs = Vec::with_capacity(request.input_artifacts.len());
    for artifact in &request.input_artifacts {
        let bytes = sources.get(&artifact.artifact_id).ok_or_else(|| {
            format!(
                "production input digest bytes are unavailable for {}",
                artifact.artifact_id
            )
        })?;
        if bytes.len() as u64 != artifact.size_bytes
            || general_compute_runtime::sha256_digest(bytes) != artifact.sha256
        {
            return Err(format!(
                "production input digest bytes do not match the manifest for {}",
                artifact.artifact_id
            ));
        }
        inputs.push(bytes.as_slice());
    }
    let expected = general_compute_runtime::canonical_input_digest(source, &inputs);
    if result.input_sha256 != expected {
        return Err(
            "production input digest does not match Nodepool-owned materialized bytes".into(),
        );
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct VerifiedManagedCompletion {
    usage_units: i64,
    output_bytes: i64,
    claim_json: String,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum ManagedCompletionError {
    #[error("Managed task source is required")]
    MissingSource,
    #[error("Managed task budget must be positive")]
    InvalidBudget,
    #[error("Managed proof claim does not match the task")]
    ClaimBinding(#[source] ClaimError),
    #[error("Managed DSL receipt identity does not match the task")]
    DslReceiptBinding,
    #[error("Managed proof usage is outside the supported range")]
    UsageOutOfRange,
    #[error("Managed proof output length is outside the supported range")]
    OutputBytesOutOfRange,
    #[error("Managed proof claim could not be encoded")]
    ClaimEncoding,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum ManagedProofGateError {
    #[error("Managed proof verification failed")]
    Verifier(#[source] ManagedProofVerifierError),
    #[error("Managed proof claim failed task binding")]
    Completion(#[source] ManagedCompletionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedProofFailureDisposition {
    RetryWithoutWorkerPenalty,
    FailWorkerResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerRpcFailureDisposition {
    RetryAfterResourceExhaustion,
    RetryWithoutWorkerPenalty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneralComputePrepareFailureDisposition {
    RetryWithoutWorkerPenalty,
    FailTaskWithoutWorkerPenalty,
}

fn general_compute_prepare_failure_disposition(
    error: &anyhow::Error,
) -> GeneralComputePrepareFailureDisposition {
    if error.downcast_ref::<CasOnlyArtifactUnavailable>().is_some() {
        GeneralComputePrepareFailureDisposition::FailTaskWithoutWorkerPenalty
    } else {
        GeneralComputePrepareFailureDisposition::RetryWithoutWorkerPenalty
    }
}

fn worker_rpc_failure_disposition(error: &tonic::Status) -> WorkerRpcFailureDisposition {
    match error.code() {
        tonic::Code::ResourceExhausted => WorkerRpcFailureDisposition::RetryAfterResourceExhaustion,
        _ => WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
    }
}

fn worker_transport_failure_disposition(
    _error: &tonic::transport::Error,
) -> WorkerRpcFailureDisposition {
    WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty
}

async fn reset_after_worker_rpc_failure(
    repo: &TaskRepository,
    task: &Task,
    worker_id: &str,
    max_redispatch: i32,
    disposition: WorkerRpcFailureDisposition,
    error: &(impl std::fmt::Display + ?Sized),
) -> Result<()> {
    if task.runtime.as_deref().map(str::trim) == Some(MANAGED_GPU_RUNTIME_VERSION) {
        let Some(manifest) = task.managed_gpu_manifest_json.as_deref() else {
            let quarantined = repo
                .quarantine_managed_gpu_without_typed_result_snapshot(
                    task,
                    worker_id,
                    None,
                    "FAILED",
                    "managed GPU request manifest is missing",
                )
                .await?;
            if quarantined.is_some() {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    "managed GPU task was quarantined because its request manifest is missing"
                );
            } else {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    "managed GPU quarantine skipped because the active attempt changed"
                );
            }
            return Ok(());
        };
        if !managed_gpu_manifest_is_valid(manifest) {
            let quarantined = repo
                .quarantine_managed_gpu_without_typed_result_snapshot(
                    task,
                    worker_id,
                    Some(manifest),
                    "FAILED",
                    "managed GPU request manifest is malformed or invalid",
                )
                .await?;
            if quarantined.is_some() {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    "managed GPU task was quarantined because its request manifest is malformed or invalid"
                );
            } else {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    "managed GPU quarantine skipped because the active attempt changed"
                );
            }
            return Ok(());
        }
        let retry_limit = effective_retry_limit(task, max_redispatch);
        if task.retry_count >= retry_limit {
            let status = match disposition {
                WorkerRpcFailureDisposition::RetryAfterResourceExhaustion => {
                    ManagedGpuStatus::ResourceExhausted
                }
                WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty => ManagedGpuStatus::Failed,
            };
            let failed = repo
                .fail_managed_gpu_without_worker_result_snapshot(
                    task,
                    worker_id,
                    manifest,
                    status,
                    "worker_rpc_retry_limit",
                    "Worker RPC retry limit exceeded",
                )
                .await?;
            if failed.is_some() {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    retry_count = task.retry_count,
                    "managed GPU task reached the redispatch limit after a Worker RPC failure"
                );
            } else {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    "managed GPU retry-limit failure arrived after the active attempt changed; leaving current task untouched"
                );
            }
            return Ok(());
        }
        let updated = repo
            .retry_to_pending_for_worker_snapshot(
                task,
                worker_id,
                retry_limit,
                "Worker RPC retry limit exceeded while resetting managed GPU task",
            )
            .await?;
        if let Some(updated) = updated {
            if updated.status != TaskStatus::Pending {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    status = updated.status.as_str(),
                    "managed GPU RPC retry exhausted during reset"
                );
            }
        } else {
            warn!(
                task_id = %task.task_id,
                worker_id,
                "managed GPU RPC failure arrived after the active attempt changed; leaving current task untouched"
            );
        }
    } else {
        let retry_limit = effective_retry_limit(task, max_redispatch);
        if task.retry_count >= retry_limit {
            let failed = repo
                .fail_for_worker_without_penalty_snapshot(
                    task,
                    worker_id,
                    "Worker RPC retry limit exceeded",
                )
                .await?;
            if failed.is_none() {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    "Worker RPC retry-limit failure arrived after the active attempt changed; leaving current task untouched"
                );
            }
            return Ok(());
        }
        let updated = repo
            .retry_to_pending_for_worker_snapshot(
                task,
                worker_id,
                retry_limit,
                "Worker RPC retry limit exceeded while resetting task",
            )
            .await?;
        if let Some(updated) = updated {
            if updated.status != TaskStatus::Pending {
                warn!(
                    task_id = %task.task_id,
                    worker_id,
                    status = updated.status.as_str(),
                    "Worker RPC retry exhausted during reset"
                );
            }
        } else {
            warn!(
                task_id = %task.task_id,
                worker_id,
                "Worker RPC failure arrived after the active attempt changed; leaving current task untouched"
            );
        }
    }
    match disposition {
        WorkerRpcFailureDisposition::RetryAfterResourceExhaustion => {
            warn!(
                "Task {} was rejected because worker {} is resource exhausted; resetting for redispatch: {}",
                task.task_id, worker_id, error
            );
        }
        WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty => {
            warn!(
                "Task {} could not be sent to worker {}; resetting for redispatch without worker penalty: {}",
                task.task_id, worker_id, error
            );
        }
    }
    Ok(())
}

fn managed_proof_dispatch_should_redispatch(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .contains("assigned Worker lacks the operator-approved managed DSL capability")
}

fn managed_proof_failure_disposition(
    error: &ManagedProofGateError,
) -> ManagedProofFailureDisposition {
    match error {
        ManagedProofGateError::Verifier(
            ManagedProofVerifierError::QueueFull | ManagedProofVerifierError::QueueDeadlineExceeded,
        ) => ManagedProofFailureDisposition::RetryWithoutWorkerPenalty,
        ManagedProofGateError::Verifier(_) | ManagedProofGateError::Completion(_) => {
            ManagedProofFailureDisposition::FailWorkerResult
        }
    }
}

async fn resolve_verified_managed_completion(
    task: &Task,
    response: &ExecuteTaskResponse,
    verification: impl Future<Output = std::result::Result<ExecutionClaim, ManagedProofVerifierError>>,
) -> std::result::Result<VerifiedManagedCompletion, ManagedProofGateError> {
    let claim = verification
        .await
        .map_err(ManagedProofGateError::Verifier)?;
    verified_managed_completion(task, response, &claim).map_err(ManagedProofGateError::Completion)
}

fn verified_managed_completion(
    task: &Task,
    response: &ExecuteTaskResponse,
    claim: &ExecutionClaim,
) -> std::result::Result<VerifiedManagedCompletion, ManagedCompletionError> {
    let source = task
        .task_source
        .as_deref()
        .filter(|source| !source.trim().is_empty())
        .ok_or(ManagedCompletionError::MissingSource)?;
    let input = task
        .torrent_source
        .as_deref()
        .filter(|input| !input.trim().is_empty())
        .unwrap_or("null");
    let max_usage_units = u64::try_from(task.max_cpt)
        .ok()
        .filter(|budget| *budget > 0)
        .ok_or(ManagedCompletionError::InvalidBudget)?;

    if task.runtime.as_deref() == Some("production_sandboxed_dsl") {
        let receipt: serde_json::Value = serde_json::from_str(&response.managed_receipt_json)
            .map_err(|_| ManagedCompletionError::DslReceiptBinding)?;
        let object = receipt
            .as_object()
            .ok_or(ManagedCompletionError::DslReceiptBinding)?;
        if object.get("runtime").and_then(serde_json::Value::as_str) != Some("managed-function-v0")
            || object
                .get("execution_mode")
                .and_then(serde_json::Value::as_str)
                != Some("production_sandboxed_dsl")
            || object.get("backend_id").and_then(serde_json::Value::as_str)
                != task.managed_dsl_backend_id.as_deref()
            || object
                .get("semantics_manifest_sha256")
                .and_then(serde_json::Value::as_str)
                != task.managed_dsl_semantics_manifest_sha256.as_deref()
        {
            return Err(ManagedCompletionError::DslReceiptBinding);
        }
    }

    let expected_proof_task_id = if task.runtime.as_deref() == Some("production_sandboxed_dsl") {
        let backend_id = task
            .managed_dsl_backend_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ManagedCompletionError::DslReceiptBinding)?;
        let semantics_digest = task
            .managed_dsl_semantics_manifest_sha256
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ManagedCompletionError::DslReceiptBinding)?;
        dsl_proof_task_id(
            &task.task_id,
            "production_sandboxed_dsl",
            backend_id,
            semantics_digest,
        )
    } else {
        task.task_id.clone()
    };

    claim
        .validate_bindings(
            &expected_proof_task_id,
            source.as_bytes(),
            input.as_bytes(),
            response.status_message.as_bytes(),
            max_usage_units,
        )
        .map_err(ManagedCompletionError::ClaimBinding)?;

    Ok(VerifiedManagedCompletion {
        usage_units: i64::try_from(claim.usage_units)
            .map_err(|_| ManagedCompletionError::UsageOutOfRange)?,
        output_bytes: i64::try_from(claim.output_bytes)
            .map_err(|_| ManagedCompletionError::OutputBytesOutOfRange)?,
        claim_json: serde_json::to_string(claim)
            .map_err(|_| ManagedCompletionError::ClaimEncoding)?,
    })
}

fn reserve_worker_for_batch(workers: &mut [WorkerNode], worker_id: &str) {
    if let Some(worker) = workers
        .iter_mut()
        .find(|worker| worker.worker_id == worker_id)
    {
        worker.status = hivemind_models::WorkerStatus::Busy;
        worker.queue_capacity = worker.queue_capacity.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use general_compute_runtime::{
        canonical_artifact_root,
        managed_gpu::{
            ManagedGpuBackendRegistration, ManagedGpuCapability, ManagedGpuLimits,
            ManagedGpuProofPolicy, ManagedGpuRequest, ManagedGpuRequirement, ManagedGpuResult,
            ManagedGpuStatus, MANAGED_GPU_BILLING_VERSION, MANAGED_GPU_COST_MODEL_VERSION,
            MANAGED_GPU_OPERATION_REGISTRY_VERSION, MANAGED_GPU_REQUEST_PROTOCOL_VERSION,
            MANAGED_GPU_RUNTIME_VERSION, MANAGED_GPU_SEMANTICS_MANIFEST_SHA256,
            MANAGED_GPU_SETTLEMENT_BASIS,
        },
        sha256_digest, ArtifactManifest, ArtifactRole, BackendRegistration, DeterminismPolicy,
        EvidenceEnvelope, ExecutionPolicy, GeneralComputeRequest, GeneralComputeResult,
        ResultStatus, TrustedWorkerCapabilityRegistration, UsageClaim, WorkerCapabilities,
        GENERAL_COMPUTE_RUNTIME_VERSION,
    };
    use hivemind_config::ManagedProofRolloutMode;
    use hivemind_managed_proof::{
        ClaimError, ExecutionClaim, ExecutionMetrics, COST_MODEL_ID, MANAGED_RUNTIME_ID,
        PROOF_PROTOCOL_VERSION,
    };
    use hivemind_models::{TaskStatus, WorkerStatus};
    use hivemind_proto::{
        general_compute_chunk_service_server::{
            GeneralComputeChunkService, GeneralComputeChunkServiceServer,
        },
        worker_node_service_server::{WorkerNodeService, WorkerNodeServiceServer},
        ExecuteTaskRequest, ExecuteTaskResponse, GeneralComputeChunkDescriptor,
        GeneralComputeChunkResumeRequest, GeneralComputeChunkResumeResponse,
        GeneralComputeChunkUpload, GeneralComputeChunkUploadResponse, GeneralComputePrepareRequest,
        GeneralComputePrepareResponse, StopTaskExecutionRequest, StopTaskExecutionResponse,
        TaskOutputRequest, TaskOutputResponse, TaskOutputUploadRequest, TaskOutputUploadResponse,
        TaskResultUploadRequest, TaskResultUploadResponse, TaskUsageRequest, TaskUsageResponse,
    };
    use std::net::SocketAddr;
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;
    use tokio::sync::oneshot;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request, Response, Status};

    fn dispatcher_db_lock() -> Arc<tokio::sync::Mutex<()>> {
        static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
        LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    async fn test_db(
        test_name: &str,
    ) -> Option<(
        hivemind_database::DatabaseManager,
        hivemind_database::postgres::IsolatedTestPool,
    )> {
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
        let db = hivemind_database::DatabaseManager {
            pool: fixture.pool.clone(),
        };
        Some((db, fixture))
    }

    #[tokio::test]
    async fn dispatcher_loads_artifact_bytes_only_from_nodepool_repository() {
        let Some((db, fixture)) = test_db("dispatcher_nodepool_artifact_source").await else {
            return;
        };
        let repo = TaskRepository::new(db.pool.clone());
        let task_id = format!("dispatcher-source-{}", uuid::Uuid::new_v4());
        let bytes = b"trusted-source".to_vec();
        let mut request = alpha_result_request();
        request.source_artifact = ArtifactManifest {
            artifact_id: "source".into(),
            role: ArtifactRole::Source,
            size_bytes: bytes.len() as u64,
            mime_type: "text/plain".into(),
            sha256: sha256_digest(&bytes),
            chunks: vec![general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(&bytes),
            }],
            inline_bytes: Some(bytes.clone()),
        };
        request.request_digest = request.canonical_request_digest();
        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        request.source_artifact.inline_bytes = None;
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let sources = load_general_compute_artifact_sources(&repo, &task)
            .await
            .unwrap();

        assert_eq!(sources.get("source"), Some(&bytes));
        fixture.cleanup().await.ok();
    }

    #[test]
    fn general_compute_chunk_plan_binds_nodepool_source_and_generation() {
        let mut task = make_task("general-compute-source-plan", TaskStatus::Assigned, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let bytes = b"source".to_vec();
        let mut request = alpha_result_request();
        request.source_artifact.inline_bytes = None;
        request.source_artifact.chunks = vec![general_compute_runtime::ArtifactChunk {
            offset: 0,
            size_bytes: bytes.len() as u64,
            sha256: sha256_digest(&bytes),
        }];
        request.source_artifact.size_bytes = bytes.len() as u64;
        request.source_artifact.sha256 = sha256_digest(&bytes);
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let mut sources = HashMap::new();
        sources.insert(request.source_artifact.artifact_id.clone(), bytes.clone());

        let uploads = general_compute_chunk_plan(&task, "nodepool-token", 7, &sources).unwrap();

        assert_eq!(uploads.len(), 1);
        let upload = &uploads[0];
        assert_eq!(upload.token, "nodepool-token");
        assert_eq!(upload.execution_id, request.execution_id);
        assert_eq!(upload.attempt_id, request.attempt_id);
        assert_eq!(upload.idempotency_key, request.idempotency_key);
        assert_eq!(upload.request_digest, request.request_digest);
        assert_eq!(upload.artifact_id, "source");
        assert_eq!(upload.bytes, bytes);
        assert_eq!(upload.sha256, request.source_artifact.chunks[0].sha256);
        assert_eq!(upload.transfer_generation, 7);
    }

    #[test]
    fn general_compute_chunk_plan_rejects_nodepool_source_drift() {
        let mut task = make_task("general-compute-source-drift", TaskStatus::Assigned, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let expected = b"source".to_vec();
        let mut request = alpha_result_request();
        request.source_artifact.inline_bytes = None;
        request.source_artifact.chunks = vec![general_compute_runtime::ArtifactChunk {
            offset: 0,
            size_bytes: expected.len() as u64,
            sha256: sha256_digest(&expected),
        }];
        request.source_artifact.size_bytes = expected.len() as u64;
        request.source_artifact.sha256 = sha256_digest(&expected);
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let mut sources = HashMap::new();
        sources.insert(
            request.source_artifact.artifact_id.clone(),
            b"sourcf".to_vec(),
        );

        let error = general_compute_chunk_plan(&task, "nodepool-token", 7, &sources)
            .expect_err("source bytes that drift from the immutable manifest must fail closed");

        assert!(error.to_string().contains("does not match manifest"));
    }

    #[tokio::test]
    async fn nodepool_prepare_binds_generation_and_uploads_only_missing_chunks() {
        let mut task = make_task("general-compute-client-transport", TaskStatus::Assigned, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let bytes = b"sources".to_vec();
        let mut request = alpha_result_request();
        request.source_artifact.inline_bytes = None;
        request.source_artifact.chunks = vec![
            general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: 6,
                sha256: sha256_digest(b"source"),
            },
            general_compute_runtime::ArtifactChunk {
                offset: 6,
                size_bytes: 1,
                sha256: sha256_digest(b"s"),
            },
        ];
        request.source_artifact.size_bytes = bytes.len() as u64;
        request.source_artifact.sha256 = sha256_digest(&bytes);
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        let mut sources = HashMap::new();
        sources.insert(request.source_artifact.artifact_id.clone(), bytes);

        let missing = &request.source_artifact.chunks[1];
        let (addr, mut prepares, mut resumes, mut uploads) =
            fake_general_compute_chunk_server(vec![GeneralComputeChunkDescriptor {
                offset: missing.offset as i64,
                size_bytes: missing.size_bytes as i64,
                sha256: missing.sha256.clone(),
            }])
            .await
            .unwrap();
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();

        prepare_general_compute_on_worker_with_sources(
            channel,
            &task,
            "nodepool-token",
            7,
            &sources,
        )
        .await
        .unwrap();

        let prepared = prepares.recv().await.unwrap();
        assert_eq!(prepared.task_id, task.task_id);
        assert_eq!(prepared.token, "nodepool-token");
        assert_eq!(prepared.runtime, GENERAL_COMPUTE_RUNTIME_VERSION);
        assert_eq!(
            prepared.general_compute_manifest_json.as_slice(),
            task.general_compute_manifest_json.as_deref().unwrap()
        );
        assert_eq!(prepared.execution_id, request.execution_id);
        assert_eq!(prepared.attempt_id, request.attempt_id);
        assert_eq!(prepared.idempotency_key, request.idempotency_key);
        assert_eq!(prepared.request_digest, request.request_digest);
        assert_eq!(prepared.transfer_generation, 7);

        let resumed = resumes.recv().await.unwrap();
        assert_eq!(resumed.token, "nodepool-token");
        assert_eq!(resumed.artifact_id, request.source_artifact.artifact_id);
        assert_eq!(resumed.transfer_generation, 7);

        let uploaded = uploads.recv().await.unwrap();
        assert_eq!(uploaded.offset, 6);
        assert_eq!(uploaded.bytes, b"s");
        assert_eq!(uploaded.transfer_generation, 7);
        assert!(uploads.try_recv().is_err());
    }

    #[test]
    fn worker_cannot_request_a_chunk_outside_the_nodepool_manifest() {
        let upload = GeneralComputeChunkUpload {
            token: "nodepool-token".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            artifact_id: "source".into(),
            offset: 0,
            size_bytes: 6,
            sha256: sha256_digest(b"source"),
            bytes: b"source".to_vec(),
            transfer_generation: 7,
        };
        let missing = GeneralComputeChunkDescriptor {
            offset: 6,
            size_bytes: 1,
            sha256: sha256_digest(b"untrusted"),
        };

        let error = select_missing_general_compute_chunks(&[upload], &[missing])
            .expect_err("an untrusted Worker cannot widen the Nodepool upload manifest");

        assert!(error
            .to_string()
            .contains("not present in the Nodepool manifest"));
    }
    #[tokio::test]
    async fn scheduler_rejects_manifest_inline_bytes_without_a_matching_nodepool_source() {
        let Some((db, fixture)) = test_db("dispatcher_source_boundary").await else {
            return;
        };
        let repo = TaskRepository::new(db.pool.clone());
        let task_id = format!("dispatcher-source-boundary-{}", uuid::Uuid::new_v4());
        let original = b"trusted-source".to_vec();
        let mut request = GeneralComputeRequest {
            execution_id: "execution-source-boundary".into(),
            attempt_id: "attempt-source-boundary".into(),
            idempotency_key: "idempotency-source-boundary".into(),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                &original,
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.source_artifact.inline_bytes = None;
        request.request_digest = request.canonical_request_digest();
        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        request.source_artifact.inline_bytes = Some(original);
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let error = load_general_compute_artifact_sources(&repo, &task)
            .await
            .expect_err("manifest inline bytes must not bypass Nodepool source persistence");
        assert!(error.to_string().contains("CAS-only"));

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn scheduler_rejects_attempt_chunk_coordinates_that_drift_from_immutable_artifact() {
        let Some((db, fixture)) = test_db("dispatcher_immutable_artifact_coordinates").await else {
            return;
        };
        let repo = TaskRepository::new(db.pool.clone());
        let task_id = format!("dispatcher-immutable-coordinates-{}", uuid::Uuid::new_v4());
        let bytes = b"immutable-source".to_vec();
        let digest = sha256_digest(&bytes);
        let mut request = GeneralComputeRequest {
            execution_id: "execution-immutable-coordinates".into(),
            attempt_id: "attempt-immutable-coordinates".into(),
            idempotency_key: "idempotency-immutable-coordinates".into(),
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
                inline_bytes: Some(bytes.clone()),
            },
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        repo.create(&task).await.unwrap();

        let mut drifted = request.clone();
        let split = bytes.len() / 2;
        drifted.source_artifact.chunks = vec![
            general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: split as u64,
                sha256: sha256_digest(&bytes[..split]),
            },
            general_compute_runtime::ArtifactChunk {
                offset: split as u64,
                size_bytes: (bytes.len() - split) as u64,
                sha256: sha256_digest(&bytes[split..]),
            },
        ];
        drifted.request_digest = drifted.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&drifted).unwrap());
        let error = load_general_compute_artifact_sources(&repo, &task)
            .await
            .expect_err("attempt coordinates must remain bound to immutable artifact identity");
        assert!(error.to_string().contains("coordinates"));

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[test]
    fn effective_retry_limit_is_the_lower_nonnegative_budget() {
        let mut task = make_task("retry-limit", TaskStatus::Assigned, 0);
        task.max_retries = 5;
        assert_eq!(effective_retry_limit(&task, 2), 2);

        task.max_retries = 1;
        assert_eq!(effective_retry_limit(&task, 2), 1);

        task.max_retries = -1;
        assert_eq!(effective_retry_limit(&task, 2), 0);
        assert_eq!(effective_retry_limit(&task, -1), 0);
    }

    fn make_task(id: &str, status: TaskStatus, retry_count: i32) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            task_id: id.into(),
            owner: "example-user".into(),
            worker_id: None,
            worker_ip: None,
            status,
            status_message: None,
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
            req_memory_gb: 4,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
            retry_count,
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

    #[test]
    fn build_execute_task_request_forwards_managed_runtime_source() {
        let mut task = make_task("managed-dispatch", TaskStatus::Pending, 0);
        task.runtime = Some("managed-function-v0".into());
        task.task_source = Some("return get(input, \"value\") + 1;".into());
        task.torrent_source = Some("{\"value\": 41}".into());

        let request = build_execute_task_request(&task);

        assert_eq!(request.runtime, "managed-function-v0");
        assert_eq!(request.task_source, "return get(input, \"value\") + 1;");
        assert_eq!(request.managed_budget_units, 1_000);
        assert_eq!(request.torrent, "{\"value\": 41}");
    }

    #[test]
    fn managed_proof_attempt_identity_is_stable_per_retry() {
        let mut task = make_task("managed-attempt-identity", TaskStatus::Pending, 0);
        let first = managed_proof_attempt_identity(&task).expect("attempt identity");
        assert!(first.0.starts_with("managed-execution-v1:"));
        assert!(first.1.ends_with(":0"));
        assert!(first.2.ends_with(":0"));

        task.retry_count = 1;
        let second = managed_proof_attempt_identity(&task).expect("retry identity");
        assert_eq!(first.0, second.0);
        assert_ne!(first.1, second.1);
        assert_ne!(first.2, second.2);
        assert!(second.1.ends_with(":1"));
        assert!(second.2.ends_with(":1"));
    }

    #[test]
    fn managed_proof_request_binds_production_dsl_identity_and_digest() {
        let mut task = make_task("managed-request-binding", TaskStatus::Pending, 0);
        task.runtime = Some("production_sandboxed_dsl".into());
        task.task_source = Some("return get(input, \"value\");".into());
        task.torrent_source = Some(r#"{"value": 41}"#.into());
        task.managed_dsl_backend_id = Some("managed-default".into());
        task.managed_dsl_semantics_manifest_sha256 =
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into());

        let request = managed_proof_request(
            &task,
            "worker-binding",
            "execution-binding",
            "attempt-binding",
            "idempotency-binding",
            7,
            Utc::now().timestamp_millis() + 60_000,
        )
        .expect("canonical managed proof request");

        assert_eq!(
            request.proof_task_id,
            hivemind_managed_proof::dsl_proof_task_id(
                &task.task_id,
                "production_sandboxed_dsl",
                "managed-default",
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
        );
        assert_eq!(request.request_digest, request.compute_digest().unwrap());
        assert!(request.validate().is_ok());

        let mut changed = request;
        changed.max_usage_units += 1;
        assert!(changed.validate().is_err());
    }

    #[test]
    fn managed_proof_credentials_fill_execute_identity_fields() {
        let task = make_task("managed-credentials", TaskStatus::Pending, 0);
        let dispatch = ManagedProofDispatch {
            token: "proof-token".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest:
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            lease_generation: 3,
            deadline_unix_ms: 4_000_000_000_000,
        };

        let request = build_execute_task_request_with_credentials(
            &task,
            "worker-token".into(),
            Some(&dispatch),
        );

        assert_eq!(request.token, "worker-token");
        assert_eq!(request.managed_proof_authorization_token, "proof-token");
        assert_eq!(request.execution_id, "execution-1");
        assert_eq!(request.attempt_id, "attempt-1");
        assert_eq!(request.idempotency_key, "idempotency-1");
        assert_eq!(request.request_digest, dispatch.request_digest);
        assert_eq!(request.managed_proof_lease_generation, 3);
        assert_eq!(request.managed_proof_deadline_unix_ms, 4_000_000_000_000);
    }

    #[test]
    fn managed_proof_deadline_rejects_expired_tasks() {
        let mut task = make_task("managed-expired", TaskStatus::Pending, 0);
        task.deadline = Some(Utc::now() - chrono::Duration::seconds(1));
        let error = managed_proof_deadline(&task).expect_err("expired task must fail closed");
        assert!(error.to_string().contains("expired"));
    }

    #[test]
    fn build_execute_task_request_forwards_general_compute_manifest_without_prefix_hack() {
        let mut task = make_task("general-compute-dispatch", TaskStatus::Pending, 0);
        task.runtime = Some("general-compute-v1alpha1".into());
        task.general_compute_manifest_json =
            Some(br#"{"runtime_version":"general-compute-v1alpha1"}"#.to_vec());

        let request = build_execute_task_request(&task);

        assert_eq!(request.task_source, "");
        assert_eq!(
            request.general_compute_manifest_json,
            br#"{"runtime_version":"general-compute-v1alpha1"}"#
        );
    }

    #[test]
    fn build_execute_task_request_forwards_general_compute_attempt_identity() {
        let mut task = make_task("general-compute-attempt-dispatch", TaskStatus::Pending, 0);
        task.runtime = Some("general-compute-v1alpha1".into());
        let mut manifest = GeneralComputeRequest {
            execution_id: "execution-1".into(),
            attempt_id: "attempt-2".into(),
            idempotency_key: "idempotency-1".into(),
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
        manifest.request_digest = manifest.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&manifest).unwrap());

        let request = build_execute_task_request(&task);

        assert_eq!(request.execution_id, "execution-1");
        assert_eq!(request.attempt_id, "attempt-2");
        assert_eq!(request.idempotency_key, "idempotency-1");
        assert_eq!(request.request_digest, manifest.request_digest);
    }

    #[test]
    fn inline_general_compute_chunk_plan_rejects_cas_only_artifacts() {
        let mut task = make_task("general-compute-cas-only", TaskStatus::Assigned, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let mut request = alpha_result_request();
        request.source_artifact.inline_bytes = None;
        request.source_artifact.chunks = vec![general_compute_runtime::ArtifactChunk {
            offset: 0,
            size_bytes: 6,
            sha256: sha256_digest(b"source"),
        }];
        request.source_artifact.size_bytes = 6;
        request.source_artifact.sha256 = sha256_digest(b"source");
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let error = inline_general_compute_chunk_plan(&task, "token").unwrap_err();
        assert!(error.to_string().contains("CAS-only"));
    }

    #[test]
    fn general_compute_chunk_plan_accepts_a_verified_nodepool_source() {
        let mut task = make_task("general-compute-persisted-source", TaskStatus::Assigned, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let bytes = b"source".to_vec();
        let mut request = alpha_result_request();
        request.source_artifact.inline_bytes = None;
        request.source_artifact.chunks = vec![general_compute_runtime::ArtifactChunk {
            offset: 0,
            size_bytes: bytes.len() as u64,
            sha256: sha256_digest(&bytes),
        }];
        request.source_artifact.size_bytes = bytes.len() as u64;
        request.source_artifact.sha256 = sha256_digest(&bytes);
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let mut source_bytes = HashMap::new();
        source_bytes.insert("source".to_string(), bytes.clone());
        let uploads = general_compute_chunk_plan(&task, "token", 1, &source_bytes).unwrap();
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].bytes, bytes);
        assert_eq!(uploads[0].sha256, request.source_artifact.sha256);
    }

    #[test]
    fn inline_general_compute_chunk_plan_binds_each_manifest_chunk() {
        let mut task = make_task("general-compute-inline-chunks", TaskStatus::Assigned, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let mut request = alpha_result_request();
        let bytes = b"source".to_vec();
        request.source_artifact.inline_bytes = Some(bytes.clone());
        request.source_artifact.chunks = vec![general_compute_runtime::ArtifactChunk {
            offset: 0,
            size_bytes: bytes.len() as u64,
            sha256: sha256_digest(&bytes),
        }];
        request.source_artifact.size_bytes = bytes.len() as u64;
        request.source_artifact.sha256 = sha256_digest(&bytes);
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let plan = inline_general_compute_chunk_plan(&task, "token").unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].token, "token");
        assert_eq!(plan[0].artifact_id, "source");
        assert_eq!(plan[0].bytes, bytes);
        assert_eq!(plan[0].request_digest, request.request_digest);
    }

    #[test]
    fn worker_missing_chunk_response_filters_uploads_to_manifest_descriptors() {
        let mut task = make_task("general-compute-resume-filter", TaskStatus::Assigned, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let bytes = b"sources".to_vec();
        let mut request = alpha_result_request();
        request.source_artifact.inline_bytes = Some(bytes.clone());
        request.source_artifact.chunks = vec![
            general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: 6,
                sha256: sha256_digest(b"source"),
            },
            general_compute_runtime::ArtifactChunk {
                offset: 6,
                size_bytes: 1,
                sha256: sha256_digest(b"s"),
            },
        ];
        request.source_artifact.size_bytes = bytes.len() as u64;
        request.source_artifact.sha256 = sha256_digest(&bytes);
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let uploads = inline_general_compute_chunk_plan(&task, "token").unwrap();
        let selected = select_missing_general_compute_chunks(
            &uploads,
            &[hivemind_proto::GeneralComputeChunkDescriptor {
                offset: 6,
                size_bytes: 1,
                sha256: sha256_digest(b"s"),
            }],
        )
        .unwrap();

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].offset, 6);
        assert_eq!(selected[0].bytes, b"s");
    }

    #[tokio::test]
    async fn nodepool_prepare_client_uploads_inline_chunks_before_execution() {
        let mut task = make_task("general-compute-client-transport", TaskStatus::Assigned, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let bytes = b"source".to_vec();
        let mut request = alpha_result_request();
        request.source_artifact.inline_bytes = Some(bytes.clone());
        request.source_artifact.chunks = vec![general_compute_runtime::ArtifactChunk {
            offset: 0,
            size_bytes: bytes.len() as u64,
            sha256: sha256_digest(&bytes),
        }];
        request.source_artifact.size_bytes = bytes.len() as u64;
        request.source_artifact.sha256 = sha256_digest(&bytes);
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let (addr, mut prepares, mut resumes, mut uploads) =
            fake_worker_execute_and_chunk_server().await.unwrap();
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        prepare_general_compute_on_worker(channel, &task, "nodepool-token")
            .await
            .expect("Nodepool should prepare and upload inline chunks");

        let prepared = prepares.recv().await.unwrap();
        assert_eq!(prepared.task_id, task.task_id);
        assert_eq!(prepared.execution_id, request.execution_id);
        assert_eq!(prepared.attempt_id, request.attempt_id);
        assert_eq!(prepared.request_digest, request.request_digest);
        let resumed = resumes.recv().await.unwrap();
        assert_eq!(resumed.artifact_id, request.source_artifact.artifact_id);
        assert_eq!(resumed.execution_id, request.execution_id);
        let uploaded = uploads.recv().await.unwrap();
        assert_eq!(uploaded.token, "nodepool-token");
        assert_eq!(uploaded.artifact_id, "source");
        assert_eq!(uploaded.bytes, bytes);
        assert_eq!(uploaded.sha256, request.source_artifact.chunks[0].sha256);
    }

    #[test]
    fn general_compute_response_identity_must_match_persisted_request_manifest() {
        let mut task = make_task("general-compute-response-identity", TaskStatus::Running, 0);
        task.runtime = Some("general-compute-v1alpha1".into());
        let mut manifest = GeneralComputeRequest {
            execution_id: "execution-1".into(),
            attempt_id: "attempt-2".into(),
            idempotency_key: "idempotency-1".into(),
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
        manifest.request_digest = manifest.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&manifest).unwrap());

        let matching = ExecuteTaskResponse {
            execution_id: manifest.execution_id.clone(),
            attempt_id: manifest.attempt_id.clone(),
            idempotency_key: manifest.idempotency_key.clone(),
            request_digest: manifest.request_digest.clone(),
            ..ExecuteTaskResponse::default()
        };
        assert!(validate_general_compute_response_identity(&task, &matching).is_ok());

        let mismatched = ExecuteTaskResponse {
            attempt_id: "old-attempt".into(),
            ..matching
        };
        let error = validate_general_compute_response_identity(&task, &mismatched)
            .expect_err("a stale attempt result must fail closed");
        assert_eq!(
            error,
            "general-compute response identity does not match the persisted request"
        );
    }

    #[tokio::test]
    async fn test_execute_on_worker_ignores_stale_general_compute_response_without_settlement() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_stale_general_compute_response").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("dispatch-stale-owner-{unique}");
        let worker_id = format!("dispatch-stale-worker-{unique}");
        let task_id = format!("dispatch-stale-task-{unique}");
        let mut manifest = GeneralComputeRequest {
            execution_id: format!("execution-{unique}"),
            attempt_id: "attempt-current".into(),
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
        manifest.request_digest = manifest.canonical_request_digest();
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "stale output".into(),
            execution_id: manifest.execution_id.clone(),
            attempt_id: "attempt-old".into(),
            idempotency_key: manifest.idempotency_key.clone(),
            request_digest: manifest.request_digest.clone(),
            ..ExecuteTaskResponse::default()
        };
        let (worker_addr, mut execute_rx) =
            match fake_worker_execute_server_with_response(response).await {
                Some(parts) => parts,
                None => return,
            };

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '10.0.0.3', 4, 16)",
        )
        .bind(&worker_id)
        .bind(format!("provider-{unique}"))
        .execute(&db.pool)
        .await
        .unwrap();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.owner = username.clone();
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(serde_json::to_vec(&manifest).unwrap());
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();

        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();
        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            &private_key,
            ManagedProofRolloutMode::Enforce,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), execute_rx.recv())
                .await
                .unwrap()
                .as_deref(),
            Some(task_id.as_str())
        );
        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());
        let redispatched: GeneralComputeRequest =
            serde_json::from_slice(stored.general_compute_manifest_json.as_deref().unwrap())
                .unwrap();
        assert_ne!(redispatched.attempt_id, manifest.attempt_id);
        assert!(stored.output.is_none());
        assert!(!stored.billing_settled);

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn execute_on_worker_prepares_authenticated_chunks_before_execution() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_authenticated_chunk_sequence").await else {
            return;
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-transfer-task-{unique}");
        let worker_id = format!("dispatch-transfer-worker-{unique}");
        let bytes = b"trusted-source".to_vec();
        let mut manifest = alpha_result_request();
        manifest.source_artifact = ArtifactManifest {
            artifact_id: "source".into(),
            role: ArtifactRole::Source,
            size_bytes: bytes.len() as u64,
            mime_type: "text/plain".into(),
            sha256: sha256_digest(&bytes),
            chunks: vec![general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(&bytes),
            }],
            inline_bytes: Some(bytes.clone()),
        };
        manifest.request_digest = manifest.canonical_request_digest();
        let (worker_addr, mut calls) =
            authenticated_general_compute_worker_server(GeneralComputeChunkDescriptor {
                offset: 0,
                size_bytes: bytes.len() as i64,
                sha256: sha256_digest(&bytes),
            })
            .await
            .unwrap();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&manifest).unwrap());
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();
        let lease = dispatcher
            .repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .unwrap();
        let (private_key, public_key) = hivemind_config::generate_worker_execution_test_key_pair();

        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            &private_key,
            ManagedProofRolloutMode::Enforce,
        )
        .await
        .unwrap();

        let mut observed = Vec::new();
        for _ in 0..4 {
            observed.push(
                tokio::time::timeout(Duration::from_secs(1), calls.recv())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        let TransportCall::Prepare(prepared) = &observed[0] else {
            panic!("first call must prepare the authenticated transfer");
        };
        assert_eq!(prepared.task_id, task_id);
        assert_eq!(prepared.transfer_generation, lease.generation);
        let claims =
            hivemind_auth::worker_execution::WorkerExecutionVerifier::from_pem(&public_key)
                .unwrap()
                .decode_execution_claims(&prepared.token)
                .unwrap();
        assert_eq!(claims.claims.worker_id.as_deref(), Some(worker_id.as_str()));
        assert_eq!(claims.transfer_generation, Some(lease.generation));

        let TransportCall::Resume(resumed) = &observed[1] else {
            panic!("second call must request the Worker's verified missing chunks");
        };
        assert_eq!(resumed.artifact_id, "source");
        assert_eq!(resumed.transfer_generation, lease.generation);
        let TransportCall::Upload(uploaded) = &observed[2] else {
            panic!("third call must upload the verified missing chunk");
        };
        assert_eq!(uploaded.bytes, bytes);
        assert_eq!(uploaded.transfer_generation, lease.generation);
        let TransportCall::Execute(execute) = &observed[3] else {
            panic!("execution must start only after authenticated transfer");
        };
        assert_eq!(execute.token, prepared.token);
        assert_eq!(execute.request_digest, manifest.request_digest);

        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn prepare_rpc_failure_redispatches_without_worker_penalty() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_prepare_rpc_failure").await else {
            return;
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-prepare-failure-task-{unique}");
        let worker_id = format!("dispatch-prepare-failure-worker-{unique}");
        let bytes = b"trusted-source".to_vec();
        let mut manifest = alpha_result_request();
        manifest.source_artifact = ArtifactManifest {
            artifact_id: "source".into(),
            role: ArtifactRole::Source,
            size_bytes: bytes.len() as u64,
            mime_type: "text/plain".into(),
            sha256: sha256_digest(&bytes),
            chunks: vec![general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(&bytes),
            }],
            inline_bytes: Some(bytes),
        };
        manifest.request_digest = manifest.canonical_request_digest();
        let (worker_addr, mut execute_rx) = worker_only_execute_server().await.unwrap();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&manifest).unwrap());
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO worker_reputation
             (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 0, 0, 100, false)",
        )
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .unwrap();
        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();

        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            &private_key,
            ManagedProofRolloutMode::Enforce,
        )
        .await
        .unwrap();

        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());
        assert!(!stored.billing_settled);
        let failed_tasks: i64 =
            sqlx::query_scalar("SELECT failed_tasks FROM worker_reputation WHERE worker_id = $1")
                .bind(&worker_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(failed_tasks, 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), execute_rx.recv())
                .await
                .is_err()
        );

        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn expired_transfer_lease_redispatches_before_signing_or_execution() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_expired_transfer_lease").await else {
            return;
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-expired-lease-task-{unique}");
        let worker_id = format!("dispatch-expired-lease-worker-{unique}");
        let bytes = b"trusted-source".to_vec();
        let mut manifest = alpha_result_request();
        manifest.source_artifact = ArtifactManifest {
            artifact_id: "source".into(),
            role: ArtifactRole::Source,
            size_bytes: bytes.len() as u64,
            mime_type: "text/plain".into(),
            sha256: sha256_digest(&bytes),
            chunks: vec![general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(&bytes),
            }],
            inline_bytes: Some(bytes),
        };
        manifest.request_digest = manifest.canonical_request_digest();
        let (worker_addr, mut execute_rx) = worker_only_execute_server().await.unwrap();
        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&manifest).unwrap());
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE general_compute_transfer_leases
             SET expires_at = NOW() - INTERVAL '1 second'
             WHERE task_id = $1 AND state = 'active'",
        )
        .bind(&task_id)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_reputation
             (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 0, 0, 100, false)",
        )
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .unwrap();
        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();

        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            &private_key,
            ManagedProofRolloutMode::Enforce,
        )
        .await
        .unwrap();

        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());
        assert!(!stored.billing_settled);
        let failed_tasks: i64 =
            sqlx::query_scalar("SELECT failed_tasks FROM worker_reputation WHERE worker_id = $1")
                .bind(&worker_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(failed_tasks, 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), execute_rx.recv())
                .await
                .is_err()
        );

        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn missing_nodepool_artifact_source_fails_typed_without_worker_penalty() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_missing_nodepool_source").await else {
            return;
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-missing-source-task-{unique}");
        let worker_id = format!("dispatch-missing-source-worker-{unique}");
        let bytes = b"unavailable-source";
        let mut manifest = alpha_result_request();
        manifest.source_artifact = ArtifactManifest {
            artifact_id: "source".into(),
            role: ArtifactRole::Source,
            size_bytes: bytes.len() as u64,
            mime_type: "text/plain".into(),
            sha256: sha256_digest(bytes),
            chunks: vec![general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(bytes),
            }],
            inline_bytes: None,
        };
        manifest.request_digest = manifest.canonical_request_digest();
        let (worker_addr, mut execute_rx) = worker_only_execute_server().await.unwrap();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.general_compute_manifest_json = Some(serde_json::to_vec(&manifest).unwrap());
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO worker_reputation
             (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 0, 0, 100, false)",
        )
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .unwrap();
        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();

        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            &private_key,
            ManagedProofRolloutMode::Enforce,
        )
        .await
        .unwrap();

        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert!(!stored.billing_settled);
        assert!(dispatcher
            .repo
            .general_compute_transfer_lease(&task_id)
            .await
            .unwrap()
            .is_none());
        let result_json: Vec<u8> = sqlx::query_scalar(
            "SELECT result_json FROM general_compute_results WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let result: GeneralComputeResult = serde_json::from_slice(&result_json).unwrap();
        assert_eq!(result.status, ResultStatus::Failed);
        assert_eq!(result.error_code.as_deref(), Some("nodepool_task_failed"));
        assert!(result.stderr.contains("no trusted source"));
        let failed_tasks: i64 =
            sqlx::query_scalar("SELECT failed_tasks FROM worker_reputation WHERE worker_id = $1")
                .bind(&worker_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(failed_tasks, 0);
        let settlement_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_settlements WHERE task_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(settlement_count, 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), execute_rx.recv())
                .await
                .is_err()
        );

        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn execution_token_signing_failure_is_contained_without_worker_penalty() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_execution_token_signing_failure").await
        else {
            return;
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-signing-failure-task-{unique}");
        let worker_id = format!("dispatch-signing-failure-worker-{unique}");
        let (worker_addr, mut execute_rx) = worker_only_execute_server().await.unwrap();
        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.runtime = Some("managed-function-v0".into());
        task.task_source = Some("return 7;".into());
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO worker_reputation
             (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 0, 0, 100, false)",
        )
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .unwrap();

        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            "not-a-valid-ed25519-private-key",
            ManagedProofRolloutMode::Enforce,
        )
        .await
        .unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(100), execute_rx.recv())
                .await
                .is_err()
        );
        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert!(stored
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("WORKER_EXECUTION_PRIVATE_KEY_PEM")));
        let failed_tasks: i64 =
            sqlx::query_scalar("SELECT failed_tasks FROM worker_reputation WHERE worker_id = $1")
                .bind(&worker_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(failed_tasks, 0);

        fixture.cleanup().await.ok();
    }

    #[test]
    fn managed_completion_requires_proof_even_with_legacy_receipt_fields() {
        let mut task = make_task("managed-proof-required", TaskStatus::Running, 0);
        task.runtime = Some("managed-function-v0".into());
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "7".into(),
            managed_executed_ops: 2_500,
            managed_output_bytes: 2_049,
            managed_receipt_json: "{\"usage_units\":2500}".into(),
            managed_proof: None,
            ..ExecuteTaskResponse::default()
        };

        let error = managed_proof_for_completion(&task, &response).unwrap_err();

        assert_eq!(error, "Managed proof is required");
    }

    #[test]
    fn managed_completion_off_mode_allows_legacy_settlement_without_proof() {
        let mut task = make_task("managed-proof-off", TaskStatus::Running, 0);
        task.runtime = Some("managed-function-v0".into());
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "legacy-output".into(),
            managed_proof: None,
            ..ExecuteTaskResponse::default()
        };

        assert!(managed_proof_for_completion_with_mode(
            ManagedProofRolloutMode::Off,
            &task,
            &response
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn managed_completion_observe_mode_allows_legacy_settlement_without_proof() {
        let mut task = make_task("managed-proof-observe", TaskStatus::Running, 0);
        task.runtime = Some("managed-function-v0".into());
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "legacy-output".into(),
            managed_proof: None,
            ..ExecuteTaskResponse::default()
        };

        assert!(managed_proof_for_completion_with_mode(
            ManagedProofRolloutMode::Observe,
            &task,
            &response
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn managed_proof_metrics_count_verification_outcomes() {
        let before = managed_proof_metrics::snapshot();

        managed_proof_metrics::record(ManagedProofMetricEvent::Verified);
        managed_proof_metrics::record(ManagedProofMetricEvent::Rejected);
        managed_proof_metrics::record(ManagedProofMetricEvent::QueueRetry);
        managed_proof_metrics::record(ManagedProofMetricEvent::ObserveFallback);
        managed_proof_metrics::record(ManagedProofMetricEvent::LegacySettlement);

        let after = managed_proof_metrics::snapshot();
        assert_eq!(
            after.verification_attempts - before.verification_attempts,
            2
        );
        assert_eq!(after.verified - before.verified, 1);
        assert_eq!(after.rejected - before.rejected, 1);
        assert_eq!(after.queue_retries - before.queue_retries, 1);
        assert_eq!(after.observe_fallbacks - before.observe_fallbacks, 1);
        assert_eq!(after.legacy_settlements - before.legacy_settlements, 1);
    }

    #[test]
    fn worker_response_rejects_oversized_status_message() {
        let mut response = managed_response("ok");
        response.status_message = "x".repeat(WORKER_STATUS_MESSAGE_MAX_BYTES + 1);

        assert_eq!(
            validate_worker_response_sizes(&response).unwrap_err(),
            WorkerResponseSizeError::StatusMessageTooLarge
        );
    }

    #[test]
    fn worker_response_rejects_oversized_legacy_receipt() {
        let mut response = managed_response("ok");
        response.managed_receipt_json = "x".repeat(LEGACY_MANAGED_RECEIPT_MAX_BYTES + 1);

        assert_eq!(
            validate_worker_response_sizes(&response).unwrap_err(),
            WorkerResponseSizeError::LegacyReceiptTooLarge
        );
    }

    #[test]
    fn worker_response_accepts_fields_at_application_caps() {
        let mut response = managed_response("ok");
        response.status_message = "x".repeat(WORKER_STATUS_MESSAGE_MAX_BYTES);
        response.managed_receipt_json = "x".repeat(LEGACY_MANAGED_RECEIPT_MAX_BYTES);

        assert_eq!(validate_worker_response_sizes(&response), Ok(()));
    }

    #[test]
    fn general_compute_result_requires_a_typed_payload() {
        let mut task = make_task("general-compute-result-required", TaskStatus::Running, 0);
        task.runtime = Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let request = alpha_result_request();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        let response = ExecuteTaskResponse::default();

        let error = decode_and_validate_general_compute_result(
            &task,
            &response,
            &alpha_capability_snapshot(&request),
        )
        .expect_err("alpha completion must carry a typed result envelope");

        assert_eq!(error, "general-compute typed result is missing");
    }

    #[test]
    fn general_compute_result_rejects_malformed_json() {
        let (task, request) = alpha_result_task("general-compute-result-malformed");
        let response = ExecuteTaskResponse {
            general_compute_result_json: b"{".to_vec(),
            ..ExecuteTaskResponse::default()
        };

        let error = decode_and_validate_general_compute_result(
            &task,
            &response,
            &alpha_capability_snapshot(&request),
        )
        .expect_err("malformed typed result must fail closed");

        assert!(error.contains("general-compute typed result is malformed"));
    }

    #[test]
    fn general_compute_result_rejects_identity_mismatch() {
        let (task, request) = alpha_result_task("general-compute-result-identity");
        let mut result = alpha_result(&request);
        result.attempt_id = "attempt-stale".into();
        let response = ExecuteTaskResponse {
            success: true,
            general_compute_result_json: serde_json::to_vec(&result).unwrap(),
            ..ExecuteTaskResponse::default()
        };

        let error = decode_and_validate_general_compute_result(
            &task,
            &response,
            &alpha_capability_snapshot(&request),
        )
        .expect_err("a stale typed result must not settle the current attempt");

        assert!(error.contains("ResultBindingMismatch"));
    }

    #[test]
    fn general_compute_result_rejects_capability_image_mismatch() {
        let (task, request) = alpha_result_task("general-compute-result-capability");
        let result = alpha_result(&request);
        let mismatched_snapshot = serde_json::to_string(&TrustedWorkerCapabilityRegistration {
            worker: WorkerCapabilities {
                guest_image_digests: vec![request.guest_image_digest.clone()],
                capabilities: vec![],
                max_threads: 1,
                gpu_available: false,
            },
            gpu_capabilities: vec![],
            managed_gpu_backends: vec![],
            backends: vec![BackendRegistration {
                backend_id: request.backend_id.clone(),
                execution_mode:
                    general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
                guest_image_digest: format!("sha256:{}", "c".repeat(64)),
                capabilities: vec![],
                max_threads: 1,
                network_allowed: false,
                filesystem_read_only: true,
                gpu_allowed: false,
            }],
        })
        .unwrap();
        let response = ExecuteTaskResponse {
            success: true,
            general_compute_result_json: serde_json::to_vec(&result).unwrap(),
            ..ExecuteTaskResponse::default()
        };

        let error =
            decode_and_validate_general_compute_result(&task, &response, &mismatched_snapshot)
                .expect_err("result validation must use the persisted capability snapshot");

        assert!(error.contains("trusted capability snapshot"));
    }

    #[test]
    fn general_compute_result_rejects_a_selected_gpu_not_in_the_trusted_snapshot() {
        let (mut task, mut request) = alpha_result_task("general-compute-result-gpu-identity");
        let image_digest = request.guest_image_digest.clone();
        request.execution_policy.gpu_required = true;
        request.execution_policy.gpu_requirement = Some(
            general_compute_runtime::gpu::GpuRequirement::new(
                general_compute_runtime::gpu::GpuVendor::Nvidia,
                "sm_80",
                general_compute_runtime::gpu::GpuRuntime::Cuda,
                "550.54",
                16 * 1024 * 1024 * 1024,
                8,
                &image_digest,
                false,
            )
            .unwrap(),
        );
        request.request_digest = request.canonical_request_digest();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let trusted_gpu = general_compute_runtime::gpu::GpuCapability::new(
            general_compute_runtime::gpu::GpuVendor::Nvidia,
            "gpu-trusted",
            "sm_80",
            general_compute_runtime::gpu::GpuRuntime::Cuda,
            "12.4",
            "550.54",
            24 * 1024 * 1024 * 1024,
            16,
            &image_digest,
        )
        .unwrap();
        let forged_gpu = general_compute_runtime::gpu::GpuCapability::new(
            general_compute_runtime::gpu::GpuVendor::Nvidia,
            "gpu-forged",
            "sm_80",
            general_compute_runtime::gpu::GpuRuntime::Cuda,
            "12.4",
            "550.54",
            24 * 1024 * 1024 * 1024,
            16,
            &image_digest,
        )
        .unwrap();
        let snapshot = serde_json::to_string(&TrustedWorkerCapabilityRegistration {
            worker: WorkerCapabilities {
                guest_image_digests: vec![image_digest.clone()],
                capabilities: vec![],
                max_threads: 1,
                gpu_available: true,
            },
            gpu_capabilities: vec![trusted_gpu],
            managed_gpu_backends: vec![],
            backends: vec![BackendRegistration {
                backend_id: request.backend_id.clone(),
                execution_mode:
                    general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
                guest_image_digest: image_digest,
                capabilities: vec![],
                max_threads: 1,
                network_allowed: false,
                filesystem_read_only: true,
                gpu_allowed: true,
            }],
        })
        .unwrap();
        let mut result = alpha_result(&request);
        result.gpu_selection = Some(general_compute_runtime::gpu::GpuSelection::Gpu(forged_gpu));
        let response = ExecuteTaskResponse {
            success: true,
            general_compute_result_json: serde_json::to_vec(&result).unwrap(),
            ..ExecuteTaskResponse::default()
        };

        let error = decode_and_validate_general_compute_result(&task, &response, &snapshot)
            .expect_err("a result must use the exact operator-selected GPU identity");

        assert!(error.contains("trusted GPU selection"));
    }

    #[test]
    fn general_compute_result_accepts_a_valid_typed_result() {
        let (task, request) = alpha_result_task("general-compute-result-valid");
        let result = alpha_result(&request);
        let response = ExecuteTaskResponse {
            success: true,
            general_compute_result_json: serde_json::to_vec(&result).unwrap(),
            ..ExecuteTaskResponse::default()
        };

        let validated = decode_and_validate_general_compute_result(
            &task,
            &response,
            &alpha_capability_snapshot(&request),
        )
        .expect("valid typed result should be accepted");

        assert_eq!(validated.stdout, "42");
        assert_eq!(validated.status, ResultStatus::Completed);
    }

    #[test]
    fn production_result_input_digest_must_match_nodepool_owned_source_bytes() {
        let (task, request) = alpha_result_task("general-compute-result-input-digest");
        let mut result = alpha_result(&request);
        let mut registration: TrustedWorkerCapabilityRegistration =
            serde_json::from_str(&alpha_capability_snapshot(&request)).unwrap();
        registration.backends[0].execution_mode =
            general_compute_runtime::sandbox::BackendExecutionMode::ProductionSandboxedOci;
        let matrix = general_compute_runtime::CapabilityMatrix::new(registration.backends);
        let sources = HashMap::from([(String::from("source"), b"source".to_vec())]);

        result.input_sha256 = sha256_digest(&[]);
        let error = validate_production_input_digest(&request, &result, &matrix, &sources)
            .expect_err("a production result must bind its digest to Nodepool source bytes");

        assert!(error.contains("input digest"));
        assert_eq!(
            task.runtime.as_deref(),
            Some(GENERAL_COMPUTE_RUNTIME_VERSION)
        );
    }

    #[test]
    fn worker_response_rejects_oversized_typed_result() {
        let mut response = managed_response("ok");
        response.general_compute_result_json = vec![b'x'; GENERAL_COMPUTE_RESULT_MAX_BYTES + 1];

        assert_eq!(
            validate_worker_response_sizes(&response).unwrap_err(),
            WorkerResponseSizeError::GeneralComputeResultTooLarge
        );
    }

    fn alpha_result_task(task_id: &str) -> (Task, GeneralComputeRequest) {
        let mut task = make_task(task_id, TaskStatus::Running, 0);
        task.runtime = Some(GENERAL_COMPUTE_RUNTIME_VERSION.into());
        let request = alpha_result_request();
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        (task, request)
    }

    fn alpha_result_request() -> GeneralComputeRequest {
        let mut request = GeneralComputeRequest {
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest: format!("sha256:{}", "b".repeat(64)),
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
        request
    }

    fn alpha_capability_snapshot(request: &GeneralComputeRequest) -> String {
        serde_json::to_string(&TrustedWorkerCapabilityRegistration {
            worker: WorkerCapabilities {
                guest_image_digests: vec![request.guest_image_digest.clone()],
                capabilities: vec![],
                max_threads: 1,
                gpu_available: false,
            },
            gpu_capabilities: vec![],
            managed_gpu_backends: vec![],
            backends: vec![BackendRegistration {
                backend_id: request.backend_id.clone(),
                execution_mode:
                    general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
                guest_image_digest: request.guest_image_digest.clone(),
                capabilities: vec![],
                max_threads: 1,
                network_allowed: false,
                filesystem_read_only: true,
                gpu_allowed: false,
            }],
        })
        .unwrap()
    }

    fn alpha_result(request: &GeneralComputeRequest) -> GeneralComputeResult {
        let output = ArtifactManifest::inline_json("stdout", ArtifactRole::Output, b"42");
        GeneralComputeResult {
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            status: ResultStatus::Completed,
            exit_code: Some(0),
            error_code: None,
            stdout: "42".into(),
            stderr: String::new(),
            output_artifacts: vec![output.clone()],
            usage: UsageClaim {
                wall_time_ms: 1,
                output_bytes: 2,
                ..UsageClaim::default()
            },
            runtime_version: request.runtime_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            input_sha256: sha256_digest(&[]),
            determinism: request.determinism.clone(),
            capability_summary: vec![],
            gpu_selection: None,
            output_manifest_root: canonical_artifact_root(&[output]),
            evidence: EvidenceEnvelope::default(),
        }
    }

    #[test]
    fn worker_transport_contract_has_finite_managed_execution_limits() {
        assert_eq!(WORKER_CONNECT_TIMEOUT, Duration::from_secs(5));
        assert_eq!(WORKER_EXECUTE_RPC_TIMEOUT, Duration::from_secs(20 * 60));
        assert_eq!(
            hivemind_proto::WORKER_RPC_MESSAGE_MAX_BYTES,
            22 * 1024 * 1024
        );
    }

    #[test]
    fn resource_exhausted_has_a_dedicated_no_penalty_redispatch_disposition() {
        assert_eq!(
            worker_rpc_failure_disposition(&Status::resource_exhausted("worker queue full")),
            WorkerRpcFailureDisposition::RetryAfterResourceExhaustion
        );
    }

    #[test]
    fn unavailable_is_redispatchable_without_worker_penalty() {
        assert_eq!(
            worker_rpc_failure_disposition(&Status::unavailable("worker temporarily offline")),
            WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty
        );
    }

    #[test]
    fn cas_only_prepare_failure_is_terminal_without_worker_penalty() {
        let error = anyhow::Error::new(CasOnlyArtifactUnavailable {
            artifact_id: "source".into(),
        });

        assert_eq!(
            general_compute_prepare_failure_disposition(&error),
            GeneralComputePrepareFailureDisposition::FailTaskWithoutWorkerPenalty
        );
    }

    #[test]
    fn transport_prepare_failure_remains_redispatchable_without_worker_penalty() {
        let error = anyhow::anyhow!("worker chunk endpoint unavailable");

        assert_eq!(
            general_compute_prepare_failure_disposition(&error),
            GeneralComputePrepareFailureDisposition::RetryWithoutWorkerPenalty
        );
    }

    #[tokio::test]
    async fn connect_transport_error_is_redispatchable_without_worker_penalty() {
        let unavailable_addr = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            drop(listener);
            addr.to_string()
        };
        let endpoint =
            tonic::transport::Endpoint::from_shared(format!("http://{unavailable_addr}"))
                .unwrap()
                .connect_timeout(Duration::from_secs(1));
        let error = tokio::time::timeout(Duration::from_secs(2), endpoint.connect())
            .await
            .expect("closed local port should complete its connection attempt")
            .expect_err("closed local port should return a transport error");

        assert_eq!(
            worker_transport_failure_disposition(&error),
            WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty
        );
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_retry_ceiling_persists_nodepool_failure() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_retry_ceiling").await else {
            return;
        };
        let repo = TaskRepository::new(db.pool.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_dispatch_case(&repo, &unique).await;
        let reputation_before =
            seed_managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await;
        let task = repo
            .find_by_task_id(&case.task_id)
            .await
            .unwrap()
            .expect("managed GPU task must exist");

        reset_managed_gpu_attempt(&repo, &task, &case.worker_id, 0, "invalid typed result")
            .await
            .unwrap();

        let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert_eq!(
            stored.status_message.as_deref(),
            Some("invalid typed result")
        );
        assert!(!stored.billing_settled);
        assert_eq!(stored.billed_amount, 0);
        let result_json: Vec<u8> =
            sqlx::query_scalar("SELECT result_json FROM managed_gpu_results WHERE task_id = $1")
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        let result: ManagedGpuResult = serde_json::from_slice(&result_json).unwrap();
        assert_eq!(result.status, ManagedGpuStatus::Failed);
        assert_eq!(result.error_code.as_deref(), Some("retry_limit_exceeded"));
        assert_eq!(result.selected_gpu, case.capability);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM managed_gpu_settlements WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "retry-ceiling Nodepool failure must not mutate Worker reputation"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM task_attestations
                 WHERE task_id = $1 AND worker_id = $2",
            )
            .bind(&case.task_id)
            .bind(&case.worker_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            0
        );

        cleanup_managed_gpu_dispatch_case(&repo, &case).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_malformed_manifest_is_quarantined() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_malformed_manifest").await else {
            return;
        };
        let repo = TaskRepository::new(db.pool.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_dispatch_case(&repo, &unique).await;
        let reputation_before =
            seed_managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await;
        let malformed_manifest = b"not-json".to_vec();
        sqlx::query("UPDATE tasks SET managed_gpu_manifest_json = $1 WHERE task_id = $2")
            .bind(&malformed_manifest)
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        let task = repo
            .find_by_task_id(&case.task_id)
            .await
            .unwrap()
            .expect("managed GPU task must exist");

        reset_managed_gpu_attempt(&repo, &task, &case.worker_id, 2, "invalid typed result")
            .await
            .unwrap();

        let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert_eq!(
            stored.status_message.as_deref(),
            Some("managed GPU request manifest is malformed or invalid")
        );
        assert!(!stored.billing_settled);
        assert_eq!(stored.billed_amount, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            0,
            "manifest quarantine must not fabricate a typed GPU identity"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM managed_gpu_settlements WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "manifest quarantine must not mutate Worker reputation"
        );

        cleanup_managed_gpu_dispatch_case(&repo, &case).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_missing_manifest_is_quarantined() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_missing_manifest").await else {
            return;
        };
        let repo = TaskRepository::new(db.pool.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatcher-gpu-missing-manifest-task-{unique}");
        let worker_id = format!("dispatcher-gpu-missing-manifest-worker-{unique}");
        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.owner = format!("dispatcher-gpu-missing-manifest-owner-{unique}");
        repo.create(&task).await.unwrap();
        sqlx::query(
            "UPDATE tasks
             SET runtime = $1, worker_id = $2, worker_ip = '127.0.0.1', status = 'ASSIGNED'
             WHERE task_id = $3",
        )
        .bind(MANAGED_GPU_RUNTIME_VERSION)
        .bind(&worker_id)
        .bind(&task_id)
        .execute(&repo.pool)
        .await
        .unwrap();
        let assigned = repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .expect("manually corrupted task must exist");

        reset_managed_gpu_attempt(
            &repo,
            &assigned,
            &worker_id,
            2,
            "managed GPU request manifest is missing",
        )
        .await
        .unwrap();

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert_eq!(
            stored.status_message.as_deref(),
            Some("managed GPU request manifest is missing")
        );
        assert!(!stored.billing_settled);
        assert_eq!(stored.billed_amount, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1",
            )
            .bind(&task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            0
        );

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_execute_path_missing_manifest_is_quarantined() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_execute_missing_manifest").await
        else {
            return;
        };
        let repo = Arc::new(TaskRepository::new(db.pool.clone()));
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_dispatch_case(repo.as_ref(), &unique).await;
        let reputation_before =
            seed_managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await;
        sqlx::query("UPDATE tasks SET managed_gpu_manifest_json = NULL WHERE task_id = $1")
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        let task = repo
            .find_by_task_id(&case.task_id)
            .await
            .unwrap()
            .expect("managed GPU task must exist");
        let (worker_addr, mut execute_rx) = worker_only_execute_server().await.unwrap();
        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();

        execute_on_worker_with_managed_proof_key(
            repo.clone(),
            task,
            case.worker_id.clone(),
            worker_addr.to_string(),
            WorkerExecutionOptions {
                worker_execution_private_key_pem: private_key,
                managed_proof_authorization_private_key_pem: String::new(),
                managed_proof_provider_configured: false,
                managed_proof_rollout_mode: ManagedProofRolloutMode::Enforce,
                max_redispatch: 0,
            },
        )
        .await
        .unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(100), execute_rx.recv())
                .await
                .is_err(),
            "missing manifest must fail before submitting a Worker RPC"
        );
        let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert_eq!(
            stored.status_message.as_deref(),
            Some("managed-function-gpu-v1 request manifest is missing")
        );
        assert!(!stored.billing_settled);
        assert_eq!(stored.billed_amount, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM managed_gpu_settlements WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "missing-manifest quarantine must not mutate Worker reputation"
        );

        cleanup_managed_gpu_dispatch_case(repo.as_ref(), &case).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_missing_or_corrupt_binding_is_quarantined() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_binding_integrity").await else {
            return;
        };
        let repo = TaskRepository::new(db.pool.clone());
        for corrupt in [false, true] {
            let unique = format!("{}-{}", uuid::Uuid::new_v4(), corrupt);
            let case = setup_managed_gpu_dispatch_case(&repo, &unique).await;
            let reputation_before =
                seed_managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await;
            if corrupt {
                sqlx::query(
                    "UPDATE managed_gpu_attempt_bindings
                     SET selected_gpu_json = $1
                     WHERE task_id = $2 AND attempt_generation = 1",
                )
                .bind(b"not-json".to_vec())
                .bind(&case.task_id)
                .execute(&repo.pool)
                .await
                .unwrap();
            } else {
                sqlx::query("DELETE FROM managed_gpu_attempt_bindings WHERE task_id = $1")
                    .bind(&case.task_id)
                    .execute(&repo.pool)
                    .await
                    .unwrap();
            }
            let task = repo
                .find_by_task_id(&case.task_id)
                .await
                .unwrap()
                .expect("managed GPU task must exist");

            reset_managed_gpu_attempt(
                &repo,
                &task,
                &case.worker_id,
                0,
                "managed GPU binding integrity failure",
            )
            .await
            .unwrap();

            let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
            assert_eq!(stored.status, TaskStatus::Failed);
            assert!(!stored.billing_settled);
            assert_eq!(stored.billed_amount, 0);
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1",
                )
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap(),
                0,
                "quarantine must not fabricate a typed GPU identity"
            );
            assert_eq!(
                managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await,
                reputation_before,
                "binding quarantine must not mutate Worker reputation"
            );
            cleanup_managed_gpu_dispatch_case(&repo, &case).await;
        }
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_response_binding_integrity_is_quarantined() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_response_binding").await else {
            return;
        };
        let repo = Arc::new(TaskRepository::new(db.pool.clone()));
        for corrupt in [false, true] {
            let unique = format!("{}-{corrupt}", uuid::Uuid::new_v4());
            let case = setup_managed_gpu_dispatch_case(repo.as_ref(), &unique).await;
            let reputation_before =
                seed_managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await;
            let request: ManagedGpuRequest = serde_json::from_slice(&case.manifest).unwrap();
            let response = managed_gpu_dispatch_completed_response(&request, &case.capability);
            if corrupt {
                sqlx::query(
                    "UPDATE managed_gpu_attempt_bindings
                     SET selected_gpu_json = $1
                     WHERE task_id = $2 AND attempt_generation = 1",
                )
                .bind(b"not-json".to_vec())
                .bind(&case.task_id)
                .execute(&repo.pool)
                .await
                .unwrap();
            } else {
                sqlx::query("DELETE FROM managed_gpu_attempt_bindings WHERE task_id = $1")
                    .bind(&case.task_id)
                    .execute(&repo.pool)
                    .await
                    .unwrap();
            }
            let (worker_addr, mut execute_rx) = fake_worker_execute_server_with_response(response)
                .await
                .unwrap();
            let task = repo
                .find_by_task_id(&case.task_id)
                .await
                .unwrap()
                .expect("managed GPU task must exist");
            let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();
            let execution = tokio::spawn(execute_on_worker_with_managed_proof_key(
                repo.clone(),
                task,
                case.worker_id.clone(),
                worker_addr.to_string(),
                WorkerExecutionOptions {
                    worker_execution_private_key_pem: private_key,
                    managed_proof_authorization_private_key_pem: String::new(),
                    managed_proof_provider_configured: false,
                    managed_proof_rollout_mode: ManagedProofRolloutMode::Enforce,
                    max_redispatch: 0,
                },
            ));
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), execute_rx.recv())
                    .await
                    .unwrap()
                    .as_deref(),
                Some(case.task_id.as_str())
            );
            execution.await.unwrap().unwrap();

            let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
            assert_eq!(stored.status, TaskStatus::Failed);
            assert!(!stored.billing_settled);
            assert_eq!(stored.billed_amount, 0);
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1",
                )
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap(),
                0,
                "binding integrity quarantine must not fabricate a typed result"
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM managed_gpu_settlements WHERE task_id = $1",
                )
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap(),
                0
            );
            assert_eq!(
                managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await,
                reputation_before,
                "response-path binding quarantine must not mutate Worker reputation"
            );
            cleanup_managed_gpu_dispatch_case(repo.as_ref(), &case).await;
        }
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_success_uses_immutable_binding_and_settles() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_success").await else {
            return;
        };
        let repo = Arc::new(TaskRepository::new(db.pool.clone()));
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_dispatch_case(repo.as_ref(), &unique).await;
        let request: ManagedGpuRequest = serde_json::from_slice(&case.manifest).unwrap();
        let binding_before = repo
            .managed_gpu_attempt_binding(&case.task_id, &case.worker_id, 1)
            .await
            .unwrap()
            .expect("managed GPU assignment must persist an immutable binding");

        let changed_capability = ManagedGpuCapability::new(
            "cuda-dispatch-mutated-0",
            request.gpu_requirement.compute_capability.clone(),
            request.gpu_requirement.runtime_version.clone(),
            request.gpu_requirement.driver_abi.clone(),
            16 * 1024 * 1024 * 1024,
            32,
            request.guest_image_digest.clone(),
            1,
            "GPU-fedcba9876543210",
        )
        .unwrap();
        let changed_registration = managed_gpu_dispatch_registration(&request, &changed_capability);
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

        let response = managed_gpu_dispatch_completed_response(&request, &case.capability);
        let (worker_addr, mut execute_rx) = fake_worker_execute_server_with_response(response)
            .await
            .expect("fake Worker must start");
        let task = repo
            .find_by_task_id(&case.task_id)
            .await
            .unwrap()
            .expect("managed GPU task must exist");
        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();
        let worker_id = case.worker_id.clone();
        let worker_addr = worker_addr.to_string();
        let execution_repo = repo.clone();
        let execution = tokio::spawn(async move {
            execute_on_worker(
                execution_repo,
                task,
                worker_id,
                worker_addr,
                &private_key,
                ManagedProofRolloutMode::Enforce,
            )
            .await
        });
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), execute_rx.recv())
                .await
                .unwrap()
                .as_deref(),
            Some(case.task_id.as_str())
        );
        execution.await.unwrap().unwrap();

        let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Completed);
        assert!(stored.billing_settled);
        assert_eq!(stored.billed_amount, request.reservation_cpt as i64);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM managed_gpu_settlements WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM ledger_entries WHERE task_id = $1")
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap(),
            3
        );

        let result_json: Vec<u8> =
            sqlx::query_scalar("SELECT result_json FROM managed_gpu_results WHERE task_id = $1")
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        let persisted: ManagedGpuResult = serde_json::from_slice(&result_json).unwrap();
        assert_eq!(persisted.selected_gpu, case.capability);
        let binding_after = repo
            .managed_gpu_attempt_binding(&case.task_id, &case.worker_id, 1)
            .await
            .unwrap()
            .expect("managed GPU binding must remain available after settlement");
        assert_eq!(binding_after, binding_before);

        cleanup_managed_gpu_dispatch_case(repo.as_ref(), &case).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_token_signing_failure_hits_retry_ceiling() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_token_failure").await else {
            return;
        };
        let repo = Arc::new(TaskRepository::new(db.pool.clone()));
        let unique = uuid::Uuid::new_v4().to_string();
        let case = setup_managed_gpu_dispatch_case(repo.as_ref(), &unique).await;
        let reputation_before =
            seed_managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await;
        let task = repo
            .find_by_task_id(&case.task_id)
            .await
            .unwrap()
            .expect("managed GPU task must exist");
        let (worker_addr, mut execute_rx) = worker_only_execute_server().await.unwrap();

        execute_on_worker_with_managed_proof_key(
            repo.clone(),
            task,
            case.worker_id.clone(),
            worker_addr.to_string(),
            WorkerExecutionOptions {
                worker_execution_private_key_pem: "not-a-valid-ed25519-private-key".into(),
                managed_proof_authorization_private_key_pem: String::new(),
                managed_proof_provider_configured: false,
                managed_proof_rollout_mode: ManagedProofRolloutMode::Enforce,
                max_redispatch: 0,
            },
        )
        .await
        .unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(100), execute_rx.recv())
                .await
                .is_err(),
            "token signing failure must not submit an execution RPC"
        );
        let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert!(stored
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("WORKER_EXECUTION_PRIVATE_KEY_PEM")));
        let result_json: Vec<u8> =
            sqlx::query_scalar("SELECT result_json FROM managed_gpu_results WHERE task_id = $1")
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        let result: ManagedGpuResult = serde_json::from_slice(&result_json).unwrap();
        assert_eq!(result.status, ManagedGpuStatus::Failed);
        assert_eq!(result.error_code.as_deref(), Some("worker_rpc_retry_limit"));
        assert_eq!(
            managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await,
            reputation_before,
            "token-signing failure must not mutate Worker reputation"
        );

        cleanup_managed_gpu_dispatch_case(repo.as_ref(), &case).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_rpc_retry_ceiling_preserves_failure_kind() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_rpc_ceiling").await else {
            return;
        };
        let repo = TaskRepository::new(db.pool.clone());
        for (suffix, disposition, expected_status) in [
            (
                "resource-exhausted",
                WorkerRpcFailureDisposition::RetryAfterResourceExhaustion,
                ManagedGpuStatus::ResourceExhausted,
            ),
            (
                "ordinary-rpc",
                WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty,
                ManagedGpuStatus::Failed,
            ),
        ] {
            let unique = format!("{}-{suffix}", uuid::Uuid::new_v4());
            let case = setup_managed_gpu_dispatch_case(&repo, &unique).await;
            let reputation_before =
                seed_managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await;
            let task = repo
                .find_by_task_id(&case.task_id)
                .await
                .unwrap()
                .expect("managed GPU task must exist");

            reset_after_worker_rpc_failure(
                &repo,
                &task,
                &case.worker_id,
                0,
                disposition,
                "worker RPC failed",
            )
            .await
            .unwrap();

            let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
            assert_eq!(stored.status, TaskStatus::Failed);
            assert!(!stored.billing_settled);
            let result_json: Vec<u8> = sqlx::query_scalar(
                "SELECT result_json FROM managed_gpu_results WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
            let result: ManagedGpuResult = serde_json::from_slice(&result_json).unwrap();
            assert_eq!(result.status, expected_status);
            assert_eq!(result.error_code.as_deref(), Some("worker_rpc_retry_limit"));
            assert_eq!(result.selected_gpu, case.capability);
            assert_eq!(
                managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await,
                reputation_before,
                "RPC retry ceiling must not mutate Worker reputation"
            );
            cleanup_managed_gpu_dispatch_case(&repo, &case).await;
        }
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_dispatch_rpc_failure_at_retry_ceiling_preserves_failure_kind() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_rpc_path").await else {
            return;
        };
        let repo = Arc::new(TaskRepository::new(db.pool.clone()));
        for (suffix, code, expected_status) in [
            (
                "resource-exhausted",
                tonic::Code::ResourceExhausted,
                ManagedGpuStatus::ResourceExhausted,
            ),
            (
                "unavailable",
                tonic::Code::Unavailable,
                ManagedGpuStatus::Failed,
            ),
        ] {
            let unique = format!("{}-{suffix}", uuid::Uuid::new_v4());
            let case = setup_managed_gpu_dispatch_case(repo.as_ref(), &unique).await;
            let reputation_before =
                seed_managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await;
            let (worker_addr, mut execute_rx) =
                worker_error_execute_server(code, "worker RPC rejected the request")
                    .await
                    .unwrap();
            let task = repo
                .find_by_task_id(&case.task_id)
                .await
                .unwrap()
                .expect("managed GPU task must exist");
            let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();
            let execution = tokio::spawn(execute_on_worker_with_managed_proof_key(
                repo.clone(),
                task,
                case.worker_id.clone(),
                worker_addr.to_string(),
                WorkerExecutionOptions {
                    worker_execution_private_key_pem: private_key,
                    managed_proof_authorization_private_key_pem: String::new(),
                    managed_proof_provider_configured: false,
                    managed_proof_rollout_mode: ManagedProofRolloutMode::Enforce,
                    max_redispatch: 0,
                },
            ));
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), execute_rx.recv())
                    .await
                    .unwrap()
                    .as_deref(),
                Some(case.task_id.as_str())
            );
            execution.await.unwrap().unwrap();

            let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
            assert_eq!(stored.status, TaskStatus::Failed);
            assert!(!stored.billing_settled);
            assert_eq!(stored.billed_amount, 0);
            let result_json: Vec<u8> = sqlx::query_scalar(
                "SELECT result_json FROM managed_gpu_results WHERE task_id = $1",
            )
            .bind(&case.task_id)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
            let result: ManagedGpuResult = serde_json::from_slice(&result_json).unwrap();
            assert_eq!(result.status, expected_status);
            assert_eq!(result.error_code.as_deref(), Some("worker_rpc_retry_limit"));
            assert_eq!(result.selected_gpu, case.capability);
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM managed_gpu_settlements WHERE task_id = $1",
                )
                .bind(&case.task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap(),
                0
            );
            assert_eq!(
                managed_gpu_dispatch_reputation(&repo.pool, &case.worker_id).await,
                reputation_before,
                "actual Worker RPC failure must not mutate Worker reputation"
            );
            cleanup_managed_gpu_dispatch_case(repo.as_ref(), &case).await;
        }
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn managed_gpu_late_worker_responses_do_not_change_terminal_state() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let Some((db, fixture)) = test_db("dispatcher_managed_gpu_late_response").await else {
            return;
        };
        let repo = Arc::new(TaskRepository::new(db.pool.clone()));
        for terminal in ["cancel", "failure", "quarantine"] {
            let unique = format!("{}-{terminal}", uuid::Uuid::new_v4());
            let case = setup_managed_gpu_dispatch_case(repo.as_ref(), &unique).await;
            let task = repo
                .find_by_task_id(&case.task_id)
                .await
                .unwrap()
                .expect("managed GPU task must exist");
            let (worker_addr, mut request_rx, response_tx) =
                blocking_worker_execute_server().await.unwrap();
            let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();
            let execution = tokio::spawn(execute_on_worker_with_managed_proof_key(
                repo.clone(),
                task,
                case.worker_id.clone(),
                worker_addr.to_string(),
                WorkerExecutionOptions {
                    worker_execution_private_key_pem: private_key,
                    managed_proof_authorization_private_key_pem: String::new(),
                    managed_proof_provider_configured: false,
                    managed_proof_rollout_mode: ManagedProofRolloutMode::Enforce,
                    max_redispatch: i32::MAX,
                },
            ));
            let worker_request = tokio::time::timeout(Duration::from_secs(5), request_rx.recv())
                .await
                .expect("Worker execute RPC must arrive")
                .expect("Worker execute RPC channel must remain open");
            let worker_gpu_request: ManagedGpuRequest =
                serde_json::from_slice(&worker_request.managed_gpu_manifest_json).unwrap();

            match terminal {
                "cancel" => {
                    repo.cancel(&case.task_id).await.unwrap();
                }
                "failure" => {
                    repo.fail_managed_gpu_without_worker_result(
                        &case.task_id,
                        &case.worker_id,
                        &case.manifest,
                        ManagedGpuStatus::BackendUnavailable,
                        "backend_unavailable",
                        "GPU backend is unavailable",
                    )
                    .await
                    .unwrap();
                }
                "quarantine" => {
                    sqlx::query("DELETE FROM managed_gpu_attempt_bindings WHERE task_id = $1")
                        .bind(&case.task_id)
                        .execute(&repo.pool)
                        .await
                        .unwrap();
                    repo.quarantine_managed_gpu_without_typed_result(
                        &case.task_id,
                        &case.worker_id,
                        Some(&case.manifest),
                        "FAILED",
                        "immutable binding unavailable",
                    )
                    .await
                    .unwrap();
                }
                _ => unreachable!(),
            }
            response_tx
                .send(managed_gpu_dispatch_completed_response(
                    &worker_gpu_request,
                    &case.capability,
                ))
                .unwrap();
            execution.await.unwrap().unwrap();

            let stored = repo.find_by_task_id(&case.task_id).await.unwrap().unwrap();
            assert!(matches!(
                stored.status,
                TaskStatus::Cancelled | TaskStatus::Failed
            ));
            if terminal == "quarantine" {
                assert_eq!(
                    sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1",
                    )
                    .bind(&case.task_id)
                    .fetch_one(&repo.pool)
                    .await
                    .unwrap(),
                    0
                );
            } else {
                assert_eq!(
                    sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM managed_gpu_results WHERE task_id = $1",
                    )
                    .bind(&case.task_id)
                    .fetch_one(&repo.pool)
                    .await
                    .unwrap(),
                    1
                );
            }
            cleanup_managed_gpu_dispatch_case(repo.as_ref(), &case).await;
        }
        fixture.cleanup().await.ok();
    }

    const MANAGED_SOURCE: &str = "return get(input, \"value\") + 1;";
    const MANAGED_INPUT: &str = "{\"value\":41}";
    const MANAGED_OUTPUT: &str = "42";
    const MANAGED_BUDGET: i64 = 100;

    fn managed_task_for_claim(task_id: &str) -> Task {
        let mut task = make_task(task_id, TaskStatus::Running, 0);
        task.runtime = Some(MANAGED_RUNTIME_ID.into());
        task.task_source = Some(MANAGED_SOURCE.into());
        task.torrent_source = Some(MANAGED_INPUT.into());
        task.max_cpt = MANAGED_BUDGET;
        task
    }

    fn managed_response(output: &str) -> ExecuteTaskResponse {
        ExecuteTaskResponse {
            success: true,
            status_message: output.into(),
            managed_executed_ops: i64::MAX,
            managed_output_bytes: i64::MAX,
            managed_receipt_json: "{\"worker_selected\":true}".into(),
            managed_proof: Some(ManagedProofEnvelope::default()),
            ..ExecuteTaskResponse::default()
        }
    }

    fn production_dsl_task_for_claim(task_id: &str) -> Task {
        let mut task = managed_task_for_claim(task_id);
        task.runtime = Some("production_sandboxed_dsl".into());
        task.managed_dsl_backend_id = Some("managed-dsl-v0".into());
        task.managed_dsl_semantics_manifest_sha256 =
            Some(general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256.into());
        task
    }

    fn production_dsl_response(output: &str) -> ExecuteTaskResponse {
        let mut response = managed_response(output);
        response.managed_receipt_json = serde_json::json!({
            "runtime": "managed-function-v0",
            "execution_mode": "production_sandboxed_dsl",
            "backend_id": "managed-dsl-v0",
            "semantics_manifest_sha256":
                general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256,
        })
        .to_string();
        response
    }

    #[test]
    fn production_dsl_receipt_identity_is_required_for_settlement() {
        let task = production_dsl_task_for_claim("managed-dsl-receipt-valid");
        let response = production_dsl_response(MANAGED_OUTPUT);
        let proof_task_id = dsl_proof_task_id(
            &task.task_id,
            "production_sandboxed_dsl",
            task.managed_dsl_backend_id.as_deref().unwrap(),
            task.managed_dsl_semantics_manifest_sha256
                .as_deref()
                .unwrap(),
        );
        let claim = managed_claim(
            &proof_task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );

        assert!(verified_managed_completion(&task, &response, &claim).is_ok());
    }

    #[test]
    fn production_dsl_receipt_rejects_identity_drift() {
        let task = production_dsl_task_for_claim("managed-dsl-receipt-drift");
        let proof_task_id = dsl_proof_task_id(
            &task.task_id,
            "production_sandboxed_dsl",
            task.managed_dsl_backend_id.as_deref().unwrap(),
            task.managed_dsl_semantics_manifest_sha256
                .as_deref()
                .unwrap(),
        );
        let claim = managed_claim(
            &proof_task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );
        let cases = [
            ("runtime", "other-runtime"),
            ("execution_mode", "production_sandboxed_oci"),
            ("backend_id", "other-backend"),
            ("semantics_manifest_sha256", "sha256:wrong"),
        ];

        for (field, value) in cases {
            let mut receipt = serde_json::json!({
                "runtime": "managed-function-v0",
                "execution_mode": "production_sandboxed_dsl",
                "backend_id": "managed-dsl-v0",
                "semantics_manifest_sha256":
                    general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256,
            });
            receipt[field] = serde_json::Value::String(value.into());
            let mut response = production_dsl_response(MANAGED_OUTPUT);
            response.managed_receipt_json = receipt.to_string();

            assert_eq!(
                verified_managed_completion(&task, &response, &claim).unwrap_err(),
                ManagedCompletionError::DslReceiptBinding,
                "receipt field {field} must be task-bound"
            );
        }
    }

    fn managed_claim(task_id: &str, source: &[u8], input: &[u8], output: &[u8]) -> ExecutionClaim {
        ExecutionClaim::new(
            task_id,
            source,
            input,
            output,
            MANAGED_BUDGET as u64,
            ExecutionMetrics {
                usage_units: 17,
                executed_ops: 11,
                function_calls: 2,
                loop_iterations: 3,
                max_call_depth: 1,
            },
        )
        .unwrap()
    }

    #[test]
    fn verified_claim_is_the_only_managed_settlement_source() {
        let task = managed_task_for_claim("managed-verified-settlement");
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );

        let completion = verified_managed_completion(&task, &response, &claim).unwrap();

        assert_eq!(completion.usage_units, 17);
        assert_eq!(completion.output_bytes, 2);
        assert_ne!(completion.usage_units, response.managed_executed_ops);
        assert_ne!(completion.output_bytes, response.managed_output_bytes);
        let persisted_claim: ExecutionClaim = serde_json::from_str(&completion.claim_json).unwrap();
        assert_eq!(persisted_claim, claim);
        assert_ne!(completion.claim_json, response.managed_receipt_json);
    }

    #[tokio::test]
    async fn managed_completion_rejects_a_failed_verifier() {
        let task = managed_task_for_claim("managed-invalid-proof");
        let response = managed_response(MANAGED_OUTPUT);

        let error = resolve_verified_managed_completion(
            &task,
            &response,
            std::future::ready(Err(ManagedProofVerifierError::VerifierFailed)),
        )
        .await
        .unwrap_err();

        assert_eq!(
            error,
            ManagedProofGateError::Verifier(ManagedProofVerifierError::VerifierFailed)
        );
    }

    #[test]
    fn verifier_local_queue_pressure_retries_without_blame() {
        for error in [
            ManagedProofVerifierError::QueueFull,
            ManagedProofVerifierError::QueueDeadlineExceeded,
        ] {
            assert_eq!(
                managed_proof_failure_disposition(&ManagedProofGateError::Verifier(error)),
                ManagedProofFailureDisposition::RetryWithoutWorkerPenalty
            );
        }
        assert_eq!(
            managed_proof_failure_disposition(&ManagedProofGateError::Verifier(
                ManagedProofVerifierError::DeadlineExceeded
            )),
            ManagedProofFailureDisposition::FailWorkerResult
        );
    }

    #[tokio::test]
    async fn managed_completion_accepts_only_the_verifier_returned_claim() {
        let task = managed_task_for_claim("managed-valid-proof");
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );

        let completion =
            resolve_verified_managed_completion(&task, &response, std::future::ready(Ok(claim)))
                .await
                .unwrap();

        assert_eq!(completion.usage_units, 17);
        assert_eq!(completion.output_bytes, 2);
    }

    #[test]
    fn verified_claim_rejects_replay_for_another_task() {
        let task = managed_task_for_claim("managed-replay-target");
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            "managed-original-task",
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::ClaimBinding(ClaimError::TaskIdMismatch)
        );
    }

    #[test]
    fn verified_claim_rejects_source_mismatch() {
        let task = managed_task_for_claim("managed-source-binding");
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            b"return 0;",
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::ClaimBinding(ClaimError::SourceMismatch)
        );
    }

    #[test]
    fn verified_claim_rejects_input_mismatch() {
        let task = managed_task_for_claim("managed-input-binding");
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            b"{\"value\":40}",
            MANAGED_OUTPUT.as_bytes(),
        );

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::ClaimBinding(ClaimError::InputMismatch)
        );
    }

    #[test]
    fn verified_claim_rejects_output_mismatch() {
        let task = managed_task_for_claim("managed-output-binding");
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            b"41",
        );

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::ClaimBinding(ClaimError::OutputMismatch)
        );
    }

    #[test]
    fn verified_claim_rejects_budget_mismatch() {
        let mut task = managed_task_for_claim("managed-budget-binding");
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );
        task.max_cpt += 1;

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::ClaimBinding(ClaimError::MaxUsageUnitsMismatch {
                expected: 101,
                received: 100,
            })
        );
    }

    #[test]
    fn verified_claim_rejects_protocol_version_mismatch() {
        let task = managed_task_for_claim("managed-protocol-binding");
        let response = managed_response(MANAGED_OUTPUT);
        let mut claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );
        claim.protocol_version = PROOF_PROTOCOL_VERSION + 1;

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::ClaimBinding(ClaimError::ProtocolVersionMismatch {
                expected: PROOF_PROTOCOL_VERSION,
                received: PROOF_PROTOCOL_VERSION + 1,
            })
        );
    }

    #[test]
    fn verified_claim_rejects_runtime_version_mismatch() {
        let task = managed_task_for_claim("managed-runtime-binding");
        let response = managed_response(MANAGED_OUTPUT);
        let mut claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );
        claim.runtime_id = "worker-runtime".into();

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::ClaimBinding(ClaimError::RuntimeIdMismatch)
        );
    }

    #[test]
    fn verified_claim_rejects_cost_model_version_mismatch() {
        let task = managed_task_for_claim("managed-cost-model-binding");
        let response = managed_response(MANAGED_OUTPUT);
        let mut claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );
        assert_eq!(claim.cost_model_id, COST_MODEL_ID);
        claim.cost_model_id = "worker-cost-model".into();

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::ClaimBinding(ClaimError::CostModelIdMismatch)
        );
    }

    #[test]
    fn verified_claim_uses_null_for_missing_managed_input() {
        let mut task = managed_task_for_claim("managed-null-input");
        task.torrent_source = None;
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            b"null",
            MANAGED_OUTPUT.as_bytes(),
        );

        assert!(verified_managed_completion(&task, &response, &claim).is_ok());
    }

    #[test]
    fn verified_claim_rejects_missing_source_contract() {
        let mut task = managed_task_for_claim("managed-missing-source");
        task.task_source = None;
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::MissingSource
        );
    }

    #[test]
    fn verified_claim_rejects_nonpositive_budget_contract() {
        let mut task = managed_task_for_claim("managed-invalid-budget");
        task.max_cpt = 0;
        let response = managed_response(MANAGED_OUTPUT);
        let claim = managed_claim(
            &task.task_id,
            MANAGED_SOURCE.as_bytes(),
            MANAGED_INPUT.as_bytes(),
            MANAGED_OUTPUT.as_bytes(),
        );

        assert_eq!(
            verified_managed_completion(&task, &response, &claim).unwrap_err(),
            ManagedCompletionError::InvalidBudget
        );
    }

    fn make_worker(id: &str, cpu: i32, mem: i32, status: WorkerStatus) -> WorkerNode {
        WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: id.into(),
            username: "test".into(),
            ip: format!("192.168.1.{}", &id[1..]),
            virtual_ip: None,
            hostname: None,
            cpu_cores: cpu,
            memory_gb: mem,
            cpu_score: cpu * 100,
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
            status,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: mem,
            queue_capacity: cpu,
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

    struct ManagedGpuDispatcherCase {
        task_id: String,
        owner: String,
        provider: String,
        worker_id: String,
        capability: ManagedGpuCapability,
        manifest: Vec<u8>,
    }

    fn managed_gpu_dispatch_request(unique: &str) -> ManagedGpuRequest {
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
            execution_id: format!("dispatcher-gpu-execution-{unique}"),
            attempt_id: format!("dispatcher-gpu-attempt-{unique}"),
            idempotency_key: format!("dispatcher-gpu-idempotency-{unique}"),
            request_digest: String::new(),
            runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
            semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
            operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
            backend_id: "managed-cuda-dispatch-test".into(),
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

    fn managed_gpu_dispatch_capability(request: &ManagedGpuRequest) -> ManagedGpuCapability {
        ManagedGpuCapability::new(
            "cuda-dispatch-test-0",
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

    fn managed_gpu_dispatch_registration(
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

    fn managed_gpu_dispatch_completed_response(
        request: &ManagedGpuRequest,
        capability: &ManagedGpuCapability,
    ) -> ExecuteTaskResponse {
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
            status: ManagedGpuStatus::Completed,
            exit_code: Some(0),
            error_code: None,
            output: String::new(),
            output_sha256: sha256_digest(b""),
            selected_gpu: capability.clone(),
            usage: general_compute_runtime::managed_gpu::ManagedGpuUsage {
                source_bytes: request.source.len() as u64,
                input_bytes: request.input_json.len() as u64,
                executed_operations: 1,
                operation_cost_units: 10,
                ..Default::default()
            },
            evidence: Default::default(),
        };
        ExecuteTaskResponse {
            success: true,
            status_message: "executed".into(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            managed_gpu_result_json: serde_json::to_vec(&result).unwrap(),
            ..ExecuteTaskResponse::default()
        }
    }

    async fn seed_managed_gpu_dispatch_reputation(
        pool: &sqlx::PgPool,
        worker_id: &str,
    ) -> (i64, i64, i32, bool) {
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

    async fn managed_gpu_dispatch_reputation(
        pool: &sqlx::PgPool,
        worker_id: &str,
    ) -> (i64, i64, i32, bool) {
        sqlx::query_as(
            "SELECT successful_tasks, failed_tasks, score, banned
             FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(worker_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn setup_managed_gpu_dispatch_case(
        repo: &TaskRepository,
        unique: &str,
    ) -> ManagedGpuDispatcherCase {
        let owner = format!("dispatcher-gpu-owner-{unique}");
        let provider = format!("dispatcher-gpu-provider-{unique}");
        let worker_id = format!("dispatcher-gpu-worker-{unique}");
        let task_id = format!("dispatcher-gpu-task-{unique}");
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance)
             VALUES ($1, 'hash', 100)",
        )
        .bind(&owner)
        .execute(&repo.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance)
             VALUES ($1, 'hash', 0)",
        )
        .bind(&provider)
        .execute(&repo.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '127.0.0.1:50053', 4, 16)",
        )
        .bind(&worker_id)
        .bind(&provider)
        .execute(&repo.pool)
        .await
        .unwrap();

        let request = managed_gpu_dispatch_request(unique);
        let capability = managed_gpu_dispatch_capability(&request);
        let registration = managed_gpu_dispatch_registration(&request, &capability);
        sqlx::query(
            "UPDATE worker_nodes
             SET general_compute_capabilities_json = $1,
                 admission_mode = $2
             WHERE worker_id = $3",
        )
        .bind(serde_json::to_string(&registration).unwrap())
        .bind(hivemind_models::PRIVATE_STATIC_ADMISSION_MODE)
        .bind(&worker_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        let manifest = serde_json::to_vec(&request).unwrap();
        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.owner = owner.clone();
        task.runtime = Some(MANAGED_GPU_RUNTIME_VERSION.into());
        task.torrent_source = None;
        task.managed_gpu_manifest_json = Some(manifest.clone());
        task.max_cpt = request.reservation_cpt as i64;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "127.0.0.1:50053")
            .await
            .unwrap();

        ManagedGpuDispatcherCase {
            task_id,
            owner,
            provider,
            worker_id,
            capability,
            manifest,
        }
    }

    async fn cleanup_managed_gpu_dispatch_case(
        repo: &TaskRepository,
        case: &ManagedGpuDispatcherCase,
    ) {
        sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM task_attestations WHERE task_id = $1")
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&case.task_id)
            .execute(&repo.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&case.worker_id)
            .execute(&repo.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&case.worker_id)
            .execute(&repo.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username IN ($1, $2)")
            .bind(&case.owner)
            .bind(&case.provider)
            .execute(&repo.pool)
            .await
            .ok();
    }

    #[test]
    fn managed_gpu_dispatch_request_binds_the_new_runtime_identity() {
        let request = managed_gpu_dispatch_request("request-contract");
        assert_eq!(request.runtime_version, MANAGED_GPU_RUNTIME_VERSION);
        assert!(!request.gpu_requirement.allow_cpu_fallback);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn test_dispatcher_creation() {
        assert_eq!(30u64, 30);
    }

    #[test]
    fn test_worker_endpoint_adds_scheme_and_rejects_unspecified_host() {
        let error = worker_endpoint("0.0.0.0:50053").unwrap_err().to_string();
        assert!(error.contains("WORKER_ADVERTISE_ADDR"));
        assert_eq!(
            worker_endpoint("127.0.0.1:50053").unwrap(),
            "http://127.0.0.1:50053"
        );
        assert_eq!(
            worker_endpoint("http://worker:50053").unwrap(),
            "http://worker:50053"
        );
    }

    #[test]
    fn test_build_execute_task_request_uses_task_requirements() {
        let task = make_task("execute-request-1", TaskStatus::Pending, 0);
        let request = build_execute_task_request(&task);
        let limits = request.resource_limits.unwrap();

        assert_eq!(request.task_id, "execute-request-1");
        assert_eq!(request.torrent, "example-btih");
        assert_eq!(limits.cpu_score, 100);
        assert_eq!(limits.memory_mb, 4096);
        assert_eq!(limits.storage_total_gb, 10);
    }

    #[test]
    fn worker_execution_token_is_bound_to_task_and_worker() {
        let task = make_task("bound-dispatch", TaskStatus::Assigned, 0);
        let (private_key, public_key) = hivemind_config::generate_worker_execution_test_key_pair();

        let token = worker_execution_token(&private_key, &task, "worker-bound-7", None).unwrap();
        let claims =
            hivemind_auth::worker_execution::WorkerExecutionVerifier::from_pem(&public_key)
                .unwrap()
                .decode(&token)
                .unwrap();

        assert_eq!(claims.role.as_deref(), Some("worker-execution"));
        assert_eq!(claims.task_id.as_deref(), Some("bound-dispatch"));
        assert_eq!(claims.worker_id.as_deref(), Some("worker-bound-7"));
        // HMAC secrets must not verify asymmetric execution tokens.
        assert!(jsonwebtoken::decode::<serde_json::Value>(
            &token,
            &jsonwebtoken::DecodingKey::from_secret(b"unit-test-control-jwt-secret-32-bytes"),
            &jsonwebtoken::Validation::default(),
        )
        .is_err());
    }

    #[test]
    fn general_compute_execution_token_is_bound_to_attempt_identity() {
        let (mut task, request) = alpha_result_task("bound-general-compute-token");
        task.status = TaskStatus::Assigned;
        let (private_key, public_key) = hivemind_config::generate_worker_execution_test_key_pair();

        let token = worker_execution_token(&private_key, &task, "worker-bound-7", Some(1)).unwrap();
        let claims =
            hivemind_auth::worker_execution::WorkerExecutionVerifier::from_pem(&public_key)
                .unwrap()
                .decode_execution_claims(&token)
                .unwrap();

        assert_eq!(
            claims.claims.task_id.as_deref(),
            Some(task.task_id.as_str())
        );
        assert_eq!(claims.claims.worker_id.as_deref(), Some("worker-bound-7"));
        assert_eq!(
            claims.execution_id.as_deref(),
            Some(request.execution_id.as_str())
        );
        assert_eq!(
            claims.attempt_id.as_deref(),
            Some(request.attempt_id.as_str())
        );
        assert_eq!(
            claims.idempotency_key.as_deref(),
            Some(request.idempotency_key.as_str())
        );
        assert_eq!(
            claims.request_digest.as_deref(),
            Some(request.request_digest.as_str())
        );
    }

    #[test]
    fn general_compute_execution_token_binds_attempt_and_transfer_generation() {
        let (mut task, request) = alpha_result_task("bound-general-compute-token");
        task.status = TaskStatus::Assigned;
        let (private_key, public_key) = hivemind_config::generate_worker_execution_test_key_pair();

        let token = worker_execution_token(&private_key, &task, "worker-bound-7", Some(7)).unwrap();
        let claims =
            hivemind_auth::worker_execution::WorkerExecutionVerifier::from_pem(&public_key)
                .unwrap()
                .decode_execution_claims(&token)
                .unwrap();

        assert_eq!(
            claims.claims.task_id.as_deref(),
            Some(task.task_id.as_str())
        );
        assert_eq!(claims.claims.worker_id.as_deref(), Some("worker-bound-7"));
        assert_eq!(
            claims.execution_id.as_deref(),
            Some(request.execution_id.as_str())
        );
        assert_eq!(
            claims.attempt_id.as_deref(),
            Some(request.attempt_id.as_str())
        );
        assert_eq!(
            claims.idempotency_key.as_deref(),
            Some(request.idempotency_key.as_str())
        );
        assert_eq!(
            claims.request_digest.as_deref(),
            Some(request.request_digest.as_str())
        );
        assert_eq!(claims.transfer_generation, Some(7));
    }
    #[test]
    fn worker_execution_token_reports_private_key_boundary() {
        // Given: a dispatchable task and a missing worker-execution private key.
        let task = make_task("missing-worker-private-key", TaskStatus::Assigned, 0);

        // When: token creation crosses the signing boundary.
        let error = worker_execution_token("", &task, "worker-7", None)
            .unwrap_err()
            .to_string();

        // Then: the failure names the private key, never JWT_SECRET.
        assert!(error.contains("WORKER_EXECUTION_PRIVATE_KEY_PEM"));
        assert!(!error.contains("JWT_SECRET"));
    }

    #[tokio::test]
    async fn dispatcher_db_tests_use_isolated_schema() {
        let (db, fixture) = match test_db("dispatcher_schema_canary").await {
            Some(parts) => parts,
            None => return,
        };

        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&db.pool)
            .await
            .unwrap();

        assert!(
            schema.starts_with("hm_test_"),
            "dispatcher DB tests must use an isolated schema, got {schema}"
        );
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_one_no_workers() {
        let (db, fixture) = match test_db("dispatcher_no_workers").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db, 30, 2);
        let task = make_task("dispatch-test-1", TaskStatus::Pending, 0);
        let result = dispatcher.dispatch_one(&task, &[]).await;
        assert!(result.is_none());
        sqlx::query("DELETE FROM tasks WHERE task_id = 'dispatch-test-1'")
            .execute(&dispatcher.db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_one_with_worker() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_with_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let task = make_task("dispatch-test-2", TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.ok();
        let workers = vec![make_worker("w1", 4, 16, WorkerStatus::Idle)];
        let result = dispatcher.dispatch_one(&task, &workers).await;
        assert!(result.is_some());
        let (wid, wip) = result.unwrap();
        assert_eq!(wid, "w1");
        assert!(wip.contains("192.168"));
        let updated = dispatcher
            .repo
            .find_by_task_id("dispatch-test-2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, TaskStatus::Assigned);
        sqlx::query("DELETE FROM tasks WHERE task_id = 'dispatch-test-2'")
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_one_does_not_overwrite_stale_assignment() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_stale_assignment").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let task_id = "dispatch-stale-assignment";
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .execute(&db.pool)
            .await
            .ok();

        let stale_task = make_task(task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&stale_task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(task_id, "w1", "192.168.1.1")
            .await
            .unwrap();

        let workers = vec![make_worker("w2", 4, 16, WorkerStatus::Idle)];
        let result = dispatcher.dispatch_one(&stale_task, &workers).await;
        assert!(result.is_none());

        let stored = dispatcher
            .repo
            .find_by_task_id(task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.worker_id.as_deref(), Some("w1"));
        assert_eq!(stored.worker_ip.as_deref(), Some("192.168.1.1"));

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_execute_on_worker_skips_task_cancelled_after_assignment() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_cancelled_after_assignment").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-cancel-race-{unique}");
        let worker_id = format!("dispatch-cancel-race-w-{unique}");
        let (worker_addr, mut execute_rx) = match fake_worker_execute_server().await {
            Some(parts) => parts,
            None => return,
        };

        let task = make_task(&task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();
        dispatcher.repo.cancel(&task_id).await.unwrap();

        let result = execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            "test-secret",
            ManagedProofRolloutMode::Enforce,
        )
        .await;

        assert!(
            result.is_ok(),
            "execution skip should not be an error: {result:?}"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(250), execute_rx.recv())
                .await
                .is_err(),
            "cancelled task should not be sent to worker"
        );
        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Cancelled);

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_execute_on_worker_redispatches_after_connect_failure_without_worker_penalty() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_connect_failure_redispatch").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-connect-failure-{unique}");
        let worker_id = format!("dispatch-connect-worker-{unique}");
        let unavailable_addr = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            drop(listener);
            addr.to_string()
        };

        let task = make_task(&task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &unavailable_addr)
            .await
            .unwrap();

        // Liveness guard only: it must return rather than hang, so the state
        // assertions below mean something. It is not a latency budget, and it
        // must stay above the production WORKER_CONNECT_TIMEOUT of 5s — a
        // refused loopback connect measured 2.04s here, so the original 2s
        // budget failed on timing alone once the test had a database to run
        // against.
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            execute_on_worker(
                dispatcher.repo.clone(),
                task,
                worker_id,
                unavailable_addr,
                "not-used-after-connect-failure",
                ManagedProofRolloutMode::Enforce,
            ),
        )
        .await;

        assert!(
            result.is_ok(),
            "connect failure must not leave a task assigned"
        );
        assert!(
            result.unwrap().is_ok(),
            "connect failure must be redispatched rather than returned"
        );
        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert_eq!(stored.status_message.as_deref(), Some("Redispatched"));
        assert!(stored.worker_id.is_none());
        assert!(stored.worker_ip.is_none());
        assert_eq!(stored.retry_count, 1);

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_execute_on_worker_fails_closed_when_execution_token_signing_fails() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_token_signing_failure").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("dispatch-token-owner-{unique}");
        let worker_id = format!("dispatch-token-worker-{unique}");
        let task_id = format!("dispatch-token-task-{unique}");
        let (worker_addr, mut execute_rx) = match fake_worker_execute_server().await {
            Some(parts) => parts,
            None => return,
        };

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '10.0.0.77', 4, 16)",
        )
        .bind(&worker_id)
        .bind(format!("provider-{unique}"))
        .execute(&db.pool)
        .await
        .unwrap();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.owner = username.clone();
        task.runtime = Some("managed-function-v0".into());
        task.task_source = Some("return 7;".into());
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();

        let result = execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            "not-a-valid-ed25519-private-key",
            ManagedProofRolloutMode::Enforce,
        )
        .await;

        assert!(
            result.is_ok(),
            "signing failure must be contained: {result:?}"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(250), execute_rx.recv())
                .await
                .is_err(),
            "worker must not receive an unsigned execution request"
        );
        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert!(stored
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("WORKER_EXECUTION_PRIVATE_KEY_PEM")));

        if let Some(failed_tasks) = sqlx::query_scalar::<_, i64>(
            "SELECT failed_tasks FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&worker_id)
        .fetch_optional(&db.pool)
        .await
        .unwrap()
        {
            assert_eq!(failed_tasks, 0);
        }
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_execute_on_worker_rejects_managed_task_without_proof() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_managed_worker_receipt").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("dispatch-managed-owner-{unique}");
        let worker_id = format!("dispatch-managed-worker-{unique}");
        let task_id = format!("dispatch-managed-task-{unique}");
        let receipt_json = "{\"executed_ops\":2500,\"output_bytes\":2049}".to_string();
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "7".into(),
            managed_executed_ops: 2_500,
            managed_output_bytes: 2_049,
            managed_receipt_json: receipt_json.clone(),
            managed_proof: None,
            ..ExecuteTaskResponse::default()
        };
        let (worker_addr, mut execute_rx) =
            match fake_worker_execute_server_with_response(response).await {
                Some(parts) => parts,
                None => return,
            };

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '10.0.0.2', 4, 16)",
        )
        .bind(&worker_id)
        .bind(format!("provider-{unique}"))
        .execute(&db.pool)
        .await
        .unwrap();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.owner = username.clone();
        task.runtime = Some("managed-function-v0".into());
        task.task_source = Some("return 7;".into());
        task.max_cpt = 25;
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();

        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();
        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            &private_key,
            ManagedProofRolloutMode::Enforce,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), execute_rx.recv())
                .await
                .unwrap()
                .as_deref(),
            Some(task_id.as_str())
        );
        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Failed);
        assert_eq!(
            stored.status_message.as_deref(),
            Some("Managed proof is required")
        );
        assert!(!stored.billing_settled);
        assert_eq!(stored.billed_amount, 0);
        assert_eq!(stored.managed_output_bytes, 0);
        assert_eq!(stored.managed_executed_ops, 0);
        assert!(stored.managed_receipt_json.is_none());

        // The audit trail is the operator-visible record of every settlement
        // decision, so exercise the real INSERT rather than trusting that the
        // statement matches the admin_audit_logs schema.
        let (audit_event, audit_mode, audit_worker): (String, String, String) = sqlx::query_as(
            "SELECT detail->>'event', detail->>'rollout_mode', detail->>'worker_id'
             FROM admin_audit_logs
             WHERE action = 'managed_proof_verification' AND target_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&db.pool)
        .await
        .expect("a rejected managed proof must leave one audit entry");
        assert_eq!(audit_event, "rejected");
        assert_eq!(audit_mode, "enforce");
        assert_eq!(audit_worker, worker_id);

        sqlx::query("DELETE FROM admin_audit_logs WHERE target_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM task_attestations WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    /// `off` is the emergency rollback mode: it must still settle the task, but
    /// it must say so in the audit trail, because the settlement no longer
    /// rests on anything the nodepool verified.
    #[tokio::test]
    async fn test_execute_on_worker_off_mode_settles_managed_task_and_audits_legacy() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_managed_rollout_off").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("dispatch-off-owner-{unique}");
        let worker_id = format!("dispatch-off-worker-{unique}");
        let task_id = format!("dispatch-off-task-{unique}");
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "7".into(),
            managed_executed_ops: 2_500,
            managed_output_bytes: 2_049,
            managed_receipt_json: "{\"executed_ops\":2500,\"output_bytes\":2049}".into(),
            managed_proof: None,
            ..ExecuteTaskResponse::default()
        };
        let (worker_addr, _execute_rx) =
            match fake_worker_execute_server_with_response(response).await {
                Some(parts) => parts,
                None => return,
            };

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '10.0.0.2', 4, 16)",
        )
        .bind(&worker_id)
        .bind(format!("provider-{unique}"))
        .execute(&db.pool)
        .await
        .unwrap();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.owner = username.clone();
        task.runtime = Some("managed-function-v0".into());
        task.task_source = Some("return 7;".into());
        task.max_cpt = 25;
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();

        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();
        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            &private_key,
            ManagedProofRolloutMode::Off,
        )
        .await
        .unwrap();

        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Completed);
        // Settlement happened, but not from a verified claim: the verified path
        // is the only one that writes a managed receipt.
        assert!(stored.managed_receipt_json.is_none());

        let (audit_event, audit_mode): (String, String) = sqlx::query_as(
            "SELECT detail->>'event', detail->>'rollout_mode'
             FROM admin_audit_logs
             WHERE action = 'managed_proof_verification' AND target_id = $1",
        )
        .bind(&task_id)
        .fetch_one(&db.pool)
        .await
        .expect("an unverified managed settlement must be auditable");
        assert_eq!(audit_event, "legacy_settlement");
        assert_eq!(audit_mode, "off");

        sqlx::query("DELETE FROM admin_audit_logs WHERE target_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM task_attestations WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_pending_from_registered_workers() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_registered_workers").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-registered-{}", unique);
        let worker_id = format!("dispatch-registered-w-{}", unique);

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();

        let task = make_task(&task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.unwrap();
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb,
             cpu_score, gpu_score, gpu_memory_gb, gpu_name, vram_mb,
             storage_total_gb, storage_available_gb, location, status, available_memory_gb, queue_capacity)
             VALUES ($1,'test','127.0.0.1:50053',4,16,400,0,0,NULL,0,500,200,'local','IDLE',16,4)",
        )
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 10, 0, 100, false)",
        )
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .unwrap();

        let count = dispatcher
            .dispatch_pending_from_registered_workers()
            .await
            .unwrap();
        assert!(count >= 1);

        let updated = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, TaskStatus::Assigned);
        assert_eq!(updated.worker_id.as_deref(), Some(worker_id.as_str()));

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_pending_multiple() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_pending_multiple").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        sqlx::query("DELETE FROM tasks WHERE task_id LIKE 'dispatch-multi-%'")
            .execute(&db.pool)
            .await
            .ok();
        for i in 0..3 {
            let task = make_task(&format!("dispatch-multi-{}", i), TaskStatus::Pending, 0);
            dispatcher.repo.create(&task).await.ok();
        }
        let workers = vec![
            make_worker("wm1", 8, 32, WorkerStatus::Idle),
            make_worker("wm2", 8, 32, WorkerStatus::Idle),
        ];
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ('wm1', 10, 0, 100, false), ('wm2', 10, 0, 100, false)
             ON CONFLICT (worker_id) DO UPDATE SET score = EXCLUDED.score, banned = EXCLUDED.banned",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        let count = dispatcher.dispatch_pending(&workers).await.unwrap();
        assert!(count >= 1);
        for i in 0..3 {
            sqlx::query("DELETE FROM tasks WHERE task_id = $1")
                .bind(format!("dispatch-multi-{}", i))
                .execute(&db.pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ('wm1', 'wm2')")
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_pending_excludes_banned_worker_by_trust() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_excludes_banned_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-trust-banned-{}", unique);
        let banned_worker_id = format!("dispatch-trust-banned-w-{}", unique);
        let trusted_worker_id = format!("dispatch-trust-ok-w-{}", unique);

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
            .bind(&banned_worker_id)
            .bind(&trusted_worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
            .bind(&banned_worker_id)
            .bind(&trusted_worker_id)
            .execute(&db.pool)
            .await
            .ok();

        let task = make_task(&task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.unwrap();

        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb,
             cpu_score, gpu_score, gpu_memory_gb, gpu_name, vram_mb,
             storage_total_gb, storage_available_gb, location, status, available_memory_gb, queue_capacity)
             VALUES
             ($1,'test','127.0.0.1:50053',4,16,400,0,0,NULL,0,500,200,'local','IDLE',16,4),
             ($2,'test','127.0.0.1:50054',4,16,400,0,0,NULL,0,500,200,'local','IDLE',16,4)",
        )
        .bind(&banned_worker_id)
        .bind(&trusted_worker_id)
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES
             ($1, 10, 0, 200, true),
             ($2, 10, 0, 200, false)",
        )
        .bind(&banned_worker_id)
        .bind(&trusted_worker_id)
        .execute(&db.pool)
        .await
        .unwrap();

        let count = dispatcher
            .dispatch_pending_from_registered_workers()
            .await
            .unwrap();
        assert!(count >= 1);

        let updated = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, TaskStatus::Assigned);
        assert_eq!(
            updated.worker_id.as_deref(),
            Some(trusted_worker_id.as_str())
        );

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
            .bind(&banned_worker_id)
            .bind(&trusted_worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
            .bind(&banned_worker_id)
            .bind(&trusted_worker_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_rank_workers_prefers_worker_with_cache_affinity() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_cache_affinity").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);

        let task_id = format!("dispatch-cache-target-{}", uuid::Uuid::new_v4());
        let hist_task_id = format!("dispatch-cache-hist-{}", uuid::Uuid::new_v4());
        let worker_a = format!("dispatch-cache-a-{}", uuid::Uuid::new_v4());
        let worker_b = format!("dispatch-cache-b-{}", uuid::Uuid::new_v4());
        let torrent = "magnet:?xt=urn:btih:cache-affinity";

        sqlx::query("DELETE FROM tasks WHERE task_id IN ($1, $2)")
            .bind(&task_id)
            .bind(&hist_task_id)
            .execute(&db.pool)
            .await
            .ok();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.torrent_source = Some(torrent.into());
        dispatcher.repo.create(&task).await.unwrap();

        sqlx::query(
            "INSERT INTO tasks (
                task_id, owner, worker_id, worker_ip, status, torrent_source,
                req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
                host_count, max_cpt, billing_settled, billed_amount, max_retries,
                deterministic, side_effects, priority, cache_hits, created_at, last_update, completed_at
             ) VALUES (
                $1, 'example-user', $2, '127.0.0.1', 'COMPLETED', $3,
                100, 0, 4, 0, 10,
                1, 1000, true, 1000, 3,
                false, false, 0, 0, NOW() - INTERVAL '30 days', NOW() - INTERVAL '30 days', NOW() - INTERVAL '30 days'
             )",
        )
        .bind(&hist_task_id)
        .bind(&worker_a)
        .bind(torrent)
        .execute(&db.pool)
        .await
        .unwrap();

        let hist_task_recent = format!("dispatch-cache-hist-recent-{}", uuid::Uuid::new_v4());
        sqlx::query(
            "INSERT INTO tasks (
                task_id, owner, worker_id, worker_ip, status, torrent_source,
                req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
                host_count, max_cpt, billing_settled, billed_amount, max_retries,
                deterministic, side_effects, priority, cache_hits, created_at, last_update, completed_at
             ) VALUES (
                $1, 'example-user', $2, '127.0.0.1', 'COMPLETED', $3,
                100, 0, 4, 0, 10,
                1, 1000, true, 1000, 3,
                false, false, 0, 1, NOW(), NOW(), NOW()
             )",
        )
        .bind(&hist_task_recent)
        .bind(&worker_b)
        .bind(torrent)
        .execute(&db.pool)
        .await
        .unwrap();

        let workers = vec![
            make_worker(&worker_a, 4, 16, WorkerStatus::Idle),
            make_worker(&worker_b, 4, 16, WorkerStatus::Idle),
        ];

        let ranked = dispatcher
            .rank_workers_by_cache_affinity(&task, &workers)
            .await
            .unwrap();
        assert_eq!(
            ranked.first().map(|w| w.worker_id.as_str()),
            Some(worker_b.as_str())
        );

        sqlx::query("DELETE FROM tasks WHERE task_id IN ($1, $2, $3)")
            .bind(&task_id)
            .bind(&hist_task_id)
            .bind(&hist_task_recent)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    async fn blocking_worker_execute_server() -> Option<(
        SocketAddr,
        tokio::sync::mpsc::Receiver<ExecuteTaskRequest>,
        oneshot::Sender<ExecuteTaskResponse>,
    )> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let addr = listener.local_addr().ok()?;
        let (request_tx, request_rx) = tokio::sync::mpsc::channel(1);
        let (response_tx, response_rx) = oneshot::channel();
        let service = WorkerNodeServiceServer::new(BlockingWorkerExecuteService {
            request_tx,
            response_rx: tokio::sync::Mutex::new(Some(response_rx)),
        });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::worker_node_service_client::WorkerNodeServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, request_rx, response_tx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    async fn worker_only_execute_server(
    ) -> Option<(SocketAddr, tokio::sync::mpsc::Receiver<String>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let addr = listener.local_addr().ok()?;
        let (execute_tx, execute_rx) = tokio::sync::mpsc::channel(1);
        let service = WorkerNodeServiceServer::new(FakeWorkerExecuteService {
            execute_tx,
            response: ExecuteTaskResponse::default(),
            execute_error: None,
        });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::worker_node_service_client::WorkerNodeServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, execute_rx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    async fn fake_worker_execute_server(
    ) -> Option<(SocketAddr, tokio::sync::mpsc::Receiver<String>)> {
        fake_worker_execute_server_with_response(ExecuteTaskResponse {
            success: true,
            status_message: "executed".into(),
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: String::new(),
            managed_proof: None,
            ..ExecuteTaskResponse::default()
        })
        .await
    }

    async fn fake_worker_execute_server_with_response(
        response: ExecuteTaskResponse,
    ) -> Option<(SocketAddr, tokio::sync::mpsc::Receiver<String>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let addr = listener.local_addr().ok()?;
        let (execute_tx, execute_rx) = tokio::sync::mpsc::channel(1);
        let service = WorkerNodeServiceServer::new(FakeWorkerExecuteService {
            execute_tx,
            response,
            execute_error: None,
        });
        let chunks = GeneralComputeChunkServiceServer::new(PermissiveChunkService);
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .add_service(chunks)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::worker_node_service_client::WorkerNodeServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, execute_rx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    async fn worker_error_execute_server(
        code: tonic::Code,
        message: &str,
    ) -> Option<(SocketAddr, tokio::sync::mpsc::Receiver<String>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let addr = listener.local_addr().ok()?;
        let (execute_tx, execute_rx) = tokio::sync::mpsc::channel(1);
        let service = WorkerNodeServiceServer::new(FakeWorkerExecuteService {
            execute_tx,
            response: ExecuteTaskResponse::default(),
            execute_error: Some((code, message.to_owned())),
        });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::worker_node_service_client::WorkerNodeServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, execute_rx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    async fn fake_worker_execute_and_chunk_server() -> Option<(
        SocketAddr,
        tokio::sync::mpsc::Receiver<GeneralComputePrepareRequest>,
        tokio::sync::mpsc::Receiver<GeneralComputeChunkResumeRequest>,
        tokio::sync::mpsc::Receiver<GeneralComputeChunkUpload>,
    )> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let addr = listener.local_addr().ok()?;
        let (execute_tx, _execute_rx) = tokio::sync::mpsc::channel(1);
        let (prepare_tx, prepare_rx) = tokio::sync::mpsc::channel(1);
        let (resume_tx, resume_rx) = tokio::sync::mpsc::channel(1);
        let (upload_tx, upload_rx) = tokio::sync::mpsc::channel(1);
        let worker = WorkerNodeServiceServer::new(FakeWorkerExecuteService {
            execute_tx,
            response: ExecuteTaskResponse::default(),
            execute_error: None,
        });
        let chunks = GeneralComputeChunkServiceServer::new(FakeGeneralComputeChunkService {
            prepare_tx,
            upload_tx,
            resume_tx: Some(resume_tx),
            resume_missing: vec![hivemind_proto::GeneralComputeChunkDescriptor {
                offset: 0,
                size_bytes: 6,
                sha256: sha256_digest(b"source"),
            }],
        });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(worker)
                .add_service(chunks)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::general_compute_chunk_service_client::GeneralComputeChunkServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, prepare_rx, resume_rx, upload_rx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    async fn fake_general_compute_chunk_server(
        resume_missing: Vec<GeneralComputeChunkDescriptor>,
    ) -> Option<(
        SocketAddr,
        tokio::sync::mpsc::Receiver<GeneralComputePrepareRequest>,
        tokio::sync::mpsc::Receiver<GeneralComputeChunkResumeRequest>,
        tokio::sync::mpsc::Receiver<GeneralComputeChunkUpload>,
    )> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let addr = listener.local_addr().ok()?;
        let (prepare_tx, prepare_rx) = tokio::sync::mpsc::channel(1);
        let (resume_tx, resume_rx) = tokio::sync::mpsc::channel(1);
        let (upload_tx, upload_rx) = tokio::sync::mpsc::channel(2);
        let service = GeneralComputeChunkServiceServer::new(FakeGeneralComputeChunkService {
            prepare_tx,
            resume_tx: Some(resume_tx),
            upload_tx,
            resume_missing,
        });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::general_compute_chunk_service_client::GeneralComputeChunkServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, prepare_rx, resume_rx, upload_rx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    struct FakeGeneralComputeChunkService {
        prepare_tx: tokio::sync::mpsc::Sender<GeneralComputePrepareRequest>,
        upload_tx: tokio::sync::mpsc::Sender<GeneralComputeChunkUpload>,
        resume_tx: Option<tokio::sync::mpsc::Sender<GeneralComputeChunkResumeRequest>>,
        resume_missing: Vec<hivemind_proto::GeneralComputeChunkDescriptor>,
    }

    #[tonic::async_trait]
    impl GeneralComputeChunkService for FakeGeneralComputeChunkService {
        async fn prepare_general_compute(
            &self,
            request: Request<GeneralComputePrepareRequest>,
        ) -> Result<Response<GeneralComputePrepareResponse>, Status> {
            let request = request.into_inner();
            let response = GeneralComputePrepareResponse {
                success: true,
                status_message: "prepared".into(),
                execution_id: request.execution_id.clone(),
                attempt_id: request.attempt_id.clone(),
                idempotency_key: request.idempotency_key.clone(),
                request_digest: request.request_digest.clone(),
                transfer_generation: request.transfer_generation,
            };
            self.prepare_tx
                .send(request)
                .await
                .map_err(|_| Status::internal("prepare capture closed"))?;
            Ok(Response::new(response))
        }

        async fn upload_chunk(
            &self,
            request: Request<GeneralComputeChunkUpload>,
        ) -> Result<Response<GeneralComputeChunkUploadResponse>, Status> {
            self.upload_tx
                .send(request.into_inner())
                .await
                .map_err(|_| Status::internal("upload capture closed"))?;
            Ok(Response::new(GeneralComputeChunkUploadResponse {
                success: true,
                status_message: "accepted".into(),
                accepted_chunks: 1,
            }))
        }

        async fn resume_chunks(
            &self,
            request: Request<GeneralComputeChunkResumeRequest>,
        ) -> Result<Response<GeneralComputeChunkResumeResponse>, Status> {
            if let Some(sender) = &self.resume_tx {
                sender
                    .send(request.into_inner())
                    .await
                    .map_err(|_| Status::internal("resume capture closed"))?;
            }
            Ok(Response::new(GeneralComputeChunkResumeResponse {
                success: true,
                status_message: "resume".into(),
                missing_chunks: self.resume_missing.clone(),
            }))
        }
    }

    #[derive(Debug)]
    enum TransportCall {
        Prepare(GeneralComputePrepareRequest),
        Resume(GeneralComputeChunkResumeRequest),
        Upload(GeneralComputeChunkUpload),
        Execute(Box<ExecuteTaskRequest>),
    }

    async fn authenticated_general_compute_worker_server(
        missing: GeneralComputeChunkDescriptor,
    ) -> Option<(SocketAddr, tokio::sync::mpsc::Receiver<TransportCall>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let addr = listener.local_addr().ok()?;
        let (calls_tx, calls_rx) = tokio::sync::mpsc::channel(8);
        let worker = WorkerNodeServiceServer::new(RecordingWorkerExecuteService {
            calls_tx: calls_tx.clone(),
        });
        let chunks =
            GeneralComputeChunkServiceServer::new(RecordingChunkService { calls_tx, missing });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(worker)
                .add_service(chunks)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::general_compute_chunk_service_client::GeneralComputeChunkServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, calls_rx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    struct RecordingChunkService {
        calls_tx: tokio::sync::mpsc::Sender<TransportCall>,
        missing: GeneralComputeChunkDescriptor,
    }

    #[tonic::async_trait]
    impl GeneralComputeChunkService for RecordingChunkService {
        async fn prepare_general_compute(
            &self,
            request: Request<GeneralComputePrepareRequest>,
        ) -> Result<Response<GeneralComputePrepareResponse>, Status> {
            let request = request.into_inner();
            let response = GeneralComputePrepareResponse {
                success: true,
                status_message: "prepared".into(),
                execution_id: request.execution_id.clone(),
                attempt_id: request.attempt_id.clone(),
                idempotency_key: request.idempotency_key.clone(),
                request_digest: request.request_digest.clone(),
                transfer_generation: request.transfer_generation,
            };
            self.calls_tx
                .send(TransportCall::Prepare(request))
                .await
                .map_err(|_| Status::internal("transport call receiver closed"))?;
            Ok(Response::new(response))
        }

        async fn upload_chunk(
            &self,
            request: Request<GeneralComputeChunkUpload>,
        ) -> Result<Response<GeneralComputeChunkUploadResponse>, Status> {
            self.calls_tx
                .send(TransportCall::Upload(request.into_inner()))
                .await
                .map_err(|_| Status::internal("transport call receiver closed"))?;
            Ok(Response::new(GeneralComputeChunkUploadResponse {
                success: true,
                status_message: "accepted".into(),
                accepted_chunks: 1,
            }))
        }

        async fn resume_chunks(
            &self,
            request: Request<GeneralComputeChunkResumeRequest>,
        ) -> Result<Response<GeneralComputeChunkResumeResponse>, Status> {
            self.calls_tx
                .send(TransportCall::Resume(request.into_inner()))
                .await
                .map_err(|_| Status::internal("transport call receiver closed"))?;
            Ok(Response::new(GeneralComputeChunkResumeResponse {
                success: true,
                status_message: "resume".into(),
                missing_chunks: vec![self.missing.clone()],
            }))
        }
    }

    struct RecordingWorkerExecuteService {
        calls_tx: tokio::sync::mpsc::Sender<TransportCall>,
    }

    #[tonic::async_trait]
    impl WorkerNodeService for RecordingWorkerExecuteService {
        async fn execute_task(
            &self,
            request: Request<ExecuteTaskRequest>,
        ) -> Result<Response<ExecuteTaskResponse>, Status> {
            let request = request.into_inner();
            let response = ExecuteTaskResponse {
                success: false,
                status_message: "fixture stops after transport".into(),
                execution_id: request.execution_id.clone(),
                attempt_id: request.attempt_id.clone(),
                idempotency_key: request.idempotency_key.clone(),
                request_digest: request.request_digest.clone(),
                ..ExecuteTaskResponse::default()
            };
            self.calls_tx
                .send(TransportCall::Execute(Box::new(request)))
                .await
                .map_err(|_| Status::internal("transport call receiver closed"))?;
            Ok(Response::new(response))
        }

        async fn task_output_upload(
            &self,
            _request: Request<TaskOutputUploadRequest>,
        ) -> Result<Response<TaskOutputUploadResponse>, Status> {
            Err(Status::unimplemented("fixture does not upload output"))
        }

        async fn task_result_upload(
            &self,
            _request: Request<TaskResultUploadRequest>,
        ) -> Result<Response<TaskResultUploadResponse>, Status> {
            Err(Status::unimplemented("fixture does not upload results"))
        }

        async fn task_output(
            &self,
            _request: Request<TaskOutputRequest>,
        ) -> Result<Response<TaskOutputResponse>, Status> {
            Err(Status::unimplemented("fixture has no output"))
        }

        async fn stop_task_execution(
            &self,
            _request: Request<StopTaskExecutionRequest>,
        ) -> Result<Response<StopTaskExecutionResponse>, Status> {
            Ok(Response::new(StopTaskExecutionResponse {
                success: true,
                status_message: "Stop requested".into(),
            }))
        }

        async fn task_usage(
            &self,
            _request: Request<TaskUsageRequest>,
        ) -> Result<Response<TaskUsageResponse>, Status> {
            Err(Status::unimplemented("fixture does not report usage"))
        }
    }

    struct PermissiveChunkService;

    #[tonic::async_trait]
    impl GeneralComputeChunkService for PermissiveChunkService {
        async fn prepare_general_compute(
            &self,
            request: Request<GeneralComputePrepareRequest>,
        ) -> Result<Response<GeneralComputePrepareResponse>, Status> {
            let request = request.into_inner();
            Ok(Response::new(GeneralComputePrepareResponse {
                success: true,
                status_message: "prepared".into(),
                execution_id: request.execution_id,
                attempt_id: request.attempt_id,
                idempotency_key: request.idempotency_key,
                request_digest: request.request_digest,
                transfer_generation: request.transfer_generation,
            }))
        }

        async fn upload_chunk(
            &self,
            _request: Request<GeneralComputeChunkUpload>,
        ) -> Result<Response<GeneralComputeChunkUploadResponse>, Status> {
            Ok(Response::new(GeneralComputeChunkUploadResponse {
                success: true,
                status_message: "accepted".into(),
                accepted_chunks: 1,
            }))
        }

        async fn resume_chunks(
            &self,
            _request: Request<GeneralComputeChunkResumeRequest>,
        ) -> Result<Response<GeneralComputeChunkResumeResponse>, Status> {
            Ok(Response::new(GeneralComputeChunkResumeResponse {
                success: true,
                status_message: "resume".into(),
                missing_chunks: Vec::new(),
            }))
        }
    }
    struct BlockingWorkerExecuteService {
        request_tx: tokio::sync::mpsc::Sender<ExecuteTaskRequest>,
        response_rx: tokio::sync::Mutex<Option<oneshot::Receiver<ExecuteTaskResponse>>>,
    }

    #[tonic::async_trait]
    impl WorkerNodeService for BlockingWorkerExecuteService {
        async fn execute_task(
            &self,
            request: Request<ExecuteTaskRequest>,
        ) -> Result<Response<ExecuteTaskResponse>, Status> {
            self.request_tx
                .send(request.into_inner())
                .await
                .map_err(|_| Status::internal("execute request capture closed"))?;
            let receiver = self
                .response_rx
                .lock()
                .await
                .take()
                .ok_or_else(|| Status::internal("execute response receiver already used"))?;
            let response = receiver
                .await
                .map_err(|_| Status::cancelled("test response was not released"))?;
            Ok(Response::new(response))
        }

        async fn task_output_upload(
            &self,
            _request: Request<TaskOutputUploadRequest>,
        ) -> Result<Response<TaskOutputUploadResponse>, Status> {
            Err(Status::unimplemented(
                "blocking worker does not upload output",
            ))
        }

        async fn task_result_upload(
            &self,
            _request: Request<TaskResultUploadRequest>,
        ) -> Result<Response<TaskResultUploadResponse>, Status> {
            Err(Status::unimplemented(
                "blocking worker does not upload results",
            ))
        }

        async fn task_output(
            &self,
            _request: Request<TaskOutputRequest>,
        ) -> Result<Response<TaskOutputResponse>, Status> {
            Err(Status::unimplemented("blocking worker has no output"))
        }

        async fn stop_task_execution(
            &self,
            _request: Request<StopTaskExecutionRequest>,
        ) -> Result<Response<StopTaskExecutionResponse>, Status> {
            Ok(Response::new(StopTaskExecutionResponse {
                success: true,
                status_message: "Stop requested".into(),
            }))
        }

        async fn task_usage(
            &self,
            _request: Request<TaskUsageRequest>,
        ) -> Result<Response<TaskUsageResponse>, Status> {
            Err(Status::unimplemented(
                "blocking worker does not report usage",
            ))
        }
    }

    struct FakeWorkerExecuteService {
        execute_tx: tokio::sync::mpsc::Sender<String>,
        response: ExecuteTaskResponse,
        execute_error: Option<(tonic::Code, String)>,
    }

    #[tonic::async_trait]
    impl WorkerNodeService for FakeWorkerExecuteService {
        async fn execute_task(
            &self,
            request: Request<ExecuteTaskRequest>,
        ) -> Result<Response<ExecuteTaskResponse>, Status> {
            let task_id = request.into_inner().task_id;
            let _ = self.execute_tx.send(task_id).await;
            if let Some((code, message)) = &self.execute_error {
                return Err(Status::new(*code, message.clone()));
            }
            Ok(Response::new(self.response.clone()))
        }

        async fn task_output_upload(
            &self,
            _request: Request<TaskOutputUploadRequest>,
        ) -> Result<Response<TaskOutputUploadResponse>, Status> {
            Err(Status::unimplemented("fake worker does not upload output"))
        }

        async fn task_result_upload(
            &self,
            _request: Request<TaskResultUploadRequest>,
        ) -> Result<Response<TaskResultUploadResponse>, Status> {
            Err(Status::unimplemented("fake worker does not upload results"))
        }

        async fn task_output(
            &self,
            _request: Request<TaskOutputRequest>,
        ) -> Result<Response<TaskOutputResponse>, Status> {
            Err(Status::unimplemented("fake worker has no output"))
        }

        async fn stop_task_execution(
            &self,
            _request: Request<StopTaskExecutionRequest>,
        ) -> Result<Response<StopTaskExecutionResponse>, Status> {
            Ok(Response::new(StopTaskExecutionResponse {
                success: true,
                status_message: "Stop requested".into(),
            }))
        }

        async fn task_usage(
            &self,
            _request: Request<TaskUsageRequest>,
        ) -> Result<Response<TaskUsageResponse>, Status> {
            Err(Status::unimplemented("fake worker does not report usage"))
        }
    }

    #[test]
    fn reserve_worker_for_batch_prefers_the_other_idle_worker() {
        let mut workers = vec![
            make_worker("worker-a", 4, 16, WorkerStatus::Idle),
            make_worker("worker-b", 4, 16, WorkerStatus::Idle),
        ];
        reserve_worker_for_batch(&mut workers, "worker-a");

        let task = make_task("batch-fairness", TaskStatus::Pending, 0);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let selected = runtime.block_on(scheduler::find_best_worker(&task, &workers));

        assert_eq!(
            selected.map(|worker| worker.worker_id),
            Some("worker-b".into())
        );
        assert_eq!(workers[0].status, WorkerStatus::Busy);
        assert_eq!(workers[0].queue_capacity, 3);
    }
}

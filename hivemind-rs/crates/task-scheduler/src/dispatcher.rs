use crate::managed_proof_verifier::{verify_managed_proof, ManagedProofVerifierError};
use anyhow::Result;
use hivemind_auth::worker_execution::WorkerExecutionSigner;
use hivemind_config::ManagedProofRolloutMode;
use hivemind_database::DatabaseManager;
use hivemind_managed_proof::{ClaimError, ExecutionClaim};
use hivemind_models::{Claims, Task, TaskStatus, WorkerNode};
use hivemind_proto::{
    worker_node_service_client::WorkerNodeServiceClient, ExecuteTaskRequest, ExecuteTaskResponse,
    ManagedProofEnvelope, ResourceSpec as ProtoResourceSpec, GENERAL_COMPUTE_RESULT_MAX_BYTES,
    LEGACY_MANAGED_RECEIPT_MAX_BYTES, WORKER_RPC_MESSAGE_MAX_BYTES,
    WORKER_STATUS_MESSAGE_MAX_BYTES,
};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::managed_proof_metrics::{self, ManagedProofMetricEvent};
use crate::scheduler;
use crate::task_repository::TaskRepository;

pub struct Dispatcher {
    repo: Arc<TaskRepository>,
    db: DatabaseManager,
    task_timeout_secs: u64,
    max_redispatch: i32,
    worker_execution_private_key_pem: String,
    managed_proof_rollout_mode: ManagedProofRolloutMode,
}

impl Dispatcher {
    pub fn new(db: DatabaseManager, task_timeout_secs: u64, max_redispatch: i32) -> Self {
        Self {
            repo: Arc::new(TaskRepository::new(db.pool.clone())),
            db,
            task_timeout_secs,
            max_redispatch,
            worker_execution_private_key_pem: std::env::var("WORKER_EXECUTION_PRIVATE_KEY_PEM")
                .unwrap_or_default(),
            managed_proof_rollout_mode: ManagedProofRolloutMode::Enforce,
        }
    }

    pub fn with_worker_execution_private_key(mut self, private_key_pem: String) -> Self {
        self.worker_execution_private_key_pem = private_key_pem;
        self
    }

    pub fn with_managed_proof_rollout_mode(
        mut self,
        rollout_mode: ManagedProofRolloutMode,
    ) -> Self {
        self.managed_proof_rollout_mode = rollout_mode;
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
        match self.repo.assign_to_worker(&task.task_id, &wid, &wip).await {
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

    pub async fn dispatch_pending(&self, workers: &[WorkerNode]) -> Result<u64> {
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
                let repo = self.repo.clone();
                let task = task.clone();
                let worker_execution_private_key_pem =
                    self.worker_execution_private_key_pem.clone();
                let managed_proof_rollout_mode = self.managed_proof_rollout_mode;
                tokio::spawn(async move {
                    if let Err(e) = execute_on_worker(
                        repo,
                        task,
                        worker_id,
                        worker_addr,
                        &worker_execution_private_key_pem,
                        managed_proof_rollout_mode,
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
            if task.retry_count >= self.max_redispatch {
                if let Some(worker_id) = task.worker_id.as_deref() {
                    match self
                        .repo
                        .fail_for_worker(
                            &task.task_id,
                            worker_id,
                            "Max redispatch attempts exceeded",
                        )
                        .await
                    {
                        Ok(_) => {
                            warn!(
                                "Task {} failed after {} retries",
                                task.task_id, task.retry_count
                            );
                            failed += 1;
                        }
                        Err(e) => error!("Failed to mark task {} as failed: {}", task.task_id, e),
                    }
                }
            } else if let Some(worker_id) = task.worker_id.as_deref() {
                match self
                    .repo
                    .reset_to_pending_for_worker(&task.task_id, worker_id)
                    .await
                {
                    Ok(_) => {
                        info!(
                            "Task {} reset to pending (retry {}/{})",
                            task.task_id,
                            task.retry_count + 1,
                            self.max_redispatch
                        );
                        redispatched += 1;
                    }
                    Err(e) => error!("Failed to reset task {}: {}", task.task_id, e),
                }
            }
        }
        let timed_out = self.repo.mark_stale_running().await?;
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

pub fn build_execute_task_request(task: &Task) -> ExecuteTaskRequest {
    build_execute_task_request_with_token(task, String::new())
}

fn build_execute_task_request_with_token(task: &Task, token: String) -> ExecuteTaskRequest {
    let identity = general_compute_identity(task);
    ExecuteTaskRequest {
        task_id: task.task_id.clone(),
        torrent: task.torrent_source.clone().unwrap_or_default(),
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
        runtime: task.runtime.clone().unwrap_or_default(),
        task_source: task.task_source.clone().unwrap_or_default(),
        token,
        managed_budget_units: if task.runtime.as_deref() == Some("managed-function-v0") {
            task.max_cpt.max(0)
        } else {
            0
        },
        general_compute_manifest_json: task
            .general_compute_manifest_json
            .clone()
            .unwrap_or_default(),
        execution_id: identity
            .as_ref()
            .map_or_else(String::new, |identity| identity.0.clone()),
        attempt_id: identity
            .as_ref()
            .map_or_else(String::new, |identity| identity.1.clone()),
        idempotency_key: identity
            .as_ref()
            .map_or_else(String::new, |identity| identity.2.clone()),
        request_digest: identity
            .as_ref()
            .map_or_else(String::new, |identity| identity.3.clone()),
    }
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

fn worker_execution_token(
    private_key_pem: &str,
    task: &Task,
    worker_id: &str,
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
        exp: (now + 300) as usize,
        iat: now as usize,
    };
    WorkerExecutionSigner::from_pem(private_key_pem)?.encode_claims(&claims)
}

async fn execute_on_worker(
    repo: Arc<TaskRepository>,
    task: Task,
    worker_id: String,
    worker_addr: String,
    worker_execution_private_key_pem: &str,
    managed_proof_rollout_mode: ManagedProofRolloutMode,
) -> Result<()> {
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

    let endpoint = match worker_transport_endpoint(&worker_addr) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            reset_after_worker_rpc_failure(
                repo.as_ref(),
                &task.task_id,
                &worker_id,
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
                &task.task_id,
                &worker_id,
                worker_transport_failure_disposition(&error),
                &error,
            )
            .await?;
            return Ok(());
        }
    };
    let mut client = WorkerNodeServiceClient::new(channel)
        .max_encoding_message_size(WORKER_RPC_MESSAGE_MAX_BYTES)
        .max_decoding_message_size(WORKER_RPC_MESSAGE_MAX_BYTES);
    repo.refresh_worker_endpoint(&task.task_id, &worker_id, &worker_addr)
        .await?;
    let token =
        worker_execution_token(worker_execution_private_key_pem, &current_task, &worker_id)?;
    let mut request =
        tonic::Request::new(build_execute_task_request_with_token(&current_task, token));
    request.set_timeout(WORKER_EXECUTE_RPC_TIMEOUT);
    match client.execute_task(request).await {
        Ok(response) => {
            let response = response.into_inner();
            if let Err(reason) = validate_worker_response_sizes(&response) {
                let reason = reason.to_string();
                if current_task.runtime.as_deref()
                    == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
                {
                    repo.reset_to_pending_for_worker(&task.task_id, &worker_id)
                        .await?;
                } else {
                    repo.fail_for_worker(&task.task_id, &worker_id, &reason)
                        .await?;
                }
                warn!(
                    "Task {} returned an oversized response from worker {}: {}",
                    task.task_id, worker_id, reason
                );
                return Ok(());
            }
            if let Err(reason) =
                validate_general_compute_response_identity(&current_task, &response)
            {
                repo.reset_to_pending_for_worker(&task.task_id, &worker_id)
                    .await?;
                warn!(
                    "Task {} returned an attempt identity mismatch from worker {}; redispatching: {}",
                    task.task_id, worker_id, reason
                );
                return Ok(());
            }
            let general_compute_result = if current_task.runtime.as_deref()
                == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION)
            {
                let capability_snapshot =
                    repo.general_compute_capability_snapshot(&worker_id).await?;
                let Some(capability_snapshot) = capability_snapshot else {
                    repo.reset_to_pending_for_worker(&task.task_id, &worker_id)
                        .await?;
                    warn!(
                        "Task {} returned a general-compute result from worker {} without a trusted capability snapshot; redispatching",
                        task.task_id, worker_id
                    );
                    return Ok(());
                };
                match decode_and_validate_general_compute_result(
                    &current_task,
                    &response,
                    &capability_snapshot,
                ) {
                    Ok(result) => Some(result),
                    Err(reason) => {
                        repo.reset_to_pending_for_worker(&task.task_id, &worker_id)
                            .await?;
                        warn!(
                            "Task {} returned an invalid typed general-compute result from worker {}; redispatching: {}",
                            task.task_id, worker_id, reason
                        );
                        return Ok(());
                    }
                }
            } else {
                None
            };
            if response.success {
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
                        repo.fail_for_worker(&task.task_id, &worker_id, reason)
                            .await?;
                        warn!(
                            "Task {} failed managed proof gate on worker {}: {}",
                            task.task_id, worker_id, reason
                        );
                        return Ok(());
                    }
                };
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
                                managed_proof_metrics::record(ManagedProofMetricEvent::Verified);
                                managed_proof_metrics::record(
                                    ManagedProofMetricEvent::ObserveFallback,
                                );
                                record_managed_proof_audit(
                                    repo.as_ref(),
                                    &task.task_id,
                                    &worker_id,
                                    managed_proof_rollout_mode,
                                    "observed_verified",
                                    None,
                                )
                                .await;
                                info!(
                                    task_id = %task.task_id,
                                    "Managed proof observation verified successfully"
                                );
                                None
                            } else {
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
                                    repo.reset_to_pending_for_worker(&task.task_id, &worker_id)
                                        .await?;
                                    warn!(
                                        "Task {} deferred because the local managed proof verifier is busy: {}",
                                        task.task_id, reason
                                    );
                                }
                                ManagedProofFailureDisposition::FailWorkerResult => {
                                    repo.fail_for_worker(&task.task_id, &worker_id, &reason)
                                        .await?;
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
                    repo.complete_for_worker_with_managed_receipt(
                        &task.task_id,
                        &worker_id,
                        Some(&response.status_message),
                        completion.usage_units,
                        completion.output_bytes,
                        &completion.claim_json,
                    )
                    .await?;
                } else {
                    if current_task.runtime.as_deref() == Some("managed-function-v0") {
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
                            managed_proof_metrics::record(
                                ManagedProofMetricEvent::LegacySettlement,
                            );
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
                    }
                    if let Some(result) = general_compute_result.as_ref() {
                        repo.complete_general_compute_for_worker(
                            &task.task_id,
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
                        .await?;
                    } else {
                        repo.complete_for_worker(
                            &task.task_id,
                            &worker_id,
                            None,
                            Some(&response.status_message),
                        )
                        .await?;
                    }
                }
                info!("Task {} completed by worker {}", task.task_id, worker_id);
            } else {
                if let Some(result) = general_compute_result.as_ref() {
                    let reason = result
                        .error_code
                        .as_deref()
                        .filter(|message| !message.trim().is_empty())
                        .unwrap_or("general-compute execution failed");
                    repo.fail_general_compute_for_worker(
                        &task.task_id,
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
                    .await?;
                    warn!(
                        "Task {} failed typed general-compute execution on worker {}: {}",
                        task.task_id, worker_id, reason
                    );
                    return Ok(());
                }
                repo.fail_for_worker(&task.task_id, &worker_id, &response.status_message)
                    .await?;
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
                &task.task_id,
                &worker_id,
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
    repo.complete_for_worker(
        &task.task_id,
        worker_id,
        None,
        Some(&response.status_message),
    )
    .await?;
    info!(
        task_id = %task.task_id,
        worker_id = %worker_id,
        "Managed proof observation retained legacy settlement"
    );
    Ok(())
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
    if task.runtime.as_deref() != Some("managed-function-v0") {
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
enum WorkerResponseSizeError {
    #[error("Worker status message exceeds the application limit")]
    StatusMessageTooLarge,
    #[error("Worker legacy managed receipt exceeds the application limit")]
    LegacyReceiptTooLarge,
    #[error("Worker typed general-compute result exceeds the application limit")]
    GeneralComputeResultTooLarge,
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
    task_id: &str,
    worker_id: &str,
    disposition: WorkerRpcFailureDisposition,
    error: &impl std::fmt::Display,
) -> Result<()> {
    repo.reset_to_pending_for_worker(task_id, worker_id).await?;
    match disposition {
        WorkerRpcFailureDisposition::RetryAfterResourceExhaustion => {
            warn!(
                "Task {} was rejected because worker {} is resource exhausted; resetting for redispatch: {}",
                task_id, worker_id, error
            );
        }
        WorkerRpcFailureDisposition::RetryWithoutWorkerPenalty => {
            warn!(
                "Task {} could not be sent to worker {}; resetting for redispatch without worker penalty: {}",
                task_id, worker_id, error
            );
        }
    }
    Ok(())
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

    claim
        .validate_bindings(
            &task.task_id,
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
        canonical_artifact_root, sha256_digest, ArtifactManifest, ArtifactRole,
        BackendRegistration, DeterminismPolicy, EvidenceEnvelope, ExecutionPolicy,
        GeneralComputeRequest, GeneralComputeResult, ResultStatus,
        TrustedWorkerCapabilityRegistration, UsageClaim, WorkerCapabilities,
        GENERAL_COMPUTE_RUNTIME_VERSION,
    };
    use hivemind_config::ManagedProofRolloutMode;
    use hivemind_managed_proof::{
        ClaimError, ExecutionClaim, ExecutionMetrics, COST_MODEL_ID, MANAGED_RUNTIME_ID,
        PROOF_PROTOCOL_VERSION,
    };
    use hivemind_models::{TaskStatus, WorkerStatus};
    use hivemind_proto::{
        worker_node_service_server::{WorkerNodeService, WorkerNodeServiceServer},
        ExecuteTaskRequest, ExecuteTaskResponse, StopTaskExecutionRequest,
        StopTaskExecutionResponse, TaskOutputRequest, TaskOutputResponse, TaskOutputUploadRequest,
        TaskOutputUploadResponse, TaskResultUploadRequest, TaskResultUploadResponse,
        TaskUsageRequest, TaskUsageResponse,
    };
    use std::net::SocketAddr;
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;
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
            backends: vec![BackendRegistration {
                backend_id: request.backend_id.clone(),
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
            gpu_capabilities: vec![],
            },
            backends: vec![BackendRegistration {
                backend_id: request.backend_id.clone(),
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
            4 * 1024 * 1024
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
            last_heartbeat: Utc::now(),
            registered_at: Utc::now(),
            updated_at: Utc::now(),
        }
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

        let token = worker_execution_token(&private_key, &task, "worker-bound-7").unwrap();
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
    fn worker_execution_token_reports_private_key_boundary() {
        // Given: a dispatchable task and a missing worker-execution private key.
        let task = make_task("missing-worker-private-key", TaskStatus::Assigned, 0);

        // When: token creation crosses the signing boundary.
        let error = worker_execution_token("", &task, "worker-7")
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

    struct FakeWorkerExecuteService {
        execute_tx: tokio::sync::mpsc::Sender<String>,
        response: ExecuteTaskResponse,
    }

    #[tonic::async_trait]
    impl WorkerNodeService for FakeWorkerExecuteService {
        async fn execute_task(
            &self,
            request: Request<ExecuteTaskRequest>,
        ) -> Result<Response<ExecuteTaskResponse>, Status> {
            let task_id = request.into_inner().task_id;
            let _ = self.execute_tx.send(task_id).await;
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

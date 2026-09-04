pub mod chunk_transport;
pub mod control_api;
pub mod executor;
pub mod grpc_server;
pub mod managed_prover;
pub mod nodepool_client;
pub mod resource_monitor;
pub mod runtime_admission;
pub mod sandbox;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_models::{Task, WorkerCapabilityReport};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopTaskOutcome {
    StopRequested,
    AlreadyStopping,
    NotRunning,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ActiveTaskKey {
    task_id: String,
    attempt_id: String,
}

impl ActiveTaskKey {
    fn new(task_id: &str, attempt_id: &str) -> Self {
        Self {
            task_id: task_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
        }
    }
}

struct ActiveTaskEntry {
    cancellation_tx: watch::Sender<bool>,
    stop_requested: bool,
    result_rx: watch::Receiver<Option<TaskResultMessage>>,
}

type ActiveTaskMap = Arc<Mutex<HashMap<ActiveTaskKey, ActiveTaskEntry>>>;
type TaskResultMessage = Result<TaskResult, String>;
type TaskRunnerFuture = Pin<Box<dyn Future<Output = Result<TaskResult>> + Send>>;
type TaskRunner = dyn Fn(
        Task,
        watch::Receiver<bool>,
        Option<managed_prover::ManagedProofTaskContext>,
    ) -> TaskRunnerFuture
    + Send
    + Sync;

pub struct WorkerExecutor {
    active_tasks: ActiveTaskMap,
    task_runner: Arc<TaskRunner>,
    dynamic_capability_report: WorkerCapabilityReport,
}

impl WorkerExecutor {
    pub fn new(config: HivemindConfig) -> Self {
        Self::try_new(config).expect("operator worker configuration must be valid")
    }

    pub fn try_new(config: HivemindConfig) -> Result<Self> {
        let runner_config = config.clone();
        let prover = Arc::new(managed_prover::ManagedProverExecutor::new(&config));
        let admission = runtime_admission::WorkerRuntimeAdmission::from_environment()?;
        let mut dynamic_capability_report = admission.public_capability_report();
        if !prover.has_configured_route() {
            dynamic_capability_report.ready = false;
            dynamic_capability_report.capabilities.clear();
            dynamic_capability_report.readiness_reason =
                "managed proof provider is not configured".into();
        }
        let trusted_registration = admission.trusted_registration();
        // ReferenceDirect is a test-only backend. Production workers must never
        // load the Python reference executor from environment configuration.
        let reference_executor = {
            #[cfg(test)]
            {
                executor::reference_executor_from_environment(&admission)
            }
            #[cfg(not(test))]
            {
                None
            }
        };
        let cas_store = executor::cas_store_from_environment();
        let production_backends = executor::production_backends_from_environment()?;
        let managed_gpu_production_backends =
            executor::managed_gpu_production_backends_from_environment()?;
        let windows_backends = executor::windows_production_backends_from_environment()?;
        let capability_matrix = if admission.capability_matrix().backends.is_empty() {
            executor::runtime_capability_matrix_from_environment()
        } else {
            Some(Arc::new(admission.capability_matrix()))
        };
        Ok(Self {
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            task_runner: Arc::new(move |task, cancellation, proof_context| {
                let config = runner_config.clone();
                let prover = Arc::clone(&prover);
                let reference_executor = reference_executor.clone();
                let cas_store = cas_store.clone();
                let production_backends = production_backends.clone();
                let managed_gpu_production_backends = managed_gpu_production_backends.clone();
                let windows_backends = windows_backends.clone();
                let capability_matrix = capability_matrix.clone();
                let trusted_registration = trusted_registration.clone();
                Box::pin(async move {
                    let mut result = executor::run_task_with_cancel_and_backends_and_trusted_registration_and_windows_and_managed_gpu(
                        &task,
                        &config,
                        cancellation.clone(),
                        reference_executor,
                        cas_store,
                        production_backends,
                        managed_gpu_production_backends,
                        windows_backends,
                        capability_matrix,
                        Some(trusted_registration),
                    )
                    .await?;
                    if result.success
                        && matches!(
                            task.runtime.as_deref(),
                            Some("managed-function-v0") | Some("production_sandboxed_dsl")
                        )
                    {
                        match prover
                            .prove_with_context(&task, cancellation.clone(), proof_context)
                            .await
                        {
                            Ok(proof) if !*cancellation.borrow() => {
                                result.managed_proof = Some(proof);
                            }
                            Ok(_) | Err(managed_prover::ManagedProverError::Failed) => {
                                let message = if *cancellation.borrow() {
                                    "Task execution stopped"
                                } else {
                                    "Managed proof generation failed"
                                };
                                result = managed_proof_failure(result, message);
                            }
                            Err(managed_prover::ManagedProverError::QueueFull) => {
                                return Err(managed_prover::ManagedProverError::QueueFull.into());
                            }
                        }
                    }
                    Ok(result)
                })
            }),
            dynamic_capability_report,
        })
    }

    #[cfg(test)]
    fn new_with_task_runner<F, Fut>(_config: HivemindConfig, task_runner: F) -> Self
    where
        F: Fn(Task, watch::Receiver<bool>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<TaskResult>> + Send + 'static,
    {
        Self {
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            task_runner: Arc::new(move |task, cancellation, _proof_context| {
                Box::pin(task_runner(task, cancellation))
            }),
            dynamic_capability_report: WorkerCapabilityReport::public_managed_dsl(),
        }
    }
    pub async fn execute_task(&self, task: &Task) -> Result<TaskResult> {
        self.execute_task_with_context(task, None).await
    }

    pub async fn execute_task_with_context(
        &self,
        task: &Task,
        proof_context: Option<managed_prover::ManagedProofTaskContext>,
    ) -> Result<TaskResult> {
        self.execute_task_with_context_and_attempt(task, proof_context, "")
            .await
    }

    pub async fn execute_task_with_context_and_attempt(
        &self,
        task: &Task,
        proof_context: Option<managed_prover::ManagedProofTaskContext>,
        attempt_id: &str,
    ) -> Result<TaskResult> {
        let (cancellation_tx, cancellation_rx) = watch::channel(false);
        let (result_tx, result_rx) = watch::channel(None);
        let active_task_key = ActiveTaskKey::new(&task.task_id, attempt_id);
        let existing_result_rx = {
            let mut active_tasks = self
                .active_tasks
                .lock()
                .map_err(|_| anyhow::anyhow!("active task registry is unavailable"))?;
            if let Some(active_task) = active_tasks.get(&active_task_key) {
                Some(active_task.result_rx.clone())
            } else {
                active_tasks.insert(
                    active_task_key.clone(),
                    ActiveTaskEntry {
                        cancellation_tx,
                        stop_requested: false,
                        result_rx: result_rx.clone(),
                    },
                );
                None
            }
        };

        if let Some(result_rx) = existing_result_rx {
            return Self::await_task_result(result_rx).await;
        }

        let task_runner = Arc::clone(&self.task_runner);
        let active_tasks = Arc::clone(&self.active_tasks);
        let task = task.clone();
        tokio::spawn(async move {
            let _active_task_guard = ActiveTaskGuard::new(active_tasks, active_task_key);
            let result = task_runner(task, cancellation_rx, proof_context)
                .await
                .map_err(|error| error.to_string());
            let _ = result_tx.send(Some(result));
        });

        Self::await_task_result(result_rx).await
    }
    async fn await_task_result(
        mut result_rx: watch::Receiver<Option<TaskResultMessage>>,
    ) -> Result<TaskResult> {
        loop {
            if let Some(result) = result_rx.borrow().as_ref().cloned() {
                return result.map_err(anyhow::Error::msg);
            }
            result_rx
                .changed()
                .await
                .map_err(|_| anyhow::anyhow!("task supervisor ended before returning a result"))?;
        }
    }

    pub fn stop_task_execution(&self, task_id: &str) -> StopTaskOutcome {
        self.stop_task_execution_for_attempt(task_id, None)
    }

    pub fn stop_task_execution_for_attempt(
        &self,
        task_id: &str,
        attempt_id: Option<&str>,
    ) -> StopTaskOutcome {
        let mut active_tasks = self
            .active_tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = ActiveTaskKey::new(task_id, attempt_id.unwrap_or_default());
        let Some(entry) = active_tasks.get_mut(&key) else {
            return StopTaskOutcome::NotRunning;
        };
        if entry.stop_requested {
            return StopTaskOutcome::AlreadyStopping;
        }
        entry.stop_requested = true;
        let _ = entry.cancellation_tx.send(true);
        StopTaskOutcome::StopRequested
    }
    pub fn get_system_resources(&self) -> SystemResources {
        resource_monitor::collect_resources()
    }
    pub fn get_resource_spec(&self) -> hivemind_models::ResourceSpec {
        resource_monitor::to_resource_spec(&self.get_system_resources())
    }
    pub fn get_resource_usage(&self) -> hivemind_models::ResourceUsage {
        resource_monitor::to_resource_usage(&self.get_system_resources())
    }

    #[must_use]
    pub fn dynamic_capability_report(&self) -> WorkerCapabilityReport {
        self.dynamic_capability_report.clone()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub exit_code: i32,
    pub cpu_time_ms: i64,
    pub wall_time_ms: i64,
    pub peak_memory_mb: i64,
    pub managed_executed_ops: i64,
    pub managed_output_bytes: i64,
    pub managed_receipt_json: Option<String>,
    #[serde(default, with = "managed_proof_serde")]
    pub managed_proof: Option<hivemind_proto::ManagedProofEnvelope>,
    /// Serialized typed result for `general-compute-v1alpha1`.
    /// Legacy managed-function results leave this unset.
    #[serde(default)]
    pub general_compute_result_json: Option<Vec<u8>>,
    /// Serialized typed result for `managed-function-gpu-v1`.
    /// GPU-v1 results never enter the proof or legacy result-torrent routes.
    #[serde(default)]
    pub managed_gpu_result_json: Option<Vec<u8>>,
}

/// Preserves `TaskResult`'s public serde contract without omitting a proof.
///
/// Prost's generated envelope deliberately has no serde derives, so the
/// JSON representation uses the same four fields and byte-vector semantics
/// rather than silently skipping the proof from externally stored results.
mod managed_proof_serde {
    use hivemind_proto::ManagedProofEnvelope;

    #[derive(serde::Serialize, serde::Deserialize)]
    struct SerializableManagedProofEnvelope {
        proof_scheme: String,
        image_id: Vec<u32>,
        journal: Vec<u8>,
        receipt_json: Vec<u8>,
    }

    impl From<&ManagedProofEnvelope> for SerializableManagedProofEnvelope {
        fn from(proof: &ManagedProofEnvelope) -> Self {
            Self {
                proof_scheme: proof.proof_scheme.clone(),
                image_id: proof.image_id.clone(),
                journal: proof.journal.clone(),
                receipt_json: proof.receipt_json.clone(),
            }
        }
    }

    impl From<SerializableManagedProofEnvelope> for ManagedProofEnvelope {
        fn from(proof: SerializableManagedProofEnvelope) -> Self {
            Self {
                proof_scheme: proof.proof_scheme,
                image_id: proof.image_id,
                journal: proof.journal,
                receipt_json: proof.receipt_json,
            }
        }
    }

    pub fn serialize<S>(
        proof: &Option<ManagedProofEnvelope>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let serializable = proof.as_ref().map(SerializableManagedProofEnvelope::from);
        serde::Serialize::serialize(&serializable, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<ManagedProofEnvelope>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let proof = <Option<SerializableManagedProofEnvelope> as serde::Deserialize>::deserialize(
            deserializer,
        )?;
        Ok(proof.map(Into::into))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemResources {
    pub cpu_cores: i32,
    pub total_memory_gb: i32,
    pub available_memory_gb: i32,
    pub cpu_usage_percent: f64,
    pub memory_usage_percent: f64,
    pub gpu_count: i32,
    pub gpu_infos: Vec<GpuInfo>,
    pub storage_total_gb: i64,
    pub storage_available_gb: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GpuInfo {
    pub index: i32,
    pub name: String,
    pub vram_total_mb: i64,
    pub vram_used_mb: i64,
    pub vram_available_mb: i64,
    pub gpu_utilization_percent: f64,
}

fn managed_proof_failure(mut result: TaskResult, message: &str) -> TaskResult {
    result.success = false;
    result.output = None;
    result.error = Some(message.to_string());
    result.exit_code = 1;
    result.managed_executed_ops = 0;
    result.managed_output_bytes = 0;
    result.managed_receipt_json = None;
    result.managed_proof = None;
    result.general_compute_result_json = None;
    result
}

struct ActiveTaskGuard {
    active_tasks: ActiveTaskMap,
    key: ActiveTaskKey,
}

impl ActiveTaskGuard {
    fn new(active_tasks: ActiveTaskMap, key: ActiveTaskKey) -> Self {
        Self { active_tasks, key }
    }
}

impl Drop for ActiveTaskGuard {
    fn drop(&mut self) {
        let mut active_tasks = self
            .active_tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active_tasks.remove(&self.key);
    }
}

#[cfg(test)]
mod worker_executor_tests;

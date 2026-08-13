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
use hivemind_models::Task;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopTaskOutcome {
    StopRequested,
    AlreadyStopping,
    NotRunning,
}

struct ActiveTaskEntry {
    cancellation_tx: watch::Sender<bool>,
    stop_requested: bool,
}

type ActiveTaskMap = Arc<Mutex<HashMap<String, ActiveTaskEntry>>>;
type TaskRunnerFuture = Pin<Box<dyn Future<Output = Result<TaskResult>> + Send>>;
type TaskRunner = dyn Fn(Task, watch::Receiver<bool>) -> TaskRunnerFuture + Send + Sync;

pub struct WorkerExecutor {
    active_tasks: ActiveTaskMap,
    task_runner: Arc<TaskRunner>,
}

impl WorkerExecutor {
    pub fn new(config: HivemindConfig) -> Self {
        let runner_config = config.clone();
        let prover = Arc::new(managed_prover::ManagedProverExecutor::new(&config));
        let reference_executor = runtime_admission::WorkerRuntimeAdmission::from_environment()
            .ok()
            .and_then(|admission| executor::reference_executor_from_environment(&admission));
        let cas_store = executor::cas_store_from_environment();
        Self {
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            task_runner: Arc::new(move |task, cancellation| {
                let config = runner_config.clone();
                let prover = Arc::clone(&prover);
                let reference_executor = reference_executor.clone();
                let cas_store = cas_store.clone();
                Box::pin(async move {
                    let mut result = executor::run_task_with_cancel_and_reference_and_cas(
                        &task,
                        &config,
                        cancellation.clone(),
                        reference_executor,
                        cas_store,
                    )
                    .await?;
                    if result.success && task.runtime.as_deref() == Some("managed-function-v0") {
                        match prover.prove(&task, cancellation.clone()).await {
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
        }
    }

    #[cfg(test)]
    fn new_with_task_runner<F, Fut>(_config: HivemindConfig, task_runner: F) -> Self
    where
        F: Fn(Task, watch::Receiver<bool>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<TaskResult>> + Send + 'static,
    {
        Self {
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            task_runner: Arc::new(move |task, cancellation| {
                Box::pin(task_runner(task, cancellation))
            }),
        }
    }
    pub async fn execute_task(&self, task: &Task) -> Result<TaskResult> {
        let (cancellation_tx, cancellation_rx) = watch::channel(false);
        {
            let mut active_tasks = self
                .active_tasks
                .lock()
                .map_err(|_| anyhow::anyhow!("active task registry is unavailable"))?;
            if active_tasks.contains_key(&task.task_id) {
                anyhow::bail!("task {} is already running", task.task_id);
            }
            active_tasks.insert(
                task.task_id.clone(),
                ActiveTaskEntry {
                    cancellation_tx,
                    stop_requested: false,
                },
            );
        }

        let (result_tx, result_rx) = oneshot::channel();
        let task_id = task.task_id.clone();
        let task_runner = Arc::clone(&self.task_runner);
        let active_tasks = Arc::clone(&self.active_tasks);
        let task = task.clone();
        tokio::spawn(async move {
            let _active_task_guard = ActiveTaskGuard::new(active_tasks, task_id);
            let result = task_runner(task, cancellation_rx).await;
            let _ = result_tx.send(result);
        });

        result_rx
            .await
            .map_err(|_| anyhow::anyhow!("task supervisor ended before returning a result"))?
    }
    pub fn stop_task_execution(&self, task_id: &str) -> StopTaskOutcome {
        let mut active_tasks = self
            .active_tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = active_tasks.get_mut(task_id) else {
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
    task_id: String,
}

impl ActiveTaskGuard {
    fn new(active_tasks: ActiveTaskMap, task_id: String) -> Self {
        Self {
            active_tasks,
            task_id,
        }
    }
}

impl Drop for ActiveTaskGuard {
    fn drop(&mut self) {
        let mut active_tasks = self
            .active_tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active_tasks.remove(&self.task_id);
    }
}

#[cfg(test)]
mod worker_executor_tests;

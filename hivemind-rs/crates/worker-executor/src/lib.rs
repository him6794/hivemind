pub mod chunk_transport;
pub mod control_api;
pub mod executor;
pub mod grpc_server;
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
type TaskRunner = dyn Fn(Task, watch::Receiver<bool>, bool) -> TaskRunnerFuture + Send + Sync;

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
        let admission = runtime_admission::WorkerRuntimeAdmission::from_environment()?;
        let dynamic_capability_report = admission.public_capability_report();
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
            task_runner: Arc::new(move |task, cancellation, _consensus_request| {
                let config = runner_config.clone();
                let reference_executor = reference_executor.clone();
                let cas_store = cas_store.clone();
                let production_backends = production_backends.clone();
                let managed_gpu_production_backends = managed_gpu_production_backends.clone();
                let windows_backends = windows_backends.clone();
                let capability_matrix = capability_matrix.clone();
                let trusted_registration = trusted_registration.clone();
                Box::pin(async move {
                    executor::run_task_with_cancel_and_backends_and_trusted_registration_and_windows_and_managed_gpu(
                        &task,
                        &config,
                        cancellation,
                        reference_executor,
                        cas_store,
                        production_backends,
                        managed_gpu_production_backends,
                        windows_backends,
                        capability_matrix,
                        Some(trusted_registration),
                    )
                    .await
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
            task_runner: Arc::new(move |task, cancellation, _consensus_request| {
                Box::pin(task_runner(task, cancellation))
            }),
            dynamic_capability_report: WorkerCapabilityReport::public_managed_dsl(),
        }
    }
    pub async fn execute_task(&self, task: &Task) -> Result<TaskResult> {
        self.execute_task_with_attempt(task, "").await
    }

    pub async fn execute_task_with_attempt(
        &self,
        task: &Task,
        attempt_id: &str,
    ) -> Result<TaskResult> {
        self.execute_task_with_attempt_mode(task, attempt_id, false)
            .await
    }

    /// Execute a managed task as a consensus replica. The request contract
    /// selects this route independently of the Worker rollout environment.
    pub async fn execute_task_with_consensus(
        &self,
        task: &Task,
        attempt_id: &str,
    ) -> Result<TaskResult> {
        self.execute_task_with_attempt_mode(task, attempt_id, true)
            .await
    }

    async fn execute_task_with_attempt_mode(
        &self,
        task: &Task,
        attempt_id: &str,
        consensus_request: bool,
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
            let result = task_runner(task, cancellation_rx, consensus_request)
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
    /// Serialized typed result for `general-compute-v1alpha1`.
    /// Legacy managed-function results leave this unset.
    #[serde(default)]
    pub general_compute_result_json: Option<Vec<u8>>,
    /// Serialized typed result for `managed-function-gpu-v1`.
    /// GPU-v1 results never enter the legacy result-torrent route.
    #[serde(default)]
    pub managed_gpu_result_json: Option<Vec<u8>>,
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

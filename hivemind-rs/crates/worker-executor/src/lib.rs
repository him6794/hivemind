pub mod attempt_stop_fence;
pub mod benchmark;
pub mod chunk_transport;
pub mod control_api;
pub mod executor;
pub mod grpc_server;
pub mod hcode_p2p;
pub mod idle;
pub mod maintenance;
pub mod nodepool_client;
pub mod resource_monitor;
pub mod runtime_admission;
pub mod sandbox;
pub mod sharing;

use anyhow::Result;
use attempt_stop_fence::AttemptStopFenceStore;
use hivemind_auth::worker_execution::WORKER_EXECUTION_TOKEN_LEEWAY_SECONDS;
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
    StoppedBeforeStart,
    StopConfirmationUnavailable,
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

fn worker_stop_fence_expiry(token_expiry: usize) -> i64 {
    i64::try_from(token_expiry)
        .unwrap_or(i64::MAX)
        .saturating_add(i64::try_from(WORKER_EXECUTION_TOKEN_LEEWAY_SECONDS).unwrap_or(i64::MAX))
}

struct ActiveTaskEntry {
    cancellation_tx: watch::Sender<bool>,
    stop_requested: bool,
    result_rx: watch::Receiver<Option<TaskResultMessage>>,
}

#[derive(Default)]
struct ActiveTaskRegistry {
    running: HashMap<ActiveTaskKey, ActiveTaskEntry>,
    stopped: HashMap<ActiveTaskKey, i64>,
}

type ActiveTaskMap = Arc<Mutex<ActiveTaskRegistry>>;
type TaskResultMessage = Result<TaskResult, String>;
type TaskRunnerFuture = Pin<Box<dyn Future<Output = Result<TaskResult>> + Send>>;
type TaskRunner = dyn Fn(Task, watch::Receiver<bool>, bool, ExecutionAttemptContext) -> TaskRunnerFuture
    + Send
    + Sync;

const RESOURCE_SAMPLE_REFRESH_INTERVAL: chrono::Duration = chrono::Duration::seconds(10);

#[derive(Debug, Clone)]
pub struct ResourceSample {
    pub resources: SystemResources,
    pub sampled_at: chrono::DateTime<chrono::Utc>,
}

fn resource_sample_is_fresh(
    sampled_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    sampled_at <= now && now.signed_duration_since(sampled_at) <= RESOURCE_SAMPLE_REFRESH_INTERVAL
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionAttemptContext {
    pub worker_id: Option<String>,
    pub transfer_generation: Option<i64>,
    pub hcode_artifact_access: Option<HcodeArtifactAccess>,
}

#[derive(Clone)]
pub struct HcodeArtifactAccess {
    pub token: String,
    pub authority: Arc<dyn grpc_server::TransferLeaseAuthority>,
}

impl std::fmt::Debug for HcodeArtifactAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HcodeArtifactAccess")
            .finish_non_exhaustive()
    }
}

pub struct WorkerExecutor {
    active_tasks: ActiveTaskMap,
    stop_fences: AttemptStopFenceStore,
    latest_resource_sample: Arc<std::sync::RwLock<Option<ResourceSample>>>,
    resource_refresh_lock: Arc<Mutex<()>>,
    pub benchmark: Arc<benchmark::BenchmarkCoordinator>,
    pub sharing: sharing::SharingController,
    task_runner: Arc<TaskRunner>,
    dynamic_capability_report: WorkerCapabilityReport,
    runtime_admission: runtime_admission::WorkerRuntimeAdmission,
}

impl WorkerExecutor {
    pub fn new(config: HivemindConfig) -> Self {
        Self::try_new(config).expect("operator worker configuration must be valid")
    }

    pub fn try_new(config: HivemindConfig) -> Result<Self> {
        let runner_config = config.clone();
        #[cfg(test)]
        let stop_fences = AttemptStopFenceStore::in_memory();
        #[cfg(not(test))]
        let stop_fences = AttemptStopFenceStore::from_environment_or_default()?;
        let configured_admission = runtime_admission::WorkerRuntimeAdmission::from_environment()?;
        let admission = configured_admission;
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
        if let Some(store) = cas_store.as_ref() {
            if let Err(error) = store.gc_transfer_metadata(
                general_compute_runtime::artifact::TRANSFER_METADATA_RETENTION,
            ) {
                tracing::warn!(error = %error, "general-compute CAS transfer metadata scavenging failed");
            }
        }
        let production_backends = executor::production_backends_from_environment()?;
        let managed_gpu_production_backends =
            executor::managed_gpu_production_backends_from_environment()?;
        let mut maintenance_roots = Vec::new();
        if let Some(backends) = production_backends.as_ref() {
            for backend in backends.registrations() {
                maintenance_roots.push(backend.bundle_root.clone());
                maintenance_roots.push(backend.artifact_root.clone());
            }
        }
        if let Some(backends) = managed_gpu_production_backends.as_ref() {
            for backend in backends.registrations() {
                maintenance_roots.push(backend.bundle_root.clone());
                maintenance_roots.push(backend.artifact_root.clone());
            }
        }
        for root in &maintenance_roots {
            if let Err(error) = maintenance::scavenge_operator_root(root) {
                tracing::warn!(
                    root = %root.display(),
                    error = %error,
                    "worker cleanup maintenance scavenger failed"
                );
            }
        }
        let capability_matrix = if admission.capability_matrix().backends.is_empty() {
            executor::runtime_capability_matrix_from_environment()
        } else {
            Some(Arc::new(admission.capability_matrix()))
        };
        let benchmark = benchmark::BenchmarkCoordinator::new(stop_fences.worker_state_root())?;
        let sharing = sharing::SharingController::new(
            config.worker_sharing.clone(),
            stop_fences.worker_state_root(),
        )?;
        Ok(Self {
            benchmark,
            sharing,
            active_tasks: Arc::new(Mutex::new(ActiveTaskRegistry {
                running: HashMap::new(),
                stopped: stop_fences.fences(),
            })),
            stop_fences,
            latest_resource_sample: Arc::new(std::sync::RwLock::new(None)),
            resource_refresh_lock: Arc::new(Mutex::new(())),
            task_runner: Arc::new(
                move |task, cancellation, _consensus_request, execution_context| {
                    let config = runner_config.clone();
                    let reference_executor = reference_executor.clone();
                    let cas_store = cas_store.clone();
                    let production_backends = production_backends.clone();
                    let managed_gpu_production_backends = managed_gpu_production_backends.clone();
                    let capability_matrix = capability_matrix.clone();
                    let trusted_registration = trusted_registration.clone();
                    Box::pin(async move {
                        if let Some(access) = execution_context.hcode_artifact_access {
                            return executor::run_hcode_artifact_task(task, cancellation, access)
                                .await;
                        }
                        executor::run_task_with_cancel_and_backends_and_trusted_registration_and_managed_gpu_with_context(
                        &task,
                        &config,
                        cancellation,
                        reference_executor,
                        cas_store,
                        production_backends,
                        managed_gpu_production_backends,
                        capability_matrix,
                        Some(trusted_registration),
                    )
                    .await
                    })
                },
            ),
            dynamic_capability_report,
            runtime_admission: admission,
        })
    }

    #[cfg(test)]
    fn new_with_task_runner<F, Fut>(_config: HivemindConfig, task_runner: F) -> Self
    where
        F: Fn(Task, watch::Receiver<bool>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<TaskResult>> + Send + 'static,
    {
        Self::new_with_task_runner_and_stop_fences(
            _config,
            task_runner,
            AttemptStopFenceStore::in_memory(),
        )
    }

    #[cfg(test)]
    fn new_with_task_runner_and_stop_fences<F, Fut>(
        _config: HivemindConfig,
        task_runner: F,
        stop_fences: AttemptStopFenceStore,
    ) -> Self
    where
        F: Fn(Task, watch::Receiver<bool>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<TaskResult>> + Send + 'static,
    {
        Self {
            benchmark: benchmark::BenchmarkCoordinator::new(None).expect("test benchmark state"),
            sharing: sharing::SharingController::new(_config.worker_sharing, None)
                .expect("test sharing settings"),
            active_tasks: Arc::new(Mutex::new(ActiveTaskRegistry {
                running: HashMap::new(),
                stopped: stop_fences.fences(),
            })),
            stop_fences,
            latest_resource_sample: Arc::new(std::sync::RwLock::new(None)),
            resource_refresh_lock: Arc::new(Mutex::new(())),
            task_runner: Arc::new(
                move |task, cancellation, _consensus_request, _execution_context| {
                    Box::pin(task_runner(task, cancellation))
                },
            ),
            dynamic_capability_report: WorkerCapabilityReport::public_managed_dsl(),
            runtime_admission: runtime_admission::WorkerRuntimeAdmission::default(),
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
        self.execute_task_with_attempt_context(
            task,
            attempt_id,
            ExecutionAttemptContext::default(),
            false,
        )
        .await
    }

    pub async fn execute_task_with_attempt_context(
        &self,
        task: &Task,
        attempt_id: &str,
        execution_context: ExecutionAttemptContext,
        consensus_request: bool,
    ) -> Result<TaskResult> {
        self.execute_task_with_attempt_mode(task, attempt_id, consensus_request, execution_context)
            .await
    }

    /// Execute a managed task as a consensus replica. The request contract
    /// selects this route independently of the Worker rollout environment.
    pub async fn execute_task_with_consensus(
        &self,
        task: &Task,
        attempt_id: &str,
    ) -> Result<TaskResult> {
        self.execute_task_with_attempt_context(
            task,
            attempt_id,
            ExecutionAttemptContext::default(),
            true,
        )
        .await
    }

    async fn execute_task_with_attempt_mode(
        &self,
        task: &Task,
        attempt_id: &str,
        consensus_request: bool,
        execution_context: ExecutionAttemptContext,
    ) -> Result<TaskResult> {
        let (cancellation_tx, cancellation_rx) = watch::channel(false);
        let (result_tx, result_rx) = watch::channel(None);
        let active_task_key = ActiveTaskKey::new(&task.task_id, attempt_id);
        let existing_result_rx = {
            let mut active_tasks = self
                .active_tasks
                .lock()
                .map_err(|_| anyhow::anyhow!("active task registry is unavailable"))?;
            let now = chrono::Utc::now().timestamp();
            active_tasks
                .stopped
                .retain(|_, expires_at| attempt_stop_fence::is_active(*expires_at, now));
            if active_tasks.stopped.contains_key(&active_task_key) {
                anyhow::bail!("task attempt was stopped before execution");
            }
            if let Some(active_task) = active_tasks.running.get(&active_task_key) {
                Some(active_task.result_rx.clone())
            } else {
                active_tasks.running.insert(
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
            let result = task_runner(task, cancellation_rx, consensus_request, execution_context)
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
        let Some(entry) = active_tasks.running.get_mut(&key) else {
            return StopTaskOutcome::NotRunning;
        };
        if entry.stop_requested {
            return StopTaskOutcome::AlreadyStopping;
        }
        entry.stop_requested = true;
        let _ = entry.cancellation_tx.send(true);
        StopTaskOutcome::StopRequested
    }

    pub async fn stop_task_execution_for_attempt_confirmed(
        &self,
        task_id: &str,
        attempt_id: &str,
        token_expiry: usize,
    ) -> StopTaskOutcome {
        if attempt_id.is_empty() {
            return StopTaskOutcome::NotRunning;
        }
        let key = ActiveTaskKey::new(task_id, attempt_id);
        let (mut result_rx, already_stopping, persistence_failed) = {
            let mut active_tasks = self
                .active_tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let now = chrono::Utc::now().timestamp();
            active_tasks
                .stopped
                .retain(|_, expires_at| attempt_stop_fence::is_active(*expires_at, now));
            let requested_expiry = worker_stop_fence_expiry(token_expiry);
            let previous_expiry = active_tasks
                .stopped
                .get(&key)
                .copied()
                .filter(|expires_at| attempt_stop_fence::is_active(*expires_at, now))
                .unwrap_or(requested_expiry);
            let requested_expiry = requested_expiry.max(previous_expiry);
            let persistence = self
                .stop_fences
                .record_fence(task_id, attempt_id, requested_expiry);
            let effective_expiry = persistence
                .as_ref()
                .copied()
                .unwrap_or(requested_expiry)
                .max(previous_expiry);
            active_tasks.stopped.insert(key.clone(), effective_expiry);
            let persistence_failed = if let Err(error) = persistence {
                tracing::error!(
                    error = %error,
                    "confirmed Worker stop could not persist its attempt fence"
                );
                true
            } else {
                false
            };
            let Some(entry) = active_tasks.running.get_mut(&key) else {
                return if persistence_failed {
                    StopTaskOutcome::StopConfirmationUnavailable
                } else {
                    StopTaskOutcome::StoppedBeforeStart
                };
            };
            let already_stopping = entry.stop_requested;
            entry.stop_requested = true;
            let _ = entry.cancellation_tx.send(true);
            (
                entry.result_rx.clone(),
                already_stopping,
                persistence_failed,
            )
        };
        if persistence_failed {
            return StopTaskOutcome::StopConfirmationUnavailable;
        }
        while result_rx.borrow().is_none() {
            if result_rx.changed().await.is_err() {
                return StopTaskOutcome::StopConfirmationUnavailable;
            }
        }
        if already_stopping {
            StopTaskOutcome::AlreadyStopping
        } else {
            StopTaskOutcome::StopRequested
        }
    }

    pub fn get_system_resources(&self) -> SystemResources {
        let _refresh_guard = self
            .resource_refresh_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(sample) = self.latest_resource_sample() {
            if resource_sample_is_fresh(sample.sampled_at, chrono::Utc::now()) {
                return sample.resources;
            }
        }

        let resources = resource_monitor::collect_resources();
        *self
            .latest_resource_sample
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(ResourceSample {
            resources: resources.clone(),
            sampled_at: chrono::Utc::now(),
        });
        resources
    }

    #[must_use]
    pub fn latest_resource_sample(&self) -> Option<ResourceSample> {
        self.latest_resource_sample
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn get_resource_spec(&self) -> hivemind_models::ResourceSpec {
        resource_monitor::to_resource_spec(&self.get_system_resources())
    }
    pub fn get_resource_usage(&self) -> hivemind_models::ResourceUsage {
        resource_monitor::to_resource_usage(&self.get_system_resources())
    }

    pub fn sharing_status(&self) -> sharing::SharingStatus {
        self.sharing.status(self.latest_resource_sample().as_ref())
    }

    /// The freshest presence-aware host observation, or `None` when no sample
    /// exists yet. Built from the resource monitor's own support flags so an
    /// absent gauge stays absent rather than a fabricated zero; Nodepool applies
    /// the observation's own TTL from `sampled_at`.
    pub fn resource_observation(
        &self,
    ) -> Option<hivemind_models::observations::ResourceObservation> {
        self.latest_resource_sample().map(|sample| {
            resource_monitor::to_resource_observation(&sample.resources, sample.sampled_at)
        })
    }

    #[must_use]
    pub fn dynamic_capability_report(&self) -> WorkerCapabilityReport {
        self.dynamic_capability_report.clone()
    }

    #[must_use]
    pub fn runtime_admission(&self) -> runtime_admission::WorkerRuntimeAdmission {
        self.runtime_admission.clone()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub exit_code: i32,
    /// Measured task CPU time. Managed DSL does not yet measure this, so it
    /// stays `None` rather than reporting a fabricated zero.
    pub cpu_time_ms: Option<i64>,
    pub wall_time_ms: Option<i64>,
    /// Measured task peak memory. Managed DSL does not yet measure this, so it
    /// stays `None` rather than substituting whole-worker RSS or a zero.
    pub peak_memory_mb: Option<i64>,
    pub managed_executed_ops: i64,
    pub managed_output_bytes: i64,
    pub managed_receipt_json: Option<String>,
    /// Serialized typed result for `general-compute-v1alpha1`.
    /// Legacy managed-function results leave this unset.
    #[serde(default)]
    pub general_compute_result_json: Option<Vec<u8>>,
    /// Serialized typed result for `hcode-gpu`.
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
    pub cpu_usage_supported: bool,
    pub memory_usage_percent: f64,
    pub memory_supported: bool,
    pub gpu_count: i32,
    pub gpu_infos: Vec<GpuInfo>,
    pub gpu_inventory_supported: bool,
    pub gpu_utilization_supported: bool,
    pub vram_total_supported: bool,
    pub vram_available_supported: bool,
    pub storage_supported: bool,
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
        active_tasks.running.remove(&self.key);
    }
}

#[cfg(test)]
mod worker_executor_tests;

pub mod control_api;
pub mod executor;
pub mod grpc_server;
pub mod nodepool_client;
pub mod resource_monitor;
pub mod sandbox;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_models::Task;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopTaskOutcome {
    StopRequested,
    AlreadyStopping,
    NotRunning,
}

struct ActiveTaskEntry {
    cancel_tx: Option<oneshot::Sender<()>>,
}

type ActiveTaskMap = Arc<Mutex<HashMap<String, ActiveTaskEntry>>>;

pub struct WorkerExecutor {
    config: HivemindConfig,
    active_tasks: ActiveTaskMap,
}

impl WorkerExecutor {
    pub fn new(config: HivemindConfig) -> Self {
        Self {
            config,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub async fn execute_task(&self, task: &Task) -> Result<TaskResult> {
        let (cancel_tx, cancel_rx) = oneshot::channel();
        {
            let mut active_tasks = self
                .active_tasks
                .lock()
                .expect("active task registry poisoned");
            if active_tasks.contains_key(&task.task_id) {
                anyhow::bail!("task {} is already running", task.task_id);
            }
            active_tasks.insert(
                task.task_id.clone(),
                ActiveTaskEntry {
                    cancel_tx: Some(cancel_tx),
                },
            );
        }

        let result = executor::run_task_with_cancel(task, &self.config, cancel_rx).await;
        self.active_tasks
            .lock()
            .expect("active task registry poisoned")
            .remove(&task.task_id);
        result
    }
    pub fn stop_task_execution(&self, task_id: &str) -> StopTaskOutcome {
        let mut active_tasks = self
            .active_tasks
            .lock()
            .expect("active task registry poisoned");
        let Some(entry) = active_tasks.get_mut(task_id) else {
            return StopTaskOutcome::NotRunning;
        };
        let Some(cancel_tx) = entry.cancel_tx.take() else {
            return StopTaskOutcome::AlreadyStopping;
        };
        let _ = cancel_tx.send(());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_resources_collection() {
        let r = resource_monitor::collect_resources();
        assert!(r.cpu_cores > 0);
        assert!(r.total_memory_gb > 0);
        assert!(r.storage_total_gb > 0);
    }

    #[test]
    fn stop_task_execution_reports_not_running_for_unknown_task() {
        let executor = WorkerExecutor::new(HivemindConfig::default());

        let outcome = executor.stop_task_execution("missing-task");

        assert_eq!(outcome, StopTaskOutcome::NotRunning);
    }
}

pub mod control_api;
pub mod executor;
pub mod grpc_server;
pub mod nodepool_client;
pub mod resource_monitor;
pub mod sandbox;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_models::Task;

pub struct WorkerExecutor {
    config: HivemindConfig,
}

impl WorkerExecutor {
    pub fn new(config: HivemindConfig) -> Self {
        Self { config }
    }
    pub async fn execute_task(&self, task: &Task) -> Result<TaskResult> {
        executor::run_task(task, &self.config).await
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
}

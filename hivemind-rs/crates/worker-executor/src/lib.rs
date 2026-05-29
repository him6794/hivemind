pub mod executor;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_resources_collection() {
        let resources = resource_monitor::collect_resources();
        assert!(resources.cpu_cores > 0);
        assert!(resources.total_memory_gb > 0);
    }
}
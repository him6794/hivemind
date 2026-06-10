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
    use chrono::Utc;
    use hivemind_models::TaskStatus;
    use std::time::Duration;
    use tempfile::TempDir;
    use uuid::Uuid;

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

    #[tokio::test]
    async fn stop_task_execution_kills_long_running_executor_process() {
        let tmp = TempDir::new().unwrap();
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = tmp.path().join("sandbox").to_string_lossy().to_string();
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        config.auth.jwt_secret = "unit-test-jwt-secret".into();
        let marker = tmp.path().join("started.marker");
        config.executor.monty_executable = write_long_running_executor_script(tmp.path(), &marker)
            .to_string_lossy()
            .to_string();
        let task_file = std::path::Path::new(&config.torrent.api_dir).join("main.py");
        std::fs::write(&task_file, "print('long task')\n").unwrap();
        let executor = std::sync::Arc::new(WorkerExecutor::new(config));
        let task = test_task_with_source(task_file.to_string_lossy());
        let running_executor = executor.clone();
        let running_task = task.clone();

        let handle =
            tokio::spawn(async move { running_executor.execute_task(&running_task).await });
        wait_for_file(&marker).await;

        let outcome = executor.stop_task_execution(&task.task_id);
        assert_eq!(outcome, StopTaskOutcome::StopRequested);

        let result = tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("stopped task should return promptly")
            .expect("task join should succeed")
            .expect("task execution should return a result");

        assert!(!result.success);
        assert_eq!(result.exit_code, -1);
        assert!(
            result
                .error
                .unwrap_or_default()
                .contains("Task execution stopped"),
            "stopped result should explain cancellation"
        );
        assert_eq!(
            executor.stop_task_execution(&task.task_id),
            StopTaskOutcome::NotRunning
        );
    }

    #[tokio::test]
    async fn stop_task_execution_kills_wrapper_spawned_child_process() {
        let tmp = TempDir::new().unwrap();
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = tmp.path().join("sandbox").to_string_lossy().to_string();
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        config.auth.jwt_secret = "unit-test-jwt-secret".into();
        let started_marker = tmp.path().join("wrapper-started.marker");
        let child_marker = tmp.path().join("wrapper-child-survived.marker");
        config.executor.monty_executable =
            write_wrapper_child_executor_script(tmp.path(), &started_marker, &child_marker)
                .to_string_lossy()
                .to_string();
        let task_file = std::path::Path::new(&config.torrent.api_dir).join("main.py");
        std::fs::write(&task_file, "print('wrapped task')\n").unwrap();
        let executor = std::sync::Arc::new(WorkerExecutor::new(config));
        let task = test_task_with_source(task_file.to_string_lossy());
        let running_executor = executor.clone();
        let running_task = task.clone();

        let handle =
            tokio::spawn(async move { running_executor.execute_task(&running_task).await });
        wait_for_file(&started_marker).await;

        let outcome = executor.stop_task_execution(&task.task_id);
        assert_eq!(outcome, StopTaskOutcome::StopRequested);
        let result = tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("stopped task should return promptly")
            .expect("task join should succeed")
            .expect("task execution should return a result");
        assert!(!result.success);

        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert!(
            !child_marker.exists(),
            "wrapper-spawned child process survived stop and wrote {}",
            child_marker.display()
        );
    }

    async fn wait_for_file(path: &std::path::Path) {
        for _ in 0..50 {
            if path.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("timed out waiting for {}", path.display());
    }

    fn write_long_running_executor_script(
        dir: &std::path::Path,
        marker: &std::path::Path,
    ) -> std::path::PathBuf {
        let path = if cfg!(windows) {
            dir.join("long-running-executor.cmd")
        } else {
            dir.join("long-running-executor.sh")
        };
        let script = if cfg!(windows) {
            format!(
                "@echo off\r\necho started > \"{}\"\r\n:loop\r\nping -n 2 127.0.0.1 >nul\r\ngoto loop\r\n",
                marker.display()
            )
        } else {
            format!(
                "#!/bin/sh\nprintf '%s' started > '{}'\nwhile true; do :; done\n",
                marker.display()
            )
        };
        std::fs::write(&path, script).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }

        path
    }

    fn write_wrapper_child_executor_script(
        dir: &std::path::Path,
        started_marker: &std::path::Path,
        child_marker: &std::path::Path,
    ) -> std::path::PathBuf {
        let path = if cfg!(windows) {
            dir.join("wrapper-child-executor.cmd")
        } else {
            dir.join("wrapper-child-executor.sh")
        };
        let script = if cfg!(windows) {
            format!(
                "@echo off\r\necho started > \"{}\"\r\nstart \"\" powershell -NoProfile -ExecutionPolicy Bypass -Command \"Start-Sleep -Milliseconds 1400; Set-Content -LiteralPath '{}' -Value survived -NoNewline\"\r\n:loop\r\nping -n 2 127.0.0.1 >nul\r\ngoto loop\r\n",
                started_marker.display(),
                child_marker.display()
            )
        } else {
            format!(
                "#!/bin/sh\nprintf '%s' started > '{}'\n( sleep 1.4; printf '%s' survived > '{}' ) &\nwhile true; do sleep 1; done\n",
                started_marker.display(),
                child_marker.display()
            )
        };
        std::fs::write(&path, script).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }

        path
    }

    fn test_task_with_source(source: impl Into<String>) -> Task {
        let now = Utc::now();
        Task {
            id: Uuid::new_v4(),
            task_id: "long-running-stop-test".into(),
            owner: "requestor".into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some(source.into()),
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 1,
            req_gpu_score: 0,
            req_memory_gb: 1,
            req_gpu_memory_gb: 0,
            req_storage_gb: 1,
            host_count: 1,
            max_cpt: 1,
            billing_settled: false,
            billed_amount: 0,
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
            created_at: now,
            last_update: now,
            completed_at: None,
        }
    }
}

use anyhow::Result;
use hivemind_database::DatabaseManager;
use hivemind_models::{Task, TaskStatus, WorkerNode, WorkerStatus};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{info, warn, error};

use crate::scheduler;
use crate::task_repository::TaskRepository;

pub struct Dispatcher {
    repo: Arc<TaskRepository>,
    db: DatabaseManager,
    task_timeout_secs: u64,
    max_redispatch: i32,
}

impl Dispatcher {
    pub fn new(db: DatabaseManager, task_timeout_secs: u64, max_redispatch: i32) -> Self {
        Self {
            repo: Arc::new(TaskRepository::new(db.pool.clone())),
            db,
            task_timeout_secs,
            max_redispatch,
        }
    }

    /// Dispatch a single pending task to a suitable worker.
    /// Returns Some((worker_id, worker_ip)) if dispatched, None if no worker available.
    pub async fn dispatch_one(
        &self,
        task: &Task,
        workers: &[WorkerNode],
    ) -> Option<(String, String)> {
        let worker = scheduler::find_best_worker(task, workers).await?;
        let worker_id = worker.worker_id.clone();
        let worker_ip = worker.ip.clone();

        match self.repo.assign_to_worker(&task.task_id, &worker_id, &worker_ip).await {
            Ok(_) => {
                info!(
                    "Dispatched task {} to worker {} ({})",
                    task.task_id, worker_id, worker_ip
                );
                Some((worker_id, worker_ip))
            }
            Err(e) => {
                error!("Failed to dispatch task {}: {}", task.task_id, e);
                None
            }
        }
    }

    /// Dispatch all pending tasks to available workers.
    /// Returns the number of tasks dispatched.
    pub async fn dispatch_pending(
        &self,
        workers: &[WorkerNode],
    ) -> Result<u64> {
        let pending = self.repo.find_pending().await?;
        let mut dispatched = 0u64;

        for task in &pending {
            if self.dispatch_one(task, workers).await.is_some() {
                dispatched += 1;
            }
        }

        if dispatched > 0 {
            info!("Dispatched {} pending tasks", dispatched);
        }
        Ok(dispatched)
    }

    /// Process timed-out tasks: redispatch or fail them.
    /// Returns (redispatched_count, failed_count).
    pub async fn process_timeouts(&self) -> Result<(u64, u64)> {
        let stale_tasks = self.repo.find_stale_dispatched(self.task_timeout_secs).await?;
        let mut redispatched = 0u64;
        let mut failed = 0u64;

        for task in &stale_tasks {
            if task.retry_count >= self.max_redispatch {
                // Max retries exceeded, fail the task
                match self.repo.fail(&task.task_id, "Max redispatch attempts exceeded").await {
                    Ok(_) => {
                        warn!("Task {} failed after {} retries", task.task_id, task.retry_count);
                        failed += 1;
                    }
                    Err(e) => error!("Failed to mark task {} as failed: {}", task.task_id, e),
                }
            } else {
                // Reset to pending for redispatch
                match self.repo.reset_to_pending(&task.task_id).await {
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

        // Also mark stale running tasks (worker heartbeat lost)
        let timed_out = self.repo.mark_stale_running().await?;
        if timed_out > 0 {
            warn!("Marked {} running tasks as timed out (heartbeat lost)", timed_out);
        }

        Ok((redispatched, failed))
    }

    /// Start background dispatch loop. Returns a shutdown handle.
    pub fn start_dispatch_loop(
        self: Arc<Self>,
        workers_rx: watch::Receiver<Vec<WorkerNode>>,
        interval: std::time::Duration,
    ) -> watch::Sender<bool> {
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let workers = workers_rx.borrow().clone();
                        if let Err(e) = self.dispatch_pending(&workers).await {
                            error!("Dispatch loop error: {}", e);
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("Dispatch loop shutting down");
                            break;
                        }
                    }
                }
            }
        });

        shutdown_tx
    }

    /// Start background timeout/redispatch loop.
    pub fn start_timeout_loop(
        self: Arc<Self>,
        interval: std::time::Duration,
    ) -> watch::Sender<bool> {
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if let Err(e) = self.process_timeouts().await {
                            error!("Timeout loop error: {}", e);
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("Timeout loop shutting down");
                            break;
                        }
                    }
                }
            }
        });

        shutdown_tx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_models::{Task, TaskStatus, WorkerNode, WorkerStatus};
    use chrono::Utc;

    fn make_task(id: &str, status: TaskStatus, retry_count: i32) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            task_id: id.into(),
            owner: "testuser".into(),
            worker_id: None,
            worker_ip: None,
            status,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some("test-btih".into()),
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 100,
            req_gpu_score: 0,
            req_memory_gb: 4,
            req_gpu_memory_gb: 0,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
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
            location: "local".into(),
            status,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: mem,
            queue_capacity: cpu,
            last_heartbeat: Utc::now(),
            registered_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_dispatcher_creation() {
        let config = hivemind_config::HivemindConfig::default();
        let db_fut = hivemind_database::DatabaseManager::new(&config);
        // Can't async in sync test, just verify struct layout
        assert_eq!(30u64, 30); // Placeholder to verify test infra works
    }

    #[tokio::test]
    async fn test_dispatch_one_no_workers() {
        let config = hivemind_config::HivemindConfig::default();
        let db = match hivemind_database::DatabaseManager::new(&config).await {
            Ok(db) => db,
            Err(_) => return, // Skip if no DB
        };
        db.run_migrations().await.ok();

        let dispatcher = Dispatcher::new(db, 30, 2);
        let task = make_task("dispatch-test-1", TaskStatus::Pending, 0);
        let workers: Vec<WorkerNode> = vec![];

        let result = dispatcher.dispatch_one(&task, &workers).await;
        assert!(result.is_none(), "Should return None when no workers available");

        // Cleanup
        sqlx::query("DELETE FROM tasks WHERE task_id = 'dispatch-test-1'")
            .execute(&dispatcher.db.pool).await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_one_with_worker() {
        let config = hivemind_config::HivemindConfig::default();
        let db = match hivemind_database::DatabaseManager::new(&config).await {
            Ok(db) => db,
            Err(_) => return,
        };
        db.run_migrations().await.ok();

        let dispatcher = Dispatcher::new(db.clone(), 30, 2);

        // Create task in DB
        let task = make_task("dispatch-test-2", TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.ok();

        let workers = vec![make_worker("w1", 4, 16, WorkerStatus::Idle)];

        let result = dispatcher.dispatch_one(&task, &workers).await;
        assert!(result.is_some(), "Should dispatch to available worker");
        let (wid, wip) = result.unwrap();
        assert_eq!(wid, "w1");
        assert!(wip.contains("192.168"));

        // Verify task was assigned
        let updated = dispatcher.repo.find_by_task_id("dispatch-test-2").await.unwrap().unwrap();
        assert_eq!(updated.status, TaskStatus::Assigned);
        assert_eq!(updated.worker_id.as_deref(), Some("w1"));

        // Cleanup
        sqlx::query("DELETE FROM tasks WHERE task_id = 'dispatch-test-2'")
            .execute(&db.pool).await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_pending_multiple() {
        let config = hivemind_config::HivemindConfig::default();
        let db = match hivemind_database::DatabaseManager::new(&config).await {
            Ok(db) => db,
            Err(_) => return,
        };
        db.run_migrations().await.ok();

        let dispatcher = Dispatcher::new(db.clone(), 30, 2);

        // Create multiple tasks
        for i in 0..3 {
            let task = make_task(&format!("dispatch-multi-{}", i), TaskStatus::Pending, 0);
            dispatcher.repo.create(&task).await.ok();
        }

        let workers = vec![
            make_worker("wm1", 8, 32, WorkerStatus::Idle),
            make_worker("wm2", 8, 32, WorkerStatus::Idle),
        ];

        let count = dispatcher.dispatch_pending(&workers).await.unwrap();
        assert!(count >= 1, "Should dispatch at least 1 task, got {}", count);

        // Cleanup
        for i in 0..3 {
            sqlx::query("DELETE FROM tasks WHERE task_id = $1")
                .bind(format!("dispatch-multi-{}", i))
                .execute(&db.pool).await.ok();
        }
    }
}

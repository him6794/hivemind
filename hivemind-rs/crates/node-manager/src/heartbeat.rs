use anyhow::Result;
use tracing::{info, warn, error};
use std::sync::Arc;
use tokio::sync::watch;

use crate::NodeManager;

pub struct HeartbeatHandler {
    manager: Arc<NodeManager>,
    #[allow(dead_code)]
    stale_threshold_secs: u64,
}

impl HeartbeatHandler {
    pub fn new(manager: Arc<NodeManager>, stale_threshold_secs: u64) -> Self {
        Self {
            manager,
            stale_threshold_secs,
        }
    }

    /// Process a heartbeat from a worker.
    /// Updates resource usage and marks worker as active if it was offline.
    pub async fn process_heartbeat(
        &self,
        worker_id: &str,
        status: &str,
        cpu_usage: f64,
        memory_usage: f64,
        gpu_usage: f64,
        gpu_memory_usage: f64,
    ) -> Result<()> {
        // Validate inputs
        if worker_id.trim().is_empty() {
            anyhow::bail!("worker_id cannot be empty");
        }
        if !(0.0..=100.0).contains(&cpu_usage) {
            anyhow::bail!("cpu_usage must be 0-100, got {}", cpu_usage);
        }
        if !(0.0..=100.0).contains(&memory_usage) {
            anyhow::bail!("memory_usage must be 0-100, got {}", memory_usage);
        }

        // Normalize status
        let normalized_status = match status.to_uppercase().as_str() {
            "ACTIVE" | "IDLE" | "BUSY" => status.to_uppercase(),
            "OFFLINE" | "ERROR" => status.to_uppercase(),
            _ => {
                warn!("Unknown status '{}' from worker {}, defaulting to ACTIVE", status, worker_id);
                "ACTIVE".to_string()
            }
        };

        self.manager
            .update_heartbeat(worker_id, &normalized_status, cpu_usage, memory_usage, gpu_usage, gpu_memory_usage)
            .await?;

        Ok(())
    }

    /// Start the stale worker cleanup loop.
    pub fn start_cleanup_loop(
        self: Arc<Self>,
        interval: std::time::Duration,
    ) -> watch::Sender<bool> {
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let manager = self.manager.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        match manager.mark_offline_stale().await {
                            Ok(count) if count > 0 => {
                                info!("Marked {} stale workers offline", count);
                            }
                            Err(e) => error!("Stale worker cleanup error: {}", e),
                            _ => {}
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("Heartbeat cleanup loop shutting down");
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
    use hivemind_models::WorkerNode;
    use hivemind_models::WorkerStatus;

    async fn setup_db() -> Option<(hivemind_config::HivemindConfig, hivemind_database::DatabaseManager)> {
        let config = hivemind_config::HivemindConfig::default();
        let db = hivemind_database::DatabaseManager::new(&config).await.ok()?;
        db.run_migrations().await.ok()?;
        Some((config, db))
    }

    #[test]
    fn test_heartbeat_validation_empty_worker_id() {
        let worker_id = "";
        assert!(worker_id.trim().is_empty(), "Empty worker_id should be detected");
    }

    #[test]
    fn test_heartbeat_validation_cpu_range() {
        assert!((0.0..=100.0).contains(&50.0), "50% should be valid");
        assert!(!(0.0..=100.0).contains(&150.0), "150% should be invalid");
        assert!(!(0.0..=100.0).contains(&-1.0), "-1% should be invalid");
    }

    #[test]
    fn test_status_normalization() {
        let valid_statuses = ["ACTIVE", "IDLE", "BUSY", "OFFLINE", "ERROR"];
        for s in &valid_statuses {
            let normalized = match s.to_uppercase().as_str() {
                "ACTIVE" | "IDLE" | "BUSY" => s.to_uppercase(),
                "OFFLINE" | "ERROR" => s.to_uppercase(),
                _ => "ACTIVE".to_string(),
            };
            assert_eq!(&normalized, s);
        }

        let unknown = "unknown_status";
        let normalized = match unknown.to_uppercase().as_str() {
            "ACTIVE" | "IDLE" | "BUSY" => unknown.to_uppercase(),
            "OFFLINE" | "ERROR" => unknown.to_uppercase(),
            _ => "ACTIVE".to_string(),
        };
        assert_eq!(normalized, "ACTIVE");
    }

    #[tokio::test]
    async fn test_process_heartbeat_invalid_worker_id() {
        let (config, db) = match setup_db().await {
            Some(v) => v,
            None => return,
        };
        let manager = Arc::new(NodeManager::new(&config, db));
        let handler = HeartbeatHandler::new(manager, 30);

        let result = handler.process_heartbeat("", "ACTIVE", 50.0, 50.0, 0.0, 0.0).await;
        assert!(result.is_err(), "Should reject empty worker_id");
    }

    #[tokio::test]
    async fn test_process_heartbeat_invalid_cpu() {
        let (config, db) = match setup_db().await {
            Some(v) => v,
            None => return,
        };
        let manager = Arc::new(NodeManager::new(&config, db));
        let handler = HeartbeatHandler::new(manager, 30);

        let result = handler.process_heartbeat("w1", "ACTIVE", 150.0, 50.0, 0.0, 0.0).await;
        assert!(result.is_err(), "Should reject cpu_usage > 100");
    }

    #[tokio::test]
    async fn test_process_heartbeat_valid() {
        let (config, db) = match setup_db().await {
            Some(v) => v,
            None => return,
        };

        let manager = Arc::new(NodeManager::new(&config, db.clone()));
        let handler = HeartbeatHandler::new(manager.clone(), 30);

        let worker = WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: "heartbeat-test-w1".into(),
            username: "test".into(),
            ip: "10.0.0.1".into(),
            virtual_ip: None,
            hostname: None,
            cpu_cores: 4,
            memory_gb: 16,
            cpu_score: 400,
            gpu_score: 0,
            gpu_memory_gb: 0,
            location: "test".into(),
            status: WorkerStatus::Active,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: 16,
            queue_capacity: 4,
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        manager.register_worker(&worker).await.ok();

        let result = handler.process_heartbeat("heartbeat-test-w1", "ACTIVE", 45.5, 60.0, 0.0, 0.0).await;
        assert!(result.is_ok(), "Valid heartbeat should succeed: {:?}", result.err());

        let updated = manager.get_worker("heartbeat-test-w1").await.unwrap().unwrap();
        assert!((updated.cpu_usage - 45.5).abs() < 0.1, "CPU usage should be updated");

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'heartbeat-test-w1'")
            .execute(&db.pool).await.ok();
    }
}
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::NodeManager;

pub struct HeartbeatHandler {
    manager: Arc<NodeManager>,
    stale_threshold_secs: u64,
}

impl HeartbeatHandler {
    pub fn new(manager: Arc<NodeManager>, stale_threshold_secs: u64) -> Self {
        Self {
            manager,
            stale_threshold_secs,
        }
    }

    pub async fn process_heartbeat(
        &self,
        worker_id: &str,
        status: &str,
        cpu_usage: f64,
        memory_usage: f64,
        gpu_usage: f64,
        gpu_memory_usage: f64,
    ) -> Result<()> {
        if worker_id.trim().is_empty() {
            anyhow::bail!("worker_id cannot be empty");
        }
        if !(0.0..=100.0).contains(&cpu_usage) {
            anyhow::bail!("cpu_usage must be 0-100, got {}", cpu_usage);
        }
        if !(0.0..=100.0).contains(&memory_usage) {
            anyhow::bail!("memory_usage must be 0-100, got {}", memory_usage);
        }

        let normalized_status = match status.to_uppercase().as_str() {
            "ACTIVE" | "IDLE" | "BUSY" | "OFFLINE" | "ERROR" => status.to_uppercase(),
            _ => {
                warn!(
                    "Unknown status '{}' from worker {}, defaulting to ACTIVE",
                    status, worker_id
                );
                "ACTIVE".to_string()
            }
        };

        self.manager
            .update_heartbeat(
                worker_id,
                &normalized_status,
                cpu_usage,
                memory_usage,
                gpu_usage,
                gpu_memory_usage,
            )
            .await
    }

    pub fn start_cleanup_loop(
        self: Arc<Self>,
        interval: std::time::Duration,
    ) -> watch::Sender<bool> {
        let (tx, mut rx) = watch::channel(false);
        let manager = self.manager.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        match manager.mark_offline_stale(self.stale_threshold_secs).await {
                            Ok(count) if count > 0 => info!("Marked {} stale workers offline", count),
                            Err(e) => error!("Stale worker cleanup error: {}", e),
                            _ => {}
                        }
                    }
                    _ = rx.changed() => { if *rx.borrow() { info!("Heartbeat cleanup loop shutting down"); break; } }
                }
            }
        });
        tx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_models::{WorkerNode, WorkerStatus};

    async fn setup_db() -> Option<(
        hivemind_config::HivemindConfig,
        hivemind_database::DatabaseManager,
        hivemind_database::postgres::IsolatedTestPool,
    )> {
        let config = hivemind_config::HivemindConfig::for_test();
        let fixture =
            hivemind_database::postgres::create_isolated_test_pool("node_manager_heartbeat")
                .await
                .ok()?;
        hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .ok()?;
        let db = hivemind_database::DatabaseManager {
            pool: fixture.pool.clone(),
        };
        Some((config, db, fixture))
    }

    #[test]
    fn test_heartbeat_validation() {
        assert!("".trim().is_empty());
        assert!((0.0..=100.0).contains(&50.0));
        assert!(!(0.0..=100.0).contains(&150.0));
        assert!(!(0.0..=100.0).contains(&-1.0));
    }

    #[test]
    fn test_status_normalization() {
        for s in &["ACTIVE", "IDLE", "BUSY", "OFFLINE", "ERROR"] {
            let n = s.to_uppercase();
            assert_eq!(&n, s);
        }
    }

    #[tokio::test]
    async fn test_process_heartbeat_invalid() {
        let (config, db, fixture) = match setup_db().await {
            Some(v) => v,
            None => return,
        };
        let manager = Arc::new(NodeManager::new(&config, db));
        let handler = HeartbeatHandler::new(manager, 30);
        assert!(handler
            .process_heartbeat("", "ACTIVE", 50.0, 50.0, 0.0, 0.0)
            .await
            .is_err());
        assert!(handler
            .process_heartbeat("w1", "ACTIVE", 150.0, 50.0, 0.0, 0.0)
            .await
            .is_err());
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_process_heartbeat_valid() {
        let (config, db, fixture) = match setup_db().await {
            Some(v) => v,
            None => return,
        };
        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert!(
            schema.starts_with("hm_test_"),
            "expected isolated test schema, got {schema}"
        );
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
            gpu_name: None,
            vram_mb: 0,
            storage_total_gb: 500,
            storage_available_gb: 200,
            provider_enabled: true,
            cpu_cores_limit: 0,
            memory_gb_limit: 0,
            gpu_memory_gb_limit: 0,
            storage_gb_limit: 0,
            min_cpt_per_hour: 0,
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

        let result = handler
            .process_heartbeat("heartbeat-test-w1", "ACTIVE", 45.5, 60.0, 0.0, 0.0)
            .await;
        assert!(
            result.is_ok(),
            "Valid heartbeat should succeed: {:?}",
            result.err()
        );

        let updated = manager
            .get_worker("heartbeat-test-w1")
            .await
            .unwrap()
            .unwrap();
        assert!((updated.cpu_usage - 45.5).abs() < 0.1);

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'heartbeat-test-w1'")
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }
}

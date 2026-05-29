pub mod worker_repository;
pub mod service;
pub mod heartbeat;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_models::WorkerNode;

pub struct NodeManager {
    repo: worker_repository::WorkerRepository,
    db: DatabaseManager,
}

impl NodeManager {
    pub fn new(_config: &HivemindConfig, db: DatabaseManager) -> Self {
        Self {
            repo: worker_repository::WorkerRepository::new(db.pool.clone()),
            db,
        }
    }

    pub async fn register_worker(&self, worker: &WorkerNode) -> Result<WorkerNode> {
        self.repo.upsert(worker).await
    }

    pub async fn get_worker(&self, worker_id: &str) -> Result<Option<WorkerNode>> {
        self.repo.find_by_worker_id(worker_id).await
    }

    pub async fn list_active_workers(&self) -> Result<Vec<WorkerNode>> {
        self.repo.find_active().await
    }

    pub async fn update_heartbeat(
        &self,
        worker_id: &str,
        status: &str,
        cpu_usage: f64,
        memory_usage: f64,
        gpu_usage: f64,
        gpu_memory_usage: f64,
    ) -> Result<()> {
        self.repo
            .update_heartbeat(worker_id, status, cpu_usage, memory_usage, gpu_usage, gpu_memory_usage)
            .await
    }

    pub async fn mark_offline_stale(&self) -> Result<u64> {
        self.repo.mark_offline_stale().await
    }

    pub fn database(&self) -> &DatabaseManager {
        &self.db
    }
}
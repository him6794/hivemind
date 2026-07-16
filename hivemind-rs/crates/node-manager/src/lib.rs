pub mod grpc;
pub mod heartbeat;
pub mod service;
pub mod worker_repository;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_models::WorkerNode;

#[derive(Clone)]
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

    pub async fn register_worker_for_owner(
        &self,
        worker: &WorkerNode,
        owner: &str,
        is_admin: bool,
    ) -> Result<WorkerNode> {
        self.repo.upsert_for_owner(worker, owner, is_admin).await
    }
    pub async fn get_worker(&self, worker_id: &str) -> Result<Option<WorkerNode>> {
        self.repo.find_by_worker_id(worker_id).await
    }
    pub async fn list_active_workers(&self) -> Result<Vec<WorkerNode>> {
        self.repo.find_active().await
    }
    pub async fn list_workers(&self, include_offline: bool) -> Result<Vec<WorkerNode>> {
        self.repo.list(include_offline).await
    }
    pub async fn remove_worker(&self, worker_id: &str) -> Result<bool> {
        let count = self.repo.delete(worker_id).await?;
        Ok(count > 0)
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
            .update_heartbeat(
                worker_id,
                status,
                cpu_usage,
                memory_usage,
                gpu_usage,
                gpu_memory_usage,
            )
            .await
    }

    pub async fn mark_offline_stale(&self, stale_threshold_secs: u64) -> Result<u64> {
        self.repo.mark_offline_stale(stale_threshold_secs).await
    }
    pub fn database(&self) -> &DatabaseManager {
        &self.db
    }
}

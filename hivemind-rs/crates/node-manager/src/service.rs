use anyhow::Result;
use hivemind_models::{WorkerNode, WorkerStatus};
use crate::NodeManager;

pub struct NodeManagerService {
    manager: NodeManager,
}

impl NodeManagerService {
    pub fn new(manager: NodeManager) -> Self {
        Self { manager }
    }

    pub async fn register_worker(
        &self,
        worker_id: &str,
        username: &str,
        ip: &str,
        cpu_cores: i32,
        memory_gb: i32,
        cpu_score: i32,
        gpu_score: i32,
        gpu_memory_gb: i32,
        location: &str,
    ) -> Result<WorkerNode> {
        let worker = WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: worker_id.to_string(),
            username: username.to_string(),
            ip: ip.to_string(),
            virtual_ip: None,
            hostname: None,
            cpu_cores,
            memory_gb,
            cpu_score,
            gpu_score,
            gpu_memory_gb,
            location: location.to_string(),
            status: WorkerStatus::Active,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: memory_gb,
            queue_capacity: cpu_cores as i32,
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        self.manager.register_worker(&worker).await
    }
}

use anyhow::Result;
use hivemind_models::{WorkerNode, WorkerStatus};
use crate::NodeManager;

pub struct WorkerRegistration {
    pub worker_id: String,
    pub username: String,
    pub ip: String,
    pub cpu_cores: i32,
    pub memory_gb: i32,
    pub cpu_score: i32,
    pub gpu_score: i32,
    pub gpu_memory_gb: i32,
    pub location: String,
}

pub struct NodeManagerService {
    manager: NodeManager,
}

impl NodeManagerService {
    pub fn new(manager: NodeManager) -> Self {
        Self { manager }
    }

    pub async fn register_worker(&self, reg: &WorkerRegistration) -> Result<WorkerNode> {
        let worker = WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: reg.worker_id.clone(),
            username: reg.username.clone(),
            ip: reg.ip.clone(),
            virtual_ip: None,
            hostname: None,
            cpu_cores: reg.cpu_cores,
            memory_gb: reg.memory_gb,
            cpu_score: reg.cpu_score,
            gpu_score: reg.gpu_score,
            gpu_memory_gb: reg.gpu_memory_gb,
            location: reg.location.clone(),
            status: WorkerStatus::Active,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: reg.memory_gb,
            queue_capacity: reg.cpu_cores,
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        self.manager.register_worker(&worker).await
    }
}
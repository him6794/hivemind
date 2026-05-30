use anyhow::Result;
use hivemind_models::{WorkerNode, WorkerStatus, ResourceSpec};
use crate::NodeManager;

pub struct WorkerRegistration {
    pub worker_id: String,
    pub username: String,
    pub ip: String,
    pub resources: ResourceSpec,
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
        let gpu_count = reg.resources.gpu_count;
        let worker = WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: reg.worker_id.clone(),
            username: reg.username.clone(),
            ip: reg.ip.clone(),
            virtual_ip: None,
            hostname: None,
            cpu_cores: reg.resources.cpu_cores,
            memory_gb: (reg.resources.memory_mb / 1024) as i32,
            cpu_score: reg.resources.cpu_score,
            gpu_score: reg.resources.gpu_score,
            gpu_memory_gb: (reg.resources.vram_mb / 1024) as i32,
            gpu_name: if gpu_count > 0 { Some(reg.resources.gpu_name.clone()) } else { None },
            vram_mb: reg.resources.vram_mb,
            storage_total_gb: reg.resources.storage_total_gb,
            storage_available_gb: reg.resources.storage_available_gb,
            location: reg.location.clone(),
            status: WorkerStatus::Active,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: (reg.resources.memory_mb / 1024) as i32,
            queue_capacity: reg.resources.cpu_cores,
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        self.manager.register_worker(&worker).await
    }
}

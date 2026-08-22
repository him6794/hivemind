use crate::NodeManager;
use anyhow::Result;
use hivemind_models::{ResourceSpec, WorkerCapabilityReport, WorkerNode, WorkerStatus};
use sha2::{Digest, Sha256};

pub struct WorkerRegistration {
    pub worker_id: String,
    pub username: String,
    pub ip: String,
    pub resources: ResourceSpec,
    pub location: String,
    pub general_compute_capabilities_json: Option<String>,
    pub managed_dsl_capabilities_json: Option<String>,
    pub admission_mode: String,
    pub dynamic_capability_report: Option<WorkerCapabilityReport>,
}

pub fn dynamic_capability_observation(
    report: &WorkerCapabilityReport,
) -> Result<(String, String, bool, Option<String>)> {
    report
        .validate_public_dynamic()
        .map_err(|error| anyhow::anyhow!("invalid worker capability report: {error}"))?;
    let capabilities_json = report
        .capabilities_json()
        .map_err(|error| anyhow::anyhow!("invalid worker capability report: {error}"))?;
    let digest = Sha256::digest(capabilities_json.as_bytes());
    Ok((
        capabilities_json,
        format!("sha256:{digest:x}"),
        report.ready,
        (!report.readiness_reason.trim().is_empty()).then(|| report.readiness_reason.clone()),
    ))
}

pub struct NodeManagerService {
    manager: NodeManager,
}

impl NodeManagerService {
    pub fn new(manager: NodeManager) -> Self {
        Self { manager }
    }

    pub async fn register_worker(&self, reg: &WorkerRegistration) -> Result<WorkerNode> {
        self.register_worker_with_authorization(reg, "", true).await
    }

    pub async fn register_worker_for_owner(
        &self,
        reg: &WorkerRegistration,
        owner: &str,
        is_admin: bool,
    ) -> Result<WorkerNode> {
        self.register_worker_with_authorization(reg, owner, is_admin)
            .await
    }

    async fn register_worker_with_authorization(
        &self,
        reg: &WorkerRegistration,
        owner: &str,
        is_admin: bool,
    ) -> Result<WorkerNode> {
        let configured_admission_mode = self.manager.admission_mode().to_string();
        if reg.admission_mode != configured_admission_mode {
            anyhow::bail!(
                "worker admission mode does not match the configured Nodepool admission mode"
            );
        }
        let public_dynamic = self.manager.is_public_dynamic_admission();
        let dynamic_observation = if public_dynamic {
            Some(dynamic_capability_observation(
                reg.dynamic_capability_report.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("public dynamic admission requires a capability report")
                })?,
            )?)
        } else {
            None
        };
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
            gpu_name: if gpu_count > 0 {
                Some(reg.resources.gpu_name.clone())
            } else {
                None
            },
            vram_mb: reg.resources.vram_mb,
            storage_total_gb: reg.resources.storage_total_gb,
            storage_available_gb: reg.resources.storage_available_gb,
            provider_enabled: true,
            cpu_cores_limit: 0,
            memory_gb_limit: 0,
            gpu_memory_gb_limit: 0,
            storage_gb_limit: 0,
            min_cpt_per_hour: 0,
            location: reg.location.clone(),
            status: WorkerStatus::Active,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: (reg.resources.memory_mb / 1024) as i32,
            queue_capacity: reg.resources.cpu_cores,
            general_compute_capabilities_json: if reg.admission_mode
                == hivemind_models::PUBLIC_DYNAMIC_ADMISSION_MODE
            {
                None
            } else {
                reg.general_compute_capabilities_json.clone()
            },
            managed_dsl_capabilities_json: if reg.admission_mode
                == hivemind_models::PUBLIC_DYNAMIC_ADMISSION_MODE
            {
                None
            } else {
                reg.managed_dsl_capabilities_json.clone()
            },
            admission_mode: reg.admission_mode.clone(),
            dynamic_capabilities_json: dynamic_observation
                .as_ref()
                .map(|observation| observation.0.clone()),
            dynamic_capabilities_digest: dynamic_observation
                .as_ref()
                .map(|observation| observation.1.clone()),
            dynamic_admission_ready: dynamic_observation
                .as_ref()
                .is_some_and(|observation| observation.2),
            dynamic_readiness_reason: dynamic_observation
                .as_ref()
                .and_then(|observation| observation.3.clone()),
            dynamic_observed_at: dynamic_observation.as_ref().map(|_| chrono::Utc::now()),
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        if is_admin {
            self.manager.register_worker(&worker).await
        } else {
            self.manager
                .register_worker_for_owner(&worker, owner, false)
                .await
        }
    }
}

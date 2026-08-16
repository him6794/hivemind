pub mod grpc;
pub mod heartbeat;
pub mod service;
pub mod worker_repository;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_models::WorkerNode;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct NodeManager {
    repo: worker_repository::WorkerRepository,
    db: DatabaseManager,
    trusted_general_compute_capabilities:
        BTreeMap<String, hivemind_config::TrustedGeneralComputeWorkerRegistration>,
    trusted_managed_dsl_capabilities:
        BTreeMap<String, hivemind_config::TrustedManagedDslWorkerRegistration>,
}

impl NodeManager {
    pub fn new(config: &HivemindConfig, db: DatabaseManager) -> Self {
        Self {
            repo: worker_repository::WorkerRepository::new(db.pool.clone()),
            db,
            trusted_general_compute_capabilities: config
                .general_compute
                .trusted_worker_capabilities
                .clone(),
            trusted_managed_dsl_capabilities: config
                .general_compute
                .trusted_managed_dsl_worker_capabilities
                .clone(),
        }
    }

    pub fn trusted_managed_dsl_capabilities_json_for_owner(
        &self,
        worker_id: &str,
        owner: &str,
        is_admin: bool,
    ) -> Result<Option<String>> {
        let Some(registered) = self.trusted_managed_dsl_capabilities.get(worker_id) else {
            return Ok(None);
        };
        if !is_admin && registered.owner != owner {
            anyhow::bail!(
                "managed DSL capability registration owner does not match authenticated owner"
            );
        }
        serde_json::to_string(&registered.registrations)
            .map(Some)
            .map_err(Into::into)
    }

    pub async fn register_worker(&self, worker: &WorkerNode) -> Result<WorkerNode> {
        self.repo.upsert(worker).await
    }

    pub fn trusted_general_compute_capabilities_json_for_owner(
        &self,
        worker_id: &str,
        owner: &str,
        is_admin: bool,
    ) -> Result<Option<String>> {
        let Some(registered) = self.trusted_general_compute_capabilities.get(worker_id) else {
            return Ok(None);
        };
        if !is_admin && registered.owner != owner {
            anyhow::bail!(
                "worker capability registration owner does not match authenticated owner"
            );
        }
        serde_json::to_string(&registered.registration)
            .map(Some)
            .map_err(Into::into)
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

#[cfg(test)]
mod tests {
    use super::*;
    use general_compute_runtime::{
        BackendRegistration, TrustedWorkerCapabilityRegistration, WorkerCapabilities,
    };

    #[tokio::test]
    async fn trusted_general_compute_capability_is_bound_to_its_operator_configured_owner() {
        let mut config = HivemindConfig::for_test();
        config.general_compute.trusted_worker_capabilities.insert(
            "worker-alpha".into(),
            hivemind_config::TrustedGeneralComputeWorkerRegistration {
                owner: "approved-owner".into(),
                registration: TrustedWorkerCapabilityRegistration {
                    worker: WorkerCapabilities {
                        guest_image_digests: vec![
                            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                .into(),
                        ],
                        capabilities: vec!["cpu".into()],
                        max_threads: 4,
                        gpu_available: false,
                    },
                    gpu_capabilities: vec![],
                    backends: vec![BackendRegistration {
                        backend_id: "python-cpython-312".into(),
                        execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
                        guest_image_digest:
                            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                .into(),
                        capabilities: vec!["cpu".into()],
                        max_threads: 4,
                        network_allowed: false,
                        filesystem_read_only: true,
                        gpu_allowed: false,
                    }],
                },
            },
        );
        let db = DatabaseManager {
            pool: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://postgres:postgres@127.0.0.1/hivemind_test")
                .unwrap(),
        };
        let manager = NodeManager::new(&config, db);

        let rejected = manager.trusted_general_compute_capabilities_json_for_owner(
            "worker-alpha",
            "different-owner",
            false,
        );
        let approved = manager.trusted_general_compute_capabilities_json_for_owner(
            "worker-alpha",
            "approved-owner",
            false,
        );

        assert!(rejected.is_err());
        assert!(approved.unwrap().is_some());
    }
}

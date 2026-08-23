use anyhow::Result;
use hivemind_models::WorkerNode;
use sqlx::PgPool;

#[derive(Clone)]
pub struct WorkerRepository {
    pool: PgPool,
}

impl WorkerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert(&self, worker: &WorkerNode) -> Result<WorkerNode> {
        let dynamic_ready = worker
            .dynamic_observed_at
            .map(|_| worker.dynamic_admission_ready);
        let existing = self.find_by_worker_id(&worker.worker_id).await?;
        if existing.is_some() {
            sqlx::query_as::<_, WorkerNode>(
                "UPDATE worker_nodes SET username = $1,
                 ip = CASE WHEN $2 = '' THEN worker_nodes.ip ELSE $2 END,
                 cpu_cores = $3, memory_gb = $4,
                 cpu_score = $5, gpu_score = $6, gpu_memory_gb = $7,
                 gpu_name = $8, vram_mb = $9,
                 storage_total_gb = $10, storage_available_gb = $11,
                 location = $12, status = $13,
                 available_memory_gb = $14, queue_capacity = $15,
                 admission_mode = $16,
                 general_compute_capabilities_json = COALESCE($17, general_compute_capabilities_json),
                 managed_dsl_capabilities_json = COALESCE($18, managed_dsl_capabilities_json),
                 dynamic_capabilities_json = COALESCE($19, dynamic_capabilities_json),
                 dynamic_capabilities_digest = COALESCE($20, dynamic_capabilities_digest),
                 dynamic_admission_ready = COALESCE($21, dynamic_admission_ready),
                 dynamic_readiness_reason = COALESCE($22, dynamic_readiness_reason),
                 dynamic_observed_at = COALESCE($23, dynamic_observed_at),
                 last_heartbeat = NOW(), updated_at = NOW()
                 WHERE worker_id = $24 RETURNING *",
            )
            .bind(&worker.username)
            .bind(&worker.ip)
            .bind(worker.cpu_cores)
            .bind(worker.memory_gb)
            .bind(worker.cpu_score)
            .bind(worker.gpu_score)
            .bind(worker.gpu_memory_gb)
            .bind(&worker.gpu_name)
            .bind(worker.vram_mb)
            .bind(worker.storage_total_gb)
            .bind(worker.storage_available_gb)
            .bind(&worker.location)
            .bind(worker.status.as_str())
            .bind(worker.available_memory_gb)
            .bind(worker.queue_capacity)
            .bind(&worker.admission_mode)
            .bind(&worker.general_compute_capabilities_json)
            .bind(&worker.managed_dsl_capabilities_json)
            .bind(&worker.dynamic_capabilities_json)
            .bind(&worker.dynamic_capabilities_digest)
            .bind(dynamic_ready)
            .bind(&worker.dynamic_readiness_reason)
            .bind(worker.dynamic_observed_at)
            .bind(&worker.worker_id)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
        } else {
            sqlx::query_as::<_, WorkerNode>(
                "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb,
                 cpu_score, gpu_score, gpu_memory_gb,
                 gpu_name, vram_mb, storage_total_gb, storage_available_gb,
                 location, status, available_memory_gb, queue_capacity,
                admission_mode, general_compute_capabilities_json,
                managed_dsl_capabilities_json, dynamic_capabilities_json,
                dynamic_capabilities_digest, dynamic_admission_ready,
                dynamic_readiness_reason, dynamic_observed_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,
                         $17,$18,$19,$20,$21,$22,$23,$24) RETURNING *",
            )
            .bind(&worker.worker_id)
            .bind(&worker.username)
            .bind(&worker.ip)
            .bind(worker.cpu_cores)
            .bind(worker.memory_gb)
            .bind(worker.cpu_score)
            .bind(worker.gpu_score)
            .bind(worker.gpu_memory_gb)
            .bind(&worker.gpu_name)
            .bind(worker.vram_mb)
            .bind(worker.storage_total_gb)
            .bind(worker.storage_available_gb)
            .bind(&worker.location)
            .bind(worker.status.as_str())
            .bind(worker.available_memory_gb)
            .bind(worker.queue_capacity)
            .bind(&worker.admission_mode)
            .bind(&worker.general_compute_capabilities_json)
            .bind(&worker.managed_dsl_capabilities_json)
            .bind(&worker.dynamic_capabilities_json)
            .bind(&worker.dynamic_capabilities_digest)
            .bind(worker.dynamic_admission_ready)
            .bind(&worker.dynamic_readiness_reason)
            .bind(worker.dynamic_observed_at)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
        }
    }

    pub async fn replace_static_capabilities(
        &self,
        worker_id: &str,
        general_compute_capabilities_json: Option<&str>,
        managed_dsl_capabilities_json: Option<&str>,
    ) -> Result<WorkerNode> {
        sqlx::query_as::<_, WorkerNode>(
            "UPDATE worker_nodes
             SET general_compute_capabilities_json = $1,
                 managed_dsl_capabilities_json = $2,
                 updated_at = NOW()
             WHERE worker_id = $3
             RETURNING *",
        )
        .bind(general_compute_capabilities_json)
        .bind(managed_dsl_capabilities_json)
        .bind(worker_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// records may only be refreshed by their current owner (or an admin).
    /// The owner predicate is part of the UPDATE so a concurrent request
    /// cannot overwrite a worker after an earlier read/check.
    pub async fn upsert_for_owner(
        &self,
        worker: &WorkerNode,
        owner: &str,
        is_admin: bool,
    ) -> Result<WorkerNode> {
        let dynamic_ready = worker
            .dynamic_observed_at
            .map(|_| worker.dynamic_admission_ready);
        let updated = sqlx::query_as::<_, WorkerNode>(
            "UPDATE worker_nodes SET username = $1,
             ip = CASE WHEN $2 = '' THEN worker_nodes.ip ELSE $2 END,
             cpu_cores = $3, memory_gb = $4,
             cpu_score = $5, gpu_score = $6, gpu_memory_gb = $7,
             gpu_name = $8, vram_mb = $9,
             storage_total_gb = $10, storage_available_gb = $11,
             location = $12, status = $13,
             available_memory_gb = $14, queue_capacity = $15,
             admission_mode = $16,
             general_compute_capabilities_json = $17,
             managed_dsl_capabilities_json = $18,
             dynamic_capabilities_json = $19,
             dynamic_capabilities_digest = $20,
             dynamic_admission_ready = COALESCE($21, dynamic_admission_ready),
             dynamic_readiness_reason = $22,
             dynamic_observed_at = $23,
             last_heartbeat = NOW(), updated_at = NOW()
             WHERE worker_id = $24 AND (username = $25 OR $26)
             RETURNING *",
        )
        .bind(&worker.username)
        .bind(&worker.ip)
        .bind(worker.cpu_cores)
        .bind(worker.memory_gb)
        .bind(worker.cpu_score)
        .bind(worker.gpu_score)
        .bind(worker.gpu_memory_gb)
        .bind(&worker.gpu_name)
        .bind(worker.vram_mb)
        .bind(worker.storage_total_gb)
        .bind(worker.storage_available_gb)
        .bind(&worker.location)
        .bind(worker.status.as_str())
        .bind(worker.available_memory_gb)
        .bind(worker.queue_capacity)
        .bind(&worker.admission_mode)
        .bind(&worker.general_compute_capabilities_json)
        .bind(&worker.managed_dsl_capabilities_json)
        .bind(&worker.dynamic_capabilities_json)
        .bind(&worker.dynamic_capabilities_digest)
        .bind(dynamic_ready)
        .bind(&worker.dynamic_readiness_reason)
        .bind(worker.dynamic_observed_at)
        .bind(&worker.worker_id)
        .bind(owner)
        .bind(is_admin)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(updated) = updated {
            return Ok(updated);
        }

        // A missing row can be inserted. If another request wins the race,
        // the unique worker_id constraint rejects the insert rather than
        // allowing an ownership-changing overwrite.
        sqlx::query_as::<_, WorkerNode>(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb,
             cpu_score, gpu_score, gpu_memory_gb,
             gpu_name, vram_mb, storage_total_gb, storage_available_gb,
             location, status, available_memory_gb, queue_capacity,
             admission_mode, general_compute_capabilities_json,
             managed_dsl_capabilities_json, dynamic_capabilities_json,
             dynamic_capabilities_digest, dynamic_admission_ready,
             dynamic_readiness_reason, dynamic_observed_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,
                     $17,$18,$19,$20,$21,$22,$23,$24)
             RETURNING *",
        )
        .bind(&worker.worker_id)
        .bind(&worker.username)
        .bind(&worker.ip)
        .bind(worker.cpu_cores)
        .bind(worker.memory_gb)
        .bind(worker.cpu_score)
        .bind(worker.gpu_score)
        .bind(worker.gpu_memory_gb)
        .bind(&worker.gpu_name)
        .bind(worker.vram_mb)
        .bind(worker.storage_total_gb)
        .bind(worker.storage_available_gb)
        .bind(&worker.location)
        .bind(worker.status.as_str())
        .bind(worker.available_memory_gb)
        .bind(worker.queue_capacity)
        .bind(&worker.admission_mode)
        .bind(&worker.general_compute_capabilities_json)
        .bind(&worker.managed_dsl_capabilities_json)
        .bind(&worker.dynamic_capabilities_json)
        .bind(&worker.dynamic_capabilities_digest)
        .bind(worker.dynamic_admission_ready)
        .bind(&worker.dynamic_readiness_reason)
        .bind(worker.dynamic_observed_at)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_worker_id(&self, worker_id: &str) -> Result<Option<WorkerNode>> {
        sqlx::query_as::<_, WorkerNode>("SELECT * FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_active(&self) -> Result<Vec<WorkerNode>> {
        sqlx::query_as::<_, WorkerNode>(
            "SELECT * FROM worker_nodes WHERE status IN ('ACTIVE', 'IDLE', 'BUSY')",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn list(&self, include_offline: bool) -> Result<Vec<WorkerNode>> {
        let query = if include_offline {
            "SELECT * FROM worker_nodes ORDER BY registered_at DESC"
        } else {
            "SELECT * FROM worker_nodes WHERE status IN ('ACTIVE', 'IDLE', 'BUSY') ORDER BY registered_at DESC"
        };
        sqlx::query_as::<_, WorkerNode>(query)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_heartbeat(
        &self,
        worker_id: &str,
        admission_mode: &str,
        status: &str,
        cpu_usage: f64,
        memory_usage: f64,
        gpu_usage: f64,
        gpu_memory_usage: f64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE worker_nodes SET status = $1, cpu_usage = $2, memory_usage = $3,
             gpu_usage = $4, gpu_memory_usage = $5, admission_mode = $6,
             last_heartbeat = NOW(), updated_at = NOW() WHERE worker_id = $7",
        )
        .bind(status)
        .bind(cpu_usage)
        .bind(memory_usage)
        .bind(gpu_usage)
        .bind(gpu_memory_usage)
        .bind(admission_mode)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_heartbeat_with_dynamic_capabilities(
        &self,
        worker_id: &str,
        admission_mode: &str,
        status: &str,
        cpu_usage: f64,
        memory_usage: f64,
        gpu_usage: f64,
        gpu_memory_usage: f64,
        capabilities_json: &str,
        capabilities_digest: &str,
        ready: bool,
        readiness_reason: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE worker_nodes SET status = $1, cpu_usage = $2, memory_usage = $3,
             gpu_usage = $4, gpu_memory_usage = $5, admission_mode = $6,
             dynamic_capabilities_json = $7, dynamic_capabilities_digest = $8,
             dynamic_admission_ready = $9, dynamic_readiness_reason = $10,
             dynamic_observed_at = NOW(), last_heartbeat = NOW(), updated_at = NOW()
             WHERE worker_id = $11",
        )
        .bind(status)
        .bind(cpu_usage)
        .bind(memory_usage)
        .bind(gpu_usage)
        .bind(gpu_memory_usage)
        .bind(admission_mode)
        .bind(capabilities_json)
        .bind(capabilities_digest)
        .bind(ready)
        .bind(readiness_reason)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_dynamic_capabilities(
        &self,
        worker_id: &str,
        capabilities_json: &str,
        capabilities_digest: &str,
        ready: bool,
        readiness_reason: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE worker_nodes SET dynamic_capabilities_json = $1,
             dynamic_capabilities_digest = $2, dynamic_admission_ready = $3,
             dynamic_readiness_reason = $4, dynamic_observed_at = NOW(),
             updated_at = NOW() WHERE worker_id = $5",
        )
        .bind(capabilities_json)
        .bind(capabilities_digest)
        .bind(ready)
        .bind(readiness_reason)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, worker_id: &str) -> Result<u64> {
        let result = sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn mark_offline_stale(&self, stale_threshold_secs: u64) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE worker_nodes SET status = 'OFFLINE', updated_at = NOW()
             WHERE status != 'OFFLINE' AND last_heartbeat < NOW() - ($1 * INTERVAL '1 second')",
        )
        .bind(stale_threshold_secs as i64)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_models::WorkerStatus;

    async fn pool(test_name: &str) -> Option<hivemind_database::postgres::IsolatedTestPool> {
        let fixture = hivemind_database::postgres::create_isolated_test_pool(test_name)
            .await
            .ok()?;
        hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .ok()?;
        Some(fixture)
    }

    #[tokio::test]
    async fn test_upsert_worker() {
        let fixture = match pool("worker_repository_upsert").await {
            Some(fixture) => fixture,
            None => return,
        };
        let repo = WorkerRepository::new(fixture.pool.clone());
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'example-upsert-1'")
            .execute(&repo.pool)
            .await
            .ok();

        let worker = WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: "example-upsert-1".into(),
            username: "example-user".into(),
            ip: "192.168.1.1".into(),
            virtual_ip: None,
            hostname: Some("test-host".into()),
            cpu_cores: 4,
            memory_gb: 16,
            cpu_score: 100,
            gpu_score: 0,
            gpu_memory_gb: 0,
            gpu_name: Some("NVIDIA RTX 4060".into()),
            vram_mb: 8192,
            storage_total_gb: 500,
            storage_available_gb: 300,
            provider_enabled: true,
            cpu_cores_limit: 0,
            memory_gb_limit: 0,
            gpu_memory_gb_limit: 0,
            storage_gb_limit: 0,
            min_cpt_per_hour: 0,
            location: "us-east".into(),
            status: WorkerStatus::Active,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: 16,
            queue_capacity: 4,
            general_compute_capabilities_json: None,
            managed_dsl_capabilities_json: None,
            admission_mode: hivemind_models::PRIVATE_STATIC_ADMISSION_MODE.into(),
            dynamic_capabilities_json: None,
            dynamic_capabilities_digest: None,
            dynamic_admission_ready: false,
            dynamic_readiness_reason: None,
            dynamic_observed_at: None,
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let created = repo.upsert(&worker).await.unwrap();
        assert_eq!(created.worker_id, "example-upsert-1");
        assert_eq!(created.cpu_cores, 4);
        assert_eq!(created.storage_total_gb, 500);

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'example-upsert-1'")
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_untrusted_worker_refresh_preserves_nodepool_capability_snapshot() {
        let fixture = match pool("worker_repository_capability_snapshot").await {
            Some(fixture) => fixture,
            None => return,
        };
        let repo = WorkerRepository::new(fixture.pool.clone());
        let worker_id = "capability-snapshot-worker";
        let snapshot = r#"{"worker":{"guest_image_digests":["sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],"capabilities":["cpu"],"max_threads":4,"gpu_available":false},"backends":[]}"#;

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&repo.pool)
            .await
            .ok();

        let mut registered = test_worker(worker_id, WorkerStatus::Active);
        registered.general_compute_capabilities_json = Some(snapshot.into());
        repo.upsert(&registered).await.unwrap();

        // A worker heartbeat is an untrusted refresh and does not carry the
        // Nodepool-owned capability snapshot.
        let mut heartbeat = registered.clone();
        heartbeat.ip = "192.168.1.99".into();
        heartbeat.general_compute_capabilities_json = None;
        let refreshed = repo.upsert(&heartbeat).await.unwrap();

        assert_eq!(refreshed.ip, "192.168.1.99");
        assert_eq!(
            refreshed.general_compute_capabilities_json.as_deref(),
            Some(snapshot)
        );

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_session_only_registration_preserves_the_stored_callback_address() {
        let fixture = match pool("worker_repository_session_only_ip").await {
            Some(fixture) => fixture,
            None => return,
        };
        let repo = WorkerRepository::new(fixture.pool.clone());
        let worker_id = "session-only-ip-worker";
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&repo.pool)
            .await
            .ok();

        let mut registered = test_worker(worker_id, WorkerStatus::Active);
        registered.ip = "10.42.0.9:50053".into();
        repo.upsert(&registered).await.unwrap();

        // A session-only re-registration reports no callback address; the
        // stored address must survive so legacy direct callers keep working.
        let mut session_only = registered.clone();
        session_only.ip = String::new();
        let updated = repo.upsert(&session_only).await.unwrap();
        assert_eq!(updated.ip, "10.42.0.9:50053");

        let updated_for_owner = repo
            .upsert_for_owner(&session_only, "example-user", false)
            .await
            .unwrap();
        assert_eq!(updated_for_owner.ip, "10.42.0.9:50053");

        // A real address update still replaces the stored value.
        let mut new_addr = registered.clone();
        new_addr.ip = "10.42.0.10:50053".into();
        let replaced = repo.upsert(&new_addr).await.unwrap();
        assert_eq!(replaced.ip, "10.42.0.10:50053");

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_operator_registration_can_revoke_capability_snapshot() {
        let fixture = match pool("worker_repository_capability_revoke").await {
            Some(fixture) => fixture,
            None => return,
        };
        let repo = WorkerRepository::new(fixture.pool.clone());
        let worker_id = "capability-revoke-worker";
        let snapshot = r#"{"worker":{"guest_image_digests":[],"capabilities":["cpu"],"max_threads":1,"gpu_available":false},"backends":[]}"#;
        let mut registered = test_worker(worker_id, WorkerStatus::Active);
        registered.general_compute_capabilities_json = Some(snapshot.into());
        repo.upsert(&registered).await.unwrap();

        let mut revoked = registered.clone();
        revoked.general_compute_capabilities_json = None;
        let stored = repo
            .upsert_for_owner(&revoked, "example-user", false)
            .await
            .unwrap();

        assert!(stored.general_compute_capabilities_json.is_none());

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_list_include_offline_preserves_offline_workers() {
        let fixture = match pool("worker_repository_list_offline").await {
            Some(fixture) => fixture,
            None => return,
        };
        let repo = WorkerRepository::new(fixture.pool.clone());
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id LIKE 'test-list-%'")
            .execute(&repo.pool)
            .await
            .ok();

        let mut active = test_worker("test-list-active", WorkerStatus::Idle);
        let mut offline = test_worker("test-list-offline", WorkerStatus::Offline);
        active.ip = "192.168.10.1".into();
        offline.ip = "192.168.10.2".into();

        repo.upsert(&active).await.unwrap();
        repo.upsert(&offline).await.unwrap();

        let online_only = repo.list(false).await.unwrap();
        assert!(online_only
            .iter()
            .any(|w| w.worker_id == "test-list-active"));
        assert!(!online_only
            .iter()
            .any(|w| w.worker_id == "test-list-offline"));

        let all = repo.list(true).await.unwrap();
        assert!(all.iter().any(|w| w.worker_id == "test-list-active"));
        assert!(all.iter().any(|w| w.worker_id == "test-list-offline"));

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id LIKE 'test-list-%'")
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_mark_offline_stale_respects_threshold() {
        let fixture = match pool("worker_repository_stale_offline").await {
            Some(fixture) => fixture,
            None => return,
        };
        let repo = WorkerRepository::new(fixture.pool.clone());
        let worker_id = "test-stale-threshold";
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&repo.pool)
            .await
            .ok();

        let worker = test_worker(worker_id, WorkerStatus::Active);
        repo.upsert(&worker).await.unwrap();
        sqlx::query(
            "UPDATE worker_nodes SET last_heartbeat = NOW() - INTERVAL '2 minutes' WHERE worker_id = $1",
        )
        .bind(worker_id)
        .execute(&repo.pool)
        .await
        .unwrap();

        let changed = repo.mark_offline_stale(30).await.unwrap();
        assert!(changed >= 1);

        let stored = repo.find_by_worker_id(worker_id).await.unwrap().unwrap();
        assert_eq!(stored.status, WorkerStatus::Offline);

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    fn test_worker(worker_id: &str, status: WorkerStatus) -> WorkerNode {
        WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: worker_id.into(),
            username: "example-user".into(),
            ip: "192.168.1.1".into(),
            virtual_ip: None,
            hostname: Some(format!("{worker_id}-host")),
            cpu_cores: 4,
            memory_gb: 16,
            cpu_score: 100,
            gpu_score: 0,
            gpu_memory_gb: 0,
            gpu_name: Some("NVIDIA RTX 4060".into()),
            vram_mb: 8192,
            storage_total_gb: 500,
            storage_available_gb: 300,
            provider_enabled: true,
            cpu_cores_limit: 0,
            memory_gb_limit: 0,
            gpu_memory_gb_limit: 0,
            storage_gb_limit: 0,
            min_cpt_per_hour: 0,
            location: "us-east".into(),
            status,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: 16,
            queue_capacity: 4,
            general_compute_capabilities_json: None,
            managed_dsl_capabilities_json: None,
            admission_mode: hivemind_models::PRIVATE_STATIC_ADMISSION_MODE.into(),
            dynamic_capabilities_json: None,
            dynamic_capabilities_digest: None,
            dynamic_admission_ready: false,
            dynamic_readiness_reason: None,
            dynamic_observed_at: None,
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}

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
        let existing = self.find_by_worker_id(&worker.worker_id).await?;
        if existing.is_some() {
            sqlx::query_as::<_, WorkerNode>(
                "UPDATE worker_nodes SET username = $1, ip = $2, cpu_cores = $3, memory_gb = $4,
                 cpu_score = $5, gpu_score = $6, gpu_memory_gb = $7,
                 gpu_name = $8, vram_mb = $9,
                 storage_total_gb = $10, storage_available_gb = $11,
                 location = $12, status = $13,
                 available_memory_gb = $14, queue_capacity = $15,
                 last_heartbeat = NOW(), updated_at = NOW()
                 WHERE worker_id = $16 RETURNING *",
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
            .bind(&worker.worker_id)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
        } else {
            sqlx::query_as::<_, WorkerNode>(
                "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb,
                 cpu_score, gpu_score, gpu_memory_gb,
                 gpu_name, vram_mb, storage_total_gb, storage_available_gb,
                 location, status, available_memory_gb, queue_capacity)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) RETURNING *",
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
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
        }
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

    pub async fn update_heartbeat(
        &self,
        worker_id: &str,
        status: &str,
        cpu_usage: f64,
        memory_usage: f64,
        gpu_usage: f64,
        gpu_memory_usage: f64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE worker_nodes SET status = $1, cpu_usage = $2, memory_usage = $3,
             gpu_usage = $4, gpu_memory_usage = $5,
             last_heartbeat = NOW(), updated_at = NOW() WHERE worker_id = $6",
        )
        .bind(status)
        .bind(cpu_usage)
        .bind(memory_usage)
        .bind(gpu_usage)
        .bind(gpu_memory_usage)
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

    pub async fn mark_offline_stale(&self) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE worker_nodes SET status = 'OFFLINE', updated_at = NOW()
             WHERE status != 'OFFLINE' AND last_heartbeat < NOW() - INTERVAL '30 seconds'",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_models::WorkerStatus;
    use sqlx::postgres::PgPoolOptions;

    async fn pool() -> Option<PgPool> {
        let url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        let p = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .ok()?;
        hivemind_database::postgres::run_migrations(&p).await.ok()?;
        Some(p)
    }

    #[tokio::test]
    async fn test_upsert_worker() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        let repo = WorkerRepository::new(p);
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'test-upsert-1'")
            .execute(&repo.pool)
            .await
            .ok();

        let worker = WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: "test-upsert-1".into(),
            username: "testuser".into(),
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
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let created = repo.upsert(&worker).await.unwrap();
        assert_eq!(created.worker_id, "test-upsert-1");
        assert_eq!(created.cpu_cores, 4);
        assert_eq!(created.storage_total_gb, 500);

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'test-upsert-1'")
            .execute(&repo.pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_list_include_offline_preserves_offline_workers() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        let repo = WorkerRepository::new(p);
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
    }

    fn test_worker(worker_id: &str, status: WorkerStatus) -> WorkerNode {
        WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: worker_id.into(),
            username: "testuser".into(),
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
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}

use anyhow::Result;
use hivemind_models::WorkerNode;
use sqlx::PgPool;

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
            let updated = sqlx::query_as::<_, WorkerNode>(
                r#"
                UPDATE worker_nodes SET
                    username = $1, ip = $2, cpu_cores = $3, memory_gb = $4,
                    cpu_score = $5, gpu_score = $6, gpu_memory_gb = $7,
                    location = $8, status = $9, last_heartbeat = NOW(),
                    updated_at = NOW()
                WHERE worker_id = $10
                RETURNING *
                "#,
            )
            .bind(&worker.username)
            .bind(&worker.ip)
            .bind(worker.cpu_cores)
            .bind(worker.memory_gb)
            .bind(worker.cpu_score)
            .bind(worker.gpu_score)
            .bind(worker.gpu_memory_gb)
            .bind(&worker.location)
            .bind(worker.status.as_str())
            .bind(&worker.worker_id)
            .fetch_one(&self.pool)
            .await?;
            Ok(updated)
        } else {
            let created = sqlx::query_as::<_, WorkerNode>(
                r#"
                INSERT INTO worker_nodes (
                    worker_id, username, ip, cpu_cores, memory_gb,
                    cpu_score, gpu_score, gpu_memory_gb, location, status
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                RETURNING *
                "#,
            )
            .bind(&worker.worker_id)
            .bind(&worker.username)
            .bind(&worker.ip)
            .bind(worker.cpu_cores)
            .bind(worker.memory_gb)
            .bind(worker.cpu_score)
            .bind(worker.gpu_score)
            .bind(worker.gpu_memory_gb)
            .bind(&worker.location)
            .bind(worker.status.as_str())
            .fetch_one(&self.pool)
            .await?;
            Ok(created)
        }
    }

    pub async fn find_by_worker_id(&self, worker_id: &str) -> Result<Option<WorkerNode>> {
        let worker = sqlx::query_as::<_, WorkerNode>(
            "SELECT * FROM worker_nodes WHERE worker_id = $1",
        )
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(worker)
    }

    pub async fn find_active(&self) -> Result<Vec<WorkerNode>> {
        let workers = sqlx::query_as::<_, WorkerNode>(
            "SELECT * FROM worker_nodes WHERE status IN ('ACTIVE', 'IDLE', 'BUSY')",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(workers)
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
            r#"
            UPDATE worker_nodes SET
                status = $1, cpu_usage = $2, memory_usage = $3,
                gpu_usage = $4, gpu_memory_usage = $5,
                last_heartbeat = NOW(), updated_at = NOW()
            WHERE worker_id = $6
            "#,
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

    pub async fn mark_offline_stale(&self) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE worker_nodes SET status = 'OFFLINE', updated_at = NOW()
             WHERE status != 'OFFLINE'
             AND last_heartbeat < NOW() - INTERVAL '30 seconds'",
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
        let p = PgPoolOptions::new().max_connections(1).connect(&url).await.ok()?;
        hivemind_database::postgres::run_migrations(&p).await.ok()?;
        Some(p)
    }

    #[tokio::test]
    async fn test_upsert_worker() {
        let p = pool().await;
        if p.is_none() { return; }
        let repo = WorkerRepository::new(p.unwrap());

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

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'test-upsert-1'")
            .execute(&repo.pool)
            .await
            .ok();
    }
}
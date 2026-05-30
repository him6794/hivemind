use anyhow::Result;
use hivemind_models::{Task, TaskStatus};
use sqlx::PgPool;

pub struct TaskRepository {
    pub pool: PgPool,
}

impl TaskRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, task: &Task) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "INSERT INTO tasks (task_id, owner, status, status_message, torrent_source, expected_btih,
             req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
             host_count, max_cpt, max_retries, deadline,
             deterministic, side_effects, priority, created_at, last_update)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,NOW(),NOW()) RETURNING *",
        )
        .bind(&task.task_id).bind(&task.owner)
        .bind(task.status.as_str()).bind(&task.status_message)
        .bind(&task.torrent_source).bind(&task.expected_btih)
        .bind(task.req_cpu_score).bind(task.req_gpu_score)
        .bind(task.req_memory_gb).bind(task.req_gpu_memory_gb)
        .bind(task.req_storage_gb)
        .bind(task.host_count).bind(task.max_cpt).bind(task.max_retries)
        .bind(task.deadline).bind(task.deterministic).bind(task.side_effects).bind(task.priority)
        .fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn find_by_task_id(&self, task_id: &str) -> Result<Option<Task>> {
        sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE task_id = $1")
            .bind(task_id).fetch_optional(&self.pool).await.map_err(Into::into)
    }

    pub async fn find_by_owner(&self, owner: &str) -> Result<Vec<Task>> {
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE owner = $1 ORDER BY created_at DESC LIMIT 100"
        ).bind(owner).fetch_all(&self.pool).await.map_err(Into::into)
    }

    pub async fn find_pending(&self) -> Result<Vec<Task>> {
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE status IN ('PENDING', 'QUEUED') ORDER BY priority DESC, created_at ASC LIMIT 100"
        ).fetch_all(&self.pool).await.map_err(Into::into)
    }

    pub async fn update_status(&self, task_id: &str, status: TaskStatus, message: Option<&str>) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = $1, status_message = $2, last_update = NOW() WHERE task_id = $3 RETURNING *"
        ).bind(status.as_str()).bind(message).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn assign_to_worker(&self, task_id: &str, worker_id: &str, worker_ip: &str) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET worker_id = $1, worker_ip = $2, status = 'ASSIGNED', last_update = NOW() WHERE task_id = $3 RETURNING *"
        ).bind(worker_id).bind(worker_ip).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn complete(&self, task_id: &str, result_torrent: Option<&str>, output: Option<&str>) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = 'COMPLETED', result_torrent = $1, output = $2, last_update = NOW(), completed_at = NOW() WHERE task_id = $3 RETURNING *"
        ).bind(result_torrent).bind(output).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn fail(&self, task_id: &str, reason: &str) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = 'FAILED', status_message = $1, last_update = NOW(), completed_at = NOW() WHERE task_id = $2 RETURNING *"
        ).bind(reason).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn cancel(&self, task_id: &str) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = 'CANCELLED', last_update = NOW(), completed_at = NOW() WHERE task_id = $1 RETURNING *"
        ).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn mark_stale_running(&self) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE tasks SET status = 'TIMED_OUT', status_message = 'Worker heartbeat lost', completed_at = NOW()
             WHERE status = 'RUNNING' AND last_update < NOW() - INTERVAL '120 seconds'",
        ).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    pub async fn find_stale_dispatched(&self, timeout_secs: u64) -> Result<Vec<Task>> {
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE status = 'ASSIGNED' AND last_update < NOW() - make_interval(secs => $1::double precision) ORDER BY priority DESC, created_at ASC"
        ).bind(timeout_secs as f64).fetch_all(&self.pool).await.map_err(Into::into)
    }

    pub async fn reset_to_pending(&self, task_id: &str) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = 'PENDING', status_message = 'Redispatched', worker_id = NULL, worker_ip = NULL, retry_count = retry_count + 1, last_update = NOW() WHERE task_id = $1 RETURNING *"
        ).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn update_resource_usage(&self, task_id: &str, cpu: f64, memory: f64, gpu: f64, gpu_mem: f64) -> Result<()> {
        sqlx::query("UPDATE tasks SET cpu_usage = $1, memory_usage = $2, gpu_usage = $3, gpu_memory_usage = $4, last_update = NOW() WHERE task_id = $5")
            .bind(cpu).bind(memory).bind(gpu).bind(gpu_mem).bind(task_id).execute(&self.pool).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use sqlx::postgres::PgPoolOptions;

    async fn pool() -> Option<PgPool> {
        let url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        PgPoolOptions::new().max_connections(1).connect(&url).await.ok()
    }

    #[tokio::test]
    async fn test_create_and_find_task() {
        let p = match pool().await { Some(p) => p, None => return };
        let repo = TaskRepository::new(p);

        let task = Task {
            id: uuid::Uuid::new_v4(), task_id: "test-task-create-1".into(),
            owner: "testuser".into(), worker_id: None, worker_ip: None,
            status: TaskStatus::Pending, status_message: Some("test task".into()),
            output: None, result_torrent: None,
            torrent_source: Some("fake-btih".into()), expected_btih: None,
            cpu_usage: 0.0, memory_usage: 0.0, gpu_usage: 0.0, gpu_memory_usage: 0.0,
            req_cpu_score: 100, req_gpu_score: 0, req_memory_gb: 8, req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1, max_cpt: 1000,
            billing_settled: false, billed_amount: 0,
            retry_count: 0, max_retries: 3, deadline: None,
            deterministic: false, side_effects: false, priority: 0,
            cpu_time_ms: 0, wall_time_ms: 0, peak_memory_mb: 0,
            download_bytes: 0, cache_hits: 0,
            created_at: Utc::now(), last_update: Utc::now(), completed_at: None,
        };

        let created = repo.create(&task).await.unwrap();
        assert_eq!(created.task_id, "test-task-create-1");
        assert_eq!(created.status, TaskStatus::Pending);
        assert_eq!(created.req_storage_gb, 10);

        let found = repo.find_by_task_id("test-task-create-1").await.unwrap();
        assert!(found.is_some());

        sqlx::query("DELETE FROM tasks WHERE task_id = 'test-task-create-1'")
            .execute(&repo.pool).await.ok();
    }
}

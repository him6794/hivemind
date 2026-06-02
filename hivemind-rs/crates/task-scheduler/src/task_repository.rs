use anyhow::Result;
use hivemind_models::{Task, TaskStatus};
use sqlx::PgPool;

pub struct TaskRepository {
    pub pool: PgPool,
}

const PLATFORM_FEE_BPS: i64 = 1000; // 10%
const MIN_WORKER_REPUTATION_SCORE: i32 = 20;

impl TaskRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

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
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_by_owner(&self, owner: &str) -> Result<Vec<Task>> {
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE owner = $1 ORDER BY created_at DESC LIMIT 100",
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_pending(&self) -> Result<Vec<Task>> {
        sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE status IN ('PENDING', 'QUEUED') ORDER BY priority DESC, created_at ASC LIMIT 100"
        ).fetch_all(&self.pool).await.map_err(Into::into)
    }

    pub async fn update_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        message: Option<&str>,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = $1, status_message = $2, last_update = NOW() WHERE task_id = $3 RETURNING *"
        ).bind(status.as_str()).bind(message).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn assign_to_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        worker_ip: &str,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET worker_id = $1, worker_ip = $2, status = 'ASSIGNED', last_update = NOW()
             WHERE task_id = $3 AND status IN ('PENDING', 'QUEUED')
             RETURNING *",
        )
        .bind(worker_id)
        .bind(worker_ip)
        .bind(task_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn claim_pending_for_worker(
        &self,
        worker_id: &str,
        worker_ip: &str,
        limit: i64,
    ) -> Result<Vec<Task>> {
        let trust = sqlx::query_as::<_, (i32, bool)>(
            "SELECT score, banned FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((score, banned)) = trust {
            if banned || score < MIN_WORKER_REPUTATION_SCORE {
                tracing::warn!(
                    "Worker {} blocked from claiming tasks (banned={}, score={})",
                    worker_id,
                    banned,
                    score
                );
                return Ok(vec![]);
            }
        }

        let limit = limit.max(1);
        sqlx::query_as::<_, Task>(
            "WITH picked AS (
                SELECT id
                FROM tasks
                WHERE status IN ('PENDING', 'QUEUED')
                ORDER BY priority DESC, created_at ASC
                FOR UPDATE SKIP LOCKED
                LIMIT $3
             )
             UPDATE tasks t
             SET worker_id = $1, worker_ip = $2, status = 'ASSIGNED', last_update = NOW()
             FROM picked
             WHERE t.id = picked.id
             RETURNING t.*",
        )
        .bind(worker_id)
        .bind(worker_ip)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn complete(
        &self,
        task_id: &str,
        result_torrent: Option<&str>,
        output: Option<&str>,
    ) -> Result<Task> {
        self.complete_guarded(task_id, None, result_torrent, output)
            .await
    }

    pub async fn complete_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        result_torrent: Option<&str>,
        output: Option<&str>,
    ) -> Result<Task> {
        self.complete_guarded(task_id, Some(worker_id), result_torrent, output)
            .await
    }

    async fn complete_guarded(
        &self,
        task_id: &str,
        worker_id: Option<&str>,
        result_torrent: Option<&str>,
        output: Option<&str>,
    ) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let completed = if let Some(worker_id) = worker_id {
            sqlx::query_as::<_, Task>(
                "UPDATE tasks
                 SET status = 'COMPLETED', result_torrent = $1, output = $2, last_update = NOW(), completed_at = NOW()
                 WHERE task_id = $3 AND worker_id = $4
                 RETURNING *",
            )
            .bind(result_torrent)
            .bind(output)
            .bind(task_id)
            .bind(worker_id)
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query_as::<_, Task>(
                "UPDATE tasks
                 SET status = 'COMPLETED', result_torrent = $1, output = $2, last_update = NOW(), completed_at = NOW()
                 WHERE task_id = $3
                 RETURNING *",
            )
            .bind(result_torrent)
            .bind(output)
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?
        };

        if let Some(worker_id) = completed.worker_id.as_deref() {
            increment_worker_success(&mut tx, worker_id).await?;
            insert_task_attestation(&mut tx, task_id, worker_id, "accepted", 100, "primary execution")
                .await?;
        }

        if completed.max_cpt <= 0 || completed.billing_settled {
            tx.commit().await?;
            return Ok(completed);
        }

        let charged = sqlx::query(
            "UPDATE users SET balance = balance - $1, updated_at = NOW()
             WHERE username = $2 AND balance >= $1",
        )
        .bind(completed.max_cpt)
        .bind(&completed.owner)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;

        if charged {
            let platform_fee_cpt = (completed.max_cpt * PLATFORM_FEE_BPS) / 10_000;
            let provider_credit_cpt = (completed.max_cpt - platform_fee_cpt).max(0);
            let provider_user: Option<String> = match completed.worker_id.as_deref() {
                Some(worker_id) => {
                    sqlx::query_scalar("SELECT username FROM worker_nodes WHERE worker_id = $1")
                        .bind(worker_id)
                        .fetch_optional(&mut *tx)
                        .await?
                }
                None => None,
            };

            insert_ledger_entry(
                &mut tx,
                task_id,
                &completed.owner,
                completed.worker_id.as_deref(),
                provider_user.as_deref(),
                "payer_debit",
                completed.max_cpt,
            )
            .await?;
            insert_ledger_entry(
                &mut tx,
                task_id,
                &completed.owner,
                completed.worker_id.as_deref(),
                provider_user.as_deref(),
                "provider_credit",
                provider_credit_cpt,
            )
            .await?;
            insert_ledger_entry(
                &mut tx,
                task_id,
                &completed.owner,
                completed.worker_id.as_deref(),
                provider_user.as_deref(),
                "platform_fee",
                platform_fee_cpt,
            )
            .await?;

            let settled = sqlx::query_as::<_, Task>(
                "UPDATE tasks SET billing_settled = true, billed_amount = $1, last_update = NOW()
                 WHERE task_id = $2 RETURNING *",
            )
            .bind(completed.max_cpt)
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(settled)
        } else {
            tracing::warn!(
                "Task {} completed but billing is pending: owner {} has insufficient balance for {}",
                task_id,
                completed.owner,
                completed.max_cpt
            );
            tx.commit().await?;
            Ok(completed)
        }
    }

    pub async fn fail(&self, task_id: &str, reason: &str) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks SET status = 'FAILED', status_message = $1, last_update = NOW(), completed_at = NOW() WHERE task_id = $2 RETURNING *"
        ).bind(reason).bind(task_id).fetch_one(&self.pool).await.map_err(Into::into)
    }

    pub async fn fail_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        reason: &str,
    ) -> Result<Task> {
        let failed = sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'FAILED', status_message = $1, last_update = NOW(), completed_at = NOW()
             WHERE task_id = $2 AND worker_id = $3 AND status IN ('ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(reason)
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&self.pool)
        .await?;

        increment_worker_failure(&self.pool, worker_id).await?;
        insert_task_attestation_pool(
            &self.pool,
            task_id,
            worker_id,
            "rejected",
            100,
            reason,
        )
        .await?;
        Ok(failed)
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

    pub async fn reset_to_pending_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'PENDING', status_message = 'Redispatched', worker_id = NULL, worker_ip = NULL,
                 retry_count = retry_count + 1, last_update = NOW()
             WHERE task_id = $1 AND worker_id = $2 AND status IN ('ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_resource_usage(
        &self,
        task_id: &str,
        cpu: f64,
        memory: f64,
        gpu: f64,
        gpu_mem: f64,
    ) -> Result<()> {
        sqlx::query("UPDATE tasks SET cpu_usage = $1, memory_usage = $2, gpu_usage = $3, gpu_memory_usage = $4, last_update = NOW() WHERE task_id = $5")
            .bind(cpu).bind(memory).bind(gpu).bind(gpu_mem).bind(task_id).execute(&self.pool).await?;
        Ok(())
    }
}

async fn insert_ledger_entry(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: &str,
    payer_user: &str,
    provider_worker_id: Option<&str>,
    provider_user: Option<&str>,
    kind: &str,
    amount_cpt: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO ledger_entries (
            task_id, payer_user, provider_worker_id, provider_user,
            kind, amount_cpt, currency, status, idempotency_key
         )
         VALUES ($1, $2, $3, $4, $5, $6, 'CPT', 'settled', $7)
         ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(task_id)
    .bind(payer_user)
    .bind(provider_worker_id)
    .bind(provider_user)
    .bind(kind)
    .bind(amount_cpt)
    .bind(format!("{task_id}:{kind}"))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn increment_worker_success(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    worker_id: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO worker_reputation (worker_id, successful_tasks, score, last_attested_at, updated_at)
         VALUES ($1, 1, 101, NOW(), NOW())
         ON CONFLICT (worker_id) DO UPDATE SET
            successful_tasks = worker_reputation.successful_tasks + 1,
            score = LEAST(1000, worker_reputation.score + 1),
            last_attested_at = NOW(),
            updated_at = NOW()",
    )
    .bind(worker_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn increment_worker_failure(pool: &PgPool, worker_id: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO worker_reputation (worker_id, failed_tasks, score, updated_at)
         VALUES ($1, 1, 95, NOW())
         ON CONFLICT (worker_id) DO UPDATE SET
            failed_tasks = worker_reputation.failed_tasks + 1,
            score = GREATEST(0, worker_reputation.score - 5),
            updated_at = NOW()",
    )
    .bind(worker_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_task_attestation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: &str,
    worker_id: &str,
    verdict: &str,
    confidence: i32,
    details: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO task_attestations (task_id, worker_id, verdict, confidence, details)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(task_id)
    .bind(worker_id)
    .bind(verdict)
    .bind(confidence)
    .bind(details)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_task_attestation_pool(
    pool: &PgPool,
    task_id: &str,
    worker_id: &str,
    verdict: &str,
    confidence: i32,
    details: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO task_attestations (task_id, worker_id, verdict, confidence, details)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(task_id)
    .bind(worker_id)
    .bind(verdict)
    .bind(confidence)
    .bind(details)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use sqlx::postgres::PgPoolOptions;

    async fn pool() -> Option<PgPool> {
        let url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .ok()
    }

    #[tokio::test]
    async fn test_create_and_find_task() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        let repo = TaskRepository::new(p);

        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: "test-task-create-1".into(),
            owner: "testuser".into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: Some("test task".into()),
            output: None,
            result_torrent: None,
            torrent_source: Some("fake-btih".into()),
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 100,
            req_gpu_score: 0,
            req_memory_gb: 8,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
            retry_count: 0,
            max_retries: 3,
            deadline: None,
            deterministic: false,
            side_effects: false,
            priority: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            download_bytes: 0,
            cache_hits: 0,
            created_at: Utc::now(),
            last_update: Utc::now(),
            completed_at: None,
        };

        let created = repo.create(&task).await.unwrap();
        assert_eq!(created.task_id, "test-task-create-1");
        assert_eq!(created.status, TaskStatus::Pending);
        assert_eq!(created.req_storage_gb, 10);

        let found = repo.find_by_task_id("test-task-create-1").await.unwrap();
        assert!(found.is_some());

        sqlx::query("DELETE FROM tasks WHERE task_id = 'test-task-create-1'")
            .execute(&repo.pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_complete_settles_billing_when_balance_is_sufficient() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("billing-ok-user-{unique}");
        let provider = format!("billing-provider-{unique}");
        let worker_id = format!("billing-worker-{unique}");
        let task_id = format!("billing-ok-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.max_cpt = 25;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.10")
            .await
            .unwrap();

        let completed = repo
            .complete(&task_id, Some("result-btih"), Some("done"))
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
        assert!(completed.billing_settled);
        assert_eq!(completed.billed_amount, 25);

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 75);

        let rows: Vec<(String, String, Option<String>, Option<String>, i64, String)> =
            sqlx::query_as(
                "SELECT kind, payer_user, provider_worker_id, provider_user, amount_cpt, status
             FROM ledger_entries WHERE task_id = $1 ORDER BY kind",
            )
            .bind(&task_id)
            .fetch_all(&repo.pool)
            .await
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "payer_debit".to_string(),
                    username.clone(),
                    Some(worker_id.clone()),
                    Some(provider.clone()),
                    25,
                    "settled".to_string(),
                ),
                (
                    "platform_fee".to_string(),
                    username.clone(),
                    Some(worker_id.clone()),
                    Some(provider.clone()),
                    2,
                    "settled".to_string(),
                ),
                (
                    "provider_credit".to_string(),
                    username.clone(),
                    Some(worker_id.clone()),
                    Some(provider.clone()),
                    23,
                    "settled".to_string(),
                ),
            ]
        );

        let reputation: (i64, i64, i32) = sqlx::query_as(
            "SELECT successful_tasks, failed_tasks, score FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(reputation, (1, 0, 101));

        let attestation_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations WHERE task_id = $1 AND worker_id = $2",
        )
        .bind(&task_id)
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(attestation_count, 1);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
    }

    #[tokio::test]
    async fn test_assign_to_worker_does_not_overwrite_existing_assignment() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("assign-owner-{unique}");
        let first_worker = format!("assign-worker-a-{unique}");
        let second_worker = format!("assign-worker-b-{unique}");
        let task_id = format!("assign-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &first_worker, "assign-provider-a").await;
        insert_worker(&repo.pool, &second_worker, "assign-provider-b").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();

        let assigned = repo
            .assign_to_worker(&task_id, &first_worker, "10.0.0.21")
            .await
            .unwrap();
        assert_eq!(assigned.worker_id.as_deref(), Some(first_worker.as_str()));

        let second = repo
            .assign_to_worker(&task_id, &second_worker, "10.0.0.22")
            .await;
        assert!(second.is_err(), "second assignment should not overwrite");

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.worker_id.as_deref(), Some(first_worker.as_str()));
        assert_eq!(stored.worker_ip.as_deref(), Some("10.0.0.21"));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&first_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&second_worker)
            .execute(&repo.pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_does_not_overlap_between_repositories() {
        let url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        let p = match PgPoolOptions::new().max_connections(5).connect(&url).await {
            Ok(pool) => pool,
            Err(_) => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo_a = TaskRepository::new(p.clone());
        let repo_b = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("claim-owner-{unique}");
        let worker_a = format!("claim-worker-a-{unique}");
        let worker_b = format!("claim-worker-b-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&p)
        .await
        .unwrap();
        insert_worker(&p, &worker_a, "claim-provider-a").await;
        insert_worker(&p, &worker_b, "claim-provider-b").await;

        let mut task_ids = Vec::new();
        for index in 0..4 {
            let task_id = format!("claim-task-{index}-{unique}");
            task_ids.push(task_id.clone());
            let task = make_task(&task_id, &username);
            repo_a.create(&task).await.unwrap();
        }

        let (claimed_a, claimed_b) = tokio::join!(
            repo_a.claim_pending_for_worker(&worker_a, "10.0.0.31", 3),
            repo_b.claim_pending_for_worker(&worker_b, "10.0.0.32", 3),
        );
        let claimed_a = claimed_a.unwrap();
        let claimed_b = claimed_b.unwrap();

        let claimed_a_ids: std::collections::HashSet<_> =
            claimed_a.iter().map(|task| task.task_id.as_str()).collect();
        let claimed_b_ids: std::collections::HashSet<_> =
            claimed_b.iter().map(|task| task.task_id.as_str()).collect();

        assert!(!claimed_a_ids.is_empty());
        assert!(!claimed_b_ids.is_empty());
        assert!(
            claimed_a_ids.is_disjoint(&claimed_b_ids),
            "claimed task sets must not overlap"
        );
        assert_eq!(claimed_a.len() + claimed_b.len(), 4);

        let assigned_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE task_id = ANY($1) AND status = 'ASSIGNED'",
        )
        .bind(&task_ids)
        .fetch_one(&p)
        .await
        .unwrap();
        assert_eq!(assigned_count, 4);

        for task_id in task_ids {
            sqlx::query("DELETE FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .execute(&p)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
            .bind(&worker_a)
            .bind(&worker_b)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&p)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_blocks_banned_worker() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("claim-ban-owner-{unique}");
        let worker_id = format!("claim-ban-worker-{unique}");
        let task_id = format!("claim-ban-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&p)
        .await
        .unwrap();
        insert_worker(&p, &worker_id, "claim-ban-provider").await;

        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 10, 0, 200, true)",
        )
        .bind(&worker_id)
        .execute(&p)
        .await
        .unwrap();

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();

        let claimed = repo
            .claim_pending_for_worker(&worker_id, "10.0.0.31", 5)
            .await
            .unwrap();
        assert!(claimed.is_empty());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&p)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_blocks_low_score_worker() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("claim-score-owner-{unique}");
        let worker_id = format!("claim-score-worker-{unique}");
        let task_id = format!("claim-score-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&p)
        .await
        .unwrap();
        insert_worker(&p, &worker_id, "claim-score-provider").await;

        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 0, 5, 10, false)",
        )
        .bind(&worker_id)
        .execute(&p)
        .await
        .unwrap();

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();

        let claimed = repo
            .claim_pending_for_worker(&worker_id, "10.0.0.32", 5)
            .await
            .unwrap();
        assert!(claimed.is_empty());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&p)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_complete_for_worker_rejects_stale_worker_after_redispatch() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("stale-complete-owner-{unique}");
        let stale_worker = format!("stale-complete-old-{unique}");
        let current_worker = format!("stale-complete-current-{unique}");
        let task_id = format!("stale-complete-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "stale-provider-old").await;
        insert_worker(&repo.pool, &current_worker, "stale-provider-current").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.41")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.42")
            .await
            .unwrap();

        let stale_complete = repo
            .complete_for_worker(
                &task_id,
                &stale_worker,
                Some("old-result"),
                Some("old output"),
            )
            .await;
        assert!(stale_complete.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));
        assert_eq!(stored.result_torrent, None);
        assert_eq!(stored.output, None);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_fail_for_worker_rejects_stale_worker_after_redispatch() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("stale-fail-owner-{unique}");
        let stale_worker = format!("stale-fail-old-{unique}");
        let current_worker = format!("stale-fail-current-{unique}");
        let task_id = format!("stale-fail-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "stale-fail-provider-old").await;
        insert_worker(&repo.pool, &current_worker, "stale-fail-provider-current").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.43")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.44")
            .await
            .unwrap();

        let stale_fail = repo
            .fail_for_worker(&task_id, &stale_worker, "old failure")
            .await;
        assert!(stale_fail.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));
        assert_ne!(stored.status_message.as_deref(), Some("old failure"));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_reset_to_pending_for_worker_rejects_stale_worker_after_redispatch() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("stale-reset-owner-{unique}");
        let stale_worker = format!("stale-reset-old-{unique}");
        let current_worker = format!("stale-reset-current-{unique}");
        let task_id = format!("stale-reset-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "stale-reset-provider-old").await;
        insert_worker(&repo.pool, &current_worker, "stale-reset-provider-current").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.45")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.46")
            .await
            .unwrap();

        let stale_reset = repo
            .reset_to_pending_for_worker(&task_id, &stale_worker)
            .await;
        assert!(stale_reset.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_complete_is_idempotent_for_settled_billing_and_ledger() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("billing-repeat-user-{unique}");
        let provider = format!("billing-repeat-provider-{unique}");
        let worker_id = format!("billing-repeat-worker-{unique}");
        let task_id = format!("billing-repeat-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.max_cpt = 25;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.11")
            .await
            .unwrap();

        repo.complete(&task_id, Some("result-btih"), Some("done"))
            .await
            .unwrap();
        let completed_again = repo
            .complete(&task_id, Some("result-btih-2"), Some("done again"))
            .await
            .unwrap();

        assert_eq!(completed_again.status, TaskStatus::Completed);
        assert!(completed_again.billing_settled);
        assert_eq!(completed_again.billed_amount, 25);

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 75);

        let ledger_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ledger_entries WHERE task_id = $1")
                .bind(&task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        assert_eq!(ledger_count, 3);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
    }

    #[tokio::test]
    async fn test_complete_does_not_fail_task_when_billing_balance_is_insufficient() {
        let p = match pool().await {
            Some(p) => p,
            None => return,
        };
        hivemind_database::postgres::run_migrations(&p).await.ok();
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("billing-zero-user-{unique}");
        let provider = format!("billing-zero-provider-{unique}");
        let worker_id = format!("billing-zero-worker-{unique}");
        let task_id = format!("billing-zero-task-{unique}");

        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 0)")
            .bind(&username)
            .execute(&repo.pool)
            .await
            .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.max_cpt = 25;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.12")
            .await
            .unwrap();

        let completed = repo
            .complete(&task_id, Some("result-btih"), Some("done"))
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
        assert!(!completed.billing_settled);
        assert_eq!(completed.billed_amount, 0);
        assert_ne!(
            completed.status_message.as_deref(),
            Some("insufficient balance")
        );

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 0);

        let ledger_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ledger_entries WHERE task_id = $1")
                .bind(&task_id)
                .fetch_one(&repo.pool)
                .await
                .unwrap();
        assert_eq!(ledger_count, 0);

        let failure_rep: (i64, i64, i32) = sqlx::query_as(
            "SELECT successful_tasks, failed_tasks, score FROM worker_reputation WHERE worker_id = $1",
        )
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap_or((0, 0, 100));
        assert_eq!(failure_rep, (1, 0, 101));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
    }

    async fn insert_worker(pool: &PgPool, worker_id: &str, username: &str) {
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '10.0.0.2', 4, 16)",
        )
        .bind(worker_id)
        .bind(username)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn cleanup_task_case(
        pool: &PgPool,
        task_id: &str,
        username: &str,
        worker_id: Option<&str>,
    ) {
        sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
            .bind(task_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM task_attestations WHERE task_id = $1")
            .bind(task_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .execute(pool)
            .await
            .ok();
        if let Some(worker_id) = worker_id {
            sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
                .bind(worker_id)
                .execute(pool)
                .await
                .ok();
            sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
                .bind(worker_id)
                .execute(pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(username)
            .execute(pool)
            .await
            .ok();
    }

    fn make_task(task_id: &str, owner: &str) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            task_id: task_id.into(),
            owner: owner.into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: Some("test task".into()),
            output: None,
            result_torrent: None,
            torrent_source: Some("fake-btih".into()),
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 100,
            req_gpu_score: 0,
            req_memory_gb: 8,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
            retry_count: 0,
            max_retries: 3,
            deadline: None,
            deterministic: false,
            side_effects: false,
            priority: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            download_bytes: 0,
            cache_hits: 0,
            created_at: Utc::now(),
            last_update: Utc::now(),
            completed_at: None,
        }
    }
}

use anyhow::Result;
use hivemind_models::{Task, TaskStatus, WorkerNode};
use sha1::{Digest, Sha1};
use sqlx::PgPool;

use crate::BatchTaskReport;

pub struct TaskRepository {
    pub pool: PgPool,
}

const PLATFORM_FEE_BPS: i64 = 1000; // 10%
const MANAGED_BASE_INVOCATION_CPT: i64 = 1;
const MANAGED_OP_BLOCK_CPT: i64 = 1;
const MANAGED_OUTPUT_KIB_CPT: i64 = 1;
pub(crate) const MIN_WORKER_REPUTATION_SCORE: i32 = 20;

struct ManagedCompletionReceipt<'a> {
    executed_ops: i64,
    output_bytes: i64,
    receipt_json: &'a str,
}

impl TaskRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn is_worker_trusted(score: i32, banned: bool) -> bool {
        !banned && score >= MIN_WORKER_REPUTATION_SCORE
    }

    pub(crate) async fn trusted_workers(&self, workers: &[WorkerNode]) -> Result<Vec<WorkerNode>> {
        if workers.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<String> = workers.iter().map(|w| w.worker_id.clone()).collect();
        let rows: Vec<(String, i32, bool)> = sqlx::query_as(
            "SELECT worker_id, score, banned
             FROM worker_reputation
             WHERE worker_id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;

        let trust_map: std::collections::HashMap<String, (i32, bool)> = rows
            .into_iter()
            .map(|(worker_id, score, banned)| (worker_id, (score, banned)))
            .collect();

        Ok(workers
            .iter()
            .filter(|worker| match trust_map.get(&worker.worker_id) {
                Some((score, banned)) => Self::is_worker_trusted(*score, *banned),
                None => false,
            })
            .cloned()
            .collect())
    }

    pub async fn create(&self, task: &Task) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "INSERT INTO tasks (task_id, owner, status, status_message, torrent_source, runtime, task_source, expected_btih,
             req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
             host_count, max_cpt, max_retries, deadline,
             deterministic, side_effects, priority, created_at, last_update)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,NOW(),NOW()) RETURNING *",
        )
        .bind(&task.task_id).bind(&task.owner)
        .bind(task.status.as_str()).bind(&task.status_message)
        .bind(&task.torrent_source).bind(&task.runtime).bind(&task.task_source).bind(&task.expected_btih)
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

    pub async fn refresh_worker_endpoint(
        &self,
        task_id: &str,
        worker_id: &str,
        worker_ip: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tasks
             SET worker_ip = $1, last_update = NOW()
             WHERE task_id = $2 AND worker_id = $3 AND status IN ('ASSIGNED', 'RUNNING')",
        )
        .bind(worker_ip)
        .bind(task_id)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
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
        match trust {
            Some((score, banned)) if Self::is_worker_trusted(score, banned) => {}
            Some((score, banned)) => {
                tracing::warn!(
                    "Worker {} blocked from claiming tasks (banned={}, score={})",
                    worker_id,
                    banned,
                    score
                );
                return Ok(vec![]);
            }
            None => {
                tracing::warn!(
                    "Worker {} blocked from claiming tasks because reputation row is missing",
                    worker_id
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
        self.complete_guarded(task_id, None, result_torrent, output, None)
            .await
    }

    pub async fn complete_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        result_torrent: Option<&str>,
        output: Option<&str>,
    ) -> Result<Task> {
        self.complete_guarded(task_id, Some(worker_id), result_torrent, output, None)
            .await
    }

    pub async fn complete_for_worker_with_managed_receipt(
        &self,
        task_id: &str,
        worker_id: &str,
        output: Option<&str>,
        executed_ops: i64,
        output_bytes: i64,
        receipt_json: &str,
    ) -> Result<Task> {
        self.complete_guarded(
            task_id,
            Some(worker_id),
            None,
            output,
            Some(ManagedCompletionReceipt {
                executed_ops,
                output_bytes,
                receipt_json,
            }),
        )
        .await
    }

    pub async fn complete_result_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        result_torrent: &str,
    ) -> Result<Task> {
        self.complete_for_worker(task_id, worker_id, Some(result_torrent), None)
            .await
    }

    pub async fn record_output_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        output: &str,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET output = $1, last_update = NOW()
             WHERE task_id = $2 AND worker_id = $3
               AND (status IN ('ASSIGNED', 'RUNNING') OR (status = 'COMPLETED' AND output IS NULL))
             RETURNING *",
        )
        .bind(output)
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn complete_guarded(
        &self,
        task_id: &str,
        worker_id: Option<&str>,
        result_torrent: Option<&str>,
        output: Option<&str>,
        managed_receipt: Option<ManagedCompletionReceipt<'_>>,
    ) -> Result<Task> {
        let mut tx = self.pool.begin().await?;
        let deterministic: bool =
            sqlx::query_scalar("SELECT deterministic FROM tasks WHERE task_id = $1")
                .bind(task_id)
                .fetch_one(&mut *tx)
                .await?;
        if deterministic
            && result_torrent
                .map(|value| value.trim().is_empty())
                .unwrap_or(true)
        {
            anyhow::bail!("deterministic task completion requires a result reference");
        }

        let mut completed = if let Some(worker_id) = worker_id {
            sqlx::query_as::<_, Task>(
                "UPDATE tasks
                 SET status = 'COMPLETED', result_torrent = $1, output = COALESCE($2, output), last_update = NOW(), completed_at = NOW()
                 WHERE task_id = $3 AND worker_id = $4 AND status IN ('ASSIGNED', 'RUNNING')
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
                 SET status = 'COMPLETED', result_torrent = $1, output = COALESCE($2, output), last_update = NOW(), completed_at = NOW()
                 WHERE task_id = $3
                 RETURNING *",
            )
            .bind(result_torrent)
            .bind(output)
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?
        };

        if let Some(receipt) = managed_receipt {
            completed = sqlx::query_as::<_, Task>(
                "UPDATE tasks
                 SET managed_executed_ops = $1,
                     managed_output_bytes = $2,
                     managed_receipt_json = $3,
                     last_update = NOW()
                 WHERE task_id = $4
                 RETURNING *",
            )
            .bind(receipt.executed_ops.max(0))
            .bind(receipt.output_bytes.max(0))
            .bind(receipt.receipt_json)
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
        }

        if let Some(worker_id) = completed.worker_id.as_deref() {
            increment_worker_success(&mut tx, worker_id).await?;
            insert_task_attestation(
                &mut tx,
                task_id,
                worker_id,
                "accepted",
                100,
                "primary execution",
            )
            .await?;
            if completed.deterministic {
                let proof =
                    checksum_proof_details(completed.result_torrent.as_deref().unwrap_or(""));
                insert_task_attestation(&mut tx, task_id, worker_id, "checksum_proof", 80, &proof)
                    .await?;
            }
        }

        if completed.max_cpt <= 0 || completed.billing_settled {
            tx.commit().await?;
            return Ok(completed);
        }

        let billable_cpt = billable_amount_cpt(&completed);

        let charged = sqlx::query(
            "UPDATE users SET balance = balance - $1, updated_at = NOW()
             WHERE username = $2 AND balance >= $1",
        )
        .bind(billable_cpt)
        .bind(&completed.owner)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;

        if charged {
            let platform_fee_cpt = (billable_cpt * PLATFORM_FEE_BPS) / 10_000;
            let provider_credit_cpt = (billable_cpt - platform_fee_cpt).max(0);
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
                billable_cpt,
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
            .bind(billable_cpt)
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
                billable_cpt
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
        insert_task_attestation_pool(&self.pool, task_id, worker_id, "rejected", 100, reason)
            .await?;
        Ok(failed)
    }

    pub async fn cancel(&self, task_id: &str) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET status = 'CANCELLED', last_update = NOW(), completed_at = NOW()
             WHERE task_id = $1 AND status IN ('PENDING', 'QUEUED', 'ASSIGNED', 'RUNNING')
             RETURNING *",
        )
        .bind(task_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
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

    pub async fn update_resource_usage_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        cpu: f64,
        memory: f64,
        gpu: f64,
        gpu_mem: f64,
    ) -> Result<()> {
        let result = sqlx::query(
            "UPDATE tasks
             SET cpu_usage = $1, memory_usage = $2, gpu_usage = $3, gpu_memory_usage = $4, last_update = NOW()
             WHERE task_id = $5 AND worker_id = $6
               AND (
                   status IN ('ASSIGNED', 'RUNNING')
                   OR (
                       status = 'COMPLETED'
                       AND cpu_usage = 0
                       AND memory_usage = 0
                       AND gpu_usage = 0
                       AND gpu_memory_usage = 0
                   )
               )",
        )
        .bind(cpu)
        .bind(memory)
        .bind(gpu)
        .bind(gpu_mem)
        .bind(task_id)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("task is not assigned to this worker or is no longer active");
        }
        Ok(())
    }

    pub async fn record_batch_report_for_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        report: BatchTaskReport<'_>,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>(
            "UPDATE tasks
             SET output = COALESCE($1, output),
                 cpu_time_ms = $2,
                 wall_time_ms = $3,
                 peak_memory_mb = $4,
                 download_bytes = $5,
                 cache_hits = $6,
                 last_update = NOW()
             WHERE task_id = $7 AND worker_id = $8 AND status IN ('COMPLETED', 'FAILED')
             RETURNING *",
        )
        .bind(report.output)
        .bind(report.cpu_time_ms)
        .bind(report.wall_time_ms)
        .bind(report.peak_memory_mb)
        .bind(report.download_bytes)
        .bind(report.cache_hits)
        .bind(task_id)
        .bind(worker_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
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

fn ceil_div_i64(value: i64, divisor: i64) -> i64 {
    if value <= 0 {
        0
    } else {
        ((value - 1) / divisor) + 1
    }
}

fn managed_receipt_amount_cpt(task: &Task) -> i64 {
    MANAGED_BASE_INVOCATION_CPT
        + ceil_div_i64(task.managed_executed_ops, 1_000) * MANAGED_OP_BLOCK_CPT
        + ceil_div_i64(task.managed_output_bytes, 1_024) * MANAGED_OUTPUT_KIB_CPT
}

fn billable_amount_cpt(task: &Task) -> i64 {
    if task.runtime.as_deref() == Some("managed-function-v0") && task.managed_receipt_json.is_some()
    {
        managed_receipt_amount_cpt(task).min(task.max_cpt).max(0)
    } else {
        task.max_cpt.max(0)
    }
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

fn checksum_proof_details(result_ref: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(result_ref.as_bytes());
    format!(
        "result_ref_sha1={:x};result_ref={}",
        hasher.finalize(),
        result_ref
    )
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
    use hivemind_database::postgres::IsolatedTestPool;

    // Ledger row shape: (kind, payer_user, provider_worker_id, provider_user, amount_cpt, status)
    type LedgerRow = (String, String, Option<String>, Option<String>, i64, String);

    async fn pool(test_name: &str) -> Option<(PgPool, IsolatedTestPool)> {
        let fixture = hivemind_database::postgres::create_isolated_test_pool(test_name)
            .await
            .ok()?;
        if hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .is_err()
        {
            fixture.cleanup().await.ok();
            return None;
        }
        Some((fixture.pool.clone(), fixture))
    }

    #[tokio::test]
    async fn task_repository_pool_uses_isolated_schema() {
        let (p, fixture) = match pool("task_repository_pool_uses_isolated_schema").await {
            Some(parts) => parts,
            None => return,
        };

        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&p)
            .await
            .unwrap();

        assert!(
            schema.starts_with("hm_test_"),
            "task repository DB tests must use an isolated schema, got {schema}"
        );
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_create_and_find_task() {
        let (p, fixture) = match pool("task_repository_create_and_find_task").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);

        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: "example-task-create-1".into(),
            owner: "example-user".into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: Some("test task".into()),
            output: None,
            result_torrent: None,
            torrent_source: Some("example-btih".into()),
            runtime: None,
            task_source: None,
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
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
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
        assert_eq!(created.task_id, "example-task-create-1");
        assert_eq!(created.status, TaskStatus::Pending);
        assert_eq!(created.req_storage_gb, 10);

        let found = repo.find_by_task_id("example-task-create-1").await.unwrap();
        assert!(found.is_some());

        sqlx::query("DELETE FROM tasks WHERE task_id = 'example-task-create-1'")
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_settles_billing_when_balance_is_sufficient() {
        let (p, fixture) = match pool("task_repository_complete_settles_billing").await {
            Some(parts) => parts,
            None => return,
        };
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

        let rows: Vec<LedgerRow> = sqlx::query_as(
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_assign_to_worker_does_not_overwrite_existing_assignment() {
        let (p, fixture) = match pool("task_repository_assign_no_overwrite").await {
            Some(parts) => parts,
            None => return,
        };
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_does_not_overlap_between_repositories() {
        let fixture = match hivemind_database::postgres::create_isolated_test_pool(
            "task_repository_claim_overlap",
        )
        .await
        {
            Ok(fixture) => fixture,
            Err(_) => return,
        };
        let p = fixture.pool.clone();
        hivemind_database::postgres::run_migrations(&p).await.ok();
        sqlx::query("DELETE FROM tasks WHERE task_id LIKE 'claim-task-%'")
            .execute(&p)
            .await
            .ok();
        sqlx::query(
            "DELETE FROM worker_reputation
             WHERE worker_id LIKE 'claim-worker-a-%' OR worker_id LIKE 'claim-worker-b-%'",
        )
        .execute(&p)
        .await
        .ok();
        sqlx::query(
            "DELETE FROM worker_nodes
             WHERE worker_id LIKE 'claim-worker-a-%' OR worker_id LIKE 'claim-worker-b-%'",
        )
        .execute(&p)
        .await
        .ok();
        sqlx::query("DELETE FROM users WHERE username LIKE 'claim-owner-%'")
            .execute(&p)
            .await
            .ok();
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
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 10, 0, 100, false), ($2, 10, 0, 100, false)",
        )
        .bind(&worker_a)
        .bind(&worker_b)
        .execute(&p)
        .await
        .unwrap();

        let mut task_ids = Vec::new();
        for index in 0..4 {
            let task_id = format!("claim-task-{index}-{unique}");
            task_ids.push(task_id.clone());
            let mut task = make_task(&task_id, &username);
            task.priority = 10_000 - index;
            repo_a.create(&task).await.unwrap();
        }

        let (claimed_a, claimed_b) = tokio::join!(
            repo_a.claim_pending_for_worker(&worker_a, "10.0.0.31", 2),
            repo_b.claim_pending_for_worker(&worker_b, "10.0.0.32", 2),
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
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_blocks_banned_worker() {
        let (p, fixture) = match pool("task_repository_claim_blocks_banned_worker").await {
            Some(parts) => parts,
            None => return,
        };
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_blocks_low_score_worker() {
        let (p, fixture) = match pool("task_repository_claim_blocks_low_score_worker").await {
            Some(parts) => parts,
            None => return,
        };
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_trusted_workers_excludes_missing_reputation_rows() {
        let (p, fixture) = match pool("task_repository_trusted_workers_missing_reputation").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let trusted_worker = format!("trust-present-worker-{unique}");
        let missing_worker = format!("trust-missing-worker-{unique}");

        insert_worker(&p, &trusted_worker, "trust-present-provider").await;
        insert_worker(&p, &missing_worker, "trust-missing-provider").await;
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 10, 0, 100, false)",
        )
        .bind(&trusted_worker)
        .execute(&p)
        .await
        .unwrap();

        let workers = vec![
            make_worker_node(&trusted_worker, "10.0.0.41"),
            make_worker_node(&missing_worker, "10.0.0.42"),
        ];
        let trusted = repo.trusted_workers(&workers).await.unwrap();

        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].worker_id, trusted_worker);

        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
            .bind(&trusted_worker)
            .bind(&missing_worker)
            .execute(&p)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
            .bind(&trusted_worker)
            .bind(&missing_worker)
            .execute(&p)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_claim_pending_for_worker_blocks_missing_reputation_row() {
        let (p, fixture) = match pool("task_repository_claim_blocks_missing_reputation").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p.clone());
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("claim-missing-rep-owner-{unique}");
        let worker_id = format!("claim-missing-rep-worker-{unique}");
        let task_id = format!("claim-missing-rep-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&p)
        .await
        .unwrap();
        insert_worker(&p, &worker_id, "claim-missing-rep-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();

        let claimed = repo
            .claim_pending_for_worker(&worker_id, "10.0.0.43", 5)
            .await
            .unwrap();
        assert!(claimed.is_empty());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        assert!(stored.worker_id.is_none());

        cleanup_task_case(&p, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_complete_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_for_worker_does_not_overwrite_cancelled_task() {
        let (p, fixture) = match pool("task_repository_complete_does_not_overwrite_cancelled").await
        {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("cancel-complete-owner-{unique}");
        let worker_id = format!("cancel-complete-worker-{unique}");
        let task_id = format!("cancel-complete-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "cancel-complete-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.44")
            .await
            .unwrap();
        repo.cancel(&task_id).await.unwrap();

        let late_complete = repo
            .complete_for_worker(
                &task_id,
                &worker_id,
                Some("late-result"),
                Some("late output"),
            )
            .await;
        assert!(late_complete.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Cancelled);
        assert_eq!(stored.result_torrent, None);
        assert_eq!(stored.output, None);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_report_output_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_report_output_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("report-output-owner-{unique}");
        let stale_worker = format!("report-output-old-{unique}");
        let current_worker = format!("report-output-current-{unique}");
        let task_id = format!("report-output-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "report-output-provider-old").await;
        insert_worker(
            &repo.pool,
            &current_worker,
            "report-output-provider-current",
        )
        .await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.47")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.48")
            .await
            .unwrap();

        let stale_output = repo
            .record_output_for_worker(&task_id, &stale_worker, "old worker output")
            .await;
        assert!(stale_output.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));
        assert_eq!(stored.output, None);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_update_resource_usage_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_usage_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("report-usage-owner-{unique}");
        let stale_worker = format!("report-usage-old-{unique}");
        let current_worker = format!("report-usage-current-{unique}");
        let task_id = format!("report-usage-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &stale_worker, "report-usage-provider-old").await;
        insert_worker(&repo.pool, &current_worker, "report-usage-provider-current").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &stale_worker, "10.0.0.49")
            .await
            .unwrap();
        repo.reset_to_pending_for_worker(&task_id, &stale_worker)
            .await
            .unwrap();
        repo.assign_to_worker(&task_id, &current_worker, "10.0.0.50")
            .await
            .unwrap();

        let stale_usage = repo
            .update_resource_usage_for_worker(&task_id, &stale_worker, 11.0, 22.0, 33.0, 44.0)
            .await;
        assert!(stale_usage.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(current_worker.as_str()));
        assert_eq!(stored.cpu_usage, 0.0);
        assert_eq!(stored.memory_usage, 0.0);
        assert_eq!(stored.gpu_usage, 0.0);
        assert_eq!(stored.gpu_memory_usage, 0.0);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&stale_worker)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&current_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_result_for_worker_preserves_reported_output() {
        let (p, fixture) = match pool("task_repository_result_preserves_output").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("report-result-owner-{unique}");
        let worker_id = format!("report-result-worker-{unique}");
        let task_id = format!("report-result-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "report-result-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.51")
            .await
            .unwrap();
        repo.record_output_for_worker(&task_id, &worker_id, "stdout before result")
            .await
            .unwrap();

        let completed = repo
            .complete_result_for_worker(&task_id, &worker_id, "btih:reported-result")
            .await
            .unwrap();

        assert_eq!(completed.status, TaskStatus::Completed);
        assert_eq!(completed.output.as_deref(), Some("stdout before result"));
        assert_eq!(
            completed.result_torrent.as_deref(),
            Some("btih:reported-result")
        );

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_record_batch_report_for_worker_rejects_wrong_worker() {
        let (p, fixture) = match pool("task_repository_batch_report_rejects_wrong_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("batch-report-owner-{unique}");
        let worker_id = format!("batch-report-worker-{unique}");
        let wrong_worker = format!("batch-report-wrong-worker-{unique}");
        let task_id = format!("batch-report-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "batch-report-provider").await;
        insert_worker(&repo.pool, &wrong_worker, "batch-report-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.52")
            .await
            .unwrap();
        repo.complete_for_worker(&task_id, &worker_id, Some("result"), None)
            .await
            .unwrap();

        let wrong_report = repo
            .record_batch_report_for_worker(
                &task_id,
                &wrong_worker,
                BatchTaskReport {
                    output: Some("wrong worker log"),
                    cpu_time_ms: 10,
                    wall_time_ms: 20,
                    peak_memory_mb: 30,
                    download_bytes: 40,
                    cache_hits: 50,
                },
            )
            .await;
        assert!(wrong_report.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.worker_id.as_deref(), Some(worker_id.as_str()));
        assert_eq!(stored.output, None);
        assert_eq!(stored.cpu_time_ms, 0);
        assert_eq!(stored.cache_hits, 0);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&wrong_worker)
            .execute(&repo.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_cancel_does_not_overwrite_completed_task() {
        let (p, fixture) = match pool("task_repository_cancel_does_not_overwrite_completed").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("cancel-completed-owner-{unique}");
        let worker_id = format!("cancel-completed-worker-{unique}");
        let task_id = format!("cancel-completed-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "cancel-completed-provider").await;

        let task = make_task(&task_id, &username);
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.45")
            .await
            .unwrap();
        repo.complete_for_worker(&task_id, &worker_id, Some("result"), Some("output"))
            .await
            .unwrap();

        let late_cancel = repo.cancel(&task_id).await;
        assert!(late_cancel.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Completed);
        assert_eq!(stored.result_torrent.as_deref(), Some("result"));
        assert_eq!(stored.output.as_deref(), Some("output"));

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_fail_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_fail_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_reset_to_pending_for_worker_rejects_stale_worker_after_redispatch() {
        let (p, fixture) = match pool("task_repository_reset_rejects_stale_worker").await {
            Some(parts) => parts,
            None => return,
        };
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_is_idempotent_for_settled_billing_and_ledger() {
        let (p, fixture) = match pool("task_repository_complete_idempotent_billing").await {
            Some(parts) => parts,
            None => return,
        };
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_complete_does_not_fail_task_when_billing_balance_is_insufficient() {
        let (p, fixture) = match pool("task_repository_complete_insufficient_balance").await {
            Some(parts) => parts,
            None => return,
        };
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
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_managed_complete_settles_billing_from_receipt() {
        let (p, fixture) = match pool("task_repository_managed_receipt_billing").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("managed-billing-user-{unique}");
        let provider = format!("managed-billing-provider-{unique}");
        let worker_id = format!("managed-billing-worker-{unique}");
        let task_id = format!("managed-billing-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.runtime = Some("managed-function-v0".into());
        task.max_cpt = 25;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.15")
            .await
            .unwrap();

        let completed = repo
            .complete_for_worker_with_managed_receipt(
                &task_id,
                &worker_id,
                Some("7"),
                2_500,
                2_049,
                "{\"executed_ops\":2500,\"output_bytes\":2049}",
            )
            .await
            .unwrap();

        assert!(completed.billing_settled);
        assert_eq!(completed.billed_amount, 7);
        assert_eq!(completed.managed_executed_ops, 2_500);
        assert_eq!(completed.managed_output_bytes, 2_049);
        assert_eq!(
            completed.managed_receipt_json.as_deref(),
            Some("{\"executed_ops\":2500,\"output_bytes\":2049}")
        );

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 93);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_managed_receipt_billing_is_capped_by_max_cpt() {
        let (p, fixture) = match pool("task_repository_managed_receipt_billing_cap").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("managed-cap-user-{unique}");
        let provider = format!("managed-cap-provider-{unique}");
        let worker_id = format!("managed-cap-worker-{unique}");
        let task_id = format!("managed-cap-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, &provider).await;

        let mut task = make_task(&task_id, &username);
        task.runtime = Some("managed-function-v0".into());
        task.max_cpt = 5;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.16")
            .await
            .unwrap();

        let completed = repo
            .complete_for_worker_with_managed_receipt(
                &task_id,
                &worker_id,
                Some("large"),
                10_000,
                8_192,
                "{\"executed_ops\":10000,\"output_bytes\":8192}",
            )
            .await
            .unwrap();

        assert!(completed.billing_settled);
        assert_eq!(completed.billed_amount, 5);

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&repo.pool)
            .await
            .unwrap();
        assert_eq!(balance, 95);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_deterministic_complete_requires_result_reference() {
        let (p, fixture) = match pool("task_repository_deterministic_requires_result").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("verify-missing-owner-{unique}");
        let worker_id = format!("verify-missing-worker-{unique}");
        let task_id = format!("verify-missing-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "verify-provider").await;

        let mut task = make_task(&task_id, &username);
        task.deterministic = true;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.13")
            .await
            .unwrap();

        let result = repo
            .complete_for_worker(&task_id, &worker_id, None, Some("done"))
            .await;
        assert!(result.is_err());

        let stored = repo.find_by_task_id(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert!(stored.result_torrent.is_none());

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_deterministic_complete_records_checksum_proof() {
        let (p, fixture) = match pool("task_repository_deterministic_checksum_proof").await {
            Some(parts) => parts,
            None => return,
        };
        let repo = TaskRepository::new(p);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("verify-proof-owner-{unique}");
        let worker_id = format!("verify-proof-worker-{unique}");
        let task_id = format!("verify-proof-task-{unique}");

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&repo.pool)
        .await
        .unwrap();
        insert_worker(&repo.pool, &worker_id, "verify-provider").await;

        let mut task = make_task(&task_id, &username);
        task.deterministic = true;
        repo.create(&task).await.unwrap();
        repo.assign_to_worker(&task_id, &worker_id, "10.0.0.14")
            .await
            .unwrap();

        let completed = repo
            .complete_for_worker(
                &task_id,
                &worker_id,
                Some("sha1:result-reference"),
                Some("done"),
            )
            .await
            .unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);

        let proof_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attestations
             WHERE task_id = $1 AND worker_id = $2 AND verdict = 'checksum_proof'",
        )
        .bind(&task_id)
        .bind(&worker_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(proof_count, 1);

        cleanup_task_case(&repo.pool, &task_id, &username, Some(&worker_id)).await;
        fixture.cleanup().await.ok();
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

    fn make_worker_node(worker_id: &str, ip: &str) -> WorkerNode {
        WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: worker_id.into(),
            username: "test".into(),
            ip: ip.into(),
            virtual_ip: None,
            hostname: None,
            cpu_cores: 4,
            memory_gb: 16,
            cpu_score: 400,
            gpu_score: 0,
            gpu_memory_gb: 0,
            gpu_name: None,
            vram_mb: 0,
            storage_total_gb: 500,
            storage_available_gb: 200,
            provider_enabled: true,
            cpu_cores_limit: 0,
            memory_gb_limit: 0,
            gpu_memory_gb_limit: 0,
            storage_gb_limit: 0,
            min_cpt_per_hour: 0,
            location: "local".into(),
            status: hivemind_models::WorkerStatus::Idle,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: 16,
            queue_capacity: 4,
            last_heartbeat: Utc::now(),
            registered_at: Utc::now(),
            updated_at: Utc::now(),
        }
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
            torrent_source: Some("example-btih".into()),
            runtime: None,
            task_source: None,
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
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
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

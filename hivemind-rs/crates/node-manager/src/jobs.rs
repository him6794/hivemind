//! Server-side jobs: one hcode function over many inputs, run by Nodepool.
//!
//! A job is stored with its items. Nodepool submits an item as a task when the job's total cap leaves room for
//! it, follows the task to its end, and records the billed amount and output. The client only creates a job and
//! reads its progress, so a script can stop after creating it. The job keeps running on Nodepool.
//!
//! Spend is bounded by the job's cap. Each submitted item reserves its own cap until its task ends, and an item
//! is submitted only when the finished spend, the reserved caps, and its own cap fit under the job's cap.

#![allow(clippy::result_large_err, reason = "gRPC uses tonic::Status")]

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use hivemind_proto::job_service_server::JobService;
use hivemind_proto::{
    CancelJobRequest, CancelJobResponse, CreateJobRequest, CreateJobResponse, GetJobRequest,
    GetJobResponse, JobItemView, JobView, ListJobItemsRequest, ListJobItemsResponse,
    ListJobsRequest, ListJobsResponse, ResourceSpec as ProtoResourceSpec, UploadTaskRequest,
};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::grpc::{GrpcMasterNodeService, NodepoolState};

/// Most inputs one job may hold. Each input is one task.
pub const MAX_JOB_ITEMS: usize = 1000;
/// Largest total of the inputs of one job.
pub const JOB_INPUT_TOTAL_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Most jobs an account may have running or cancelling at once.
pub const JOB_MAX_ACTIVE_PER_OWNER: i64 = 10;
/// Longest name a job may carry.
pub const JOB_NAME_MAX_CHARS: usize = 128;
/// Items one tick submits for one job. The runner ticks every few seconds, so this bounds the submission rate.
pub const JOB_SUBMIT_PER_TICK: i64 = 10;
/// Active jobs one tick advances. The runner visits the jobs it has visited least recently first.
pub const JOB_SCAN_PER_TICK: i64 = 100;
/// Time between runner ticks.
pub const JOB_TICK_INTERVAL: Duration = Duration::from_secs(2);
/// Characters of an item's output that a listing carries. Four bytes per character at most keeps it at 16 KiB.
pub const JOB_OUTPUT_PREVIEW_CHARS: i32 = 4096;
/// Largest gRPC message the job service accepts or sends: a job's inputs travel in one request.
pub const JOB_RPC_MESSAGE_MAX_BYTES: usize = 128 * 1024 * 1024;

/// A request that passed validation, with the defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedJob {
    pub name: String,
    pub job_max_cpt: i64,
    pub replica_count: u16,
    pub host_count: i32,
}

/// Checks a create request. The error is the message the caller sees.
pub fn validate_new_job(
    req: &CreateJobRequest,
    default_replicas: u16,
    max_replicas: u16,
) -> Result<ValidatedJob, String> {
    let name = req.name.trim().to_string();
    if name.chars().count() > JOB_NAME_MAX_CHARS {
        return Err(format!(
            "job name must be at most {JOB_NAME_MAX_CHARS} characters"
        ));
    }
    let source = req.source.trim();
    if source.is_empty() {
        return Err("job source is required".into());
    }
    if req.source.len() > hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES {
        return Err(format!(
            "job source exceeds {} bytes",
            hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES
        ));
    }
    if req.inputs.is_empty() {
        return Err("a job needs at least one input".into());
    }
    if req.inputs.len() > MAX_JOB_ITEMS {
        return Err(format!("a job holds at most {MAX_JOB_ITEMS} inputs"));
    }
    let mut total = 0_usize;
    for (position, input) in req.inputs.iter().enumerate() {
        if input.trim().is_empty() {
            return Err(format!("input {} is empty", position + 1));
        }
        if serde_json::from_str::<serde_json::Value>(input).is_err() {
            return Err(format!("input {} is not valid JSON", position + 1));
        }
        if input.len() > hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES {
            return Err(format!(
                "input {} exceeds {} bytes",
                position + 1,
                hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES
            ));
        }
        total += input.len();
    }
    if total > JOB_INPUT_TOTAL_MAX_BYTES {
        return Err(format!(
            "a job's inputs total at most {JOB_INPUT_TOTAL_MAX_BYTES} bytes"
        ));
    }
    if req.per_item_max_cpt <= 0 {
        return Err("per_item_max_cpt must be positive".into());
    }
    let items = req.inputs.len() as i64;
    let job_max_cpt = if req.job_max_cpt == 0 {
        req.per_item_max_cpt
            .checked_mul(items)
            .ok_or_else(|| "the job's total cap is out of range".to_string())?
    } else {
        req.job_max_cpt
    };
    if job_max_cpt < req.per_item_max_cpt {
        return Err("job_max_cpt must cover at least one item".into());
    }
    let replica_count = match req.replica_count {
        0 => default_replicas,
        value => {
            let value =
                u16::try_from(value).map_err(|_| "replica_count is out of range".to_string())?;
            if !(2..=7).contains(&value) || value > max_replicas {
                return Err(format!(
                    "replica_count must be between 2 and {}",
                    max_replicas.min(7)
                ));
            }
            value
        }
    };
    for (label, value) in [
        ("cpu_score", i64::from(req.cpu_score)),
        ("gpu_score", i64::from(req.gpu_score)),
        ("memory_gb", req.memory_gb),
        ("gpu_memory_gb", req.gpu_memory_gb),
        ("storage_gb", req.storage_gb),
    ] {
        if value < 0 {
            return Err(format!("{label} must be non-negative"));
        }
    }
    let host_count = if req.host_count == 0 {
        1
    } else {
        req.host_count
    };
    if host_count < 1 {
        return Err("host_count must be at least 1".into());
    }
    Ok(ValidatedJob {
        name,
        job_max_cpt,
        replica_count,
        host_count,
    })
}

/// The task an item becomes. The job id keeps ids unique across jobs, and the index keeps them ordered.
pub fn job_task_id(job_id: Uuid, index: i32) -> String {
    let short = &job_id.simple().to_string()[..12];
    format!("job-{short}-{:05}", index + 1)
}

/// The quorum a replica count needs: a strict majority.
pub fn majority(replicas: u16) -> u32 {
    u32::from(replicas) / 2 + 1
}

/// Whether a request error is worth retrying on the next tick rather than recording against the item.
fn is_transient(code: tonic::Code) -> bool {
    matches!(
        code,
        tonic::Code::Internal
            | tonic::Code::Unavailable
            | tonic::Code::Unknown
            | tonic::Code::DeadlineExceeded
    )
}

/// What `submit_pending` should do with the next pending item, given the spend so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetStep {
    /// The item fits. Its cap is reserved.
    Submit,
    /// Earlier items are still running and may return part of their caps, so this item waits.
    Wait,
    /// Nothing is running and the item cannot fit: the rest of the job cannot run.
    Exhausted,
}

/// The budget rule, separated so it can be tested directly. All amounts are CPT.
pub fn budget_step(spent: i64, reserved: i64, item_cap: i64, job_max_cpt: i64) -> BudgetStep {
    // An overflowing sum does not fit under any cap, so it is never submitted.
    let projected = spent
        .checked_add(reserved)
        .and_then(|value| value.checked_add(item_cap));
    match projected {
        Some(total) if total <= job_max_cpt => BudgetStep::Submit,
        _ if reserved > 0 => BudgetStep::Wait,
        _ => BudgetStep::Exhausted,
    }
}

#[derive(Debug, Clone, FromRow)]
struct JobRow {
    id: Uuid,
    owner: String,
    name: String,
    state: String,
    note: String,
    source: String,
    per_item_max_cpt: i64,
    job_max_cpt: i64,
    replica_count: i32,
    cpu_score: i32,
    gpu_score: i32,
    memory_gb: i64,
    gpu_memory_gb: i64,
    storage_gb: i64,
    host_count: i32,
    item_count: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StateCounts {
    pending: i64,
    submitted: i64,
    completed: i64,
    failed: i64,
    refused: i64,
    cancelled: i64,
    spent: i64,
}

fn count_states(rows: &[(String, i64, i64)]) -> StateCounts {
    let mut counts = StateCounts::default();
    for (state, n, billed) in rows {
        match state.as_str() {
            "pending" => counts.pending += n,
            "submitted" => counts.submitted += n,
            "completed" => {
                counts.completed += n;
                counts.spent += billed;
            }
            "failed" => {
                counts.failed += n;
                counts.spent += billed;
            }
            "refused" => counts.refused += n,
            "cancelled" => counts.cancelled += n,
            _ => {}
        }
    }
    counts
}

fn job_view(job: &JobRow, counts: &StateCounts) -> JobView {
    JobView {
        job_id: job.id.to_string(),
        name: job.name.clone(),
        state: job.state.clone(),
        item_count: i64::from(job.item_count),
        pending: counts.pending,
        submitted: counts.submitted,
        completed: counts.completed,
        failed: counts.failed,
        refused: counts.refused,
        cancelled: counts.cancelled,
        spent_cpt: counts.spent,
        reserved_cpt: counts.submitted * job.per_item_max_cpt,
        per_item_max_cpt: job.per_item_max_cpt,
        job_max_cpt: job.job_max_cpt,
        created_at: job.created_at.to_rfc3339(),
        updated_at: job.updated_at.to_rfc3339(),
        finished_at: job
            .finished_at
            .map(|at| at.to_rfc3339())
            .unwrap_or_default(),
        note: job.note.clone(),
    }
}

fn build_upload(job: &JobRow, task_id: &str, input: &str) -> UploadTaskRequest {
    let replicas = u32::try_from(job.replica_count).unwrap_or(0);
    UploadTaskRequest {
        task_id: task_id.into(),
        torrent: input.into(),
        requirements: Some(ProtoResourceSpec {
            cpu_cores: 0,
            memory_mb: job.memory_gb.saturating_mul(1024),
            gpu_count: 0,
            gpu_name: String::new(),
            vram_mb: job.gpu_memory_gb.saturating_mul(1024),
            cpu_score: job.cpu_score,
            gpu_score: job.gpu_score,
            storage_total_gb: job.storage_gb,
            storage_available_gb: 0,
            score_suite: None,
        }),
        location: "local".into(),
        host_count: job.host_count,
        token: String::new(),
        max_cpt: job.per_item_max_cpt,
        runtime: "hcode".into(),
        task_source: job.source.clone(),
        package_data: Vec::new(),
        package_filename: String::new(),
        general_compute_manifest_json: Vec::new(),
        managed_dsl_backend_id: String::new(),
        managed_dsl_semantics_manifest_sha256: String::new(),
        managed_gpu_manifest_json: Vec::new(),
        managed_consensus_version: 0,
        managed_replica_count: replicas,
        managed_quorum: majority(u16::try_from(replicas).unwrap_or(0)),
    }
}

/// Whether task `t`, with its consensus policy `p`, is the work of item `i` of job `j`.
///
/// A task id does not show that. Ids are unique across accounts, and a Worker that ran one item of a job can
/// work out the ids of the items not yet submitted and create those tasks first. A task is an item's work only
/// when the owner, program, input, cap, resources, and consensus policy stored for it are the ones the runner
/// submits for that item. Each of those columns is written once, when the task is created.
///
/// A tensor input raises the stored memory to at least 1 GB, so a job that asks for none matches either value.
const ITEM_TASK_IS_WORK: &str = "t.owner = j.owner AND t.runtime = 'hcode'
     AND t.task_source = j.source AND t.torrent_source = i.input_json
     AND t.max_cpt = j.per_item_max_cpt
     AND t.req_cpu_score = j.cpu_score AND t.req_gpu_score = j.gpu_score
     AND (t.req_memory_gb = j.memory_gb OR (j.memory_gb = 0 AND t.req_memory_gb = 1))
     AND t.req_gpu_memory_gb = j.gpu_memory_gb
     AND t.req_storage_gb = j.storage_gb AND t.host_count = j.host_count
     AND t.deterministic AND NOT t.side_effects
     AND t.general_compute_manifest_json IS NULL
     AND t.managed_gpu_manifest_json IS NULL
     AND t.managed_dsl_backend_id IS NULL
     AND t.managed_dsl_semantics_manifest_sha256 IS NULL
     AND p.protocol_version = 1 AND p.replica_count = j.replica_count
     AND p.quorum = j.replica_count / 2 + 1 AND p.deterministic_required
     AND p.mode IN ('enforce', 'observe')";

/// The query behind every read of a job's items.
///
/// `bound_items` pairs each item with the task and consensus policy stored under its task id, and says whether
/// that task is really the item's work. `safe_items` hides the private details of an item whose stored task is
/// not its own: its state stays, so the counts stay honest, but its output, billing, and message are blanked, so
/// a colliding task cannot leak a stranger's work or spend. The runner uses the same view to decide whether an
/// item may adopt a task, and a read uses it to keep a mismatched item from exposing what it does not own.
fn job_items_query(filter: &str, select: &str) -> String {
    format!(
        "WITH bound_items AS (
             SELECT i.job_id, i.item_index, i.task_id, i.input_json, i.state, i.updated_at,
                    j.per_item_max_cpt,
                    i.output AS raw_output,
                    i.billed_cpt AS raw_billed_cpt,
                    i.status_message AS raw_status_message,
                    t.task_id IS NOT NULL AS task_exists,
                    COALESCE({ITEM_TASK_IS_WORK}, false) AS task_matches,
                    t.status AS task_status,
                    t.output AS task_output,
                    t.billed_amount AS task_billed_amount,
                    t.status_message AS task_status_message,
                    t.billing_settled AS task_billing_settled
             FROM job_items i
             JOIN jobs j ON j.id = i.job_id
             LEFT JOIN tasks t ON t.task_id = i.task_id
             LEFT JOIN managed_consensus_policies p ON p.task_id = t.task_id
             WHERE {filter}
         ),
         safe_items AS (
             SELECT job_id, item_index, task_id, input_json, state, updated_at, per_item_max_cpt,
                    task_exists, task_matches, task_status, task_output, task_billed_amount,
                    task_status_message,
                    CASE WHEN task_exists AND NOT task_matches THEN '' ELSE raw_output END AS output,
                    CASE WHEN task_exists AND NOT task_matches THEN 0::bigint ELSE raw_billed_cpt END AS billed_cpt,
                    CASE WHEN task_exists AND NOT task_matches THEN '' ELSE raw_status_message END AS status_message
             FROM bound_items
         )
         {select}"
    )
}

async fn fetch_job_locked(
    tx: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
) -> Result<Option<JobRow>> {
    let row = sqlx::query_as::<_, JobRow>(
        "SELECT * FROM jobs WHERE id = $1 AND state IN ('running', 'cancelling') FOR UPDATE SKIP LOCKED",
    )
    .bind(job_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row)
}

async fn set_item(
    tx: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
    index: i32,
    state: &str,
    billed: i64,
    output: &str,
    message: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE job_items SET state = $3, billed_cpt = $4, output = $5, status_message = $6, updated_at = NOW()
         WHERE job_id = $1 AND item_index = $2",
    )
    .bind(job_id)
    .bind(index)
    .bind(state)
    .bind(billed)
    .bind(output)
    .bind(message)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Follows the tasks of submitted items. A task that reached an end moves its item to the matching state.
async fn sync_submitted(tx: &mut Transaction<'_, Postgres>, job: &JobRow) -> Result<()> {
    #[derive(FromRow)]
    struct SubmittedTask {
        item_index: i32,
        updated_at: DateTime<Utc>,
        task_exists: bool,
        task_matches: bool,
        task_status: Option<String>,
        task_output: Option<String>,
        task_billed_amount: Option<i64>,
        task_status_message: Option<String>,
        task_billing_settled: Option<bool>,
    }
    let submitted: Vec<SubmittedTask> = sqlx::query_as(sqlx::AssertSqlSafe(job_items_query(
        "i.job_id = $1 AND i.state = 'submitted'",
        "SELECT item_index, updated_at, task_exists, task_matches, task_status,
                    task_output, task_billed_amount, task_status_message, task_billing_settled
             FROM bound_items",
    )))
    .bind(job.id)
    .fetch_all(&mut **tx)
    .await?;
    for task in submitted {
        if !task.task_exists {
            // The task is created before the submission is recorded, so a missing task means it was lost.
            if Utc::now() - task.updated_at > chrono::Duration::seconds(60) {
                set_item(
                    tx,
                    job.id,
                    task.item_index,
                    "failed",
                    0,
                    "",
                    "task not found",
                )
                .await?;
            }
            continue;
        }
        if !task.task_matches {
            set_item(
                tx,
                job.id,
                task.item_index,
                "refused",
                0,
                "",
                "task binding mismatch",
            )
            .await?;
            continue;
        }
        let settled = task.task_billing_settled.unwrap_or(false);
        let next = match task.task_status.as_deref() {
            // A completed task that is not settled (quorum reached but not yet billed) keeps its cap
            // reserved, so the job does not commit more than its total until the charge is recorded.
            Some("COMPLETED") if settled => "completed",
            Some("COMPLETED") => continue,
            Some("FAILED" | "TIMED_OUT") => "failed",
            Some("CANCELLED") => "cancelled",
            _ => continue,
        };
        set_item(
            tx,
            job.id,
            task.item_index,
            next,
            task.task_billed_amount.unwrap_or_default(),
            task.task_output.as_deref().unwrap_or_default(),
            task.task_status_message.as_deref().unwrap_or_default(),
        )
        .await?;
    }
    Ok(())
}

async fn set_note(tx: &mut Transaction<'_, Postgres>, job_id: Uuid, note: &str) -> Result<()> {
    sqlx::query("UPDATE jobs SET note = $2, updated_at = NOW() WHERE id = $1 AND note <> $2")
        .bind(job_id)
        .bind(note)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn submit_pending(
    tx: &mut Transaction<'_, Postgres>,
    state: &Arc<NodepoolState>,
    job: &JobRow,
) -> Result<()> {
    let counts = count_item_states(tx, job.id).await?;
    let spent = counts.spent;
    let mut reserved = counts.submitted * job.per_item_max_cpt;
    let pending: Vec<(i32, String, String)> = sqlx::query_as(
        "SELECT item_index, task_id, input_json FROM job_items
         WHERE job_id = $1 AND state = 'pending' ORDER BY item_index LIMIT $2",
    )
    .bind(job.id)
    .bind(JOB_SUBMIT_PER_TICK)
    .fetch_all(&mut **tx)
    .await?;
    let master = GrpcMasterNodeService::new(state.clone());
    let mut note = String::new();
    for (index, task_id, input) in pending {
        match budget_step(spent, reserved, job.per_item_max_cpt, job.job_max_cpt) {
            BudgetStep::Wait => {
                note = "waiting for running items to settle".into();
                break;
            }
            BudgetStep::Exhausted => {
                sqlx::query(
                    "UPDATE job_items SET state = 'refused', status_message = $2, updated_at = NOW()
                     WHERE job_id = $1 AND state = 'pending'",
                )
                .bind(job.id)
                .bind("the job's total budget is used up")
                .execute(&mut **tx)
                .await?;
                break;
            }
            BudgetStep::Submit => {}
        }
        let request = build_upload(job, &task_id, &input);
        match master.create_task_for_owner(&job.owner, request).await {
            Ok(response) => {
                let reply = response.into_inner();
                if reply.success || reply.status_message == "task_id already exists" {
                    // Recover an interrupted create only if the persisted task is exactly this item.
                    let matches: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(job_items_query(
                        "i.job_id = $1 AND i.item_index = $2",
                        "SELECT task_matches FROM bound_items",
                    )))
                    .bind(job.id)
                    .bind(index)
                    .fetch_one(&mut **tx)
                    .await?;
                    if matches {
                        set_item(tx, job.id, index, "submitted", 0, "", "").await?;
                        reserved += job.per_item_max_cpt;
                    } else {
                        set_item(tx, job.id, index, "refused", 0, "", "task binding mismatch")
                            .await?;
                    }
                } else if reply.status_message.starts_with("insufficient balance") {
                    // Later items would fail the same way, so the job waits for funds instead of refusing them.
                    note = "waiting for CPT".into();
                    break;
                } else {
                    set_item(tx, job.id, index, "refused", 0, "", &reply.status_message).await?;
                }
            }
            Err(status) if is_transient(status.code()) => {
                note = "retrying submissions".into();
                break;
            }
            Err(status) => {
                set_item(tx, job.id, index, "refused", 0, "", status.message()).await?;
            }
        }
    }
    set_note(tx, job.id, &note).await
}

async fn count_item_states(
    tx: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
) -> Result<StateCounts> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(job_items_query(
        "i.job_id = $1",
        "SELECT state, COUNT(*)::bigint, COALESCE(SUM(billed_cpt), 0)::bigint
             FROM safe_items GROUP BY state",
    )))
    .bind(job_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(count_states(&rows))
}

/// Cancelling stops the items that were not submitted, and asks the running tasks to stop once.
async fn cancel_items(
    tx: &mut Transaction<'_, Postgres>,
    state: &Arc<NodepoolState>,
    job: &JobRow,
) -> Result<()> {
    sqlx::query(
        "UPDATE job_items SET state = 'cancelled', updated_at = NOW()
         WHERE job_id = $1 AND state = 'pending'",
    )
    .bind(job.id)
    .execute(&mut **tx)
    .await?;
    let running: Vec<(String,)> = sqlx::query_as(sqlx::AssertSqlSafe(job_items_query(
        "i.job_id = $1 AND i.state = 'submitted' AND i.status_message <> 'stop requested'",
        "SELECT task_id FROM bound_items WHERE task_matches",
    )))
    .bind(job.id)
    .fetch_all(&mut **tx)
    .await?;
    let master = GrpcMasterNodeService::new(state.clone());
    for (task_id,) in running {
        // The stop is recorded even if it fails, so it is not repeated every tick. The task's own end is
        // what moves the item, and that is picked up by `sync_submitted`.
        let _ = master.stop_task_for_owner(&job.owner, &task_id).await;
        sqlx::query(
            "UPDATE job_items SET status_message = 'stop requested', updated_at = NOW()
             WHERE job_id = $1 AND task_id = $2",
        )
        .bind(job.id)
        .bind(&task_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn finish_if_done(tx: &mut Transaction<'_, Postgres>, job: &JobRow) -> Result<()> {
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM job_items WHERE job_id = $1 AND state IN ('pending', 'submitted')",
    )
    .bind(job.id)
    .fetch_one(&mut **tx)
    .await?;
    if open > 0 {
        return Ok(());
    }
    let next = if job.state == "cancelling" {
        "cancelled"
    } else {
        "completed"
    };
    sqlx::query(
        "UPDATE jobs SET state = $2, note = '', updated_at = NOW(), finished_at = NOW() WHERE id = $1",
    )
    .bind(job.id)
    .bind(next)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Moves one job forward: follow submitted tasks, then stop or submit, then finish if nothing is open.
/// The job row is locked for the whole step, and a job another process holds is skipped.
pub async fn advance_job(state: &Arc<NodepoolState>, job_id: Uuid) -> Result<()> {
    let pool: PgPool = state.scheduler.database().pool.clone();
    let mut tx = pool.begin().await?;
    let Some(job) = fetch_job_locked(&mut tx, job_id).await? else {
        tx.commit().await?;
        return Ok(());
    };
    sync_submitted(&mut tx, &job).await?;
    if job.state == "cancelling" {
        cancel_items(&mut tx, state, &job).await?;
    } else {
        submit_pending(&mut tx, state, &job).await?;
    }
    finish_if_done(&mut tx, &job).await?;
    tx.commit().await?;
    Ok(())
}

/// Advances the running and cancelling jobs that were visited longest ago, up to `JOB_SCAN_PER_TICK`.
///
/// A job is stamped when it is picked, before it is advanced. A job that keeps waiting or failing therefore
/// moves to the back of the order like any other, so it cannot hold a slot in every tick.
pub async fn run_job_tick(state: &Arc<NodepoolState>) -> Result<()> {
    let pool = state.scheduler.database().pool.clone();
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE jobs SET last_scanned_at = NOW()
         WHERE id IN (
             SELECT id FROM jobs
             WHERE state IN ('running', 'cancelling')
             ORDER BY last_scanned_at, id
             LIMIT $1
             FOR UPDATE SKIP LOCKED
         )
         RETURNING id",
    )
    .bind(JOB_SCAN_PER_TICK)
    .fetch_all(&pool)
    .await?;
    for id in ids {
        if let Err(error) = advance_job(state, id).await {
            tracing::warn!(job_id = %id, error = %error, "job step failed; it will be retried");
        }
    }
    Ok(())
}

/// Starts the job runner. It runs until the process stops.
pub fn spawn_job_runner(state: Arc<NodepoolState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(JOB_TICK_INTERVAL);
        loop {
            interval.tick().await;
            if let Err(error) = run_job_tick(&state).await {
                tracing::warn!(error = %error, "job runner tick failed");
            }
        }
    })
}

/// The gRPC job service. Every call needs a token, and an account sees only its own jobs.
pub struct GrpcJobService {
    state: Arc<NodepoolState>,
}

impl GrpcJobService {
    pub fn new(state: Arc<NodepoolState>) -> Self {
        Self { state }
    }

    async fn owner_of(&self, token: &str) -> Result<String, Status> {
        self.state
            .auth
            .validate_session(token)
            .await
            .map(|claims| claims.sub)
            .map_err(|_| Status::unauthenticated("Invalid token"))
    }

    async fn load_view(&self, owner: &str, job_id: Uuid) -> Result<Option<JobView>, Status> {
        let pool = &self.state.scheduler.database().pool;
        let job: Option<JobRow> = sqlx::query_as("SELECT * FROM jobs WHERE id = $1 AND owner = $2")
            .bind(job_id)
            .bind(owner)
            .fetch_optional(pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let Some(job) = job else {
            return Ok(None);
        };
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(job_items_query(
            "i.job_id = $1",
            "SELECT state, COUNT(*)::bigint, COALESCE(SUM(billed_cpt), 0)::bigint
                 FROM safe_items GROUP BY state",
        )))
        .bind(job_id)
        .fetch_all(pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Some(job_view(&job, &count_states(&rows))))
    }
}

fn reply_create(success: bool, message: impl Into<String>) -> CreateJobResponse {
    CreateJobResponse {
        success,
        status_message: message.into(),
        job_id: String::new(),
        item_count: 0,
        job_max_cpt: 0,
    }
}

#[tonic::async_trait]
impl JobService for GrpcJobService {
    async fn create_job(
        &self,
        request: Request<CreateJobRequest>,
    ) -> Result<Response<CreateJobResponse>, Status> {
        let req = request.into_inner();
        let owner = self.owner_of(&req.token).await?;
        let validated = match validate_new_job(
            &req,
            self.state.managed_consensus_replica_count,
            self.state.managed_consensus_max_replicas,
        ) {
            Ok(validated) => validated,
            Err(message) => return Ok(Response::new(reply_create(false, message))),
        };
        let pool = self.state.scheduler.database().pool.clone();
        let balance: Option<i64> = sqlx::query_scalar(
            "SELECT balance FROM users WHERE username = $1 AND is_active = true",
        )
        .bind(&owner)
        .fetch_optional(&pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        let Some(balance) = balance else {
            return Ok(Response::new(reply_create(
                false,
                "account not found or inactive",
            )));
        };
        if balance < req.per_item_max_cpt {
            return Ok(Response::new(reply_create(
                false,
                format!(
                    "insufficient balance: need {} CPT to start a job item",
                    req.per_item_max_cpt
                ),
            )));
        }
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM jobs WHERE owner = $1 AND state IN ('running', 'cancelling')",
        )
        .bind(&owner)
        .fetch_one(&pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        if active >= JOB_MAX_ACTIVE_PER_OWNER {
            return Ok(Response::new(reply_create(
                false,
                format!("an account may run at most {JOB_MAX_ACTIVE_PER_OWNER} jobs at once"),
            )));
        }

        let job_id = Uuid::new_v4();
        let item_count = req.inputs.len() as i32;
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        sqlx::query(
            "INSERT INTO jobs (id, owner, name, state, source, per_item_max_cpt, job_max_cpt,
                 replica_count, cpu_score, gpu_score, memory_gb, gpu_memory_gb, storage_gb,
                 host_count, item_count)
             VALUES ($1, $2, $3, 'running', $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(job_id)
        .bind(&owner)
        .bind(&validated.name)
        .bind(req.source.trim_end())
        .bind(req.per_item_max_cpt)
        .bind(validated.job_max_cpt)
        .bind(i32::from(validated.replica_count))
        .bind(req.cpu_score)
        .bind(req.gpu_score)
        .bind(req.memory_gb)
        .bind(req.gpu_memory_gb)
        .bind(req.storage_gb)
        .bind(validated.host_count)
        .bind(item_count)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        let indexes: Vec<i32> = (0..item_count).collect();
        let task_ids: Vec<String> = indexes
            .iter()
            .map(|index| job_task_id(job_id, *index))
            .collect();
        sqlx::query(
            "INSERT INTO job_items (job_id, item_index, task_id, input_json, state)
             SELECT $1, idx, task, input, 'pending'
             FROM UNNEST($2::int[], $3::text[], $4::text[]) AS t(idx, task, input)",
        )
        .bind(job_id)
        .bind(&indexes)
        .bind(&task_ids)
        .bind(&req.inputs)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(CreateJobResponse {
            success: true,
            status_message: "OK".into(),
            job_id: job_id.to_string(),
            item_count: i64::from(item_count),
            job_max_cpt: validated.job_max_cpt,
        }))
    }

    async fn get_job(
        &self,
        request: Request<GetJobRequest>,
    ) -> Result<Response<GetJobResponse>, Status> {
        let req = request.into_inner();
        let owner = self.owner_of(&req.token).await?;
        let Ok(job_id) = Uuid::parse_str(&req.job_id) else {
            return Ok(Response::new(GetJobResponse {
                success: false,
                status_message: "job_id is not valid".into(),
                job: None,
            }));
        };
        match self.load_view(&owner, job_id).await? {
            Some(view) => Ok(Response::new(GetJobResponse {
                success: true,
                status_message: "OK".into(),
                job: Some(view),
            })),
            None => Ok(Response::new(GetJobResponse {
                success: false,
                status_message: "Job not found".into(),
                job: None,
            })),
        }
    }

    async fn list_jobs(
        &self,
        request: Request<ListJobsRequest>,
    ) -> Result<Response<ListJobsResponse>, Status> {
        let req = request.into_inner();
        let owner = self.owner_of(&req.token).await?;
        let pool = &self.state.scheduler.database().pool;
        let jobs: Vec<JobRow> =
            sqlx::query_as("SELECT * FROM jobs WHERE owner = $1 ORDER BY created_at DESC LIMIT 50")
                .bind(&owner)
                .fetch_all(pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        let ids: Vec<Uuid> = jobs.iter().map(|job| job.id).collect();
        let rows: Vec<(Uuid, String, i64, i64)> =
            sqlx::query_as(sqlx::AssertSqlSafe(job_items_query(
                "i.job_id = ANY($1)",
                "SELECT job_id, state, COUNT(*)::bigint, COALESCE(SUM(billed_cpt), 0)::bigint
                 FROM safe_items GROUP BY job_id, state",
            )))
            .bind(&ids)
            .fetch_all(pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let views = jobs
            .iter()
            .map(|job| {
                let per_job: Vec<(String, i64, i64)> = rows
                    .iter()
                    .filter(|row| row.0 == job.id)
                    .map(|row| (row.1.clone(), row.2, row.3))
                    .collect();
                job_view(job, &count_states(&per_job))
            })
            .collect();
        Ok(Response::new(ListJobsResponse {
            success: true,
            status_message: "OK".into(),
            jobs: views,
        }))
    }

    async fn list_job_items(
        &self,
        request: Request<ListJobItemsRequest>,
    ) -> Result<Response<ListJobItemsResponse>, Status> {
        let req = request.into_inner();
        let owner = self.owner_of(&req.token).await?;
        let Ok(job_id) = Uuid::parse_str(&req.job_id) else {
            return Ok(Response::new(ListJobItemsResponse {
                success: false,
                status_message: "job_id is not valid".into(),
                items: Vec::new(),
                total: 0,
            }));
        };
        let pool = &self.state.scheduler.database().pool;
        let total: Option<i32> =
            sqlx::query_scalar("SELECT item_count FROM jobs WHERE id = $1 AND owner = $2")
                .bind(job_id)
                .bind(&owner)
                .fetch_optional(pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        let Some(total) = total else {
            return Ok(Response::new(ListJobItemsResponse {
                success: false,
                status_message: "Job not found".into(),
                items: Vec::new(),
                total: 0,
            }));
        };
        let offset = req.offset.max(0);
        let limit = req.limit.clamp(1, 100);
        #[allow(clippy::type_complexity)]
        let rows: Vec<(i32, String, String, i64, i64, String, i32, String, i64)> =
            sqlx::query_as(sqlx::AssertSqlSafe(job_items_query(
                "i.job_id = $1 AND j.owner = $2",
                "SELECT item_index, task_id, state, per_item_max_cpt, billed_cpt,
                        left(output, $4), octet_length(output), status_message, octet_length(input_json)::bigint
                 FROM safe_items
                 ORDER BY item_index
                 OFFSET $3 LIMIT $5",
            )))
        .bind(job_id)
        .bind(&owner)
        .bind(offset)
        .bind(JOB_OUTPUT_PREVIEW_CHARS)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        let items = rows
            .into_iter()
            .map(
                |(
                    index,
                    task_id,
                    state,
                    max_cpt,
                    billed,
                    preview,
                    output_bytes,
                    message,
                    input_bytes,
                )| {
                    let output_bytes = i64::from(output_bytes);
                    JobItemView {
                        index: i64::from(index) + 1,
                        task_id,
                        state,
                        max_cpt,
                        billed_cpt: billed,
                        output_truncated: output_bytes > preview.len() as i64,
                        output_preview: preview,
                        output_bytes,
                        status_message: message,
                        input_bytes,
                    }
                },
            )
            .collect();
        Ok(Response::new(ListJobItemsResponse {
            success: true,
            status_message: "OK".into(),
            items,
            total: i64::from(total),
        }))
    }

    async fn cancel_job(
        &self,
        request: Request<CancelJobRequest>,
    ) -> Result<Response<CancelJobResponse>, Status> {
        let req = request.into_inner();
        let owner = self.owner_of(&req.token).await?;
        let Ok(job_id) = Uuid::parse_str(&req.job_id) else {
            return Ok(Response::new(CancelJobResponse {
                success: false,
                status_message: "job_id is not valid".into(),
            }));
        };
        let pool = &self.state.scheduler.database().pool;
        let state: Option<String> =
            sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1 AND owner = $2")
                .bind(job_id)
                .bind(&owner)
                .fetch_optional(pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        let reply = match state.as_deref() {
            None => CancelJobResponse {
                success: false,
                status_message: "Job not found".into(),
            },
            Some("running") => {
                sqlx::query(
                    "UPDATE jobs SET state = 'cancelling', updated_at = NOW() WHERE id = $1",
                )
                .bind(job_id)
                .execute(pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
                CancelJobResponse {
                    success: true,
                    status_message: "cancellation requested".into(),
                }
            }
            Some("cancelling") => CancelJobResponse {
                success: true,
                status_message: "cancellation already requested".into(),
            },
            Some("cancelled") => CancelJobResponse {
                success: true,
                status_message: "job already cancelled".into(),
            },
            Some(_) => CancelJobResponse {
                success: false,
                status_message: "job already completed".into(),
            },
        };
        Ok(Response::new(reply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(inputs: &[&str]) -> CreateJobRequest {
        CreateJobRequest {
            token: "t".into(),
            name: "sweep".into(),
            source: "let x = get(input, \"n\");\nx * 2;".into(),
            inputs: inputs.iter().map(|value| (*value).to_string()).collect(),
            per_item_max_cpt: 100,
            job_max_cpt: 0,
            replica_count: 0,
            cpu_score: 0,
            gpu_score: 0,
            memory_gb: 0,
            gpu_memory_gb: 0,
            storage_gb: 0,
            host_count: 0,
        }
    }

    #[test]
    fn a_valid_job_gets_the_default_replicas_and_a_cap_for_every_item() {
        let job = validate_new_job(&request(&["{\"n\":1}", "{\"n\":2}"]), 3, 7).unwrap();
        assert_eq!(job.job_max_cpt, 200);
        assert_eq!(job.replica_count, 3);
        assert_eq!(job.host_count, 1);
        assert_eq!(job.name, "sweep");
    }

    #[test]
    fn job_inputs_must_be_non_empty_json_within_the_limits() {
        assert!(validate_new_job(&request(&[]), 3, 7).is_err());
        assert!(validate_new_job(&request(&["   "]), 3, 7)
            .unwrap_err()
            .contains("input 1 is empty"));
        assert!(validate_new_job(&request(&["{\"n\":1}", "{nope"]), 3, 7)
            .unwrap_err()
            .contains("input 2 is not valid JSON"));
        let too_many: Vec<&str> = vec!["1"; MAX_JOB_ITEMS + 1];
        assert!(validate_new_job(&request(&too_many), 3, 7)
            .unwrap_err()
            .contains("at most 1000 inputs"));
    }

    #[test]
    fn the_caps_must_be_positive_and_fund_one_item() {
        let mut req = request(&["1"]);
        req.per_item_max_cpt = 0;
        assert!(validate_new_job(&req, 3, 7).is_err());

        let mut req = request(&["1", "2"]);
        req.job_max_cpt = 50;
        assert!(validate_new_job(&req, 3, 7)
            .unwrap_err()
            .contains("at least one item"));

        let mut req = request(&["1", "2"]);
        req.job_max_cpt = 150;
        assert_eq!(validate_new_job(&req, 3, 7).unwrap().job_max_cpt, 150);
    }

    #[test]
    fn replica_counts_follow_the_same_rule_as_a_task() {
        let mut req = request(&["1"]);
        req.replica_count = 1;
        assert!(validate_new_job(&req, 3, 7).is_err());
        req.replica_count = 8;
        assert!(validate_new_job(&req, 3, 7).is_err());
        req.replica_count = 5;
        assert_eq!(validate_new_job(&req, 3, 7).unwrap().replica_count, 5);
        assert_eq!(majority(5), 3);
        assert_eq!(majority(2), 2);
    }

    #[test]
    fn a_job_names_its_items_by_the_job_and_the_position() {
        let id = Uuid::parse_str("0f1e2d3c-4b5a-4968-8778-6a5b4c3d2e1f").unwrap();
        assert_eq!(job_task_id(id, 0), "job-0f1e2d3c4b5a-00001");
        assert_eq!(job_task_id(id, 999), "job-0f1e2d3c4b5a-01000");
        assert!(hivemind_proto::TASK_ID_MAX_BYTES > job_task_id(id, 999).len());
    }

    #[test]
    fn the_budget_never_lets_spend_and_reservations_pass_the_cap() {
        // Three items of 10 under a cap of 25: the first two fit, the third must wait for them to settle.
        assert_eq!(budget_step(0, 0, 10, 25), BudgetStep::Submit);
        assert_eq!(budget_step(0, 10, 10, 25), BudgetStep::Submit);
        assert_eq!(budget_step(0, 20, 10, 25), BudgetStep::Wait);
        // Once they finish for 8 each, 16 is spent, nothing is reserved, and 16 + 10 is over the cap.
        assert_eq!(budget_step(16, 0, 10, 25), BudgetStep::Exhausted);
        // An overflowing sum is treated as over the cap, never as room.
        assert_eq!(budget_step(i64::MAX, 1, 1, i64::MAX), BudgetStep::Wait);
        assert_eq!(budget_step(i64::MAX, 0, 1, 10), BudgetStep::Exhausted);
    }

    #[test]
    fn counts_separate_spend_from_reservations() {
        let rows = vec![
            ("completed".to_string(), 2, 16),
            ("failed".to_string(), 1, 4),
            ("submitted".to_string(), 3, 0),
            ("refused".to_string(), 1, 0),
            ("pending".to_string(), 5, 0),
        ];
        let counts = count_states(&rows);
        assert_eq!(counts.spent, 20);
        assert_eq!(counts.submitted, 3);
        assert_eq!(counts.completed, 2);
        assert_eq!(counts.pending, 5);
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::NodeManager;
    use hivemind_auth::api_tokens::ApiTokenStore;
    use hivemind_config::HivemindConfig;
    use hivemind_proto::master_node_service_server::MasterNodeService;
    use hivemind_task_scheduler::TaskScheduler;

    // Without a database these tests return early, as the other database tests in this crate do.
    // HIVEMIND_REQUIRE_TEST_DATABASE=1 turns the skip into a failure.
    async fn fixture(
        test_name: &str,
    ) -> Option<(
        Arc<NodepoolState>,
        hivemind_database::postgres::IsolatedTestPool,
    )> {
        let config = HivemindConfig::for_test();
        let pool_fixture =
            hivemind_database::postgres::create_isolated_test_pool_with_config(&config, test_name)
                .await
                .ok()?;
        if hivemind_database::postgres::run_migrations(&pool_fixture.pool)
            .await
            .is_err()
        {
            pool_fixture.cleanup().await.ok();
            return None;
        }
        let db = hivemind_database::DatabaseManager {
            pool: pool_fixture.pool.clone(),
        };
        let auth = hivemind_auth::AuthManager::new(&db, "job-test-secret", 24);
        let scheduler = TaskScheduler::new(db.clone(), auth.clone());
        let node_manager = Arc::new(NodeManager::new(&config, db.clone()));
        let state = Arc::new(NodepoolState {
            auth,
            worker_execution_private_key_pem: config.auth.worker_execution_private_key_pem.clone(),
            worker_execution_public_key_pem: config.auth.worker_execution_public_key_pem.clone(),
            managed_consensus_rollout_mode: config.managed_consensus.rollout_mode,
            managed_consensus_replica_count: config.managed_consensus.replica_count,
            managed_consensus_quorum: config.managed_consensus.quorum,
            managed_consensus_max_replicas: config.managed_consensus.max_replicas,
            managed_consensus_timeout_secs: config.managed_consensus.timeout_secs,
            node_manager,
            session_registry: hivemind_client_core::SessionRegistry::shared(Default::default()),
            dispatcher: None,
            scheduler,
            artifact_root: crate::grpc::artifact_root_for_config(&config),
        });
        Some((state, pool_fixture))
    }

    // An account with a balance, and an API token for it. The token is what a script sends.
    async fn account(state: &NodepoolState, username: &str, balance: i64) -> String {
        let pool = &state.scheduler.database().pool;
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance, is_active)
             VALUES ($1, '!test-only', $2, true)",
        )
        .bind(username)
        .bind(balance)
        .execute(pool)
        .await
        .unwrap();
        ApiTokenStore::new(pool.clone())
            .create(username, "jobs", 30)
            .await
            .unwrap()
            .token
    }

    async fn set_balance(state: &NodepoolState, username: &str, balance: i64) {
        sqlx::query("UPDATE users SET balance = $2 WHERE username = $1")
            .bind(username)
            .bind(balance)
            .execute(&state.scheduler.database().pool)
            .await
            .unwrap();
    }

    fn create_request(
        token: &str,
        inputs: &[&str],
        per_item: i64,
        job_max: i64,
    ) -> CreateJobRequest {
        CreateJobRequest {
            token: token.into(),
            name: "sweep".into(),
            source: "let x = get(input, \"n\");\nx * 2;".into(),
            inputs: inputs.iter().map(|value| (*value).to_string()).collect(),
            per_item_max_cpt: per_item,
            job_max_cpt: job_max,
            replica_count: 0,
            cpu_score: 0,
            gpu_score: 0,
            memory_gb: 0,
            gpu_memory_gb: 0,
            storage_gb: 0,
            host_count: 0,
        }
    }

    async fn items(service: &GrpcJobService, token: &str, job_id: &str) -> Vec<JobItemView> {
        service
            .list_job_items(Request::new(ListJobItemsRequest {
                token: token.into(),
                job_id: job_id.into(),
                offset: 0,
                limit: 100,
            }))
            .await
            .unwrap()
            .into_inner()
            .items
    }

    fn states(items: &[JobItemView]) -> Vec<&str> {
        items.iter().map(|item| item.state.as_str()).collect()
    }

    async fn job_view(service: &GrpcJobService, token: &str, job_id: &str) -> JobView {
        service
            .get_job(Request::new(GetJobRequest {
                token: token.into(),
                job_id: job_id.into(),
            }))
            .await
            .unwrap()
            .into_inner()
            .job
            .unwrap()
    }

    // Stands in for a worker: the task reaches its end with this status, output, and billed amount.
    // A task that has recorded its final charge is settled, so the flag follows the recorded amount.
    async fn finish_task(
        state: &NodepoolState,
        task_id: &str,
        status: &str,
        output: &str,
        billed: i64,
    ) {
        sqlx::query(
            "UPDATE tasks SET status = $2, output = $3, billed_amount = $4, billing_settled = true
             WHERE task_id = $1",
        )
        .bind(task_id)
        .bind(status)
        .bind(output)
        .bind(billed)
        .execute(&state.scheduler.database().pool)
        .await
        .unwrap();
    }

    async fn tick(state: &Arc<NodepoolState>, job_id: &str) {
        advance_job(state, Uuid::parse_str(job_id).unwrap())
            .await
            .unwrap();
    }

    async fn precreate_item_task(
        state: &Arc<NodepoolState>,
        job_id: &str,
        token: &str,
        mismatch: &str,
    ) -> String {
        let job: JobRow = sqlx::query_as("SELECT * FROM jobs WHERE id = $1")
            .bind(Uuid::parse_str(job_id).unwrap())
            .fetch_one(&state.scheduler.database().pool)
            .await
            .unwrap();
        let (task_id, input): (String, String) = sqlx::query_as(
            "SELECT task_id, input_json FROM job_items WHERE job_id = $1 AND item_index = 0",
        )
        .bind(job.id)
        .fetch_one(&state.scheduler.database().pool)
        .await
        .unwrap();
        let mut upload = build_upload(&job, &task_id, &input);
        upload.token = token.into();
        match mismatch {
            "source" => upload.task_source = "return 987654321;".into(),
            "input" => upload.torrent = "{\"private\":true}".into(),
            "cap" => upload.max_cpt += 10,
            "resources" => upload.requirements.as_mut().unwrap().cpu_score += 1,
            "policy" => {
                upload.managed_replica_count = 5;
                upload.managed_quorum = 3;
            }
            "owner" | "exact" => {}
            _ => panic!("unknown mismatch"),
        }
        let response = GrpcMasterNodeService::new(state.clone())
            .upload_task(Request::new(upload))
            .await
            .unwrap()
            .into_inner();
        assert!(response.success, "{}", response.status_message);
        task_id
    }

    async fn assert_item_collision_is_refused(mismatch: &str, already_submitted: bool) {
        let (state, fixture) = fixture("job_binding_conflict")
            .await
            .expect("binding regressions require the test database");
        let owner = account(&state, "binding-owner", 1000).await;
        let other = account(&state, "binding-other", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let created = service
            .create_job(Request::new(create_request(&owner, &["1"], 20, 20)))
            .await
            .unwrap()
            .into_inner();
        assert!(created.success);
        let task_id = precreate_item_task(
            &state,
            &created.job_id,
            if mismatch == "owner" { &other } else { &owner },
            mismatch,
        )
        .await;
        finish_task(
            &state,
            &task_id,
            "COMPLETED",
            "private-conflicting-output",
            7,
        )
        .await;
        if already_submitted {
            sqlx::query("UPDATE job_items SET state = 'submitted' WHERE task_id = $1")
                .bind(&task_id)
                .execute(&state.scheduler.database().pool)
                .await
                .unwrap();
        }
        tick(&state, &created.job_id).await;
        tick(&state, &created.job_id).await;
        let result = items(&service, &owner, &created.job_id).await;
        assert_eq!(
            result[0].state, "refused",
            "must not adopt {mismatch} collision"
        );
        assert!(result[0].output_preview.is_empty());
        assert_eq!(result[0].output_bytes, 0);
        assert_eq!(result[0].billed_cpt, 0);
        assert_eq!(
            job_view(&service, &owner, &created.job_id).await.spent_cpt,
            0
        );
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn task_binding_collision_cannot_adopt_another_accounts_task() {
        assert_item_collision_is_refused("owner", false).await;
    }

    #[tokio::test]
    async fn task_binding_collision_requires_exact_source_input_resources_cap_and_policy() {
        for mismatch in ["source", "input", "resources", "cap", "policy"] {
            assert_item_collision_is_refused(mismatch, false).await;
        }
    }

    #[tokio::test]
    async fn task_binding_is_rechecked_for_preexisting_submitted_rows() {
        for mismatch in ["owner", "source", "input"] {
            assert_item_collision_is_refused(mismatch, true).await;
        }
    }

    #[tokio::test]
    async fn task_binding_mismatch_hides_historical_cached_output_and_billing() {
        let (state, fixture) = fixture("job_binding_history").await.unwrap();
        let owner = account(&state, "history-owner", 1000).await;
        let victim = account(&state, "history-victim", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let created = service
            .create_job(Request::new(create_request(&owner, &["1"], 20, 20)))
            .await
            .unwrap()
            .into_inner();
        let task_id = precreate_item_task(&state, &created.job_id, &victim, "owner").await;
        finish_task(&state, &task_id, "COMPLETED", "victim-secret", 7).await;
        sqlx::query(
            "UPDATE job_items SET state = 'completed', output = 'victim-secret', billed_cpt = 7,
             status_message = 'victim-private-message' WHERE task_id = $1",
        )
        .bind(&task_id)
        .execute(&state.scheduler.database().pool)
        .await
        .unwrap();
        sqlx::query("UPDATE jobs SET state = 'completed' WHERE id = $1")
            .bind(Uuid::parse_str(&created.job_id).unwrap())
            .execute(&state.scheduler.database().pool)
            .await
            .unwrap();
        let exposed = items(&service, &owner, &created.job_id).await;
        assert!(
            exposed[0].output_preview.is_empty(),
            "historical cross-owner output must not escape"
        );
        assert_eq!(exposed[0].output_bytes, 0);
        assert_eq!(exposed[0].billed_cpt, 0);
        assert!(!exposed[0].status_message.contains("victim"));
        assert_eq!(
            job_view(&service, &owner, &created.job_id).await.spent_cpt,
            0
        );
        let listed = service
            .list_jobs(Request::new(ListJobsRequest { token: owner }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(listed.jobs[0].spent_cpt, 0);
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn task_binding_mismatch_never_cancels_an_unrelated_same_owner_task() {
        let (state, fixture) = fixture("job_binding_cancel").await.unwrap();
        let owner = account(&state, "cancel-binding-owner", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let created = service
            .create_job(Request::new(create_request(&owner, &["1"], 20, 20)))
            .await
            .unwrap()
            .into_inner();
        let task_id = precreate_item_task(&state, &created.job_id, &owner, "source").await;
        sqlx::query("UPDATE job_items SET state = 'submitted' WHERE task_id = $1")
            .bind(&task_id)
            .execute(&state.scheduler.database().pool)
            .await
            .unwrap();
        service
            .cancel_job(Request::new(CancelJobRequest {
                token: owner,
                job_id: created.job_id.clone(),
            }))
            .await
            .unwrap();
        tick(&state, &created.job_id).await;
        let task = state.scheduler.get_task(&task_id).await.unwrap().unwrap();
        assert_eq!(
            task.status.as_str(),
            "PENDING",
            "cancellation must not touch a differently bound task"
        );
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn task_binding_exact_match_recovers_after_create_before_item_checkpoint() {
        let (state, fixture) = fixture("job_binding_recovery").await.unwrap();
        let owner = account(&state, "recover-binding-owner", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let created = service
            .create_job(Request::new(create_request(&owner, &["1"], 20, 20)))
            .await
            .unwrap()
            .into_inner();
        let task_id = precreate_item_task(&state, &created.job_id, &owner, "exact").await;
        tick(&state, &created.job_id).await;
        assert_eq!(
            items(&service, &owner, &created.job_id).await[0].state,
            "submitted"
        );
        finish_task(&state, &task_id, "COMPLETED", "2", 7).await;
        tick(&state, &created.job_id).await;
        let result = items(&service, &owner, &created.job_id).await;
        assert_eq!(result[0].state, "completed");
        assert_eq!(result[0].output_preview, "2");
        assert_eq!(result[0].billed_cpt, 7);
        let task_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(&state.scheduler.database().pool)
            .await
            .unwrap();
        assert_eq!(task_count, 1);
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn task_binding_recovery_preserves_canonical_tensor_memory_and_persisted_policy() {
        let (state, fixture) = fixture("job_binding_canonical").await.unwrap();
        let owner = account(&state, "canonical-binding-owner", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let digest = general_compute_runtime::sha256_digest(&[0; 10]);
        let input = serde_json::json!({
            "$hcode_tensor_inputs": 1,
            "input": 1,
            "artifacts": [{
                "artifact_id": "tensor", "role": "input", "size_bytes": 10,
                "mime_type": "application/x-safetensors", "sha256": digest,
                "chunks": [{ "offset": 0, "size_bytes": 10, "sha256": digest }]
            }]
        })
        .to_string();
        let mut request = create_request(&owner, &[&input], 20, 20);
        request.replica_count = 5;
        let created = service
            .create_job(Request::new(request))
            .await
            .unwrap()
            .into_inner();
        assert!(created.success);
        let task_id = precreate_item_task(&state, &created.job_id, &owner, "exact").await;
        sqlx::query("UPDATE managed_consensus_policies SET mode = 'observe' WHERE task_id = $1")
            .bind(&task_id)
            .execute(&state.scheduler.database().pool)
            .await
            .unwrap();
        let memory: i32 = sqlx::query_scalar("SELECT req_memory_gb FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .fetch_one(&state.scheduler.database().pool)
            .await
            .unwrap();
        assert_eq!(memory, 1);
        tick(&state, &created.job_id).await;
        assert_eq!(
            items(&service, &owner, &created.job_id).await[0].state,
            "submitted"
        );
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn items_run_within_the_cap_and_the_job_completes_as_its_tasks_end() {
        let Some((state, fixture)) = fixture("job_budget").await else {
            return;
        };
        let token = account(&state, "job-alice", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let created = service
            .create_job(Request::new(create_request(
                &token,
                &["{\"n\":1}", "{\"n\":2}", "{\"n\":3}"],
                10,
                25,
            )))
            .await
            .unwrap()
            .into_inner();
        assert!(created.success, "{}", created.status_message);
        assert_eq!(created.job_max_cpt, 25);

        // Two items fit under 25 (10 + 10). The third would make 30, so it waits for them.
        tick(&state, &created.job_id).await;
        let first = items(&service, &token, &created.job_id).await;
        assert_eq!(states(&first), vec!["submitted", "submitted", "pending"]);
        assert!(first.iter().all(|item| item.max_cpt == 10));
        assert!(first[0].task_id.starts_with("job-"));

        finish_task(&state, &first[0].task_id, "COMPLETED", "20", 8).await;
        finish_task(&state, &first[1].task_id, "COMPLETED", "40", 8).await;
        tick(&state, &created.job_id).await;

        // Sixteen is spent and nothing is running, so 16 + 10 = 26 is over the cap: the third item is refused.
        let after = items(&service, &token, &created.job_id).await;
        assert_eq!(states(&after), vec!["completed", "completed", "refused"]);
        assert_eq!(after[0].output_preview, "20");
        assert_eq!(after[0].billed_cpt, 8);
        assert!(after[2].status_message.contains("budget is used up"));

        let job = job_view(&service, &token, &created.job_id).await;
        assert_eq!(job.state, "completed");
        assert_eq!(job.spent_cpt, 16);
        assert_eq!(job.refused, 1);
        assert!(!job.finished_at.is_empty());
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn cancelling_stops_the_running_items_and_settles_the_job() {
        let Some((state, fixture)) = fixture("job_cancel").await else {
            return;
        };
        let token = account(&state, "job-bob", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let created = service
            .create_job(Request::new(create_request(
                &token,
                &["1", "2", "3"],
                10,
                100,
            )))
            .await
            .unwrap()
            .into_inner();
        tick(&state, &created.job_id).await;
        assert!(items(&service, &token, &created.job_id)
            .await
            .iter()
            .all(|item| item.state == "submitted"));

        let cancelled = service
            .cancel_job(Request::new(CancelJobRequest {
                token: token.clone(),
                job_id: created.job_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(cancelled.success, "{}", cancelled.status_message);

        // The next tick asks each running task to stop once, and records that it did.
        tick(&state, &created.job_id).await;
        let asked = items(&service, &token, &created.job_id).await;
        assert!(asked
            .iter()
            .all(|item| item.status_message == "stop requested"));

        // The tasks end as cancelled, and the job settles.
        for item in &asked {
            finish_task(&state, &item.task_id, "CANCELLED", "", 0).await;
        }
        tick(&state, &created.job_id).await;
        let settled = items(&service, &token, &created.job_id).await;
        assert_eq!(
            states(&settled),
            vec!["cancelled", "cancelled", "cancelled"]
        );
        assert_eq!(
            job_view(&service, &token, &created.job_id).await.state,
            "cancelled"
        );

        let again = service
            .cancel_job(Request::new(CancelJobRequest {
                token,
                job_id: created.job_id,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(again.success, "cancelling a cancelled job is not an error");
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn a_completed_item_holds_its_cap_until_the_charge_is_settled() {
        let Some((state, fixture)) = fixture("job_settlement").await else {
            return;
        };
        let token = account(&state, "job-settle", 1000).await;
        let service = GrpcJobService::new(state.clone());
        // Two items of cap 100 each under a total of 100: only one may run at a time.
        let created = service
            .create_job(Request::new(create_request(&token, &["1", "2"], 100, 100)))
            .await
            .unwrap()
            .into_inner();
        assert!(created.success);
        tick(&state, &created.job_id).await;
        let first = items(&service, &token, &created.job_id).await;
        assert_eq!(states(&first), vec!["submitted", "pending"]);

        // The first task reached quorum but is not billed yet: quorum_stop_pending is
        // COMPLETED with no settled charge. While it is unsettled the item keeps running.
        sqlx::query(
            "UPDATE tasks SET status = 'COMPLETED', output = '42',
                billing_settled = false, billed_amount = 0 WHERE task_id = $1",
        )
        .bind(&first[0].task_id)
        .execute(&state.scheduler.database().pool)
        .await
        .unwrap();
        tick(&state, &created.job_id).await;
        let held = items(&service, &token, &created.job_id).await;
        assert_eq!(
            states(&held),
            vec!["submitted", "pending"],
            "an unsettled charge must keep the item running and its cap reserved"
        );
        assert_eq!(
            job_view(&service, &token, &created.job_id)
                .await
                .reserved_cpt,
            100
        );

        // Settlement records the real charge. The item completes with it, and the budget
        // then has no room left for the second item.
        sqlx::query(
            "UPDATE tasks SET billing_settled = true, billed_amount = 99 WHERE task_id = $1",
        )
        .bind(&first[0].task_id)
        .execute(&state.scheduler.database().pool)
        .await
        .unwrap();
        tick(&state, &created.job_id).await;
        let settled = items(&service, &token, &created.job_id).await;
        assert_eq!(states(&settled), vec!["completed", "refused"]);
        assert_eq!(
            settled[0].billed_cpt, 99,
            "the settled charge is recorded, not zero"
        );
        let job = job_view(&service, &token, &created.job_id).await;
        assert_eq!(job.state, "completed");
        assert_eq!(job.spent_cpt, 99);
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn a_job_waits_for_funds_and_says_why_then_continues_when_funded() {
        let Some((state, fixture)) = fixture("job_funds").await else {
            return;
        };
        let token = account(&state, "job-carol", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let created = service
            .create_job(Request::new(create_request(&token, &["1", "2"], 10, 100)))
            .await
            .unwrap()
            .into_inner();
        set_balance(&state, "job-carol", 0).await;
        tick(&state, &created.job_id).await;

        assert_eq!(
            states(&items(&service, &token, &created.job_id).await),
            vec!["pending", "pending"]
        );
        let job = job_view(&service, &token, &created.job_id).await;
        assert_eq!(job.state, "running");
        assert_eq!(job.note, "waiting for CPT");

        set_balance(&state, "job-carol", 1000).await;
        tick(&state, &created.job_id).await;
        assert_eq!(
            states(&items(&service, &token, &created.job_id).await),
            vec!["submitted", "submitted"]
        );
        assert_eq!(job_view(&service, &token, &created.job_id).await.note, "");
        fixture.cleanup().await.ok();
    }

    // The runner advances a bounded batch of active jobs per tick. Idle unfunded jobs must not keep
    // every slot of that batch, so a funded job created after them still advances.
    #[tokio::test]
    async fn a_funded_job_advances_past_persistently_unfunded_older_jobs() {
        let Some((state, fixture)) = fixture("job_scan_fairness").await else {
            return;
        };
        let service = GrpcJobService::new(state.clone());
        // Ten accounts, each running the most jobs one account may hold: 100 idle jobs in all.
        for owner in 0..10 {
            let name = format!("job-idle-{owner}");
            let token = account(&state, &name, 1000).await;
            for _ in 0..JOB_MAX_ACTIVE_PER_OWNER {
                let created = service
                    .create_job(Request::new(create_request(&token, &["1"], 10, 10)))
                    .await
                    .unwrap()
                    .into_inner();
                assert!(created.success, "{}", created.status_message);
            }
            set_balance(&state, &name, 0).await;
        }
        run_job_tick(&state).await.unwrap();

        let funded = account(&state, "job-funded-late", 1000).await;
        let late = service
            .create_job(Request::new(create_request(&funded, &["1"], 10, 10)))
            .await
            .unwrap()
            .into_inner();
        assert!(late.success, "{}", late.status_message);
        for _ in 0..3 {
            run_job_tick(&state).await.unwrap();
        }

        assert_eq!(
            states(&items(&service, &funded, &late.job_id).await),
            vec!["submitted"]
        );
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn an_account_sees_only_its_own_jobs() {
        let Some((state, fixture)) = fixture("job_owners").await else {
            return;
        };
        let alice = account(&state, "job-alice-2", 1000).await;
        let mallory = account(&state, "job-mallory", 1000).await;
        let service = GrpcJobService::new(state.clone());
        let created = service
            .create_job(Request::new(create_request(&alice, &["1"], 10, 10)))
            .await
            .unwrap()
            .into_inner();
        assert!(created.success);

        let peek = service
            .get_job(Request::new(GetJobRequest {
                token: mallory.clone(),
                job_id: created.job_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!peek.success);
        assert!(peek.job.is_none());
        let listed = service
            .list_jobs(Request::new(ListJobsRequest {
                token: mallory.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(listed.jobs.is_empty());
        let stop = service
            .cancel_job(Request::new(CancelJobRequest {
                token: mallory.clone(),
                job_id: created.job_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!stop.success);
        assert!(items(&service, &mallory, &created.job_id).await.is_empty());
        let mine = service
            .list_jobs(Request::new(ListJobsRequest { token: alice }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(mine.jobs.len(), 1);
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn creation_refuses_a_bad_request_or_an_unfunded_account_and_stores_nothing() {
        let Some((state, fixture)) = fixture("job_create_checks").await else {
            return;
        };
        let poor = account(&state, "job-dave", 5).await;
        let rich = account(&state, "job-erin", 1000).await;
        let service = GrpcJobService::new(state.clone());

        let unfunded = service
            .create_job(Request::new(create_request(&poor, &["1"], 10, 10)))
            .await
            .unwrap()
            .into_inner();
        assert!(!unfunded.success);
        assert!(
            unfunded.status_message.starts_with("insufficient balance"),
            "{}",
            unfunded.status_message
        );

        let empty = service
            .create_job(Request::new(create_request(&rich, &[], 10, 10)))
            .await
            .unwrap()
            .into_inner();
        assert!(!empty.success);
        assert!(empty.status_message.contains("at least one input"));

        let listed = service
            .list_jobs(Request::new(ListJobsRequest { token: rich }))
            .await
            .unwrap()
            .into_inner();
        assert!(
            listed.jobs.is_empty(),
            "a refused request leaves no job behind"
        );
        fixture.cleanup().await.ok();
    }
}

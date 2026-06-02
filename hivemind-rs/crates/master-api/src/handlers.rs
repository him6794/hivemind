use axum::{
    body::Body,
    extract::{Multipart, Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use hivemind_auth::AuthManager;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_models::{Task, TaskInfo, TaskStatus};
use hivemind_task_scheduler::TaskScheduler;
use sqlx::PgPool;
use hivemind_torrent_service::TorrentService;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio_util::io::ReaderStream;

use crate::middleware::AuthClaims;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseManager,
    pub auth: AuthManager,
    pub scheduler: TaskScheduler,
    pub nodepool_grpc_addr: String,
    pub config: HivemindConfig,
}

// --- Request/Response types ---

#[derive(Debug, Deserialize)]
pub struct LoginBody {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub message: String,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskBody {
    pub task_id: String,
    pub torrent: Option<String>,
    pub zip_path: Option<String>,
    pub memory_gb: Option<i32>,
    pub cpu_score: Option<i32>,
    pub gpu_score: Option<i32>,
    pub gpu_memory_gb: Option<i32>,
    pub storage_gb: Option<i64>,
    pub location: Option<String>,
    pub host_count: Option<i32>,
    pub max_cpt: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PricingBreakdown {
    pub base: i64,
    pub cpu: i64,
    pub gpu: i64,
    pub memory: i64,
    pub gpu_memory: i64,
    pub storage: i64,
    pub host_count: i64,
    pub per_host_total: i64,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct QuoteResponse {
    pub success: bool,
    pub quoted_cpt: i64,
    pub currency: &'static str,
    pub breakdown: PricingBreakdown,
}

#[derive(Debug, Deserialize)]
pub struct RegisterWorkerBody {
    pub username: Option<String>,
    pub ip: String,
    pub cpu_cores: i32,
    pub memory_gb: i32,
    pub cpu_score: i32,
    pub gpu_score: Option<i32>,
    pub gpu_memory_gb: Option<i32>,
    pub location: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RemoveWorkerBody {
    pub worker_id: String,
}

struct TaskDistribution {
    torrent_source: Option<String>,
    expected_btih: Option<String>,
}

fn bad_task_response(message: impl Into<String>) -> (StatusCode, Json<TaskResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(TaskResponse {
            success: false,
            message: message.into(),
            task: None,
        }),
    )
}

fn budget_guard_response(quoted_cpt: i64, max_cpt: i64) -> (StatusCode, Json<TaskResponse>) {
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(TaskResponse {
            success: false,
            message: format!(
                "quote exceeds max_cpt: quoted_cpt={} max_cpt={}",
                quoted_cpt, max_cpt
            ),
            task: None,
        }),
    )
}

fn task_pricing_quote(body: &CreateTaskBody) -> PricingBreakdown {
    let base = 10;
    let cpu = i64::from(body.cpu_score.unwrap_or(0).max(0)) / 100;
    let gpu = i64::from(body.gpu_score.unwrap_or(0).max(0)) / 50;
    let memory = i64::from(body.memory_gb.unwrap_or(0).max(0)) * 2;
    let gpu_memory = i64::from(body.gpu_memory_gb.unwrap_or(0).max(0)) * 3;
    let storage = body.storage_gb.unwrap_or(0).max(0) / 10;
    let host_count = i64::from(body.host_count.unwrap_or(1).max(1));
    let per_host_total = (base + cpu + gpu + memory + gpu_memory + storage).max(1);
    let total = (per_host_total * host_count).max(1);

    PricingBreakdown {
        base,
        cpu,
        gpu,
        memory,
        gpu_memory,
        storage,
        host_count,
        per_host_total,
        total,
    }
}

fn is_safe_task_id(task_id: &str) -> bool {
    !task_id.trim().is_empty()
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !task_id.contains("..")
}

fn task_submit_limit_per_minute() -> i64 {
    std::env::var("HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(60)
        .max(0)
}

fn parse_optional_i32(name: &str, value: &str) -> anyhow::Result<i32> {
    value
        .trim()
        .parse::<i32>()
        .map_err(|e| anyhow::anyhow!("Invalid {}: {}", name, e))
}

fn parse_optional_i64(name: &str, value: &str) -> anyhow::Result<i64> {
    value
        .trim()
        .parse::<i64>()
        .map_err(|e| anyhow::anyhow!("Invalid {}: {}", name, e))
}

fn set_upload_text_field(body: &mut CreateTaskBody, name: &str, value: &str) -> anyhow::Result<()> {
    match name {
        "task_id" => body.task_id = value.trim().to_string(),
        "memory_gb" => body.memory_gb = Some(parse_optional_i32(name, value)?),
        "cpu_score" => body.cpu_score = Some(parse_optional_i32(name, value)?),
        "gpu_score" => body.gpu_score = Some(parse_optional_i32(name, value)?),
        "gpu_memory_gb" => body.gpu_memory_gb = Some(parse_optional_i32(name, value)?),
        "storage_gb" => body.storage_gb = Some(parse_optional_i64(name, value)?),
        "host_count" => body.host_count = Some(parse_optional_i32(name, value)?),
        "max_cpt" => body.max_cpt = Some(parse_optional_i64(name, value)?),
        _ => {}
    }
    Ok(())
}

fn is_admin_user(username: &str) -> bool {
    let configured = std::env::var("HIVEMIND_ADMIN_USERS").unwrap_or_else(|_| "testuser".into());
    configured
        .split(',')
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .any(|admin| admin == username)
}

fn task_upload_path(config: &HivemindConfig, task_id: &str) -> PathBuf {
    PathBuf::from(&config.torrent.api_dir)
        .join("uploads")
        .join(format!("{}.zip", task_id))
}

async fn resolve_task_distribution(
    body: &CreateTaskBody,
    config: &HivemindConfig,
) -> anyhow::Result<TaskDistribution> {
    if let Some(zip_path) = body
        .zip_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        let torrent = TorrentService::new(config);
        let info = torrent
            .zip_to_torrent(Path::new(zip_path), &config.torrent.announce_url)
            .await?;
        let name = Path::new(zip_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&body.task_id);
        return Ok(TaskDistribution {
            torrent_source: Some(torrent.magnet_uri(&info.info_hash, name)),
            expected_btih: Some(info.info_hash),
        });
    }

    Ok(TaskDistribution {
        torrent_source: body.torrent.clone(),
        expected_btih: None,
    })
}

#[cfg(test)]
mod torrent_pipeline_tests {
    use super::*;

    #[tokio::test]
    async fn zip_path_is_converted_to_magnet_and_btih() {
        let tmp = tempfile::TempDir::new().unwrap();
        let zip_path = tmp.path().join("task.zip");
        std::fs::write(&zip_path, b"task-package").unwrap();

        let mut config = hivemind_config::HivemindConfig::default();
        config.torrent.api_dir = tmp.path().join("api").display().to_string();
        config.torrent.bt_dir = tmp.path().join("bt").display().to_string();

        let body = CreateTaskBody {
            task_id: "task-with-zip".into(),
            torrent: None,
            zip_path: Some(zip_path.display().to_string()),
            memory_gb: None,
            cpu_score: None,
            gpu_score: None,
            gpu_memory_gb: None,
            storage_gb: None,
            location: None,
            host_count: None,
            max_cpt: None,
        };

        let payload = resolve_task_distribution(&body, &config)
            .await
            .expect("zip should be converted to torrent");

        assert!(payload
            .torrent_source
            .as_deref()
            .unwrap_or_default()
            .starts_with("magnet:?xt=urn:btih:"));
        assert!(payload.expected_btih.is_some());
        assert!(tmp.path().join("api").join("task.zip").exists());
        assert!(std::fs::read_dir(tmp.path().join("bt"))
            .unwrap()
            .next()
            .is_some());
    }
}

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub success: bool,
    pub message: String,
    pub task: Option<TaskInfo>,
}

#[derive(Debug, Serialize)]
pub struct TaskListResponse {
    pub success: bool,
    pub tasks: Vec<TaskInfo>,
}

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub success: bool,
    pub balance: i64,
}

#[derive(Debug, Deserialize)]
pub struct ProviderEarningsQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ProviderEarningsEntry {
    pub task_id: String,
    pub payer_user: String,
    pub provider_worker_id: Option<String>,
    pub amount_cpt: i64,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct ProviderEarningsEntryRow {
    task_id: String,
    payer_user: String,
    provider_worker_id: Option<String>,
    amount_cpt: i64,
    status: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct ProviderEarningsResponse {
    pub success: bool,
    pub total_earned_cpt: i64,
    pub currency: &'static str,
    pub entries: Vec<ProviderEarningsEntry>,
}

#[derive(Debug, Serialize)]
pub struct AdminBillingOverviewResponse {
    pub success: bool,
    pub total_payer_debit_cpt: i64,
    pub total_provider_credit_cpt: i64,
    pub total_platform_fee_cpt: i64,
    pub pending_billing_tasks: i64,
    pub currency: &'static str,
}

#[derive(Debug, Serialize)]
pub struct AdminArtifactOverviewResponse {
    pub success: bool,
    pub total_artifacts: i64,
    pub total_size_bytes: i64,
    pub dedup_hits: i64,
    pub resumable_artifacts: i64,
    pub expiring_in_24h: i64,
}

#[derive(Debug, Deserialize)]
pub struct AdminArtifactCleanupBody {
    pub dry_run: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct AdminArtifactCleanupResponse {
    pub success: bool,
    pub dry_run: bool,
    pub expired_candidates: i64,
    pub deleted_rows: i64,
    pub deleted_files: i64,
    pub file_delete_errors: i64,
}

#[derive(Debug, Serialize)]
pub struct WorkerCacheAffinityMetric {
    pub worker_id: String,
    pub completed_tasks: i64,
    pub cache_hits: i64,
    pub recent_completed_tasks_7d: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminSchedulingCacheMetricsResponse {
    pub success: bool,
    pub total_completed_tasks: i64,
    pub total_cache_hits: i64,
    pub cache_hit_rate: f64,
    pub top_workers: Vec<WorkerCacheAffinityMetric>,
}

#[derive(Debug, Deserialize)]
pub struct AdminSchedulingCacheAlertQuery {
    pub low: Option<f64>,
    pub high: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AdminSchedulingCacheAlertResponse {
    pub success: bool,
    pub low_threshold: f64,
    pub high_threshold: f64,
    pub cache_hit_rate: f64,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct AdminSchedulingCacheAnomalyQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdminSchedulingCacheAnomalyEntry {
    pub severity: String,
    pub cache_hit_rate: f64,
    pub low_threshold: f64,
    pub high_threshold: f64,
    pub message: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct AdminSchedulingCacheAnomalyListResponse {
    pub success: bool,
    pub entries: Vec<AdminSchedulingCacheAnomalyEntry>,
}

#[derive(Debug, Deserialize)]
pub struct AdminAuditLogQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdminAuditLogEntry {
    pub admin_user: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub detail: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct AdminAuditLogListResponse {
    pub success: bool,
    pub entries: Vec<AdminAuditLogEntry>,
}

async fn record_admin_audit_log(
    pool: &PgPool,
    admin_user: &str,
    action: &str,
    target_type: &str,
    target_id: &str,
    detail: serde_json::Value,
) {
    if let Err(e) = sqlx::query(
        "INSERT INTO admin_audit_logs (admin_user, action, target_type, target_id, detail)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(admin_user)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(detail)
    .execute(pool)
    .await
    {
        tracing::warn!("Failed to record admin audit log: {}", e);
    }
}

async fn record_cache_alert_anomaly(
    pool: &PgPool,
    severity: &str,
    cache_hit_rate: f64,
    low_threshold: f64,
    high_threshold: f64,
    message: &str,
) {
    if let Err(e) = sqlx::query(
        "INSERT INTO cache_alert_anomalies
         (severity, cache_hit_rate, low_threshold, high_threshold, message)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(severity)
    .bind(cache_hit_rate)
    .bind(low_threshold)
    .bind(high_threshold)
    .bind(message)
    .execute(pool)
    .await
    {
        tracing::warn!("Failed to record cache alert anomaly: {}", e);
    }
}

pub async fn cleanup_expired_artifacts(
    pool: &PgPool,
    dry_run: bool,
) -> anyhow::Result<AdminArtifactCleanupResponse> {
    let expired: Vec<(String, String)> = sqlx::query_as(
        "SELECT artifact_key, storage_path
         FROM artifacts
         WHERE expires_at IS NOT NULL AND expires_at <= NOW()",
    )
    .fetch_all(pool)
    .await?;

    let expired_candidates = expired.len() as i64;
    if dry_run {
        return Ok(AdminArtifactCleanupResponse {
            success: true,
            dry_run: true,
            expired_candidates,
            deleted_rows: 0,
            deleted_files: 0,
            file_delete_errors: 0,
        });
    }

    let mut deleted_files = 0i64;
    let mut file_delete_errors = 0i64;
    for (_, storage_path) in &expired {
        let path = std::path::Path::new(storage_path);
        if !path.exists() {
            continue;
        }
        match std::fs::remove_file(path) {
            Ok(_) => deleted_files += 1,
            Err(e) => {
                tracing::warn!("Failed to delete artifact file {}: {}", storage_path, e);
                file_delete_errors += 1;
            }
        }
    }

    let keys: Vec<String> = expired.into_iter().map(|(key, _)| key).collect();
    let deleted_rows = if keys.is_empty() {
        0
    } else {
        sqlx::query("DELETE FROM artifacts WHERE artifact_key = ANY($1)")
            .bind(&keys)
            .execute(pool)
            .await?
            .rows_affected() as i64
    };

    Ok(AdminArtifactCleanupResponse {
        success: true,
        dry_run: false,
        expired_candidates,
        deleted_rows,
        deleted_files,
        file_delete_errors,
    })
}

#[derive(Debug, Deserialize)]
pub struct ProviderWorkerSettingsBody {
    pub enabled: bool,
    pub cpu_cores_limit: i32,
    pub memory_gb_limit: i32,
    pub gpu_memory_gb_limit: i32,
    pub storage_gb_limit: i64,
    pub min_cpt_per_hour: i64,
}

#[derive(Debug, Serialize)]
pub struct ProviderWorkerSettings {
    pub enabled: bool,
    pub cpu_cores_limit: i32,
    pub memory_gb_limit: i32,
    pub gpu_memory_gb_limit: i32,
    pub storage_gb_limit: i64,
    pub min_cpt_per_hour: i64,
}

#[derive(Debug, Serialize)]
pub struct ProviderWorkerSettingsResponse {
    pub success: bool,
    pub worker_id: String,
    pub message: String,
    pub settings: Option<ProviderWorkerSettings>,
}

#[derive(Debug, Serialize)]
pub struct WorkerTrustProfile {
    pub worker_id: String,
    pub successful_tasks: i64,
    pub failed_tasks: i64,
    pub score: i32,
    pub banned: bool,
    pub last_attested_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
pub struct WorkerTrustProfileResponse {
    pub success: bool,
    pub message: String,
    pub trust: Option<WorkerTrustProfile>,
}

#[derive(Debug, Deserialize)]
pub struct WorkerTrustControlBody {
    pub banned: bool,
    pub score: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct WorkerTrustControlResponse {
    pub success: bool,
    pub worker_id: String,
    pub banned: bool,
    pub score: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct AdminWorkerTrustEntry {
    pub worker_id: String,
    pub username: String,
    pub worker_status: String,
    pub score: i32,
    pub banned: bool,
    pub successful_tasks: i64,
    pub failed_tasks: i64,
    pub last_attested_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AdminWorkerTrustListResponse {
    pub success: bool,
    pub entries: Vec<AdminWorkerTrustEntry>,
}

#[derive(Debug, Serialize)]
pub struct WorkerListResponse {
    pub success: bool,
    pub workers: Vec<WorkerInfo>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub success: bool,
    pub status_message: String,
}

#[derive(Debug, Serialize)]
pub struct WorkerInfo {
    pub id: String,
    pub worker_id: String,
    pub addr: String,
    pub ip: String,
    pub status: String,
    pub cpu_cores: i32,
    pub memory_gb: i32,
    pub cpu_score: i32,
    pub gpu_score: i32,
    pub gpu_memory_gb: i32,
    pub provider_enabled: bool,
    pub cpu_cores_limit: i32,
    pub memory_gb_limit: i32,
    pub gpu_memory_gb_limit: i32,
    pub storage_gb_limit: i64,
    pub min_cpt_per_hour: i64,
    pub location: String,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub gpu_memory_usage: f64,
}

/// POST /api/login
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> (StatusCode, Json<LoginResponse>) {
    match state
        .auth
        .authenticate(&body.username, &body.password)
        .await
    {
        Ok(Some(token)) => (
            StatusCode::OK,
            Json(LoginResponse {
                success: true,
                message: "Login successful".into(),
                token: Some(token),
            }),
        ),
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                message: "Invalid credentials".into(),
                token: None,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse {
                success: false,
                message: format!("Error: {}", e),
                token: None,
            }),
        ),
    }
}

/// POST /api/tasks — Master submits a task to the nodepool
pub async fn create_task(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    Json(body): Json<CreateTaskBody>,
) -> (StatusCode, Json<TaskResponse>) {
    create_task_from_body(state, claims.sub, body).await
}

/// POST /api/tasks/quote - Return deterministic MVP pricing before task submission.
pub async fn quote_task(
    AuthClaims(_claims): AuthClaims,
    Json(body): Json<CreateTaskBody>,
) -> (StatusCode, Json<QuoteResponse>) {
    let breakdown = task_pricing_quote(&body);
    (
        StatusCode::OK,
        Json(QuoteResponse {
            success: true,
            quoted_cpt: breakdown.total,
            currency: "CPT",
            breakdown,
        }),
    )
}

async fn create_task_from_body(
    state: AppState,
    owner: String,
    body: CreateTaskBody,
) -> (StatusCode, Json<TaskResponse>) {
    let per_minute_limit = task_submit_limit_per_minute();
    if per_minute_limit > 0 {
        let recent_submit_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT
             FROM tasks
             WHERE owner = $1
               AND created_at >= NOW() - INTERVAL '1 minute'",
        )
        .bind(&owner)
        .fetch_one(&state.db.pool)
        .await
        .unwrap_or(0);

        if recent_submit_count >= per_minute_limit {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(TaskResponse {
                    success: false,
                    message: format!(
                        "submission rate limit exceeded: {} tasks/min",
                        per_minute_limit
                    ),
                    task: None,
                }),
            );
        }
    }

    let quote = task_pricing_quote(&body).total;
    if let Some(max_cpt) = body.max_cpt {
        if max_cpt < quote {
            return budget_guard_response(quote, max_cpt);
        }
    }

    let distribution = match resolve_task_distribution(&body, &state.config).await {
        Ok(distribution) => distribution,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(TaskResponse {
                    success: false,
                    message: format!("Failed to prepare task package: {}", e),
                    task: None,
                }),
            );
        }
    };

    let task = Task {
        id: uuid::Uuid::new_v4(),
        task_id: body.task_id,
        owner: owner.clone(),
        worker_id: None,
        worker_ip: None,
        status: TaskStatus::Pending,
        status_message: None,
        output: None,
        result_torrent: None,
        torrent_source: distribution.torrent_source,
        expected_btih: distribution.expected_btih,
        cpu_usage: 0.0,
        memory_usage: 0.0,
        gpu_usage: 0.0,
        gpu_memory_usage: 0.0,
        req_cpu_score: body.cpu_score.unwrap_or(0),
        req_gpu_score: body.gpu_score.unwrap_or(0),
        req_memory_gb: body.memory_gb.unwrap_or(0),
        req_gpu_memory_gb: body.gpu_memory_gb.unwrap_or(0),
        req_storage_gb: body.storage_gb.unwrap_or(0),
        host_count: body.host_count.unwrap_or(1),
        max_cpt: quote,
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
        created_at: chrono::Utc::now(),
        last_update: chrono::Utc::now(),
        completed_at: None,
    };

    match state.scheduler.create_task(&task).await {
        Ok(t) => {
            tracing::info!("Master submitted task {} for user {}", t.task_id, owner);
            (
                StatusCode::CREATED,
                Json(TaskResponse {
                    success: true,
                    message: format!("Task {} created", t.task_id),
                    task: Some(t.into()),
                }),
            )
        }
        Err(e) => {
            let message = e.to_string();
            let status = if message.contains("duplicate key")
                || message.contains("unique constraint")
                || message.contains("tasks_task_id_key")
            {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(TaskResponse {
                    success: false,
                    message: format!("Failed: {}", e),
                    task: None,
                }),
            )
        }
    }
}

/// POST /api/tasks/upload - Browser-friendly ZIP task upload.
pub async fn upload_task(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    mut multipart: Multipart,
) -> (StatusCode, Json<TaskResponse>) {
    let mut body = CreateTaskBody {
        task_id: String::new(),
        torrent: None,
        zip_path: None,
        memory_gb: None,
        cpu_score: None,
        gpu_score: None,
        gpu_memory_gb: None,
        storage_gb: None,
        location: None,
        host_count: None,
        max_cpt: None,
    };
    let mut file_bytes = None;
    let mut file_name = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => return bad_task_response(format!("Invalid multipart payload: {}", e)),
        };

        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            file_name = field.file_name().map(|value| value.to_string());
            match field.bytes().await {
                Ok(bytes) if !bytes.is_empty() => file_bytes = Some(bytes),
                Ok(_) => return bad_task_response("file is required"),
                Err(e) => return bad_task_response(format!("Failed to read file: {}", e)),
            }
        } else {
            match field.text().await {
                Ok(value) => {
                    if let Err(e) = set_upload_text_field(&mut body, &name, &value) {
                        return bad_task_response(e.to_string());
                    }
                }
                Err(e) => {
                    return bad_task_response(format!("Failed to read field {}: {}", name, e))
                }
            }
        }
    }

    if !is_safe_task_id(&body.task_id) {
        return bad_task_response("task_id is required and must be a safe file name");
    }

    if let Some(name) = file_name.as_deref() {
        if !name.to_ascii_lowercase().ends_with(".zip") {
            return bad_task_response("file must be a .zip");
        }
    }

    let Some(file_bytes) = file_bytes else {
        return bad_task_response("file is required");
    };

    let zip_path = task_upload_path(&state.config, &body.task_id);
    if let Some(parent) = zip_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return bad_task_response(format!("Failed to create upload directory: {}", e));
        }
    }
    if let Err(e) = std::fs::write(&zip_path, &file_bytes) {
        return bad_task_response(format!("Failed to save uploaded file: {}", e));
    }
    body.zip_path = Some(zip_path.display().to_string());

    create_task_from_body(state, claims.sub, body).await
}

/// GET /api/tasks — List all tasks for the authenticated user
pub async fn list_tasks(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
) -> (StatusCode, Json<TaskListResponse>) {
    match state.scheduler.list_user_tasks(&claims.sub).await {
        Ok(tasks) => {
            let infos: Vec<TaskInfo> = tasks.into_iter().map(TaskInfo::from).collect();
            (
                StatusCode::OK,
                Json(TaskListResponse {
                    success: true,
                    tasks: infos,
                }),
            )
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(TaskListResponse {
                success: false,
                tasks: vec![],
            }),
        ),
    }
}

pub async fn get_balance(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
) -> (StatusCode, Json<BalanceResponse>) {
    match sqlx::query_scalar::<_, i64>("SELECT balance FROM users WHERE username = $1")
        .bind(&claims.sub)
        .fetch_optional(&state.db.pool)
        .await
    {
        Ok(balance) => (
            StatusCode::OK,
            Json(BalanceResponse {
                success: true,
                balance: balance.unwrap_or(0),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to get balance: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(BalanceResponse {
                    success: false,
                    balance: 0,
                }),
            )
        }
    }
}

pub async fn get_provider_earnings(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    Query(query): Query<ProviderEarningsQuery>,
) -> (StatusCode, Json<ProviderEarningsResponse>) {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);

    let total_result = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(amount_cpt), 0)::BIGINT
         FROM ledger_entries
         WHERE provider_user = $1 AND kind = 'provider_credit'",
    )
    .bind(&claims.sub)
    .fetch_one(&state.db.pool)
    .await;

    let total_earned_cpt = match total_result {
        Ok(total) => total,
        Err(e) => {
            tracing::error!("Failed to calculate provider earnings total: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProviderEarningsResponse {
                    success: false,
                    total_earned_cpt: 0,
                    currency: "CPT",
                    entries: vec![],
                }),
            );
        }
    };

    match sqlx::query_as::<_, ProviderEarningsEntryRow>(
        "SELECT task_id, payer_user, provider_worker_id, amount_cpt, status, created_at
         FROM ledger_entries
         WHERE provider_user = $1 AND kind = 'provider_credit'
         ORDER BY created_at DESC
         LIMIT $2",
    )
    .bind(&claims.sub)
    .bind(limit)
    .fetch_all(&state.db.pool)
    .await
    {
        Ok(entries) => (
            StatusCode::OK,
            Json(ProviderEarningsResponse {
                success: true,
                total_earned_cpt,
                currency: "CPT",
                entries: entries
                    .into_iter()
                    .map(|entry| ProviderEarningsEntry {
                        task_id: entry.task_id,
                        payer_user: entry.payer_user,
                        provider_worker_id: entry.provider_worker_id,
                        amount_cpt: entry.amount_cpt,
                        status: entry.status,
                        created_at: entry.created_at,
                    })
                    .collect(),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to list provider earnings: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProviderEarningsResponse {
                    success: false,
                    total_earned_cpt: 0,
                    currency: "CPT",
                    entries: vec![],
                }),
            )
        }
    }
}

pub async fn get_admin_billing_overview(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
) -> (StatusCode, Json<AdminBillingOverviewResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(AdminBillingOverviewResponse {
                success: false,
                total_payer_debit_cpt: 0,
                total_provider_credit_cpt: 0,
                total_platform_fee_cpt: 0,
                pending_billing_tasks: 0,
                currency: "CPT",
            }),
        );
    }

    let totals = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
            COALESCE(SUM(CASE WHEN kind = 'payer_debit' THEN amount_cpt ELSE 0 END), 0)::BIGINT,
            COALESCE(SUM(CASE WHEN kind = 'provider_credit' THEN amount_cpt ELSE 0 END), 0)::BIGINT,
            COALESCE(SUM(CASE WHEN kind = 'platform_fee' THEN amount_cpt ELSE 0 END), 0)::BIGINT
         FROM ledger_entries
         WHERE status = 'settled'",
    )
    .fetch_one(&state.db.pool)
    .await;

    let pending = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::BIGINT FROM tasks WHERE status = 'COMPLETED' AND billing_settled = false",
    )
    .fetch_one(&state.db.pool)
    .await;

    match (totals, pending) {
        (Ok((payer, provider, platform_fee)), Ok(pending_billing_tasks)) => (
            StatusCode::OK,
            Json(AdminBillingOverviewResponse {
                success: true,
                total_payer_debit_cpt: payer,
                total_provider_credit_cpt: provider,
                total_platform_fee_cpt: platform_fee,
                pending_billing_tasks,
                currency: "CPT",
            }),
        ),
        (Err(e), _) | (_, Err(e)) => {
            tracing::error!("Failed to load admin billing overview: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminBillingOverviewResponse {
                    success: false,
                    total_payer_debit_cpt: 0,
                    total_provider_credit_cpt: 0,
                    total_platform_fee_cpt: 0,
                    pending_billing_tasks: 0,
                    currency: "CPT",
                }),
            )
        }
    }
}

pub async fn get_admin_artifact_overview(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
) -> (StatusCode, Json<AdminArtifactOverviewResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(AdminArtifactOverviewResponse {
                success: false,
                total_artifacts: 0,
                total_size_bytes: 0,
                dedup_hits: 0,
                resumable_artifacts: 0,
                expiring_in_24h: 0,
            }),
        );
    }

    match sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        "SELECT
            COUNT(*)::BIGINT,
            COALESCE(SUM(size_bytes), 0)::BIGINT,
            COALESCE(SUM(CASE WHEN dedup_hit THEN 1 ELSE 0 END), 0)::BIGINT,
            COALESCE(SUM(CASE WHEN resume_supported THEN 1 ELSE 0 END), 0)::BIGINT,
            COALESCE(SUM(CASE
                WHEN expires_at IS NOT NULL AND expires_at <= NOW() + INTERVAL '24 hours' THEN 1
                ELSE 0
            END), 0)::BIGINT
         FROM artifacts",
    )
    .fetch_one(&state.db.pool)
    .await
    {
        Ok((total_artifacts, total_size_bytes, dedup_hits, resumable_artifacts, expiring_in_24h)) => (
            StatusCode::OK,
            Json(AdminArtifactOverviewResponse {
                success: true,
                total_artifacts,
                total_size_bytes,
                dedup_hits,
                resumable_artifacts,
                expiring_in_24h,
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to load admin artifact overview: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminArtifactOverviewResponse {
                    success: false,
                    total_artifacts: 0,
                    total_size_bytes: 0,
                    dedup_hits: 0,
                    resumable_artifacts: 0,
                    expiring_in_24h: 0,
                }),
            )
        }
    }
}

pub async fn get_admin_scheduling_cache_metrics(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
) -> (StatusCode, Json<AdminSchedulingCacheMetricsResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(AdminSchedulingCacheMetricsResponse {
                success: false,
                total_completed_tasks: 0,
                total_cache_hits: 0,
                cache_hit_rate: 0.0,
                top_workers: vec![],
            }),
        );
    }

    let totals = sqlx::query_as::<_, (i64, i64)>(
        "SELECT
            COUNT(*)::BIGINT AS total_completed_tasks,
            COALESCE(SUM(cache_hits), 0)::BIGINT AS total_cache_hits
         FROM tasks
         WHERE status = 'COMPLETED'",
    )
    .fetch_one(&state.db.pool)
    .await;

    let top_workers = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT
            worker_id,
            COUNT(*)::BIGINT AS completed_tasks,
            COALESCE(SUM(cache_hits), 0)::BIGINT AS cache_hits,
            COALESCE(SUM(CASE
                WHEN completed_at IS NOT NULL AND completed_at >= NOW() - INTERVAL '7 days' THEN 1
                ELSE 0
            END), 0)::BIGINT AS recent_completed_tasks_7d
         FROM tasks
         WHERE status = 'COMPLETED'
           AND worker_id IS NOT NULL
         GROUP BY worker_id
         ORDER BY cache_hits DESC, completed_tasks DESC, worker_id ASC
         LIMIT 20",
    )
    .fetch_all(&state.db.pool)
    .await;

    match (totals, top_workers) {
        (Ok((total_completed_tasks, total_cache_hits)), Ok(rows)) => {
            let cache_hit_rate = if total_completed_tasks > 0 {
                total_cache_hits as f64 / total_completed_tasks as f64
            } else {
                0.0
            };
            (
                StatusCode::OK,
                Json(AdminSchedulingCacheMetricsResponse {
                    success: true,
                    total_completed_tasks,
                    total_cache_hits,
                    cache_hit_rate,
                    top_workers: rows
                        .into_iter()
                        .map(
                            |(worker_id, completed_tasks, cache_hits, recent_completed_tasks_7d)| {
                                WorkerCacheAffinityMetric {
                                    worker_id,
                                    completed_tasks,
                                    cache_hits,
                                    recent_completed_tasks_7d,
                                }
                            },
                        )
                        .collect(),
                }),
            )
        }
        (Err(e), _) | (_, Err(e)) => {
            tracing::error!("Failed to load scheduling cache metrics: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminSchedulingCacheMetricsResponse {
                    success: false,
                    total_completed_tasks: 0,
                    total_cache_hits: 0,
                    cache_hit_rate: 0.0,
                    top_workers: vec![],
                }),
            )
        }
    }
}

pub async fn get_admin_scheduling_cache_alert(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    Query(query): Query<AdminSchedulingCacheAlertQuery>,
) -> (StatusCode, Json<AdminSchedulingCacheAlertResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(AdminSchedulingCacheAlertResponse {
                success: false,
                low_threshold: query.low.unwrap_or(0.3),
                high_threshold: query.high.unwrap_or(3.0),
                cache_hit_rate: 0.0,
                severity: "forbidden".into(),
                message: "admin access required".into(),
            }),
        );
    }

    let low_threshold = query.low.unwrap_or(0.3);
    let high_threshold = query.high.unwrap_or(3.0);
    if !(low_threshold.is_finite() && high_threshold.is_finite()) || low_threshold >= high_threshold {
        return (
            StatusCode::BAD_REQUEST,
            Json(AdminSchedulingCacheAlertResponse {
                success: false,
                low_threshold,
                high_threshold,
                cache_hit_rate: 0.0,
                severity: "invalid_threshold".into(),
                message: "invalid threshold range; require finite values and low < high".into(),
            }),
        );
    }

    let totals = sqlx::query_as::<_, (i64, i64)>(
        "SELECT
            COUNT(*)::BIGINT AS total_completed_tasks,
            COALESCE(SUM(cache_hits), 0)::BIGINT AS total_cache_hits
         FROM tasks
         WHERE status = 'COMPLETED'",
    )
    .fetch_one(&state.db.pool)
    .await;

    match totals {
        Ok((total_completed_tasks, total_cache_hits)) => {
            let cache_hit_rate = if total_completed_tasks > 0 {
                total_cache_hits as f64 / total_completed_tasks as f64
            } else {
                0.0
            };

            let (severity, message) = if cache_hit_rate < low_threshold {
                (
                    "low".to_string(),
                    "cache hit rate is below the configured low threshold".to_string(),
                )
            } else if cache_hit_rate > high_threshold {
                (
                    "high".to_string(),
                    "cache hit rate is above the configured high threshold".to_string(),
                )
            } else {
                (
                    "normal".to_string(),
                    "cache hit rate is within configured thresholds".to_string(),
                )
            };

            if severity == "low" || severity == "high" {
                record_cache_alert_anomaly(
                    &state.db.pool,
                    &severity,
                    cache_hit_rate,
                    low_threshold,
                    high_threshold,
                    &message,
                )
                .await;
            }

            (
                StatusCode::OK,
                Json(AdminSchedulingCacheAlertResponse {
                    success: true,
                    low_threshold,
                    high_threshold,
                    cache_hit_rate,
                    severity,
                    message,
                }),
            )
        }
        Err(e) => {
            tracing::error!("Failed to calculate scheduling cache alert: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminSchedulingCacheAlertResponse {
                    success: false,
                    low_threshold,
                    high_threshold,
                    cache_hit_rate: 0.0,
                    severity: "error".into(),
                    message: e.to_string(),
                }),
            )
        }
    }
}

pub async fn list_admin_scheduling_cache_anomalies(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    Query(query): Query<AdminSchedulingCacheAnomalyQuery>,
) -> (StatusCode, Json<AdminSchedulingCacheAnomalyListResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(AdminSchedulingCacheAnomalyListResponse {
                success: false,
                entries: vec![],
            }),
        );
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    match sqlx::query_as::<_, (String, f64, f64, f64, String, chrono::DateTime<chrono::Utc>)>(
        "SELECT severity, cache_hit_rate, low_threshold, high_threshold, message, created_at
         FROM cache_alert_anomalies
         ORDER BY created_at DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db.pool)
    .await
    {
        Ok(rows) => (
            StatusCode::OK,
            Json(AdminSchedulingCacheAnomalyListResponse {
                success: true,
                entries: rows
                    .into_iter()
                    .map(
                        |(
                            severity,
                            cache_hit_rate,
                            low_threshold,
                            high_threshold,
                            message,
                            created_at,
                        )| AdminSchedulingCacheAnomalyEntry {
                            severity,
                            cache_hit_rate,
                            low_threshold,
                            high_threshold,
                            message,
                            created_at,
                        },
                    )
                    .collect(),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to list scheduling cache anomalies: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminSchedulingCacheAnomalyListResponse {
                    success: false,
                    entries: vec![],
                }),
            )
        }
    }
}

pub async fn cleanup_admin_artifacts(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    Json(body): Json<AdminArtifactCleanupBody>,
) -> (StatusCode, Json<AdminArtifactCleanupResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(AdminArtifactCleanupResponse {
                success: false,
                dry_run: body.dry_run.unwrap_or(true),
                expired_candidates: 0,
                deleted_rows: 0,
                deleted_files: 0,
                file_delete_errors: 0,
            }),
        );
    }

    let dry_run = body.dry_run.unwrap_or(true);
    match cleanup_expired_artifacts(&state.db.pool, dry_run).await {
        Ok(summary) => {
            let detail = serde_json::json!({
                "dry_run": summary.dry_run,
                "expired_candidates": summary.expired_candidates,
                "deleted_rows": summary.deleted_rows,
                "deleted_files": summary.deleted_files,
                "file_delete_errors": summary.file_delete_errors,
            });
            record_admin_audit_log(
                &state.db.pool,
                &claims.sub,
                "artifact_cleanup",
                "artifact",
                "",
                detail,
            )
            .await;
            (StatusCode::OK, Json(summary))
        }
        Err(e) => {
            tracing::error!("Artifact cleanup failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminArtifactCleanupResponse {
                    success: false,
                    dry_run,
                    expired_candidates: 0,
                    deleted_rows: 0,
                    deleted_files: 0,
                    file_delete_errors: 0,
                }),
            )
        }
    }
}

fn settings_from_worker(worker: &hivemind_models::WorkerNode) -> ProviderWorkerSettings {
    ProviderWorkerSettings {
        enabled: worker.provider_enabled,
        cpu_cores_limit: worker.cpu_cores_limit,
        memory_gb_limit: worker.memory_gb_limit,
        gpu_memory_gb_limit: worker.gpu_memory_gb_limit,
        storage_gb_limit: worker.storage_gb_limit,
        min_cpt_per_hour: worker.min_cpt_per_hour,
    }
}

async fn provider_owned_worker(
    state: &AppState,
    username: &str,
    worker_id: &str,
) -> Result<hivemind_models::WorkerNode, (StatusCode, Json<ProviderWorkerSettingsResponse>)> {
    match sqlx::query_as::<_, hivemind_models::WorkerNode>(
        "SELECT * FROM worker_nodes WHERE worker_id = $1",
    )
    .bind(worker_id)
    .fetch_optional(&state.db.pool)
    .await
    {
        Ok(Some(worker)) if worker.username == username => Ok(worker),
        Ok(Some(_)) => Err((
            StatusCode::FORBIDDEN,
            Json(ProviderWorkerSettingsResponse {
                success: false,
                worker_id: worker_id.into(),
                message: "worker does not belong to authenticated provider".into(),
                settings: None,
            }),
        )),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ProviderWorkerSettingsResponse {
                success: false,
                worker_id: worker_id.into(),
                message: "worker not found".into(),
                settings: None,
            }),
        )),
        Err(e) => {
            tracing::error!("Failed to load provider worker settings: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProviderWorkerSettingsResponse {
                    success: false,
                    worker_id: worker_id.into(),
                    message: e.to_string(),
                    settings: None,
                }),
            ))
        }
    }
}

fn validate_provider_settings(
    worker: &hivemind_models::WorkerNode,
    body: &ProviderWorkerSettingsBody,
) -> Option<String> {
    if body.cpu_cores_limit < 0
        || body.memory_gb_limit < 0
        || body.gpu_memory_gb_limit < 0
        || body.storage_gb_limit < 0
        || body.min_cpt_per_hour < 0
    {
        return Some("limits and minimum price must be non-negative".into());
    }
    if body.cpu_cores_limit > worker.cpu_cores {
        return Some("cpu_cores_limit exceeds registered CPU cores".into());
    }
    if body.memory_gb_limit > worker.memory_gb {
        return Some("memory_gb_limit exceeds registered memory".into());
    }
    if body.gpu_memory_gb_limit > worker.gpu_memory_gb {
        return Some("gpu_memory_gb_limit exceeds registered GPU memory".into());
    }
    if worker.storage_total_gb > 0 && body.storage_gb_limit > worker.storage_total_gb {
        return Some("storage_gb_limit exceeds registered storage".into());
    }
    None
}

pub async fn get_provider_worker_settings(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    AxumPath(worker_id): AxumPath<String>,
) -> (StatusCode, Json<ProviderWorkerSettingsResponse>) {
    let worker = match provider_owned_worker(&state, &claims.sub, &worker_id).await {
        Ok(worker) => worker,
        Err(response) => return response,
    };

    (
        StatusCode::OK,
        Json(ProviderWorkerSettingsResponse {
            success: true,
            worker_id,
            message: "OK".into(),
            settings: Some(settings_from_worker(&worker)),
        }),
    )
}

pub async fn update_provider_worker_settings(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    AxumPath(worker_id): AxumPath<String>,
    Json(body): Json<ProviderWorkerSettingsBody>,
) -> (StatusCode, Json<ProviderWorkerSettingsResponse>) {
    let worker = match provider_owned_worker(&state, &claims.sub, &worker_id).await {
        Ok(worker) => worker,
        Err(response) => return response,
    };

    if let Some(message) = validate_provider_settings(&worker, &body) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProviderWorkerSettingsResponse {
                success: false,
                worker_id,
                message,
                settings: None,
            }),
        );
    }

    match sqlx::query_as::<_, hivemind_models::WorkerNode>(
        "UPDATE worker_nodes SET
            provider_enabled = $1,
            cpu_cores_limit = $2,
            memory_gb_limit = $3,
            gpu_memory_gb_limit = $4,
            storage_gb_limit = $5,
            min_cpt_per_hour = $6,
            updated_at = NOW()
         WHERE worker_id = $7 AND username = $8
         RETURNING *",
    )
    .bind(body.enabled)
    .bind(body.cpu_cores_limit)
    .bind(body.memory_gb_limit)
    .bind(body.gpu_memory_gb_limit)
    .bind(body.storage_gb_limit)
    .bind(body.min_cpt_per_hour)
    .bind(&worker_id)
    .bind(&claims.sub)
    .fetch_one(&state.db.pool)
    .await
    {
        Ok(worker) => (
            StatusCode::OK,
            Json(ProviderWorkerSettingsResponse {
                success: true,
                worker_id,
                message: "OK".into(),
                settings: Some(settings_from_worker(&worker)),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to update provider worker settings: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProviderWorkerSettingsResponse {
                    success: false,
                    worker_id,
                    message: e.to_string(),
                    settings: None,
                }),
            )
        }
    }
}

pub async fn get_provider_worker_trust_profile(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    AxumPath(worker_id): AxumPath<String>,
) -> (StatusCode, Json<WorkerTrustProfileResponse>) {
    if let Err(response) = provider_owned_worker(&state, &claims.sub, &worker_id).await {
        let (status, body) = response;
        return (
            status,
            Json(WorkerTrustProfileResponse {
                success: false,
                message: body.message.clone(),
                trust: None,
            }),
        );
    }

    match sqlx::query_as::<_, (i64, i64, i32, bool, Option<chrono::DateTime<chrono::Utc>>)>(
        "SELECT successful_tasks, failed_tasks, score, banned, last_attested_at
         FROM worker_reputation
         WHERE worker_id = $1",
    )
    .bind(&worker_id)
    .fetch_optional(&state.db.pool)
    .await
    {
        Ok(row) => {
            let (successful_tasks, failed_tasks, score, banned, last_attested_at) =
                row.unwrap_or((0, 0, 100, false, None));
            (
                StatusCode::OK,
                Json(WorkerTrustProfileResponse {
                    success: true,
                    message: "OK".into(),
                    trust: Some(WorkerTrustProfile {
                        worker_id,
                        successful_tasks,
                        failed_tasks,
                        score,
                        banned,
                        last_attested_at,
                    }),
                }),
            )
        }
        Err(e) => {
            tracing::error!("Failed to load worker trust profile: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WorkerTrustProfileResponse {
                    success: false,
                    message: e.to_string(),
                    trust: None,
                }),
            )
        }
    }
}

pub async fn update_worker_trust_control(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    AxumPath(worker_id): AxumPath<String>,
    Json(body): Json<WorkerTrustControlBody>,
) -> (StatusCode, Json<WorkerTrustControlResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(WorkerTrustControlResponse {
                success: false,
                worker_id,
                banned: body.banned,
                score: body.score.unwrap_or(100).clamp(0, 1000),
                message: "admin access required".into(),
            }),
        );
    }

    let requested_score = body.score.unwrap_or(100).clamp(0, 1000);
    match sqlx::query_as::<_, (bool, i32)>(
        "INSERT INTO worker_reputation (worker_id, score, banned, updated_at)
         VALUES ($1, $2, $3, NOW())
         ON CONFLICT (worker_id) DO UPDATE SET
            score = $2,
            banned = $3,
            updated_at = NOW()
         RETURNING banned, score",
    )
    .bind(&worker_id)
    .bind(requested_score)
    .bind(body.banned)
    .fetch_one(&state.db.pool)
    .await
    {
        Ok((banned, score)) => {
            let detail = serde_json::json!({
                "banned": banned,
                "score": score,
            });
            record_admin_audit_log(
                &state.db.pool,
                &claims.sub,
                "worker_trust_control",
                "worker",
                &worker_id,
                detail,
            )
            .await;
            (
                StatusCode::OK,
                Json(WorkerTrustControlResponse {
                    success: true,
                    worker_id,
                    banned,
                    score,
                    message: "OK".into(),
                }),
            )
        }
        Err(e) => {
            tracing::error!("Failed to update worker trust control: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WorkerTrustControlResponse {
                    success: false,
                    worker_id,
                    banned: body.banned,
                    score: requested_score,
                    message: e.to_string(),
                }),
            )
        }
    }
}

pub async fn list_admin_audit_logs(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    Query(query): Query<AdminAuditLogQuery>,
) -> (StatusCode, Json<AdminAuditLogListResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(AdminAuditLogListResponse {
                success: false,
                entries: vec![],
            }),
        );
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    match sqlx::query_as::<_, (String, String, String, String, serde_json::Value, chrono::DateTime<chrono::Utc>)>(
        "SELECT admin_user, action, target_type, target_id, detail, created_at
         FROM admin_audit_logs
         ORDER BY created_at DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db.pool)
    .await
    {
        Ok(rows) => (
            StatusCode::OK,
            Json(AdminAuditLogListResponse {
                success: true,
                entries: rows
                    .into_iter()
                    .map(
                        |(admin_user, action, target_type, target_id, detail, created_at)| {
                            AdminAuditLogEntry {
                                admin_user,
                                action,
                                target_type,
                                target_id,
                                detail,
                                created_at,
                            }
                        },
                    )
                    .collect(),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to list admin audit logs: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminAuditLogListResponse {
                    success: false,
                    entries: vec![],
                }),
            )
        }
    }
}

pub async fn list_admin_worker_trust(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
) -> (StatusCode, Json<AdminWorkerTrustListResponse>) {
    if !is_admin_user(&claims.sub) {
        return (
            StatusCode::FORBIDDEN,
            Json(AdminWorkerTrustListResponse {
                success: false,
                entries: vec![],
            }),
        );
    }

    match sqlx::query_as::<_, (String, String, String, i32, bool, i64, i64, Option<chrono::DateTime<chrono::Utc>>)>(
        "SELECT
            wn.worker_id,
            wn.username,
            wn.status,
            COALESCE(wr.score, 100) AS score,
            COALESCE(wr.banned, false) AS banned,
            COALESCE(wr.successful_tasks, 0) AS successful_tasks,
            COALESCE(wr.failed_tasks, 0) AS failed_tasks,
            wr.last_attested_at
         FROM worker_nodes wn
         LEFT JOIN worker_reputation wr ON wr.worker_id = wn.worker_id
         ORDER BY COALESCE(wr.banned, false) DESC, COALESCE(wr.score, 100) ASC, wn.worker_id ASC",
    )
    .fetch_all(&state.db.pool)
    .await
    {
        Ok(rows) => (
            StatusCode::OK,
            Json(AdminWorkerTrustListResponse {
                success: true,
                entries: rows
                    .into_iter()
                    .map(
                        |(
                            worker_id,
                            username,
                            worker_status,
                            score,
                            banned,
                            successful_tasks,
                            failed_tasks,
                            last_attested_at,
                        )| AdminWorkerTrustEntry {
                            worker_id,
                            username,
                            worker_status,
                            score,
                            banned,
                            successful_tasks,
                            failed_tasks,
                            last_attested_at,
                        },
                    )
                    .collect(),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to list admin worker trust entries: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminWorkerTrustListResponse {
                    success: false,
                    entries: vec![],
                }),
            )
        }
    }
}

pub async fn list_workers(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<WorkerListResponse>) {
    let include_offline = query
        .get("include_offline")
        .or_else(|| query.get("includeOffline"))
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);

    let sql = if include_offline {
        "SELECT * FROM worker_nodes ORDER BY registered_at DESC"
    } else {
        "SELECT * FROM worker_nodes WHERE status IN ('ACTIVE', 'IDLE', 'BUSY') ORDER BY registered_at DESC"
    };

    match sqlx::query_as::<_, hivemind_models::WorkerNode>(sql)
        .fetch_all(&state.db.pool)
        .await
    {
        Ok(workers) => (
            StatusCode::OK,
            Json(WorkerListResponse {
                success: true,
                workers: workers.into_iter().map(WorkerInfo::from).collect(),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to list workers: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WorkerListResponse {
                    success: false,
                    workers: vec![],
                }),
            )
        }
    }
}

pub async fn register_worker(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    Json(body): Json<RegisterWorkerBody>,
) -> (StatusCode, Json<StatusResponse>) {
    if body.ip.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "ip is required".into(),
            }),
        );
    }

    let owner = claims.sub;
    if let Some(username) = body.username.as_deref().map(str::trim) {
        if !username.is_empty() && username != owner {
            return (
                StatusCode::FORBIDDEN,
                Json(StatusResponse {
                    success: false,
                    status_message: "username does not match authenticated subject".into(),
                }),
            );
        }
    }

    let gpu_memory_gb = body.gpu_memory_gb.unwrap_or(0);
    let result = sqlx::query(
        "INSERT INTO worker_nodes (
            worker_id, username, ip, cpu_cores, memory_gb, cpu_score, gpu_score,
            gpu_memory_gb, gpu_name, vram_mb, storage_total_gb, storage_available_gb,
            provider_enabled, cpu_cores_limit, memory_gb_limit, gpu_memory_gb_limit,
            storage_gb_limit, min_cpt_per_hour,
            location, status, available_memory_gb, queue_capacity
         )
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,NULL,$9,0,0,true,$4,$5,$8,0,0,$10,'IDLE',$11,$12)
         ON CONFLICT (worker_id) DO UPDATE SET
            username = EXCLUDED.username,
            ip = EXCLUDED.ip,
            cpu_cores = EXCLUDED.cpu_cores,
            memory_gb = EXCLUDED.memory_gb,
            cpu_score = EXCLUDED.cpu_score,
            gpu_score = EXCLUDED.gpu_score,
            gpu_memory_gb = EXCLUDED.gpu_memory_gb,
            vram_mb = EXCLUDED.vram_mb,
            location = EXCLUDED.location,
            status = 'IDLE',
            available_memory_gb = EXCLUDED.available_memory_gb,
            queue_capacity = EXCLUDED.queue_capacity,
            last_heartbeat = NOW(),
            updated_at = NOW()",
    )
    .bind(&owner)
    .bind(&owner)
    .bind(&body.ip)
    .bind(body.cpu_cores)
    .bind(body.memory_gb)
    .bind(body.cpu_score)
    .bind(body.gpu_score.unwrap_or(0))
    .bind(gpu_memory_gb)
    .bind(gpu_memory_gb as i64 * 1024)
    .bind(body.location.unwrap_or_else(|| "local".into()))
    .bind(body.memory_gb)
    .bind(body.cpu_cores)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(_) => (
            StatusCode::OK,
            Json(StatusResponse {
                success: true,
                status_message: "OK".into(),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to register worker: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(StatusResponse {
                    success: false,
                    status_message: e.to_string(),
                }),
            )
        }
    }
}

pub async fn remove_worker(
    State(state): State<AppState>,
    Json(body): Json<RemoveWorkerBody>,
) -> (StatusCode, Json<StatusResponse>) {
    let result = sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
        .bind(&body.worker_id)
        .execute(&state.db.pool)
        .await;

    match result {
        Ok(result) if result.rows_affected() > 0 => (
            StatusCode::OK,
            Json(StatusResponse {
                success: true,
                status_message: "OK".into(),
            }),
        ),
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(StatusResponse {
                success: false,
                status_message: "worker not found".into(),
            }),
        ),
        Err(e) => {
            tracing::error!("Failed to remove worker: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(StatusResponse {
                    success: false,
                    status_message: e.to_string(),
                }),
            )
        }
    }
}

impl From<hivemind_models::WorkerNode> for WorkerInfo {
    fn from(worker: hivemind_models::WorkerNode) -> Self {
        Self {
            id: worker.worker_id.clone(),
            worker_id: worker.worker_id,
            addr: worker.ip.clone(),
            ip: worker.ip,
            status: worker.status.as_str().into(),
            cpu_cores: worker.cpu_cores,
            memory_gb: worker.memory_gb,
            cpu_score: worker.cpu_score,
            gpu_score: worker.gpu_score,
            gpu_memory_gb: worker.gpu_memory_gb,
            provider_enabled: worker.provider_enabled,
            cpu_cores_limit: worker.cpu_cores_limit,
            memory_gb_limit: worker.memory_gb_limit,
            gpu_memory_gb_limit: worker.gpu_memory_gb_limit,
            storage_gb_limit: worker.storage_gb_limit,
            min_cpt_per_hour: worker.min_cpt_per_hour,
            location: worker.location,
            cpu_usage: worker.cpu_usage,
            memory_usage: worker.memory_usage,
            gpu_usage: worker.gpu_usage,
            gpu_memory_usage: worker.gpu_memory_usage,
        }
    }
}

/// GET /api/tasks/:task_id/log — Get task execution log
pub async fn get_task_log(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.scheduler.get_task(&task_id).await {
        Ok(Some(task)) => {
            if task.owner != claims.sub {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"success": false, "message": "Not authorized"})),
                );
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true, "task_id": task.task_id,
                    "status": task.status.as_str(), "output": task.output,
                    "status_message": task.status_message,
                    "cpu_usage": task.cpu_usage, "memory_usage": task.memory_usage,
                    "gpu_usage": task.gpu_usage,
                    "wall_time_ms": task.wall_time_ms, "peak_memory_mb": task.peak_memory_mb,
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "message": "Task not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "message": format!("Error: {}", e)})),
        ),
    }
}

pub async fn get_task_result(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.scheduler.get_task(&task_id).await {
        Ok(Some(task)) => {
            if task.owner != claims.sub {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"success": false, "message": "Not authorized"})),
                );
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "task_id": task.task_id,
                    "status": task.status.as_str(),
                    "result_torrent": task.result_torrent,
                    "status_message": task.status_message,
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "message": "Task not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "message": format!("Error: {}", e)})),
        ),
    }
}

/// POST /api/tasks/:task_id/stop — Stop a running task
/// GET /api/tasks/:task_id/artifact/download ??Download task artifact file
pub async fn download_task_artifact(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> Response {
    let task = match state.scheduler.get_task(&task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"success":false,"message":"Task not found"}))).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success":false,"message":e.to_string()}))).into_response();
        }
    };

    if task.owner != claims.sub && !is_admin_user(&claims.sub) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"success":false,"message":"Not authorized"}))).into_response();
    }

    let row: (String, String) = match sqlx::query_as(
        "SELECT storage_path, artifact_key FROM artifacts WHERE task_id = $1 ORDER BY created_at ASC LIMIT 1",
    )
    .bind(&task_id)
    .fetch_optional(&state.db.pool)
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"success":false,"message":"No artifact for task"}))).into_response();
        }
        Err(e) => {
            tracing::error!("artifact lookup failed for {}: {}", task_id, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success":false,"message":e.to_string()}))).into_response();
        }
    };

    let (storage_path, artifact_key) = row;
    let path = Path::new(&storage_path);
    if !path.exists() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"success":false,"message":"artifact file missing on disk"}))).into_response();
    }

    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("artifact open failed {}: {}", storage_path, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success":false,"message":e.to_string()}))).into_response();
        }
    };

    let filename = format!("{}.artifact", artifact_key);
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .header("Content-Disposition", format!("attachment; filename=\"{}\"", filename))
        .body(body)
        .unwrap()
}

pub async fn stop_task(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.scheduler.get_task(&task_id).await {
        Ok(Some(task)) => {
            if task.owner != claims.sub {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"success": false, "message": "Not authorized"})),
                );
            }
            match task.status {
                TaskStatus::Pending
                | TaskStatus::Queued
                | TaskStatus::Assigned
                | TaskStatus::Running => match state.scheduler.cancel_task(&task_id).await {
                    Ok(_) => (
                        StatusCode::OK,
                        Json(serde_json::json!({"success": true, "message": "Task stopped"})),
                    ),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(
                            serde_json::json!({"success": false, "message": format!("Failed: {}", e)}),
                        ),
                    ),
                },
                _ => (
                    StatusCode::CONFLICT,
                    Json(
                        serde_json::json!({"success": false, "message": format!("Already in terminal state: {}", task.status.as_str())}),
                    ),
                ),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "message": "Task not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "message": format!("Error: {}", e)})),
        ),
    }
}

/// GET /health
pub async fn health_check() -> &'static str {
    "OK"
}

use axum::{
    body::Body,
    extract::{Multipart, Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use hivemind_config::HivemindConfig;
use hivemind_proto::ResourceSpec as ProtoResourceSpec;
use hivemind_torrent_service::TorrentService;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::grpc_client::GrpcClient;
use crate::middleware::AuthUser;

// ---- Shared App State ----

/// Shared application state - Master is now a pure HTTP-to-gRPC proxy (no DB access).
#[derive(Clone)]
pub struct AppState {
    pub jwt_secret: String,
    pub token_expiry_hours: i64,
    pub grpc_client: Arc<Mutex<GrpcClient>>,
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
    pub currency: String,
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
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ProviderEarningsResponse {
    pub success: bool,
    pub total_earned_cpt: i64,
    pub currency: String,
    pub entries: Vec<ProviderEarningsEntry>,
}

#[derive(Debug, Serialize)]
pub struct AdminBillingOverviewResponse {
    pub success: bool,
    pub total_payer_debit_cpt: i64,
    pub total_provider_credit_cpt: i64,
    pub total_platform_fee_cpt: i64,
    pub pending_billing_tasks: i64,
    pub currency: String,
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

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct TaskInfo {
    pub task_id: String,
    pub owner: String,
    pub status: String,
    pub status_message: String,
    pub worker_ip: String,
    pub output: String,
    pub result_torrent: String,
    pub billed_amount: i64,
    pub billing_settled: bool,
    pub retry_count: i32,
    pub wall_time_ms: i64,
    pub peak_memory_mb: i64,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub gpu_memory_usage: f64,
    pub deterministic: bool,
}

impl From<hivemind_proto::PricingBreakdown> for PricingBreakdown {
    fn from(pb: hivemind_proto::PricingBreakdown) -> Self {
        Self {
            base: pb.base, cpu: pb.cpu, gpu: pb.gpu, memory: pb.memory,
            gpu_memory: pb.gpu_memory, storage: pb.storage,
            host_count: pb.host_count, per_host_total: pb.per_host_total, total: pb.total,
        }
    }
}

impl From<hivemind_proto::ProviderWorkerSettings> for ProviderWorkerSettings {
    fn from(s: hivemind_proto::ProviderWorkerSettings) -> Self {
        Self {
            enabled: s.enabled, cpu_cores_limit: s.cpu_cores_limit,
            memory_gb_limit: s.memory_gb_limit, gpu_memory_gb_limit: s.gpu_memory_gb_limit,
            storage_gb_limit: s.storage_gb_limit, min_cpt_per_hour: s.min_cpt_per_hour,
        }
    }
}

impl From<hivemind_proto::WorkerInfo> for WorkerInfo {
    fn from(w: hivemind_proto::WorkerInfo) -> Self {
        Self {
            id: w.worker_id.clone(), worker_id: w.worker_id.clone(),
            addr: w.ip.clone(), ip: w.ip,
            status: w.status, cpu_cores: w.cpu_cores, memory_gb: w.memory_gb,
            cpu_score: w.cpu_score, gpu_score: w.gpu_score,
            gpu_memory_gb: w.gpu_memory_gb,
            provider_enabled: w.provider_enabled,
            cpu_cores_limit: w.cpu_cores_limit,
            memory_gb_limit: w.memory_gb_limit,
            gpu_memory_gb_limit: w.gpu_memory_gb_limit,
            storage_gb_limit: w.storage_gb_limit,
            min_cpt_per_hour: w.min_cpt_per_hour,
            location: w.location, cpu_usage: w.cpu_usage,
            memory_usage: w.memory_usage, gpu_usage: w.gpu_usage,
            gpu_memory_usage: w.gpu_memory_usage,
        }
    }
}

// Remove old From<hivemind_models::WorkerNode> impl - not needed

// ---- Handlers ----

/// GET /health
pub async fn health_check() -> &'static str { "OK" }

/// POST /api/login
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> (StatusCode, Json<LoginResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.login(&body.username, &body.password).await {
        Ok(resp) if resp.success => (
            StatusCode::OK,
            Json(LoginResponse { success: true, message: "Login successful".into(), token: Some(resp.token) }),
        ),
        Ok(resp) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse { success: false, message: resp.status_message, token: None }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse { success: false, message: format!("gRPC error: {}", e), token: None }),
        ),
    }
}

/// POST /api/tasks/quote
pub async fn quote_task(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Json(body): Json<CreateTaskBody>,
) -> (StatusCode, Json<QuoteResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.quote_task(&token, body.cpu_score.unwrap_or(0), body.gpu_score.unwrap_or(0),
        body.memory_gb.unwrap_or(0), body.gpu_memory_gb.unwrap_or(0),
        body.storage_gb.unwrap_or(0), body.host_count.unwrap_or(1)).await
    {
        Ok(resp) => {
            let b: PricingBreakdown = resp.breakdown.unwrap_or_default().into();
            (StatusCode::OK, Json(QuoteResponse { success: resp.success, quoted_cpt: resp.quoted_cpt, currency: resp.currency, breakdown: b }))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(QuoteResponse { success: false, quoted_cpt: 0, currency: String::from("CPT"),
                breakdown: PricingBreakdown { base:0,cpu:0,gpu:0,memory:0,gpu_memory:0,storage:0,host_count:0,per_host_total:0,total:0 } }),
        ),
    }
}

/// POST /api/tasks
pub async fn create_task(
    State(state): State<AppState>,
    AuthUser { claims, token }: AuthUser,
    Json(body): Json<CreateTaskBody>,
) -> (StatusCode, Json<TaskResponse>) {
    create_task_from_body(state, claims.sub, token, body).await
}

async fn create_task_from_body(
    state: AppState, _owner: String, token: String, body: CreateTaskBody,
) -> (StatusCode, Json<TaskResponse>) {
    if !is_safe_task_id(&body.task_id) {
        return bad_task_response("task_id is required and must be a safe file name");
    }
    let mut grpc = state.grpc_client.lock().await;
    let qr = grpc.quote_task(&token, body.cpu_score.unwrap_or(0), body.gpu_score.unwrap_or(0),
        body.memory_gb.unwrap_or(0), body.gpu_memory_gb.unwrap_or(0),
        body.storage_gb.unwrap_or(0), body.host_count.unwrap_or(1)).await;
    let quoted_cpt = match qr { Ok(ref r) => r.quoted_cpt, Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR,
        Json(TaskResponse { success: false, message: format!("Quote failed: {}", e), task: None })) };
    if let Some(mc) = body.max_cpt { if mc < quoted_cpt { return budget_guard_response(quoted_cpt, mc); } }
    let ts = match resolve_task_distribution(&body, &state.config).await {
        Ok(m) => m.torrent_source.unwrap_or_default(),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(TaskResponse { success: false, message: format!("Failed to prepare task package: {}", e), task: None })),
    };
    let req = ProtoResourceSpec { cpu_cores:0, memory_mb:body.memory_gb.unwrap_or(0) as i64*1024, gpu_count:0,
        gpu_name:String::new(), vram_mb:body.gpu_memory_gb.unwrap_or(0) as i64*1024,
        cpu_score:body.cpu_score.unwrap_or(0), gpu_score:body.gpu_score.unwrap_or(0),
        storage_total_gb:body.storage_gb.unwrap_or(0), storage_available_gb:0 };
    match grpc.upload_task(&body.task_id, &ts, req, &body.location.unwrap_or_else(|| "local".into()),
        body.host_count.unwrap_or(1), &token, body.max_cpt.unwrap_or(quoted_cpt)).await
    {
        Ok(resp) => {
            let s = if resp.success { StatusCode::CREATED } else { StatusCode::BAD_REQUEST };
            (s, Json(TaskResponse { success: resp.success, message: resp.status_message, task: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(TaskResponse { success: false, message: format!("gRPC error: {}", e), task: None })),
    }
}

/// POST /api/tasks/upload
pub async fn upload_task(
    State(state): State<AppState>,
    AuthUser { claims, token }: AuthUser,
    mut multipart: Multipart,
) -> (StatusCode, Json<TaskResponse>) {
    let mut body = CreateTaskBody { task_id:String::new(),torrent:None,zip_path:None,memory_gb:None,
        cpu_score:None,gpu_score:None,gpu_memory_gb:None,storage_gb:None,location:None,host_count:None,max_cpt:None };
    let mut fb = None;
    loop {
        let field = match multipart.next_field().await { Ok(Some(f)) => f, Ok(None) => break, Err(e) => return bad_task_response(format!("Invalid multipart payload: {}", e)) };
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            match field.bytes().await { Ok(b) if !b.is_empty() => fb = Some(b), Ok(_) => return bad_task_response("file is required"), Err(e) => return bad_task_response(format!("Failed to read file: {}", e)) }
        } else {
            match field.text().await { Ok(v) => { if let Err(e) = set_upload_text_field(&mut body, &name, &v) { return bad_task_response(e.to_string()); } }, Err(e) => return bad_task_response(format!("Failed to read field {}: {}", name, e)) }
        }
    }
    if !is_safe_task_id(&body.task_id) { return bad_task_response("task_id is required and must be a safe file name"); }
    let Some(fb) = fb else { return bad_task_response("file is required"); };
    let zp = task_upload_path(&state.config, &body.task_id);
    if let Some(p) = zp.parent() { if let Err(e) = std::fs::create_dir_all(p) { return bad_task_response(format!("Failed to create upload directory: {}", e)); } }
    if let Err(e) = std::fs::write(&zp, &fb) { return bad_task_response(format!("Failed to save uploaded file: {}", e)); }
    body.zip_path = Some(zp.display().to_string());
    create_task_from_body(state, claims.sub, token, body).await
}

/// GET /api/tasks
pub async fn list_tasks(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<TaskListResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_all_user_tasks(&token).await {
        Ok(resp) => {
            let tasks: Vec<TaskInfo> = resp.tasks.into_iter().map(|t| TaskInfo {
                task_id:t.task_id,owner:t.owner,status:t.status,status_message:t.status_message,
                worker_ip:t.worker_ip,output:t.output,result_torrent:t.result_torrent,
                billed_amount:t.billed_amount,billing_settled:t.billing_settled,
                retry_count:t.retry_count,wall_time_ms:t.wall_time_ms,peak_memory_mb:t.peak_memory_mb,
                cpu_usage:t.cpu_usage,memory_usage:t.memory_usage,gpu_usage:t.gpu_usage,
                gpu_memory_usage:t.gpu_memory_usage,deterministic:t.deterministic,
            }).collect();
            (StatusCode::OK, Json(TaskListResponse { success: true, tasks }))
        }
        Err(_e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(TaskListResponse { success: false, tasks: vec![] })),
    }
}

/// GET /api/balance
pub async fn get_balance(
    State(state): State<AppState>, AuthUser { claims, token }: AuthUser,
) -> (StatusCode, Json<BalanceResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_balance(&claims.sub, &token).await {
        Ok(resp) => (StatusCode::OK, Json(BalanceResponse { success: resp.success, balance: resp.balance })),
        Err(_e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(BalanceResponse { success: false, balance: 0 })),
    }
}

/// GET /api/workers
pub async fn list_workers(
    State(state): State<AppState>, AuthUser { .. }: AuthUser, Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<WorkerListResponse>) {
    let io = query.get("include_offline").or_else(|| query.get("includeOffline"))
        .map(|v| matches!(v.as_str(), "1"|"true"|"TRUE"|"yes"|"YES")).unwrap_or(false);
    let mut grpc = state.grpc_client.lock().await;
    match grpc.list_workers(io).await {
        Ok(resp) => (StatusCode::OK, Json(WorkerListResponse { success: resp.success, workers: resp.workers.into_iter().map(WorkerInfo::from).collect() })),
        Err(_e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(WorkerListResponse { success: false, workers: vec![] })),
    }
}

/// POST /api/register-worker
pub async fn register_worker(
    State(state): State<AppState>, AuthUser { claims, .. }: AuthUser, Json(body): Json<RegisterWorkerBody>,
) -> (StatusCode, Json<StatusResponse>) {
    if body.ip.trim().is_empty() { return (StatusCode::BAD_REQUEST, Json(StatusResponse { success: false, status_message: "ip is required".into() })); }
    let owner = claims.sub;
    if let Some(u) = body.username.as_deref().map(str::trim) { if !u.is_empty() && u != owner { return (StatusCode::FORBIDDEN, Json(StatusResponse { success: false, status_message: "username does not match authenticated subject".into() })); } }
    let r = ProtoResourceSpec { cpu_cores:body.cpu_cores, memory_mb:body.memory_gb as i64*1024, gpu_count:0, gpu_name:String::new(), vram_mb:body.gpu_memory_gb.unwrap_or(0) as i64*1024, cpu_score:body.cpu_score, gpu_score:body.gpu_score.unwrap_or(0), storage_total_gb:0, storage_available_gb:0 };
    let mut grpc = state.grpc_client.lock().await;
    match grpc.register_worker_node(&owner, &body.ip, r, &body.location.unwrap_or_else(|| "local".into())).await {
        Ok(resp) => (StatusCode::OK, Json(StatusResponse { success: resp.success, status_message: resp.status_message })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(StatusResponse { success: false, status_message: e.to_string() })),
    }
}

/// POST /api/remove-worker
pub async fn remove_worker(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser, Json(body): Json<RemoveWorkerBody>,
) -> (StatusCode, Json<StatusResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.remove_worker(&body.worker_id, &token).await {
        Ok(resp) => (StatusCode::OK, Json(StatusResponse { success: resp.success, status_message: resp.status_message })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(StatusResponse { success: false, status_message: e.to_string() })),
    }
}

/// GET /api/tasks/:task_id/log
pub async fn get_task_log(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser, AxumPath(task_id): AxumPath<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_tasklog(&task_id, &token).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::json!({"success":resp.success,"task_id":task_id,"log":resp.log}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success":false,"message":format!("gRPC error: {}",e)}))),
    }
}

/// GET /api/tasks/:task_id/result
pub async fn get_task_result(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser, AxumPath(task_id): AxumPath<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_task_result(&task_id, &token).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::json!({"success":resp.success,"task_id":task_id,"result_torrent":resp.result_torrent,"status_message":resp.status_message}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success":false,"message":format!("gRPC error: {}",e)}))),
    }
}

/// POST /api/tasks/:task_id/stop
pub async fn stop_task(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser, AxumPath(task_id): AxumPath<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.stop_task(&task_id, &token).await {
        Ok(resp) if resp.success => (StatusCode::OK, Json(serde_json::json!({"success":true,"message":"Task stopped"}))),
        Ok(resp) => (StatusCode::CONFLICT, Json(serde_json::json!({"success":false,"message":resp.status_message}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success":false,"message":format!("gRPC error: {}",e)}))),
    }
}

/// GET /api/provider/earnings
pub async fn get_provider_earnings(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser, Query(query): Query<ProviderEarningsQuery>,
) -> (StatusCode, Json<ProviderEarningsResponse>) {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_provider_earnings(&token, limit).await {
        Ok(resp) => (StatusCode::OK, Json(ProviderEarningsResponse { success:resp.success, total_earned_cpt:resp.total_earned_cpt, currency:resp.currency,
            entries:resp.entries.into_iter().map(|e| ProviderEarningsEntry { task_id:e.task_id, payer_user:e.payer_user,
                provider_worker_id:if e.provider_worker_id.is_empty(){None}else{Some(e.provider_worker_id)},
                amount_cpt:e.amount_cpt, status:e.status, created_at:e.created_at }).collect() })),
        Err(_e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ProviderEarningsResponse { success:false,total_earned_cpt:0,currency:String::from("CPT"),entries:vec![] })),
    }
}

/// GET /api/provider/workers/:worker_id/settings
pub async fn get_provider_worker_settings(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser, AxumPath(worker_id): AxumPath<String>,
) -> (StatusCode, Json<ProviderWorkerSettingsResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_provider_worker_settings(&token, &worker_id).await {
        Ok(resp) => (StatusCode::OK, Json(ProviderWorkerSettingsResponse { success:resp.success, worker_id:resp.worker_id, message:resp.message, settings:resp.settings.map(|s|s.into()) })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ProviderWorkerSettingsResponse { success:false, worker_id, message:e.to_string(), settings:None })),
    }
}

/// PUT /api/provider/workers/:worker_id/settings
pub async fn update_provider_worker_settings(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser, AxumPath(worker_id): AxumPath<String>,
    Json(body): Json<ProviderWorkerSettingsBody>,
) -> (StatusCode, Json<ProviderWorkerSettingsResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.update_provider_worker_settings(&token, &worker_id, body.enabled, body.cpu_cores_limit,
        body.memory_gb_limit, body.gpu_memory_gb_limit, body.storage_gb_limit, body.min_cpt_per_hour).await
    {
        Ok(resp) => (StatusCode::OK, Json(ProviderWorkerSettingsResponse { success:resp.success, worker_id:resp.worker_id, message:resp.message, settings:resp.settings.map(|s|s.into()) })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ProviderWorkerSettingsResponse { success:false, worker_id, message:e.to_string(), settings:None })),
    }
}

#[derive(Debug, Serialize)]
pub struct AdminSchedulingCacheAnomalyListResponse {
    pub success: bool,
    pub entries: Vec<AdminSchedulingCacheAnomalyEntry>,
}

#[derive(Debug, Serialize)]
pub struct AdminAuditLogEntry {
    pub id: String,
    pub username: String,
    pub action: String,
    pub resource: String,
    pub details: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct AdminAuditLogListResponse {
    pub success: bool,
    pub entries: Vec<AdminAuditLogEntry>,
}

/// GET /api/admin/billing/overview
pub async fn get_admin_billing_overview(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<AdminBillingOverviewResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_admin_billing_overview(&token).await {
        Ok(resp) => (StatusCode::OK, Json(AdminBillingOverviewResponse {
            success: resp.success,
            total_payer_debit_cpt: resp.total_payer_debit_cpt,
            total_provider_credit_cpt: resp.total_provider_credit_cpt,
            total_platform_fee_cpt: resp.total_platform_fee_cpt,
            pending_billing_tasks: resp.pending_billing_tasks,
            currency: resp.currency,
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AdminBillingOverviewResponse {
            success: false, total_payer_debit_cpt: 0, total_provider_credit_cpt: 0,
            total_platform_fee_cpt: 0, pending_billing_tasks: 0,
            currency: "CPT".into(),
        })),
    }
}

/// GET /api/admin/artifacts/overview
pub async fn get_admin_artifact_overview(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<AdminArtifactOverviewResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_admin_artifact_overview(&token).await {
        Ok(resp) => (StatusCode::OK, Json(AdminArtifactOverviewResponse {
            success: resp.success,
            total_artifacts: resp.total_artifacts,
            total_size_bytes: resp.total_size_bytes,
            dedup_hits: resp.dedup_hits,
            resumable_artifacts: resp.resumable_artifacts,
            expiring_in_24h: resp.expiring_in_24h,
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AdminArtifactOverviewResponse {
            success: false, total_artifacts: 0, total_size_bytes: 0,
            dedup_hits: 0, resumable_artifacts: 0, expiring_in_24h: 0,
        })),
    }
}

/// POST /api/admin/artifacts/cleanup
pub async fn cleanup_admin_artifacts(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
    Json(body): Json<AdminArtifactCleanupBody>,
) -> (StatusCode, Json<AdminArtifactCleanupResponse>) {
    let dry_run = body.dry_run.unwrap_or(true);
    let mut grpc = state.grpc_client.lock().await;
    match grpc.cleanup_admin_artifacts(&token, dry_run).await {
        Ok(resp) => (StatusCode::OK, Json(AdminArtifactCleanupResponse {
            success: resp.success,
            dry_run: resp.dry_run,
            expired_candidates: resp.expired_candidates,
            deleted_rows: resp.deleted_rows,
            deleted_files: resp.deleted_files,
            file_delete_errors: resp.file_delete_errors,
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AdminArtifactCleanupResponse {
            success: false, dry_run, expired_candidates: 0,
            deleted_rows: 0, deleted_files: 0, file_delete_errors: 0,
        })),
    }
}

/// GET /api/admin/scheduling/cache-metrics
pub async fn get_admin_scheduling_cache_metrics(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<AdminSchedulingCacheMetricsResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_admin_scheduling_cache_metrics(&token).await {
        Ok(resp) => (StatusCode::OK, Json(AdminSchedulingCacheMetricsResponse {
            success: resp.success,
            total_completed_tasks: resp.total_completed_tasks,
            total_cache_hits: resp.total_cache_hits,
            cache_hit_rate: resp.cache_hit_rate,
            top_workers: resp.top_workers.into_iter().map(|w| WorkerCacheAffinityMetric {
                worker_id: w.worker_id,
                completed_tasks: w.completed_tasks,
                cache_hits: w.cache_hits,
                recent_completed_tasks_7d: w.recent_completed_tasks_7d,
            }).collect(),
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AdminSchedulingCacheMetricsResponse {
            success: false, total_completed_tasks: 0, total_cache_hits: 0,
            cache_hit_rate: 0.0, top_workers: vec![],
        })),
    }
}

/// GET /api/admin/scheduling/cache-alert
pub async fn get_admin_scheduling_cache_alert(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
    Query(query): Query<AdminSchedulingCacheAlertQuery>,
) -> (StatusCode, Json<AdminSchedulingCacheAlertResponse>) {
    let low = query.low.unwrap_or(40.0);
    let high = query.high.unwrap_or(70.0);
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_admin_scheduling_cache_alert(&token, low, high).await {
        Ok(resp) => (StatusCode::OK, Json(AdminSchedulingCacheAlertResponse {
            success: resp.success,
            low_threshold: resp.low_threshold,
            high_threshold: resp.high_threshold,
            cache_hit_rate: resp.cache_hit_rate,
            severity: resp.severity,
            message: resp.status_message,
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AdminSchedulingCacheAlertResponse {
            success: false, low_threshold: low, high_threshold: high,
            cache_hit_rate: 0.0, severity: "unknown".into(),
            message: e.to_string(),
        })),
    }
}

/// GET /api/admin/scheduling/cache-anomalies
pub async fn list_admin_scheduling_cache_anomalies(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
    Query(query): Query<AdminSchedulingCacheAnomalyQuery>,
) -> (StatusCode, Json<AdminSchedulingCacheAnomalyListResponse>) {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let mut grpc = state.grpc_client.lock().await;
    match grpc.list_admin_scheduling_cache_anomalies(&token, limit).await {
        Ok(resp) => (StatusCode::OK, Json(AdminSchedulingCacheAnomalyListResponse {
            success: resp.success,
            entries: resp.entries.into_iter().map(|e| AdminSchedulingCacheAnomalyEntry {
                severity: e.severity,
                cache_hit_rate: e.cache_hit_rate,
                low_threshold: e.low_threshold,
                high_threshold: e.high_threshold,
                message: e.message,
                created_at: e.created_at.parse().unwrap_or_default(),
            }).collect(),
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AdminSchedulingCacheAnomalyListResponse {
            success: false, entries: vec![],
        })),
    }
}

/// GET /api/provider/workers/:worker_id/trust
pub async fn get_provider_worker_trust_profile(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
    AxumPath(worker_id): AxumPath<String>,
) -> (StatusCode, Json<WorkerTrustProfileResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.get_worker_trust_profile(&token, &worker_id).await {
        Ok(resp) => (StatusCode::OK, Json(WorkerTrustProfileResponse {
            success: resp.success,
            message: resp.status_message,
            trust: resp.trust.map(|t| WorkerTrustProfile {
                worker_id: t.worker_id,
                successful_tasks: t.successful_tasks,
                failed_tasks: t.failed_tasks,
                score: t.score,
                banned: t.banned,
                last_attested_at: t.last_attested_at.parse().ok(),
            }),
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(WorkerTrustProfileResponse {
            success: false, message: e.to_string(), trust: None,
        })),
    }
}

/// PUT /api/admin/workers/:worker_id/trust-control
pub async fn update_worker_trust_control(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
    AxumPath(worker_id): AxumPath<String>,
    Json(body): Json<WorkerTrustControlBody>,
) -> (StatusCode, Json<WorkerTrustControlResponse>) {
    let score = body.score.unwrap_or(0);
    let mut grpc = state.grpc_client.lock().await;
    match grpc.update_worker_trust_control(&token, &worker_id, body.banned, score).await {
        Ok(resp) => (StatusCode::OK, Json(WorkerTrustControlResponse {
            success: resp.success,
            worker_id: resp.worker_id,
            banned: resp.banned,
            score: resp.score,
            message: resp.status_message,
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(WorkerTrustControlResponse {
            success: false, worker_id, banned: false, score: 0, message: e.to_string(),
        })),
    }
}

/// GET /api/admin/workers/trust
pub async fn list_admin_worker_trust(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<AdminWorkerTrustListResponse>) {
    let mut grpc = state.grpc_client.lock().await;
    match grpc.list_admin_worker_trust(&token).await {
        Ok(resp) => (StatusCode::OK, Json(AdminWorkerTrustListResponse {
            success: resp.success,
            entries: resp.entries.into_iter().map(|e| AdminWorkerTrustEntry {
                worker_id: e.worker_id,
                username: e.username,
                worker_status: e.worker_status,
                score: e.score,
                banned: e.banned,
                successful_tasks: e.successful_tasks,
                failed_tasks: e.failed_tasks,
                last_attested_at: e.last_attested_at.parse().ok(),
            }).collect(),
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AdminWorkerTrustListResponse {
            success: false, entries: vec![],
        })),
    }
}

/// GET /api/admin/audit/logs
pub async fn list_admin_audit_logs(
    State(state): State<AppState>, AuthUser { token, .. }: AuthUser,
    Query(query): Query<AdminSchedulingCacheAnomalyQuery>,
) -> (StatusCode, Json<AdminAuditLogListResponse>) {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let mut grpc = state.grpc_client.lock().await;
    match grpc.list_admin_audit_logs(&token, limit).await {
        Ok(resp) => (StatusCode::OK, Json(AdminAuditLogListResponse {
            success: resp.success,
            entries: resp.entries.into_iter().map(|e| AdminAuditLogEntry {
                id: e.id,
                username: e.username,
                action: e.action,
                resource: e.resource,
                details: e.details,
                created_at: e.created_at.parse().unwrap_or_default(),
            }).collect(),
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AdminAuditLogListResponse {
            success: false, entries: vec![],
        })),
    }
}

pub async fn download_task_artifact(AxumPath(_task_id): AxumPath<String>) -> Response {
    (StatusCode::NOT_IMPLEMENTED, Json(serde_json::json!({"success":false,"message":"Artifact download not yet implemented via gRPC. Artifacts live on the nodepool server."}))).into_response()
}

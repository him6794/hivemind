use axum::{
    extract::{Multipart, Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hivemind_config::HivemindConfig;
use hivemind_proto::ResourceSpec as ProtoResourceSpec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::grpc_client::GrpcClient;
use crate::middleware::AuthUser;

// ---- Shared App State ----

/// Shared application state - Master is now a pure HTTP-to-gRPC proxy (no DB access).
#[derive(Clone)]
pub struct AppState {
    pub jwt_secret: String,
    pub token_expiry_hours: i64,
    pub grpc_client: GrpcClient,
    pub config: HivemindConfig,
    pub task_submit_limiter: Arc<tokio::sync::Mutex<TaskSubmitRateLimiter>>,
}

#[derive(Clone, Debug)]
struct RateWindow {
    started_at: Instant,
    count: i64,
}

#[derive(Debug, Default)]
pub struct TaskSubmitRateLimiter {
    windows: HashMap<String, RateWindow>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ArtifactDownloadQuery {
    pub artifact_key: Option<String>,
}

impl TaskSubmitRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow(&mut self, owner: &str, limit_per_minute: i64, now: Instant) -> bool {
        if limit_per_minute <= 0 {
            return false;
        }

        let window = self
            .windows
            .entry(owner.to_string())
            .or_insert_with(|| RateWindow {
                started_at: now,
                count: 0,
            });

        if now.duration_since(window.started_at) >= Duration::from_secs(60) {
            window.started_at = now;
            window.count = 0;
        }

        if window.count >= limit_per_minute {
            return false;
        }

        window.count += 1;
        true
    }
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
pub struct RegisterBody {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskBody {
    pub task_id: String,
    pub torrent: Option<String>,
    pub runtime: Option<String>,
    pub task_source: Option<String>,
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
    pub worker_id: Option<String>,
    pub ip: String,
    pub cpu_cores: i32,
    pub memory_gb: i32,
    pub cpu_score: i32,
    pub gpu_score: Option<i32>,
    pub gpu_memory_gb: Option<i32>,
    pub gpu_name: Option<String>,
    pub storage_total_gb: Option<i64>,
    pub storage_available_gb: Option<i64>,
    pub location: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RemoveWorkerBody {
    pub worker_id: String,
}

struct TaskDistribution {
    torrent_source: Option<String>,
    package_data: Vec<u8>,
    package_filename: String,
}

struct TaskSubmission {
    body: CreateTaskBody,
    uploaded_package: Option<TaskDistribution>,
}

fn bad_task_response(message: impl Into<String>) -> (StatusCode, Json<TaskResponse>) {
    task_response(StatusCode::BAD_REQUEST, false, message)
}

fn task_response(
    status: StatusCode,
    success: bool,
    message: impl Into<String>,
) -> (StatusCode, Json<TaskResponse>) {
    (
        status,
        Json(TaskResponse {
            success,
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

fn validate_task_resources(body: &CreateTaskBody) -> Result<(), &'static str> {
    if body.cpu_score.unwrap_or(0) < 0
        || body.gpu_score.unwrap_or(0) < 0
        || body.memory_gb.unwrap_or(0) < 0
        || body.gpu_memory_gb.unwrap_or(0) < 0
        || body.storage_gb.unwrap_or(0) < 0
    {
        return Err("task resource values must be non-negative");
    }
    if body.host_count.unwrap_or(1) < 1 {
        return Err("host_count must be at least 1");
    }
    if body.max_cpt.unwrap_or(0) < 0 {
        return Err("max_cpt must be non-negative");
    }
    Ok(())
}

fn is_safe_task_id(task_id: &str) -> bool {
    if task_id.len() == 1 && task_id.as_bytes()[0] == b'.' {
        return false;
    }
    !task_id.trim().is_empty()
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !task_id.contains("..")
}

fn normalized_task_id(task_id: &str) -> Option<String> {
    let task_id = task_id.trim();
    if is_safe_task_id(task_id) {
        Some(task_id.to_string())
    } else {
        None
    }
}

fn is_safe_worker_id(worker_id: &str) -> bool {
    is_safe_task_id(worker_id)
}

fn normalized_worker_id(worker_id: &str) -> Option<String> {
    let worker_id = worker_id.trim();
    if is_safe_worker_id(worker_id) {
        Some(worker_id.to_string())
    } else {
        None
    }
}

fn task_submit_limit_per_minute() -> i64 {
    std::env::var("HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(60)
        .max(0)
}

fn max_task_upload_bytes() -> usize {
    std::env::var("HIVEMIND_MAX_TASK_UPLOAD_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(100 * 1024 * 1024)
}

fn uploaded_file_size_error(file_len: usize, max_bytes: usize) -> Option<String> {
    if file_len > max_bytes {
        Some(format!(
            "uploaded task package is too large: {} bytes > {} bytes",
            file_len, max_bytes
        ))
    } else {
        None
    }
}

fn is_reserved_admin_username(username: &str) -> bool {
    std::env::var("HIVEMIND_ADMIN_USERS")
        .ok()
        .map(|users| {
            users
                .split(',')
                .map(str::trim)
                .filter(|configured| !configured.is_empty())
                .any(|configured| configured == username)
        })
        .unwrap_or(false)
}

async fn enforce_task_submit_rate_limit(
    state: &AppState,
    owner: &str,
) -> Option<(StatusCode, Json<TaskResponse>)> {
    let limit = task_submit_limit_per_minute();
    let mut limiter = state.task_submit_limiter.lock().await;
    if limiter.allow(owner, limit, Instant::now()) {
        None
    } else {
        Some(task_response(
            StatusCode::TOO_MANY_REQUESTS,
            false,
            format!("task submission rate limit exceeded: {} per minute", limit),
        ))
    }
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
        "runtime" => body.runtime = Some(value.trim().to_string()),
        "task_source" => body.task_source = Some(value.to_string()),
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

fn task_upload_path(config: &HivemindConfig, task_id: &str) -> PathBuf {
    PathBuf::from(&config.torrent.api_dir)
        .join("uploads")
        .join(format!("{}.zip", task_id))
}

fn resolve_task_distribution(
    body: &CreateTaskBody,
    uploaded_package: Option<TaskDistribution>,
) -> TaskDistribution {
    uploaded_package.unwrap_or_else(|| TaskDistribution {
        torrent_source: body.torrent.clone(),
        package_data: Vec::new(),
        package_filename: String::new(),
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
            base: pb.base,
            cpu: pb.cpu,
            gpu: pb.gpu,
            memory: pb.memory,
            gpu_memory: pb.gpu_memory,
            storage: pb.storage,
            host_count: pb.host_count,
            per_host_total: pb.per_host_total,
            total: pb.total,
        }
    }
}

impl From<hivemind_proto::ProviderWorkerSettings> for ProviderWorkerSettings {
    fn from(s: hivemind_proto::ProviderWorkerSettings) -> Self {
        Self {
            enabled: s.enabled,
            cpu_cores_limit: s.cpu_cores_limit,
            memory_gb_limit: s.memory_gb_limit,
            gpu_memory_gb_limit: s.gpu_memory_gb_limit,
            storage_gb_limit: s.storage_gb_limit,
            min_cpt_per_hour: s.min_cpt_per_hour,
        }
    }
}

impl From<hivemind_proto::WorkerInfo> for WorkerInfo {
    fn from(w: hivemind_proto::WorkerInfo) -> Self {
        Self {
            id: w.worker_id.clone(),
            worker_id: w.worker_id.clone(),
            addr: w.ip.clone(),
            ip: w.ip,
            status: w.status,
            cpu_cores: w.cpu_cores,
            memory_gb: w.memory_gb,
            cpu_score: w.cpu_score,
            gpu_score: w.gpu_score,
            gpu_memory_gb: w.gpu_memory_gb,
            provider_enabled: w.provider_enabled,
            cpu_cores_limit: w.cpu_cores_limit,
            memory_gb_limit: w.memory_gb_limit,
            gpu_memory_gb_limit: w.gpu_memory_gb_limit,
            storage_gb_limit: w.storage_gb_limit,
            min_cpt_per_hour: w.min_cpt_per_hour,
            location: w.location,
            cpu_usage: w.cpu_usage,
            memory_usage: w.memory_usage,
            gpu_usage: w.gpu_usage,
            gpu_memory_usage: w.gpu_memory_usage,
        }
    }
}

// Remove old From<hivemind_models::WorkerNode> impl - not needed

fn worker_registration_resources(body: &RegisterWorkerBody) -> ProtoResourceSpec {
    let gpu_score = body.gpu_score.unwrap_or(0);
    let gpu_memory_gb = body.gpu_memory_gb.unwrap_or(0);
    let gpu_name = body
        .gpu_name
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let has_gpu = gpu_score > 0 || gpu_memory_gb > 0 || !gpu_name.is_empty();

    ProtoResourceSpec {
        cpu_cores: body.cpu_cores,
        memory_mb: i64::from(body.memory_gb) * 1024,
        gpu_count: if has_gpu { 1 } else { 0 },
        gpu_name,
        vram_mb: i64::from(gpu_memory_gb) * 1024,
        cpu_score: body.cpu_score,
        gpu_score,
        storage_total_gb: body.storage_total_gb.unwrap_or(0),
        storage_available_gb: body.storage_available_gb.unwrap_or(0),
    }
}

// ---- Handlers ----

/// GET /health
pub async fn health_check() -> &'static str {
    "OK"
}

/// POST /api/login
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> (StatusCode, Json<LoginResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.login(&body.username, &body.password).await {
        Ok(resp) if resp.success => (
            StatusCode::OK,
            Json(LoginResponse {
                success: true,
                message: "Login successful".into(),
                token: Some(resp.token),
            }),
        ),
        Ok(resp) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                message: resp.status_message,
                token: None,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse {
                success: false,
                message: format!("gRPC error: {}", e),
                token: None,
            }),
        ),
    }
}

/// POST /api/register
pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterBody>,
) -> (StatusCode, Json<RegisterResponse>) {
    let username = body.username.trim();
    if username.len() < 3 {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                success: false,
                message: "Username must be at least 3 characters".into(),
            }),
        );
    }
    if is_reserved_admin_username(username) {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                success: false,
                message: "Username is unavailable".into(),
            }),
        );
    }
    if body.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                success: false,
                message: "Password must be at least 8 characters".into(),
            }),
        );
    }

    let mut grpc = state.grpc_client.clone();
    match grpc.register_user(username, &body.password).await {
        Ok(resp) if resp.success => (
            StatusCode::CREATED,
            Json(RegisterResponse {
                success: true,
                message: resp.status_message,
            }),
        ),
        Ok(resp) => (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                success: false,
                message: resp.status_message,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RegisterResponse {
                success: false,
                message: format!("gRPC error: {}", e),
            }),
        ),
    }
}

/// POST /api/tasks/quote
pub async fn quote_task(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Json(body): Json<CreateTaskBody>,
) -> (StatusCode, Json<QuoteResponse>) {
    if validate_task_resources(&body).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(QuoteResponse {
                success: false,
                quoted_cpt: 0,
                currency: String::from("CPT"),
                breakdown: PricingBreakdown {
                    base: 0,
                    cpu: 0,
                    gpu: 0,
                    memory: 0,
                    gpu_memory: 0,
                    storage: 0,
                    host_count: 0,
                    per_host_total: 0,
                    total: 0,
                },
            }),
        );
    }
    let mut grpc = state.grpc_client.clone();
    match grpc
        .quote_task(
            &token,
            body.cpu_score.unwrap_or(0),
            body.gpu_score.unwrap_or(0),
            body.memory_gb.unwrap_or(0),
            body.gpu_memory_gb.unwrap_or(0),
            body.storage_gb.unwrap_or(0),
            body.host_count.unwrap_or(1),
        )
        .await
    {
        Ok(resp) => {
            let b: PricingBreakdown = resp.breakdown.unwrap_or_default().into();
            (
                StatusCode::OK,
                Json(QuoteResponse {
                    success: resp.success,
                    quoted_cpt: resp.quoted_cpt,
                    currency: resp.currency,
                    breakdown: b,
                }),
            )
        }
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(QuoteResponse {
                success: false,
                quoted_cpt: 0,
                currency: String::from("CPT"),
                breakdown: PricingBreakdown {
                    base: 0,
                    cpu: 0,
                    gpu: 0,
                    memory: 0,
                    gpu_memory: 0,
                    storage: 0,
                    host_count: 0,
                    per_host_total: 0,
                    total: 0,
                },
            }),
        ),
    }
}

/// POST /api/tasks
pub async fn create_task(
    State(state): State<AppState>,
    AuthUser { claims, token }: AuthUser,
    Json(body): Json<CreateTaskBody>,
) -> (StatusCode, Json<TaskResponse>) {
    let owner = claims.sub;
    if let Some(response) = enforce_task_submit_rate_limit(&state, &owner).await {
        return response;
    }
    let mut body = body;
    body.task_id = uuid::Uuid::new_v4().to_string();
    create_task_from_submission(
        state,
        token,
        TaskSubmission {
            body,
            uploaded_package: None,
        },
    )
    .await
}

async fn create_task_from_submission(
    state: AppState,
    token: String,
    submission: TaskSubmission,
) -> (StatusCode, Json<TaskResponse>) {
    let TaskSubmission {
        body,
        uploaded_package,
    } = submission;
    if !is_safe_task_id(&body.task_id) {
        return bad_task_response("task_id is required and must be a safe file name");
    }
    if let Err(message) = validate_task_resources(&body) {
        return bad_task_response(message);
    }
    let mut grpc = state.grpc_client.clone();
    let qr = grpc
        .quote_task(
            &token,
            body.cpu_score.unwrap_or(0),
            body.gpu_score.unwrap_or(0),
            body.memory_gb.unwrap_or(0),
            body.gpu_memory_gb.unwrap_or(0),
            body.storage_gb.unwrap_or(0),
            body.host_count.unwrap_or(1),
        )
        .await;
    let quoted_cpt = match qr {
        Ok(ref r) => r.quoted_cpt,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TaskResponse {
                    success: false,
                    message: format!("Quote failed: {}", e),
                    task: None,
                }),
            )
        }
    };
    if let Some(mc) = body.max_cpt {
        if mc < quoted_cpt {
            return budget_guard_response(quoted_cpt, mc);
        }
    }
    let distribution = resolve_task_distribution(&body, uploaded_package);
    let ts = distribution.torrent_source.unwrap_or_default();
    let req = ProtoResourceSpec {
        cpu_cores: 0,
        memory_mb: body.memory_gb.unwrap_or(0) as i64 * 1024,
        gpu_count: 0,
        gpu_name: String::new(),
        vram_mb: body.gpu_memory_gb.unwrap_or(0) as i64 * 1024,
        cpu_score: body.cpu_score.unwrap_or(0),
        gpu_score: body.gpu_score.unwrap_or(0),
        storage_total_gb: body.storage_gb.unwrap_or(0),
        storage_available_gb: 0,
    };
    match grpc
        .upload_task(
            &body.task_id,
            &ts,
            req,
            &body.location.unwrap_or_else(|| "local".into()),
            body.host_count.unwrap_or(1),
            &token,
            body.max_cpt.unwrap_or(quoted_cpt),
            body.runtime.as_deref().unwrap_or(""),
            body.task_source.as_deref().unwrap_or(""),
            distribution.package_data,
            &distribution.package_filename,
        )
        .await
    {
        Ok(resp) => {
            let s = if resp.success {
                StatusCode::CREATED
            } else if resp.status_message == "task_id already exists" {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                s,
                Json(TaskResponse {
                    success: resp.success,
                    message: resp.status_message,
                    task: None,
                }),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(TaskResponse {
                success: false,
                message: format!("gRPC error: {}", e),
                task: None,
            }),
        ),
    }
}

/// POST /api/tasks/upload
pub async fn upload_task(
    State(state): State<AppState>,
    AuthUser { claims, token }: AuthUser,
    mut multipart: Multipart,
) -> (StatusCode, Json<TaskResponse>) {
    let owner = claims.sub;
    if let Some(response) = enforce_task_submit_rate_limit(&state, &owner).await {
        return response;
    }

    let mut body = CreateTaskBody {
        task_id: String::new(),
        torrent: None,
        runtime: None,
        task_source: None,
        memory_gb: None,
        cpu_score: None,
        gpu_score: None,
        gpu_memory_gb: None,
        storage_gb: None,
        location: None,
        host_count: None,
        max_cpt: None,
    };
    let mut fb = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return bad_task_response(format!("Invalid multipart payload: {}", e)),
        };
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            match field.bytes().await {
                Ok(b) if !b.is_empty() => fb = Some(b),
                Ok(_) => return bad_task_response("file is required"),
                Err(e) => return bad_task_response(format!("Failed to read file: {}", e)),
            }
        } else {
            match field.text().await {
                Ok(v) => {
                    if let Err(e) = set_upload_text_field(&mut body, &name, &v) {
                        return bad_task_response(e.to_string());
                    }
                }
                Err(e) => {
                    return bad_task_response(format!("Failed to read field {}: {}", name, e))
                }
            }
        }
    }
    let Some(fb) = fb else {
        return bad_task_response("file is required");
    };
    if let Some(message) = uploaded_file_size_error(fb.len(), max_task_upload_bytes()) {
        return task_response(StatusCode::PAYLOAD_TOO_LARGE, false, message);
    }
    body.task_id = uuid::Uuid::new_v4().to_string();
    let zp = task_upload_path(&state.config, &body.task_id);
    if let Some(p) = zp.parent() {
        if let Err(e) = std::fs::create_dir_all(p) {
            return bad_task_response(format!("Failed to create upload directory: {}", e));
        }
    }
    if let Err(e) = std::fs::write(&zp, &fb) {
        return bad_task_response(format!("Failed to save uploaded file: {}", e));
    }
    let package_filename = zp
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&body.task_id)
        .to_string();
    create_task_from_submission(
        state,
        token,
        TaskSubmission {
            body,
            uploaded_package: Some(TaskDistribution {
                torrent_source: None,
                package_data: fb.to_vec(),
                package_filename,
            }),
        },
    )
    .await
}

/// GET /api/tasks
pub async fn list_tasks(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<TaskListResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.get_all_user_tasks(&token).await {
        Ok(resp) => {
            let tasks: Vec<TaskInfo> = resp
                .tasks
                .into_iter()
                .map(|t| TaskInfo {
                    task_id: t.task_id,
                    owner: t.owner,
                    status: t.status,
                    status_message: t.status_message,
                    worker_ip: t.worker_ip,
                    output: t.output,
                    result_torrent: t.result_torrent,
                    billed_amount: t.billed_amount,
                    billing_settled: t.billing_settled,
                    retry_count: t.retry_count,
                    wall_time_ms: t.wall_time_ms,
                    peak_memory_mb: t.peak_memory_mb,
                    cpu_usage: t.cpu_usage,
                    memory_usage: t.memory_usage,
                    gpu_usage: t.gpu_usage,
                    gpu_memory_usage: t.gpu_memory_usage,
                    deterministic: t.deterministic,
                })
                .collect();
            (
                StatusCode::OK,
                Json(TaskListResponse {
                    success: true,
                    tasks,
                }),
            )
        }
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(TaskListResponse {
                success: false,
                tasks: vec![],
            }),
        ),
    }
}

/// GET /api/balance
pub async fn get_balance(
    State(state): State<AppState>,
    AuthUser { claims, token }: AuthUser,
) -> (StatusCode, Json<BalanceResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.get_balance(&claims.sub, &token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(BalanceResponse {
                success: resp.success,
                balance: resp.balance,
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(BalanceResponse {
                success: false,
                balance: 0,
            }),
        ),
    }
}

/// GET /api/workers
pub async fn list_workers(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<WorkerListResponse>) {
    let io = query
        .get("include_offline")
        .or_else(|| query.get("includeOffline"))
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);
    let mut grpc = state.grpc_client.clone();
    match grpc.list_workers(io, &token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(WorkerListResponse {
                success: resp.success,
                workers: resp.workers.into_iter().map(WorkerInfo::from).collect(),
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WorkerListResponse {
                success: false,
                workers: vec![],
            }),
        ),
    }
}

/// POST /api/register-worker
pub async fn register_worker(
    State(state): State<AppState>,
    AuthUser { claims, token }: AuthUser,
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
    if let Some(u) = body.username.as_deref().map(str::trim) {
        if !u.is_empty() && u != owner {
            return (
                StatusCode::FORBIDDEN,
                Json(StatusResponse {
                    success: false,
                    status_message: "username does not match authenticated subject".into(),
                }),
            );
        }
    }
    let r = worker_registration_resources(&body);
    let worker_id = body
        .worker_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&owner)
        .to_string();
    if !is_safe_worker_id(&worker_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "Invalid worker_id".into(),
            }),
        );
    }
    let mut grpc = state.grpc_client.clone();
    match grpc
        .register_worker_node(
            &owner,
            &worker_id,
            &body.ip,
            r,
            &body.location.unwrap_or_else(|| "local".into()),
            &token,
        )
        .await
    {
        Ok(resp) => (
            StatusCode::OK,
            Json(StatusResponse {
                success: resp.success,
                status_message: resp.status_message,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(StatusResponse {
                success: false,
                status_message: e.to_string(),
            }),
        ),
    }
}

/// POST /api/remove-worker
pub async fn remove_worker(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Json(body): Json<RemoveWorkerBody>,
) -> (StatusCode, Json<StatusResponse>) {
    let worker_id = body.worker_id.trim();
    if !is_safe_worker_id(worker_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "Invalid worker_id".into(),
            }),
        );
    }
    let mut grpc = state.grpc_client.clone();
    match grpc.remove_worker(worker_id, &token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(StatusResponse {
                success: resp.success,
                status_message: resp.status_message,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(StatusResponse {
                success: false,
                status_message: e.to_string(),
            }),
        ),
    }
}

/// GET /api/tasks/:task_id/log
pub async fn get_task_log(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    AxumPath(task_id): AxumPath<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(task_id) = normalized_task_id(&task_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success":false,"message":"Invalid task_id"})),
        );
    };
    let mut grpc = state.grpc_client.clone();
    match grpc.get_tasklog(&task_id, &token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(serde_json::json!({"success":resp.success,"task_id":task_id,"log":resp.log})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success":false,"message":format!("gRPC error: {}",e)})),
        ),
    }
}

/// GET /api/tasks/:task_id/result
pub async fn get_task_result(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    AxumPath(task_id): AxumPath<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(task_id) = normalized_task_id(&task_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success":false,"message":"Invalid task_id"})),
        );
    };
    let mut grpc = state.grpc_client.clone();
    match grpc.get_task_result(&task_id, &token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(
                serde_json::json!({"success":resp.success,"task_id":task_id,"result_torrent":resp.result_torrent,"status_message":resp.status_message}),
            ),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success":false,"message":format!("gRPC error: {}",e)})),
        ),
    }
}

/// POST /api/tasks/:task_id/stop
pub async fn stop_task(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    AxumPath(task_id): AxumPath<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(task_id) = normalized_task_id(&task_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success":false,"message":"Invalid task_id"})),
        );
    };
    let mut grpc = state.grpc_client.clone();
    match grpc.stop_task(&task_id, &token).await {
        Ok(resp) if resp.success => (
            StatusCode::OK,
            Json(serde_json::json!({"success":true,"message":resp.status_message})),
        ),
        Ok(resp) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"success":false,"message":resp.status_message})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success":false,"message":format!("gRPC error: {}",e)})),
        ),
    }
}

/// GET /api/provider/earnings
pub async fn get_provider_earnings(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Query(query): Query<ProviderEarningsQuery>,
) -> (StatusCode, Json<ProviderEarningsResponse>) {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let mut grpc = state.grpc_client.clone();
    match grpc.get_provider_earnings(&token, limit).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(ProviderEarningsResponse {
                success: resp.success,
                total_earned_cpt: resp.total_earned_cpt,
                currency: resp.currency,
                entries: resp
                    .entries
                    .into_iter()
                    .map(|e| ProviderEarningsEntry {
                        task_id: e.task_id,
                        payer_user: e.payer_user,
                        provider_worker_id: if e.provider_worker_id.is_empty() {
                            None
                        } else {
                            Some(e.provider_worker_id)
                        },
                        amount_cpt: e.amount_cpt,
                        status: e.status,
                        created_at: e.created_at,
                    })
                    .collect(),
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProviderEarningsResponse {
                success: false,
                total_earned_cpt: 0,
                currency: String::from("CPT"),
                entries: vec![],
            }),
        ),
    }
}

/// GET /api/provider/workers/:worker_id/settings
pub async fn get_provider_worker_settings(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    AxumPath(worker_id): AxumPath<String>,
) -> (StatusCode, Json<ProviderWorkerSettingsResponse>) {
    let Some(worker_id) = normalized_worker_id(&worker_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProviderWorkerSettingsResponse {
                success: false,
                worker_id: String::new(),
                message: "Invalid worker_id".into(),
                settings: None,
            }),
        );
    };
    let mut grpc = state.grpc_client.clone();
    match grpc.get_provider_worker_settings(&token, &worker_id).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(ProviderWorkerSettingsResponse {
                success: resp.success,
                worker_id: resp.worker_id,
                message: resp.message,
                settings: resp.settings.map(|s| s.into()),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProviderWorkerSettingsResponse {
                success: false,
                worker_id,
                message: e.to_string(),
                settings: None,
            }),
        ),
    }
}

/// PUT /api/provider/workers/:worker_id/settings
pub async fn update_provider_worker_settings(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    AxumPath(worker_id): AxumPath<String>,
    Json(body): Json<ProviderWorkerSettingsBody>,
) -> (StatusCode, Json<ProviderWorkerSettingsResponse>) {
    let Some(worker_id) = normalized_worker_id(&worker_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProviderWorkerSettingsResponse {
                success: false,
                worker_id: String::new(),
                message: "Invalid worker_id".into(),
                settings: None,
            }),
        );
    };
    let mut grpc = state.grpc_client.clone();
    match grpc
        .update_provider_worker_settings(
            &token,
            &worker_id,
            body.enabled,
            body.cpu_cores_limit,
            body.memory_gb_limit,
            body.gpu_memory_gb_limit,
            body.storage_gb_limit,
            body.min_cpt_per_hour,
        )
        .await
    {
        Ok(resp) => (
            StatusCode::OK,
            Json(ProviderWorkerSettingsResponse {
                success: resp.success,
                worker_id: resp.worker_id,
                message: resp.message,
                settings: resp.settings.map(|s| s.into()),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProviderWorkerSettingsResponse {
                success: false,
                worker_id,
                message: e.to_string(),
                settings: None,
            }),
        ),
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
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<AdminBillingOverviewResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.get_admin_billing_overview(&token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(AdminBillingOverviewResponse {
                success: resp.success,
                total_payer_debit_cpt: resp.total_payer_debit_cpt,
                total_provider_credit_cpt: resp.total_provider_credit_cpt,
                total_platform_fee_cpt: resp.total_platform_fee_cpt,
                pending_billing_tasks: resp.pending_billing_tasks,
                currency: resp.currency,
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminBillingOverviewResponse {
                success: false,
                total_payer_debit_cpt: 0,
                total_provider_credit_cpt: 0,
                total_platform_fee_cpt: 0,
                pending_billing_tasks: 0,
                currency: "CPT".into(),
            }),
        ),
    }
}

/// GET /api/admin/artifacts/overview
pub async fn get_admin_artifact_overview(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<AdminArtifactOverviewResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.get_admin_artifact_overview(&token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(AdminArtifactOverviewResponse {
                success: resp.success,
                total_artifacts: resp.total_artifacts,
                total_size_bytes: resp.total_size_bytes,
                dedup_hits: resp.dedup_hits,
                resumable_artifacts: resp.resumable_artifacts,
                expiring_in_24h: resp.expiring_in_24h,
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminArtifactOverviewResponse {
                success: false,
                total_artifacts: 0,
                total_size_bytes: 0,
                dedup_hits: 0,
                resumable_artifacts: 0,
                expiring_in_24h: 0,
            }),
        ),
    }
}

/// POST /api/admin/artifacts/cleanup
pub async fn cleanup_admin_artifacts(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Json(body): Json<AdminArtifactCleanupBody>,
) -> (StatusCode, Json<AdminArtifactCleanupResponse>) {
    let dry_run = body.dry_run.unwrap_or(true);
    let mut grpc = state.grpc_client.clone();
    match grpc.cleanup_admin_artifacts(&token, dry_run).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(AdminArtifactCleanupResponse {
                success: resp.success,
                dry_run: resp.dry_run,
                expired_candidates: resp.expired_candidates,
                deleted_rows: resp.deleted_rows,
                deleted_files: resp.deleted_files,
                file_delete_errors: resp.file_delete_errors,
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminArtifactCleanupResponse {
                success: false,
                dry_run,
                expired_candidates: 0,
                deleted_rows: 0,
                deleted_files: 0,
                file_delete_errors: 0,
            }),
        ),
    }
}

/// GET /api/admin/scheduling/cache-metrics
pub async fn get_admin_scheduling_cache_metrics(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<AdminSchedulingCacheMetricsResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.get_admin_scheduling_cache_metrics(&token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(AdminSchedulingCacheMetricsResponse {
                success: resp.success,
                total_completed_tasks: resp.total_completed_tasks,
                total_cache_hits: resp.total_cache_hits,
                cache_hit_rate: resp.cache_hit_rate,
                top_workers: resp
                    .top_workers
                    .into_iter()
                    .map(|w| WorkerCacheAffinityMetric {
                        worker_id: w.worker_id,
                        completed_tasks: w.completed_tasks,
                        cache_hits: w.cache_hits,
                        recent_completed_tasks_7d: w.recent_completed_tasks_7d,
                    })
                    .collect(),
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminSchedulingCacheMetricsResponse {
                success: false,
                total_completed_tasks: 0,
                total_cache_hits: 0,
                cache_hit_rate: 0.0,
                top_workers: vec![],
            }),
        ),
    }
}

/// GET /api/admin/scheduling/cache-alert
pub async fn get_admin_scheduling_cache_alert(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Query(query): Query<AdminSchedulingCacheAlertQuery>,
) -> (StatusCode, Json<AdminSchedulingCacheAlertResponse>) {
    let low = query.low.unwrap_or(40.0);
    let high = query.high.unwrap_or(70.0);
    let mut grpc = state.grpc_client.clone();
    match grpc
        .get_admin_scheduling_cache_alert(&token, low, high)
        .await
    {
        Ok(resp) => (
            StatusCode::OK,
            Json(AdminSchedulingCacheAlertResponse {
                success: resp.success,
                low_threshold: resp.low_threshold,
                high_threshold: resp.high_threshold,
                cache_hit_rate: resp.cache_hit_rate,
                severity: resp.severity,
                message: resp.status_message,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminSchedulingCacheAlertResponse {
                success: false,
                low_threshold: low,
                high_threshold: high,
                cache_hit_rate: 0.0,
                severity: "unknown".into(),
                message: e.to_string(),
            }),
        ),
    }
}

/// GET /api/admin/scheduling/cache-anomalies
pub async fn list_admin_scheduling_cache_anomalies(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Query(query): Query<AdminSchedulingCacheAnomalyQuery>,
) -> (StatusCode, Json<AdminSchedulingCacheAnomalyListResponse>) {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let mut grpc = state.grpc_client.clone();
    match grpc
        .list_admin_scheduling_cache_anomalies(&token, limit)
        .await
    {
        Ok(resp) => (
            StatusCode::OK,
            Json(AdminSchedulingCacheAnomalyListResponse {
                success: resp.success,
                entries: resp
                    .entries
                    .into_iter()
                    .map(|e| AdminSchedulingCacheAnomalyEntry {
                        severity: e.severity,
                        cache_hit_rate: e.cache_hit_rate,
                        low_threshold: e.low_threshold,
                        high_threshold: e.high_threshold,
                        message: e.message,
                        created_at: e.created_at.parse().unwrap_or_default(),
                    })
                    .collect(),
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminSchedulingCacheAnomalyListResponse {
                success: false,
                entries: vec![],
            }),
        ),
    }
}

/// GET /api/provider/workers/:worker_id/trust
pub async fn get_provider_worker_trust_profile(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    AxumPath(worker_id): AxumPath<String>,
) -> (StatusCode, Json<WorkerTrustProfileResponse>) {
    let Some(worker_id) = normalized_worker_id(&worker_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(WorkerTrustProfileResponse {
                success: false,
                message: "Invalid worker_id".into(),
                trust: None,
            }),
        );
    };
    let mut grpc = state.grpc_client.clone();
    match grpc.get_worker_trust_profile(&token, &worker_id).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(WorkerTrustProfileResponse {
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
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WorkerTrustProfileResponse {
                success: false,
                message: e.to_string(),
                trust: None,
            }),
        ),
    }
}

/// PUT /api/admin/workers/:worker_id/trust-control
pub async fn update_worker_trust_control(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    AxumPath(worker_id): AxumPath<String>,
    Json(body): Json<WorkerTrustControlBody>,
) -> (StatusCode, Json<WorkerTrustControlResponse>) {
    let Some(worker_id) = normalized_worker_id(&worker_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(WorkerTrustControlResponse {
                success: false,
                worker_id: String::new(),
                banned: false,
                score: 0,
                message: "Invalid worker_id".into(),
            }),
        );
    };
    let score = body.score.unwrap_or(0);
    let mut grpc = state.grpc_client.clone();
    match grpc
        .update_worker_trust_control(&token, &worker_id, body.banned, score)
        .await
    {
        Ok(resp) => (
            StatusCode::OK,
            Json(WorkerTrustControlResponse {
                success: resp.success,
                worker_id: resp.worker_id,
                banned: resp.banned,
                score: resp.score,
                message: resp.status_message,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WorkerTrustControlResponse {
                success: false,
                worker_id,
                banned: false,
                score: 0,
                message: e.to_string(),
            }),
        ),
    }
}

/// GET /api/admin/workers/trust
pub async fn list_admin_worker_trust(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
) -> (StatusCode, Json<AdminWorkerTrustListResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.list_admin_worker_trust(&token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(AdminWorkerTrustListResponse {
                success: resp.success,
                entries: resp
                    .entries
                    .into_iter()
                    .map(|e| AdminWorkerTrustEntry {
                        worker_id: e.worker_id,
                        username: e.username,
                        worker_status: e.worker_status,
                        score: e.score,
                        banned: e.banned,
                        successful_tasks: e.successful_tasks,
                        failed_tasks: e.failed_tasks,
                        last_attested_at: e.last_attested_at.parse().ok(),
                    })
                    .collect(),
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminWorkerTrustListResponse {
                success: false,
                entries: vec![],
            }),
        ),
    }
}

/// GET /api/admin/audit/logs
pub async fn list_admin_audit_logs(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Query(query): Query<AdminSchedulingCacheAnomalyQuery>,
) -> (StatusCode, Json<AdminAuditLogListResponse>) {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let mut grpc = state.grpc_client.clone();
    match grpc.list_admin_audit_logs(&token, limit).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(AdminAuditLogListResponse {
                success: resp.success,
                entries: resp
                    .entries
                    .into_iter()
                    .map(|e| AdminAuditLogEntry {
                        id: e.id,
                        username: e.username,
                        action: e.action,
                        resource: e.resource,
                        details: e.details,
                        created_at: e.created_at.parse().unwrap_or_default(),
                    })
                    .collect(),
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminAuditLogListResponse {
                success: false,
                entries: vec![],
            }),
        ),
    }
}

pub async fn download_task_artifact(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    AxumPath(task_id): AxumPath<String>,
    Query(query): Query<ArtifactDownloadQuery>,
) -> Response {
    let Some(task_id) = normalized_task_id(&task_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success":false,"message":"Invalid task_id"})),
        )
            .into_response();
    };
    let artifact_key = match normalized_artifact_key(query.artifact_key.as_deref()) {
        Ok(artifact_key) => artifact_key,
        Err(()) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"success":false,"message":"Invalid artifact key"})),
            )
                .into_response();
        }
    };
    if artifact_key.is_some_and(|value| value.len() > 255) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success":false,"message":"Invalid artifact key"})),
        )
            .into_response();
    }

    let mut grpc = state.grpc_client.clone();
    match grpc
        .download_task_artifact(&task_id, &token, artifact_key)
        .await
    {
        Ok(resp) if resp.success => {
            let filename = safe_download_filename(&resp.filename);
            let content_type = if resp.content_type.trim().is_empty() {
                "application/octet-stream".to_string()
            } else {
                resp.content_type
            };
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                resp.data,
            )
                .into_response()
        }
        Ok(resp) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success":false,"message":resp.status_message})),
        )
            .into_response(),
        Err(e) => (
            artifact_grpc_error_status(&e),
            Json(serde_json::json!({"success":false,"message":e.to_string()})),
        )
            .into_response(),
    }
}

fn normalized_artifact_key(value: Option<&str>) -> Result<Option<&str>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(());
    }
    Ok(Some(trimmed))
}

fn artifact_grpc_error_status(error: &tonic::Status) -> StatusCode {
    match error.code() {
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        tonic::Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn safe_download_filename(filename: &str) -> String {
    let sanitized: String = filename
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if sanitized.is_empty() || sanitized.chars().all(|c| c == '.') {
        "artifact.bin".into()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::post, Router};
    use tower::ServiceExt;

    fn admin_users_env_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[test]
    fn public_registration_reserves_configured_admin_username() {
        // Given: the public HTTP registration boundary and a configured admin name.
        let _lock = admin_users_env_lock().lock().unwrap();
        let previous = std::env::var_os("HIVEMIND_ADMIN_USERS");
        std::env::set_var("HIVEMIND_ADMIN_USERS", "other-admin, platform-admin");

        // When: the registration policy evaluates the configured admin username.
        let reserved = is_reserved_admin_username("platform-admin");

        // Then: the HTTP boundary reserves it from public registration.
        match previous {
            Some(value) => std::env::set_var("HIVEMIND_ADMIN_USERS", value),
            None => std::env::remove_var("HIVEMIND_ADMIN_USERS"),
        }
        assert!(reserved);
    }

    #[test]
    fn public_registration_allows_non_admin_username() {
        // Given: one configured admin and a distinct public username.
        let _lock = admin_users_env_lock().lock().unwrap();
        let previous = std::env::var_os("HIVEMIND_ADMIN_USERS");
        std::env::set_var("HIVEMIND_ADMIN_USERS", "platform-admin");

        // When: the registration policy evaluates the non-admin username.
        let reserved = is_reserved_admin_username("ordinary-user");

        // Then: the HTTP boundary leaves that username available.
        match previous {
            Some(value) => std::env::set_var("HIVEMIND_ADMIN_USERS", value),
            None => std::env::remove_var("HIVEMIND_ADMIN_USERS"),
        }
        assert!(!reserved);
    }

    #[tokio::test]
    async fn json_task_creation_rejects_non_empty_zip_path_at_http_boundary() {
        // Given: an authenticated JSON task-create payload naming a local master path.
        let app = Router::new().route(
            "/api/tasks",
            post(|Json(_body): Json<CreateTaskBody>| async { StatusCode::CREATED }),
        );
        let request = Request::builder()
            .method("POST")
            .uri("/api/tasks")
            .header(header::AUTHORIZATION, "Bearer authenticated-test-token")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "task_id": "caller-path-probe",
                    "zip_path": "C:\\master-secrets\\private.zip"
                })
                .to_string(),
            ))
            .unwrap();

        // When: axum parses the request through the production JSON boundary type.
        let response = app.oneshot(request).await.unwrap();

        // Then: parsing rejects the path before the task handler can access the filesystem.
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn task_submit_limiter_blocks_after_limit_until_window_resets() {
        let mut limiter = TaskSubmitRateLimiter::new();
        let now = Instant::now();

        assert!(limiter.allow("alice", 2, now));
        assert!(limiter.allow("alice", 2, now + Duration::from_secs(1)));
        assert!(!limiter.allow("alice", 2, now + Duration::from_secs(2)));
        assert!(limiter.allow("alice", 2, now + Duration::from_secs(60)));
    }

    #[test]
    fn task_submit_limiter_zero_limit_blocks_all_submissions() {
        let mut limiter = TaskSubmitRateLimiter::new();

        assert!(!limiter.allow("alice", 0, Instant::now()));
    }

    #[test]
    fn uploaded_file_size_error_allows_exact_limit_and_rejects_over_limit() {
        assert!(uploaded_file_size_error(1024, 1024).is_none());
        assert!(uploaded_file_size_error(1025, 1024)
            .unwrap()
            .contains("too large"));
    }

    #[test]
    fn task_id_safety_rejects_single_dot_segment() {
        assert!(is_safe_task_id("task-123"));
        assert!(!is_safe_task_id("."));
    }

    #[test]
    fn worker_id_safety_rejects_path_normalizing_values() {
        assert!(is_safe_worker_id("worker-123"));
        assert!(!is_safe_worker_id("."));
        assert!(!is_safe_worker_id("../worker"));
        assert!(!is_safe_worker_id("worker/child"));
    }

    #[test]
    fn artifact_download_grpc_errors_map_to_http_statuses() {
        assert_eq!(
            artifact_grpc_error_status(&tonic::Status::not_found("missing artifact")),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            artifact_grpc_error_status(&tonic::Status::unauthenticated("invalid token")),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            artifact_grpc_error_status(&tonic::Status::permission_denied("forbidden")),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            artifact_grpc_error_status(&tonic::Status::invalid_argument("bad key")),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            artifact_grpc_error_status(&tonic::Status::unavailable("nodepool down")),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            artifact_grpc_error_status(&tonic::Status::internal("backend error")),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn artifact_key_normalization_rejects_blank_explicit_selector() {
        assert_eq!(normalized_artifact_key(None).unwrap(), None);
        assert_eq!(normalized_artifact_key(Some("")).unwrap(), None);
        assert_eq!(
            normalized_artifact_key(Some(" stdout ")).unwrap(),
            Some("stdout")
        );
        assert!(normalized_artifact_key(Some("   ")).is_err());
    }

    #[test]
    fn safe_download_filename_falls_back_for_dot_only_names() {
        assert_eq!(safe_download_filename("."), "artifact.bin");
        assert_eq!(safe_download_filename(".."), "artifact.bin");
        assert_eq!(safe_download_filename("../.."), "artifact.bin");
        assert_eq!(safe_download_filename("..\\.."), "artifact.bin");
    }

    #[test]
    fn worker_registration_resources_preserve_ui_capacity_fields() {
        let body = RegisterWorkerBody {
            username: Some("provider".into()),
            worker_id: Some("worker-42".into()),
            ip: "127.0.0.1:50053".into(),
            cpu_cores: 8,
            memory_gb: 32,
            cpu_score: 900,
            gpu_score: Some(1200),
            gpu_memory_gb: Some(16),
            gpu_name: Some("RTX Test".into()),
            storage_total_gb: Some(1000),
            storage_available_gb: Some(750),
            location: Some("taipei".into()),
        };

        let resources = worker_registration_resources(&body);

        assert_eq!(resources.cpu_cores, 8);
        assert_eq!(resources.memory_mb, 32 * 1024);
        assert_eq!(resources.gpu_count, 1);
        assert_eq!(resources.gpu_name, "RTX Test");
        assert_eq!(resources.vram_mb, 16 * 1024);
        assert_eq!(resources.cpu_score, 900);
        assert_eq!(resources.gpu_score, 1200);
        assert_eq!(resources.storage_total_gb, 1000);
        assert_eq!(resources.storage_available_gb, 750);
    }

    #[test]
    fn uploaded_distribution_preserves_package_bytes_for_nodepool_seed() {
        // Given: multipart upload bytes already read at the HTTP boundary.
        let body = CreateTaskBody {
            task_id: "zip-nodepool".into(),
            torrent: None,
            runtime: None,
            task_source: None,
            memory_gb: None,
            cpu_score: None,
            gpu_score: None,
            gpu_memory_gb: None,
            storage_gb: None,
            location: None,
            host_count: None,
            max_cpt: None,
        };
        let uploaded_package = TaskDistribution {
            torrent_source: None,
            package_data: b"package-bytes-for-nodepool".to_vec(),
            package_filename: "zip-nodepool.zip".into(),
        };

        // When: task distribution selects the trusted multipart package.
        let distribution = resolve_task_distribution(&body, Some(uploaded_package));

        // Then: nodepool receives the same bytes and generated package name.
        assert!(distribution.torrent_source.is_none());
        assert_eq!(distribution.package_data, b"package-bytes-for-nodepool");
        assert_eq!(distribution.package_filename, "zip-nodepool.zip");
    }

    #[test]
    fn torrent_only_distribution_keeps_reference_without_package_bytes() {
        // Given: JSON task creation with a supported torrent reference.
        let body = CreateTaskBody {
            task_id: "magnet-only".into(),
            torrent: Some("magnet:?xt=urn:btih:abc".into()),
            runtime: None,
            task_source: None,
            memory_gb: None,
            cpu_score: None,
            gpu_score: None,
            gpu_memory_gb: None,
            storage_gb: None,
            location: None,
            host_count: None,
            max_cpt: None,
        };

        // When: task distribution has no multipart package.
        let distribution = resolve_task_distribution(&body, None);

        // Then: the torrent reference is preserved without package bytes.
        assert_eq!(
            distribution.torrent_source.as_deref(),
            Some("magnet:?xt=urn:btih:abc")
        );
        assert!(distribution.package_data.is_empty());
    }
}

use anyhow::Result;
use axum::{
    extract::State,
    http::{header, HeaderValue, Method, StatusCode},
    routing::{get, post},
    Json, Router,
};
use hivemind_client_runtime::{self as client_runtime, ClientRole};
use hivemind_config::{HivemindConfig, WorkerAdmissionMode};
use hivemind_models::ResourceSpec;
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;

use crate::grpc_server::{GrpcWorkerNodeService, WorkerIdentityHandle};
use crate::nodepool_client::{
    self, capability_report_to_proto, login_to_nodepool, register_once_with_capability_report,
};
use crate::{ResourceSample, WorkerExecutor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerProfile {
    pub worker_id: String,
    pub ip: String,
    pub location: String,
    pub cpu_cores: i32,
    pub memory_gb: i64,
    pub cpu_score: i32,
    pub gpu_score: i32,
    pub gpu_memory_gb: i64,
    pub storage_total_gb: i64,
    pub storage_available_gb: i64,
    pub gpu_name: String,
}

impl WorkerProfile {
    pub fn from_resource_spec(
        worker_id: String,
        ip: String,
        location: String,
        spec: ResourceSpec,
    ) -> Self {
        Self {
            worker_id,
            ip,
            location,
            cpu_cores: spec.cpu_cores,
            memory_gb: spec.memory_mb / 1024,
            cpu_score: spec.cpu_score,
            gpu_score: spec.gpu_score,
            gpu_memory_gb: spec.vram_mb / 1024,
            storage_total_gb: spec.storage_total_gb,
            storage_available_gb: spec.storage_available_gb,
            gpu_name: spec.gpu_name,
        }
    }

    fn to_resource_spec(&self) -> ResourceSpec {
        ResourceSpec {
            cpu_cores: self.cpu_cores,
            memory_mb: self.memory_gb * 1024,
            gpu_count: if self.gpu_score > 0 || self.gpu_memory_gb > 0 || !self.gpu_name.is_empty()
            {
                1
            } else {
                0
            },
            gpu_name: self.gpu_name.clone(),
            vram_mb: self.gpu_memory_gb * 1024,
            cpu_score: self.cpu_score,
            gpu_score: self.gpu_score,
            storage_total_gb: self.storage_total_gb,
            storage_available_gb: self.storage_available_gb,
        }
    }
}

#[derive(Clone)]
pub struct ControlApiState {
    pub profile: WorkerProfile,
    pub worker_addr: std::sync::Arc<std::sync::Mutex<String>>,
    pub nodepool_addr: std::sync::Arc<std::sync::Mutex<String>>,
    pub config: HivemindConfig,
    pub executor: std::sync::Arc<WorkerExecutor>,
    pub worker_service: Option<std::sync::Arc<GrpcWorkerNodeService>>,
    pub worker_identity: WorkerIdentityHandle,
    pub registration_shutdown:
        std::sync::Arc<std::sync::Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
    pub session_shutdown:
        std::sync::Arc<std::sync::Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
}

impl ControlApiState {
    fn set_worker_addr(&self, addr: impl Into<String>) {
        let mut guard = self
            .worker_addr
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        *guard = addr.into();
    }

    fn nodepool_addr(&self) -> String {
        self.nodepool_addr
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn set_nodepool_addr(&self, endpoint: impl Into<String>) {
        let endpoint = endpoint.into();
        let mut guard = self
            .nodepool_addr
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        *guard = endpoint;
    }

    fn set_worker_identity(&self, worker_id: &str) {
        let mut identity = self
            .worker_identity
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        *identity = Some(worker_id.to_string());
    }

    fn current_worker_identity(&self) -> Option<String> {
        self.worker_identity
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn ensure_registration_loop(&self, username: &str, worker_id: &str, token: &str) {
        let mut guard = self
            .registration_shutdown
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if guard.is_some() {
            return;
        }
        let shutdown = nodepool_client::start_registration_loop(
            self.executor.clone(),
            nodepool_client::RegistrationLoopConfig {
                nodepool_addr: self.nodepool_addr.clone(),
                worker_id: worker_id.to_string(),
                username: username.to_string(),
                worker_addr: self.worker_addr.clone(),
                worker_grpc_addr: self.config.server.worker_grpc_addr.clone(),
                location: self.profile.location.clone(),
                token: token.to_string(),
                interval: std::time::Duration::from_secs(10),
                require_external_overlay: client_runtime::external_overlay_required(
                    &self.config,
                    ClientRole::Worker,
                ),
            },
        );
        *guard = Some(shutdown);
    }

    fn ensure_session_loop(&self, username: &str, worker_id: &str, token: &str) {
        let client_instance_id = match client_runtime::client_instance_id(ClientRole::Worker) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!("Worker session identity is unavailable: {error}");
                return;
            }
        };
        let mut guard = self
            .session_shutdown
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if guard.as_ref().is_some_and(|shutdown| !shutdown.is_closed()) {
            return;
        }
        let Some(worker_service) = self.worker_service.clone() else {
            tracing::warn!("Worker session service is unavailable");
            return;
        };
        tracing::info!(
            worker_id = %worker_id,
            "Starting Worker outbound session loop"
        );
        let shutdown = nodepool_client::start_session_loop(
            self.executor.clone(),
            nodepool_client::SessionLoopConfig {
                nodepool_addr: self.nodepool_addr.clone(),
                worker_id: worker_id.to_string(),
                username: username.to_string(),
                client_instance_id,
                token: token.to_string(),
                interval: std::time::Duration::from_secs(10),
                service: worker_service,
                require_external_overlay: client_runtime::external_overlay_required(
                    &self.config,
                    ClientRole::Worker,
                ),
            },
        );
        *guard = Some(shutdown);
    }
}

#[derive(Debug, Clone, Serialize)]
struct WorkerInfoResponse {
    success: bool,
    profile: WorkerProfile,
}

#[derive(Debug, Clone, Serialize)]
struct WorkerDashboardHost {
    cpu_cores: i32,
    cpu_usage_percent: f64,
    memory_total_gb: i32,
    memory_available_gb: i32,
    memory_usage_percent: f64,
    gpu_count: Option<i32>,
    gpu_utilization_percent: Option<f64>,
    vram_total_mb: Option<i64>,
    vram_available_mb: Option<i64>,
    storage_total_gb: Option<i64>,
    storage_available_gb: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct WorkerDashboardAssignment {
    task_id: String,
    submitter: String,
    status: String,
    max_cpt: i64,
    reported_usage_cpt: Option<i64>,
    usage_basis: String,
    usage_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct WorkerDashboardResponse {
    success: bool,
    worker_id: String,
    sampled_at: String,
    stale: bool,
    host: WorkerDashboardHost,
    assignments: Vec<WorkerDashboardAssignment>,
    settled_provider_credits_cpt: i64,
    currency: String,
}

#[derive(Debug, Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegisterWorkerBody {
    username: Option<String>,
    worker_id: Option<String>,
    /// Optional callback address. Workers that deliver results only through
    /// the outbound session may omit it; legacy direct callers keep sending
    /// a reachable host:port.
    ip: Option<String>,
    cpu_cores: i32,
    memory_gb: i64,
    cpu_score: i32,
    gpu_score: Option<i32>,
    gpu_memory_gb: Option<i64>,
    gpu_name: Option<String>,
    storage_total_gb: Option<i64>,
    storage_available_gb: Option<i64>,
    location: Option<String>,
}

#[derive(Debug, Serialize)]
struct VpnBootstrapResponse {
    success: bool,
    state: String,
    endpoint: Option<String>,
    overlay_ip: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    success: bool,
    status_message: String,
}

pub fn router(profile: WorkerProfile) -> Router {
    let config = HivemindConfig::default();
    router_with_allowed_origins(
        ControlApiState {
            profile: profile.clone(),
            worker_addr: std::sync::Arc::new(std::sync::Mutex::new(profile.ip.clone())),
            nodepool_addr: std::sync::Arc::new(std::sync::Mutex::new(
                client_runtime::resolve_nodepool_grpc_endpoint(&config),
            )),
            config: config.clone(),
            executor: std::sync::Arc::new(WorkerExecutor::new(config.clone())),
            worker_service: None,
            worker_identity: std::sync::Arc::new(std::sync::Mutex::new(Some(
                profile.worker_id.clone(),
            ))),
            registration_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
            session_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
        },
        &config.server.worker_control_cors_allowed_origins,
    )
}

pub fn router_with_allowed_origins(state: ControlApiState, allowed_origins: &[String]) -> Router {
    router_with_ui_dir(state, allowed_origins, None)
}

// Backward-compatible helper used by older call sites/tests that only pass a profile.
pub fn router_with_profile_and_allowed_origins(
    profile: WorkerProfile,
    allowed_origins: &[String],
) -> Router {
    let config = HivemindConfig::default();
    router_with_allowed_origins(
        ControlApiState {
            profile: profile.clone(),
            worker_addr: std::sync::Arc::new(std::sync::Mutex::new(profile.ip.clone())),
            nodepool_addr: std::sync::Arc::new(std::sync::Mutex::new(
                client_runtime::resolve_nodepool_grpc_endpoint(&config),
            )),
            config: config.clone(),
            executor: std::sync::Arc::new(WorkerExecutor::new(config.clone())),
            worker_service: None,
            worker_identity: std::sync::Arc::new(std::sync::Mutex::new(Some(
                profile.worker_id.clone(),
            ))),
            registration_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
            session_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
        },
        allowed_origins,
    )
}

pub fn router_with_ui_dir(
    state: ControlApiState,
    allowed_origins: &[String],
    ui_dir: Option<&str>,
) -> Router {
    let cors = build_cors_layer(allowed_origins);

    let app = Router::new()
        .route("/api/worker-info", get(worker_info))
        .route("/api/worker-dashboard", get(worker_dashboard))
        .route("/api/vpn/bootstrap", post(bootstrap_vpn))
        .route("/api/vpn/status", get(vpn_status))
        .route("/api/login", post(login))
        .route("/api/register-worker", post(register_worker))
        .with_state(state)
        .layer(cors);
    match ui_dir.filter(|dir| std::path::Path::new(dir).is_dir()) {
        Some(dir) => {
            app.fallback_service(ServeDir::new(dir).append_index_html_on_directories(true))
        }
        None => app,
    }
}

fn build_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins = allowed_origins
        .iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

pub async fn serve(addr: &str, profile: WorkerProfile) -> Result<()> {
    let config = HivemindConfig::default();
    serve_with_allowed_origins(
        addr,
        ControlApiState {
            profile: profile.clone(),
            worker_addr: std::sync::Arc::new(std::sync::Mutex::new(profile.ip.clone())),
            nodepool_addr: std::sync::Arc::new(std::sync::Mutex::new(
                client_runtime::resolve_nodepool_grpc_endpoint(&config),
            )),
            config: config.clone(),
            executor: std::sync::Arc::new(WorkerExecutor::new(config.clone())),
            worker_service: None,
            worker_identity: std::sync::Arc::new(std::sync::Mutex::new(Some(
                profile.worker_id.clone(),
            ))),
            registration_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
            session_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
        },
        &config.server.worker_control_cors_allowed_origins,
        Some(&config.server.worker_ui_dir),
    )
    .await
}

pub async fn serve_with_allowed_origins(
    addr: &str,
    state: ControlApiState,
    allowed_origins: &[String],
    ui_dir: Option<&str>,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let listen_addr = listener.local_addr()?;
    let ui_available = ui_dir
        .map(|dir| std::path::Path::new(dir).join("index.html").is_file())
        .unwrap_or(false);
    tokio::spawn(async move {
        client_runtime::open_worker_ui_when_ready(listen_addr, ui_available).await;
    });
    axum::serve(listener, router_with_ui_dir(state, allowed_origins, ui_dir)).await?;
    Ok(())
}

async fn worker_info(
    State(state): State<ControlApiState>,
) -> std::result::Result<Json<WorkerInfoResponse>, (StatusCode, Json<StatusResponse>)> {
    let mut profile = state.profile.clone();
    if let Some(worker_id) = state.current_worker_identity() {
        profile.worker_id = worker_id;
    }
    if let Some(session) = client_runtime::current_vpn_session(ClientRole::Worker).await {
        if let Some(ip) = session.overlay_ip.as_deref() {
            profile.ip = worker_info_overlay_addr(
                &profile.ip,
                &state.config.server.worker_grpc_addr,
                ip,
            )
            .map_err(|error| {
                tracing::warn!(error = %error, "Worker VPN callback address is unavailable");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(StatusResponse {
                        success: false,
                        status_message: "Worker VPN callback address is unavailable".into(),
                    }),
                )
            })?;
        }
    }
    state.set_worker_addr(profile.ip.clone());
    Ok(Json(WorkerInfoResponse {
        success: true,
        profile,
    }))
}

const RESOURCE_SAMPLE_STALE_AFTER: chrono::Duration = chrono::Duration::seconds(30);

async fn worker_dashboard(
    State(state): State<ControlApiState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<WorkerDashboardResponse>, (StatusCode, Json<StatusResponse>)> {
    let token = bearer_token(&headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(StatusResponse {
                success: false,
                status_message: "Bearer token is required".into(),
            }),
        )
    })?;
    let worker_id = state.current_worker_identity().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(StatusResponse {
                success: false,
                status_message: "Worker is not registered with Nodepool".into(),
            }),
        )
    })?;
    let sample = state.executor.latest_resource_sample().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(StatusResponse {
                success: false,
                status_message: "Worker resource sample is not yet available".into(),
            }),
        )
    })?;
    let sampled_at = sample.sampled_at;
    let stale = resource_sample_is_stale(sampled_at, chrono::Utc::now());

    let (assignments, earnings) = nodepool_client::get_provider_worker_dashboard_once(
        &state.nodepool_addr(),
        &token,
        &worker_id,
    )
    .await
    .map_err(|error| {
        tracing::warn!(worker_id = %worker_id, error = %error, "Worker dashboard Nodepool request failed");
        let status = nodepool_dashboard_http_status(&error);
        (
            status,
            Json(StatusResponse {
                success: false,
                status_message: match status {
                    StatusCode::UNAUTHORIZED => "Bearer token was rejected by Nodepool",
                    StatusCode::FORBIDDEN => "Worker is not owned by the authenticated account",
                    _ => "Nodepool dashboard data is unavailable",
                }
                .into(),
            }),
        )
    })?;

    if !assignments.success || assignments.worker_id != worker_id {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(StatusResponse {
                success: false,
                status_message: "Nodepool returned an invalid Worker assignment response".into(),
            }),
        ));
    }
    if !earnings.success {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(StatusResponse {
                success: false,
                status_message: "Nodepool provider earnings data is unavailable".into(),
            }),
        ));
    }

    Ok(Json(WorkerDashboardResponse {
        success: true,
        worker_id,
        sampled_at: sampled_at.to_rfc3339(),
        stale,
        host: worker_dashboard_host(&sample),
        assignments: assignments
            .assignments
            .into_iter()
            .map(|assignment| WorkerDashboardAssignment {
                task_id: assignment.task_id,
                submitter: assignment.submitter,
                status: assignment.status,
                max_cpt: assignment.max_cpt,
                reported_usage_cpt: assignment.reported_usage_cpt,
                usage_basis: assignment.usage_basis,
                usage_updated_at: assignment.usage_updated_at,
            })
            .collect(),
        settled_provider_credits_cpt: earnings.total_earned_cpt,
        currency: earnings.currency,
    }))
}

fn resource_sample_is_stale(
    sampled_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    now.signed_duration_since(sampled_at) > RESOURCE_SAMPLE_STALE_AFTER
}

fn worker_dashboard_host(sample: &ResourceSample) -> WorkerDashboardHost {
    let resources = &sample.resources;
    let has_gpu = resources.gpu_count > 0;
    let vram_total = resources
        .gpu_infos
        .iter()
        .map(|gpu| gpu.vram_total_mb.max(0))
        .sum::<i64>();
    let vram_available = resources
        .gpu_infos
        .iter()
        .map(|gpu| gpu.vram_available_mb.max(0))
        .sum::<i64>();

    WorkerDashboardHost {
        cpu_cores: resources.cpu_cores,
        cpu_usage_percent: resources.cpu_usage_percent,
        memory_total_gb: resources.total_memory_gb,
        memory_available_gb: resources.available_memory_gb,
        memory_usage_percent: resources.memory_usage_percent,
        gpu_count: resources
            .gpu_inventory_supported
            .then_some(resources.gpu_count),
        gpu_utilization_percent: (resources.gpu_utilization_supported && has_gpu).then(|| {
            resources
                .gpu_infos
                .iter()
                .map(|gpu| gpu.gpu_utilization_percent)
                .fold(0.0_f64, f64::max)
        }),
        vram_total_mb: (resources.vram_total_supported && has_gpu && vram_total > 0)
            .then_some(vram_total),
        vram_available_mb: (resources.vram_available_supported && has_gpu && vram_total > 0)
            .then_some(vram_available),
        storage_total_gb: (resources.storage_supported && resources.storage_total_gb > 0)
            .then_some(resources.storage_total_gb),
        storage_available_gb: (resources.storage_supported && resources.storage_total_gb > 0)
            .then_some(resources.storage_available_gb),
    }
}

fn nodepool_dashboard_http_status(error: &anyhow::Error) -> StatusCode {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<tonic::Status>())
        .find_map(|status| match status.code() {
            tonic::Code::Unauthenticated => Some(StatusCode::UNAUTHORIZED),
            tonic::Code::PermissionDenied => Some(StatusCode::FORBIDDEN),
            _ => None,
        })
        .unwrap_or(StatusCode::BAD_GATEWAY)
}

fn worker_info_overlay_addr(
    profile_ip: &str,
    worker_grpc_addr: &str,
    overlay_ip: &str,
) -> Result<String> {
    nodepool_client::forwarded_overlay_advertise_addr(worker_grpc_addr, profile_ip, overlay_ip)
}

async fn bootstrap_vpn(
    State(state): State<ControlApiState>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<VpnBootstrapResponse>) {
    let Some(token) = bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(vpn_bootstrap_response(
                &state.config,
                ClientRole::Worker,
                client_runtime::current_vpn_status(ClientRole::Worker),
                Some("missing bearer token".into()),
            )),
        );
    };

    match client_runtime::ensure_user_vpn_for_token(&state.config, ClientRole::Worker, &token).await
    {
        Ok(Some(endpoint)) => {
            state.set_nodepool_addr(endpoint);
            let status = client_runtime::current_vpn_status(ClientRole::Worker);
            let http_status =
                vpn_bootstrap_http_status_for(&state.config, ClientRole::Worker, &status);
            (
                http_status,
                Json(vpn_bootstrap_response(
                    &state.config,
                    ClientRole::Worker,
                    status,
                    None,
                )),
            )
        }
        Ok(None) => {
            let status = client_runtime::current_vpn_status(ClientRole::Worker);
            let http_status =
                vpn_bootstrap_http_status_for(&state.config, ClientRole::Worker, &status);
            (
                http_status,
                Json(vpn_bootstrap_response(
                    &state.config,
                    ClientRole::Worker,
                    status,
                    None,
                )),
            )
        }
        Err(err) => {
            tracing::warn!("Worker VPN bootstrap failed: {err}");
            let status = client_runtime::current_vpn_status(ClientRole::Worker);
            let http_status =
                vpn_bootstrap_http_status_for(&state.config, ClientRole::Worker, &status);
            (
                http_status,
                Json(vpn_bootstrap_response(
                    &state.config,
                    ClientRole::Worker,
                    status,
                    Some("VPN/Nodepool bootstrap failed".into()),
                )),
            )
        }
    }
}

async fn vpn_status(
    State(state): State<ControlApiState>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<VpnBootstrapResponse>) {
    if bearer_token(&headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(vpn_bootstrap_response(
                &state.config,
                ClientRole::Worker,
                client_runtime::current_vpn_status(ClientRole::Worker),
                Some("missing bearer token".into()),
            )),
        );
    }
    let status = client_runtime::current_vpn_status(ClientRole::Worker);
    let http_status = vpn_bootstrap_http_status_for(&state.config, ClientRole::Worker, &status);
    (
        http_status,
        Json(vpn_bootstrap_response(
            &state.config,
            ClientRole::Worker,
            status,
            None,
        )),
    )
}

fn vpn_bootstrap_response(
    config: &HivemindConfig,
    role: ClientRole,
    status: client_runtime::VpnBootstrapStatus,
    fallback_message: Option<String>,
) -> VpnBootstrapResponse {
    let success = client_runtime::vpn_bootstrap_status_success(config, role, &status);
    VpnBootstrapResponse {
        success,
        state: status.state.as_str().to_string(),
        endpoint: status.endpoint,
        overlay_ip: status.overlay_ip,
        message: status.message.or(fallback_message),
    }
}

fn vpn_bootstrap_http_status_for(
    config: &HivemindConfig,
    role: ClientRole,
    status: &client_runtime::VpnBootstrapStatus,
) -> StatusCode {
    if client_runtime::vpn_bootstrap_status_success(config, role, status) {
        StatusCode::OK
    } else {
        vpn_bootstrap_http_status(status.state)
    }
}

fn vpn_bootstrap_http_status(state: client_runtime::VpnBootstrapState) -> StatusCode {
    match state {
        client_runtime::VpnBootstrapState::ReauthenticationRequired => StatusCode::UNAUTHORIZED,
        client_runtime::VpnBootstrapState::RetryableFailure => StatusCode::BAD_GATEWAY,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn login(
    State(state): State<ControlApiState>,
    Json(body): Json<LoginBody>,
) -> (StatusCode, Json<LoginResponse>) {
    let require_external_overlay =
        client_runtime::external_overlay_required(&state.config, ClientRole::Worker);
    // Prefer automatic website-api VPN bootstrap for remote workers. Local
    // compose can disable it with WORKER_DISABLE_WEBSITE_VPN=1.
    let bootstrap_endpoint = match client_runtime::ensure_user_vpn(
        &state.config,
        ClientRole::Worker,
        &body.username,
        &body.password,
        None,
    )
    .await
    {
        Ok(Some(endpoint)) => {
            state.set_nodepool_addr(endpoint.clone());
            tracing::info!("Worker VPN bootstrap succeeded before nodepool login");
            Some(endpoint)
        }
        Ok(None) if require_external_overlay => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(LoginResponse {
                    success: false,
                    message: "External overlay enrollment is required before Worker login".into(),
                    token: None,
                }),
            );
        }
        Ok(None) => None,
        Err(err) => {
            let message = err.to_string();
            tracing::warn!("Worker VPN bootstrap before login failed: {}", message);
            if require_external_overlay
                || message.contains("nodepool endpoint")
                || message.contains("VPN bootstrap")
                || message.contains("tailscale")
                || message.contains("website-api")
            {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(LoginResponse {
                        success: false,
                        message: format!("VPN/nodepool bootstrap failed: {message}"),
                        token: None,
                    }),
                );
            }
            None
        }
    };

    let nodepool_addr = state.nodepool_addr();
    match login_to_nodepool(&nodepool_addr, &body.username, &body.password).await {
        Ok(token) => {
            // If nodepool was already reachable, still ensure VPN for subsequent
            // overlay-only control-plane operations when website-api is configured.
            match client_runtime::ensure_user_vpn(
                &state.config,
                ClientRole::Worker,
                &body.username,
                &body.password,
                Some(token.as_str()),
            )
            .await
            {
                Ok(Some(endpoint)) => state.set_nodepool_addr(endpoint),
                Ok(None) if require_external_overlay => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(LoginResponse {
                            success: false,
                            message: "External overlay enrollment was lost after Worker login"
                                .into(),
                            token: None,
                        }),
                    );
                }
                Ok(None) => {}
                Err(err) if require_external_overlay => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(LoginResponse {
                            success: false,
                            message: format!("VPN/nodepool bootstrap failed after login: {err}"),
                            token: None,
                        }),
                    );
                }
                Err(err) => {
                    tracing::warn!("Worker VPN bootstrap after login failed: {}", err);
                }
            }
            (
                StatusCode::OK,
                Json(LoginResponse {
                    success: true,
                    message: "Login successful".into(),
                    token: Some(token),
                }),
            )
        }
        Err(err) => {
            // VPN bootstrap already completed. Retry the configured endpoint
            // directly instead of issuing another website login/VPN config,
            // which previously added another 15-30 seconds to every failure.
            if let Some(endpoint) = bootstrap_endpoint {
                state.set_nodepool_addr(endpoint);
                let retry_addr = state.nodepool_addr();
                if let Err(retry_err) =
                    login_to_nodepool(&retry_addr, &body.username, &body.password).await
                {
                    let message = retry_err.to_string();
                    let status = if message.contains("invalid credentials")
                        || message.contains("nodepool login failed")
                    {
                        StatusCode::UNAUTHORIZED
                    } else {
                        StatusCode::BAD_GATEWAY
                    };
                    return (
                        status,
                        Json(LoginResponse {
                            success: false,
                            message: format!("nodepool unavailable after VPN bootstrap: {message}"),
                            token: None,
                        }),
                    );
                }
            }

            // Common remote path: website-api is public, nodepool is VPN-only.
            if let Ok(Some(endpoint)) = client_runtime::ensure_user_vpn(
                &state.config,
                ClientRole::Worker,
                &body.username,
                &body.password,
                None,
            )
            .await
            {
                state.set_nodepool_addr(endpoint);
                let nodepool_addr = state.nodepool_addr();
                match login_to_nodepool(&nodepool_addr, &body.username, &body.password).await {
                    Ok(token) => {
                        return (
                            StatusCode::OK,
                            Json(LoginResponse {
                                success: true,
                                message: "Login successful".into(),
                                token: Some(token),
                            }),
                        );
                    }
                    Err(retry_err) => {
                        let message = retry_err.to_string();
                        let status = if message.contains("invalid credentials")
                            || message.contains("nodepool login failed")
                        {
                            StatusCode::UNAUTHORIZED
                        } else {
                            StatusCode::BAD_GATEWAY
                        };
                        return (
                            status,
                            Json(LoginResponse {
                                success: false,
                                message: format!(
                                    "nodepool unavailable after VPN bootstrap: {message}"
                                ),
                                token: None,
                            }),
                        );
                    }
                }
            }

            let message = err.to_string();
            let status = if message.contains("invalid credentials")
                || message.contains("nodepool login failed")
            {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_GATEWAY
            };
            (
                status,
                Json(LoginResponse {
                    success: false,
                    message,
                    token: None,
                }),
            )
        }
    }
}

async fn register_worker(
    State(state): State<ControlApiState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RegisterWorkerBody>,
) -> (StatusCode, Json<StatusResponse>) {
    let token = bearer_token(&headers).unwrap_or_default();
    let require_external_overlay =
        client_runtime::external_overlay_required(&state.config, ClientRole::Worker);
    if token.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(StatusResponse {
                success: false,
                status_message: "missing bearer token".into(),
            }),
        );
    }

    match client_runtime::ensure_user_vpn_for_token(&state.config, ClientRole::Worker, &token).await
    {
        Ok(Some(endpoint)) => state.set_nodepool_addr(endpoint),
        Ok(None) if require_external_overlay => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(StatusResponse {
                    success: false,
                    status_message:
                        "External overlay enrollment is required before Worker registration".into(),
                }),
            );
        }
        Ok(None) => {}
        Err(err) => {
            let vpn_status = client_runtime::current_vpn_status(ClientRole::Worker);
            tracing::warn!(
                "Worker registration VPN readiness gate failed (state={}): {}",
                vpn_status.state.as_str(),
                err
            );
            return (
                vpn_bootstrap_http_status(vpn_status.state),
                Json(StatusResponse {
                    success: false,
                    status_message: vpn_status
                        .message
                        .unwrap_or_else(|| "VPN/Nodepool bootstrap failed".into()),
                }),
            );
        }
    }

    let mut server_enrollment = None;
    if state.config.general_compute.admission_mode == WorkerAdmissionMode::PublicDynamic {
        match client_runtime::ensure_client_enrollment(&state.config, ClientRole::Worker, &token)
            .await
        {
            Ok(enrollment) => server_enrollment = Some(enrollment),
            // A deployment without a reachable Website API (private/local mode
            // sets HIVEMIND_DISABLE_WEBSITE_VPN=1) still supports direct
            // owner registration against the Nodepool. Only fall through when
            // website-api is disabled; real enrollment failures stay fatal so
            // public onboarding keeps failing closed.
            Err(error)
                if !require_external_overlay
                    && error.to_string().contains("enrollment is disabled") =>
            {
                tracing::info!(
                    "website-api enrollment disabled; registering directly with Nodepool as {}",
                    body.username.as_deref().unwrap_or_default()
                );
            }
            Err(error) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(StatusResponse {
                        success: false,
                        status_message: format!("automatic Worker enrollment failed: {error}"),
                    }),
                )
            }
        }
    }

    let endpoint = match body
        .ip
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(requested) => match effective_worker_advertise_addr(&state, requested).await {
            Ok(endpoint) => Some(endpoint),
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(StatusResponse {
                        success: false,
                        status_message: err.to_string(),
                    }),
                )
            }
        },
        // Session-only registration: the outbound session carries task
        // delivery and results, so no inbound callback address is required.
        None => None,
    };

    let owner = if let Some(enrollment) = server_enrollment.as_ref() {
        enrollment.owner.clone()
    } else {
        body.username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string()
    };
    if owner.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "username is required".into(),
            }),
        );
    }

    let worker_id = if let Some(enrollment) = server_enrollment.as_ref() {
        match enrollment.worker_id.clone() {
            Some(worker_id) => worker_id,
            None => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(StatusResponse {
                        success: false,
                        status_message: "automatic enrollment did not return a worker identity"
                            .into(),
                    }),
                )
            }
        }
    } else {
        body.worker_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&owner)
            .to_string()
    };
    if !is_safe_worker_id(&worker_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "Invalid worker_id".into(),
            }),
        );
    }

    if body.cpu_cores < 0
        || body.memory_gb < 0
        || body.cpu_score < 0
        || body.gpu_score.unwrap_or(0) < 0
        || body.gpu_memory_gb.unwrap_or(0) < 0
        || body.storage_total_gb.unwrap_or(0) < 0
        || body.storage_available_gb.unwrap_or(0) < 0
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "capacity values must be non-negative".into(),
            }),
        );
    }
    let storage_total = body
        .storage_total_gb
        .unwrap_or(state.profile.storage_total_gb);
    let storage_available = body
        .storage_available_gb
        .unwrap_or(state.profile.storage_available_gb);
    if storage_available > storage_total {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "storage_available_gb cannot exceed storage_total_gb".into(),
            }),
        );
    }

    let profile = WorkerProfile {
        worker_id: worker_id.to_string(),
        // An empty address marks a session-only registration; Nodepool keeps
        // the previous callback address for re-registrations of an existing
        // Worker and the dispatcher relies on the outbound session instead.
        ip: endpoint.unwrap_or_default(),
        location: body
            .location
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&state.profile.location)
            .to_string(),
        cpu_cores: body.cpu_cores,
        memory_gb: body.memory_gb,
        cpu_score: body.cpu_score,
        gpu_score: body.gpu_score.unwrap_or(0),
        gpu_memory_gb: body.gpu_memory_gb.unwrap_or(0),
        storage_total_gb: storage_total,
        storage_available_gb: storage_available,
        gpu_name: body
            .gpu_name
            .unwrap_or_else(|| state.profile.gpu_name.clone()),
    };

    let capability_report =
        match capability_report_to_proto(&state.executor.dynamic_capability_report()) {
            Ok(report) => Some(report),
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(StatusResponse {
                        success: false,
                        status_message: error.to_string(),
                    }),
                );
            }
        };
    match register_once_with_capability_report(
        &state.nodepool_addr(),
        &profile.worker_id,
        &owner,
        &profile.ip,
        profile.to_resource_spec(),
        &profile.location,
        &token,
        capability_report,
    )
    .await
    {
        Ok(()) => {
            state.set_worker_identity(&profile.worker_id);
            if !profile.ip.is_empty() {
                state.set_worker_addr(profile.ip.clone());
            }
            // UI-authenticated workers do not start the pre-provisioned
            // registration loop during process startup. Start it after the
            // first successful registration so the node remains online and
            // the dispatcher can continue seeing it after 30 seconds.
            state.ensure_registration_loop(&owner, &profile.worker_id, &token);
            state.ensure_session_loop(&owner, &profile.worker_id, &token);
            (
                StatusCode::OK,
                Json(StatusResponse {
                    success: true,
                    status_message: "OK".into(),
                }),
            )
        }
        Err(err) => {
            // An expired or rejected Nodepool token must surface as 401 so
            // the browser UI logs the user out instead of showing a raw
            // gRPC status behind a generic bad gateway.
            let status = if nodepool_client::is_nodepool_authentication_error(&err) {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_GATEWAY
            };
            (
                status,
                Json(StatusResponse {
                    success: false,
                    status_message: err.to_string(),
                }),
            )
        }
    }
}

async fn effective_worker_advertise_addr(
    state: &ControlApiState,
    requested: &str,
) -> Result<String> {
    let requested = requested.trim();
    if requested.is_empty() {
        anyhow::bail!("ip is required");
    }

    let require_external_overlay =
        client_runtime::external_overlay_required(&state.config, ClientRole::Worker);
    if require_external_overlay {
        let session = client_runtime::current_vpn_session(ClientRole::Worker)
            .await
            .ok_or_else(|| anyhow::anyhow!("authenticated overlay session is not ready"))?;
        if session.transport != client_runtime::VpnTransport::Tailscale {
            anyhow::bail!("strict external overlay requires the embedded libtailscale transport");
        }
        let overlay_ip = session
            .overlay_ip
            .as_deref()
            .map(str::trim)
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("authenticated overlay session has no assigned overlay address")
            })?;
        let port_source = state
            .config
            .server
            .worker_advertise_addr
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(requested);
        return nodepool_client::forwarded_overlay_advertise_addr(
            &state.config.server.worker_grpc_addr,
            port_source,
            overlay_ip,
        );
    }

    if let Some(configured) = state
        .config
        .server
        .worker_advertise_addr
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return nodepool_client::validate_advertise_addr(configured);
    }

    let Some(overlay_ip) = client_runtime::current_vpn_session(ClientRole::Worker)
        .await
        .and_then(|session| session.overlay_ip.clone())
    else {
        return Ok(requested.to_string());
    };

    let port = requested
        .rsplit_once(':')
        .map(|(_, port)| port.trim_matches(']'))
        .filter(|port| !port.is_empty())
        .ok_or_else(|| anyhow::anyhow!("worker endpoint must include a port"))?;
    let host = if overlay_ip.contains(':') && !overlay_ip.starts_with('[') {
        format!("[{overlay_ip}]")
    } else {
        overlay_ip
    };
    nodepool_client::validate_advertise_addr(&format!("{host}:{port}"))
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn is_safe_worker_id(worker_id: &str) -> bool {
    let worker_id = worker_id.trim();
    !worker_id.is_empty()
        && worker_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !worker_id.contains("..")
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use hivemind_config::HivemindConfig;
    use hivemind_models::ResourceSpec;
    use serde_json::Value;
    use std::fs;
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn sample_profile() -> super::WorkerProfile {
        super::WorkerProfile {
            worker_id: "worker-1".into(),
            ip: "127.0.0.1:50053".into(),
            location: "local".into(),
            cpu_cores: 1,
            memory_gb: 1,
            cpu_score: 1,
            gpu_score: 0,
            gpu_memory_gb: 0,
            storage_total_gb: 1,
            storage_available_gb: 1,
            gpu_name: String::new(),
        }
    }

    fn sample_state() -> super::ControlApiState {
        super::ControlApiState {
            profile: sample_profile(),
            worker_addr: std::sync::Arc::new(std::sync::Mutex::new("127.0.0.1:50053".into())),
            nodepool_addr: std::sync::Arc::new(std::sync::Mutex::new("127.0.0.1:50051".into())),
            config: HivemindConfig::default(),
            executor: std::sync::Arc::new(super::WorkerExecutor::new(HivemindConfig::default())),
            worker_service: None,
            worker_identity: std::sync::Arc::new(std::sync::Mutex::new(Some("worker-1".into()))),
            registration_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
            session_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    #[tokio::test]
    async fn worker_dashboard_requires_bearer_auth_and_registered_identity() {
        let state = sample_state();
        let app = super::router_with_allowed_origins(state, &[]);
        let missing_bearer = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/worker-dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_bearer.status(), StatusCode::UNAUTHORIZED);

        let mut state = sample_state();
        state.worker_identity = std::sync::Arc::new(std::sync::Mutex::new(None));
        let app = super::router_with_allowed_origins(state, &[]);
        let unregistered = app
            .oneshot(
                Request::builder()
                    .uri("/api/worker-dashboard")
                    .header(axum::http::header::AUTHORIZATION, "Bearer account-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unregistered.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn worker_dashboard_does_not_probe_resources_when_cache_is_empty() {
        let state = sample_state();
        let executor = state.executor.clone();
        let app = super::router_with_allowed_origins(state, &[]);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/worker-dashboard")
                    .header(axum::http::header::AUTHORIZATION, "Bearer account-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(executor.latest_resource_sample().is_none());
    }

    #[test]
    fn dashboard_keeps_individually_unsupported_gpu_metrics_unknown() {
        let sample = super::ResourceSample {
            resources: crate::SystemResources {
                cpu_cores: 8,
                total_memory_gb: 16,
                available_memory_gb: 4,
                cpu_usage_percent: 25.0,
                memory_usage_percent: 75.0,
                gpu_count: 1,
                gpu_infos: vec![crate::GpuInfo {
                    index: 0,
                    name: "NVIDIA Example".into(),
                    vram_total_mb: 0,
                    vram_used_mb: 0,
                    vram_available_mb: 0,
                    gpu_utilization_percent: 0.0,
                }],
                gpu_inventory_supported: true,
                gpu_utilization_supported: false,
                vram_total_supported: false,
                vram_available_supported: false,
                storage_supported: false,
                storage_total_gb: 0,
                storage_available_gb: 0,
            },
            sampled_at: chrono::Utc::now(),
        };

        let host = super::worker_dashboard_host(&sample);
        assert_eq!(host.gpu_count, Some(1));
        assert_eq!(host.gpu_utilization_percent, None);
        assert_eq!(host.vram_total_mb, None);
        assert_eq!(host.vram_available_mb, None);
    }

    #[test]
    fn dashboard_marks_unsupported_telemetry_unknown_and_samples_stale() {
        let now = chrono::Utc::now();
        let sample = super::ResourceSample {
            resources: crate::SystemResources {
                cpu_cores: 8,
                total_memory_gb: 16,
                available_memory_gb: 4,
                cpu_usage_percent: 25.0,
                memory_usage_percent: 75.0,
                gpu_count: 0,
                gpu_infos: Vec::new(),
                gpu_inventory_supported: false,
                gpu_utilization_supported: false,
                vram_total_supported: false,
                vram_available_supported: false,
                storage_supported: false,
                storage_total_gb: 0,
                storage_available_gb: 0,
            },
            sampled_at: now - chrono::Duration::seconds(31),
        };

        let host = super::worker_dashboard_host(&sample);
        assert_eq!(host.gpu_count, None);
        assert_eq!(host.gpu_utilization_percent, None);
        assert_eq!(host.vram_total_mb, None);
        assert_eq!(host.vram_available_mb, None);
        assert_eq!(host.storage_total_gb, None);
        assert_eq!(host.storage_available_gb, None);
        assert!(super::resource_sample_is_stale(sample.sampled_at, now));
        assert!(!super::resource_sample_is_stale(
            now - chrono::Duration::seconds(30),
            now
        ));
        assert!(!super::resource_sample_is_stale(
            now - chrono::Duration::seconds(10),
            now
        ));

        let response = super::WorkerDashboardResponse {
            success: true,
            worker_id: "worker-1".into(),
            sampled_at: sample.sampled_at.to_rfc3339(),
            stale: true,
            host,
            assignments: vec![super::WorkerDashboardAssignment {
                task_id: "task-1".into(),
                submitter: "alice".into(),
                status: "active".into(),
                max_cpt: 100,
                reported_usage_cpt: Some(7),
                usage_basis: "worker_reported_managed_usage".into(),
                usage_updated_at: Some(now.to_rfc3339()),
            }],
            settled_provider_credits_cpt: 0,
            currency: "CPT".into(),
        };
        let json = serde_json::to_value(response).unwrap();
        assert!(json["host"]["gpu_count"].is_null());
        assert!(json["host"]["storage_total_gb"].is_null());
        assert_eq!(json["assignments"][0]["reported_usage_cpt"], 7);
        assert_eq!(
            json["assignments"][0]["usage_basis"],
            "worker_reported_managed_usage"
        );
        assert!(json["assignments"][0].get("task_source").is_none());
        assert!(json["assignments"][0].get("result").is_none());
        assert!(json["assignments"][0].get("logs").is_none());
    }

    #[tokio::test]
    async fn worker_ui_fallback_serves_index_without_shadowing_api() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("index.html"), "worker-ui").unwrap();
        let app = super::router_with_ui_dir(
            sample_state(),
            &["http://localhost:3000".into()],
            directory.path().to_str(),
        );

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"worker-ui");
    }

    #[test]
    fn worker_profile_converts_resource_spec_to_worker_ui_shape() {
        let spec = ResourceSpec {
            cpu_cores: 12,
            memory_mb: 32 * 1024,
            gpu_count: 1,
            gpu_name: "RTX 4090".into(),
            vram_mb: 24 * 1024,
            cpu_score: 1200,
            gpu_score: 2400,
            storage_total_gb: 2000,
            storage_available_gb: 1500,
        };

        let profile = super::WorkerProfile::from_resource_spec(
            "worker-1".to_string(),
            "127.0.0.1:50053".to_string(),
            "local".to_string(),
            spec,
        );

        assert_eq!(profile.worker_id, "worker-1");
        assert_eq!(profile.ip, "127.0.0.1:50053");
        assert_eq!(profile.location, "local");
        assert_eq!(profile.cpu_cores, 12);
        assert_eq!(profile.memory_gb, 32);
        assert_eq!(profile.gpu_memory_gb, 24);
        assert_eq!(profile.cpu_score, 1200);
        assert_eq!(profile.gpu_score, 2400);
        assert_eq!(profile.gpu_name, "RTX 4090");
        assert_eq!(profile.storage_total_gb, 2000);
        assert_eq!(profile.storage_available_gb, 1500);
    }

    #[test]
    fn worker_info_advertises_forwarded_overlay_callback_after_join() {
        assert_eq!(
            super::worker_info_overlay_addr("", "0.0.0.0:15054", "100.64.0.16").unwrap(),
            "100.64.0.16:15054"
        );
        assert_eq!(
            super::worker_info_overlay_addr("0.0.0.0:15054", "0.0.0.0:15054", "100.64.0.16")
                .unwrap(),
            "100.64.0.16:15054"
        );
        assert!(
            super::worker_info_overlay_addr("0.0.0.0:50053", "0.0.0.0:15054", "100.64.0.16")
                .is_err()
        );
        assert_eq!(
            super::worker_info_overlay_addr("", "[::]:15055", "fd7a:115c:a1e0::1").unwrap(),
            "[fd7a:115c:a1e0::1]:15055"
        );
        assert!(super::worker_info_overlay_addr("", "127.0.0.1:18080", "not-an-ip").is_err());
    }

    #[tokio::test]
    async fn worker_info_route_returns_success_and_profile_json() {
        let spec = ResourceSpec {
            cpu_cores: 8,
            memory_mb: 16 * 1024,
            gpu_count: 0,
            gpu_name: String::new(),
            vram_mb: 0,
            cpu_score: 800,
            gpu_score: 0,
            storage_total_gb: 512,
            storage_available_gb: 256,
        };
        let profile = super::WorkerProfile::from_resource_spec(
            "worker-1".into(),
            "127.0.0.1:50053".into(),
            "local".into(),
            spec,
        );
        let app = super::router(profile);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/worker-info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        assert_eq!(json["profile"]["worker_id"], "worker-1");
        assert_eq!(json["profile"]["ip"], "127.0.0.1:50053");
        assert_eq!(json["profile"]["location"], "local");
        assert_eq!(json["profile"]["cpu_cores"], 8);
        assert_eq!(json["profile"]["memory_gb"], 16);
        assert_eq!(json["profile"]["gpu_memory_gb"], 0);
        assert_eq!(json["profile"]["storage_available_gb"], 256);
    }

    #[tokio::test]
    async fn worker_info_cors_allows_only_configured_origins_without_wildcard() {
        let app = super::router_with_allowed_origins(
            sample_state(),
            &["http://localhost:5174".to_string()],
        );

        let allowed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/worker-info")
                    .header(axum::http::header::ORIGIN, "http://localhost:5174")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            allowed
                .headers()
                .get(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&"http://localhost:5174".parse().unwrap())
        );

        let rejected = app
            .oneshot(
                Request::builder()
                    .uri("/api/worker-info")
                    .header(axum::http::header::ORIGIN, "http://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(rejected
            .headers()
            .get(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }

    #[tokio::test]
    async fn vpn_routes_require_bearer_and_return_no_secret_fields() {
        let app = super::router_with_allowed_origins(sample_state(), &[]);
        for (method, path) in [("POST", "/api/vpn/bootstrap"), ("GET", "/api/vpn/status")] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {path}"
            );
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let json: Value = serde_json::from_slice(&body).unwrap();
            assert!(json.get("auth_key").is_none());
            assert!(!json.to_string().contains("tskey-auth"));
            assert!(!json.to_string().contains("HEADSCALE_API_KEY"));
        }
    }
    #[tokio::test]
    async fn register_worker_requires_bearer_token() {
        let app = super::router_with_allowed_origins(sample_state(), &[]);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/register-worker")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"username":"alice","ip":"127.0.0.1:50053","cpu_cores":1,"memory_gb":1,"cpu_score":1}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn register_worker_body_accepts_a_session_only_registration_without_ip() {
        let body: super::RegisterWorkerBody = serde_json::from_str(
            r#"{"username":"alice","cpu_cores":1,"memory_gb":1,"cpu_score":1}"#,
        )
        .expect("session-only registration omits the callback address");
        assert!(body.ip.is_none());

        let body: super::RegisterWorkerBody = serde_json::from_str(
            r#"{"username":"alice","ip":"","cpu_cores":1,"memory_gb":1,"cpu_score":1}"#,
        )
        .expect("a blank ip is treated the same as an omitted one");
        assert_eq!(body.ip.as_deref().unwrap_or_default(), "");
    }

    #[test]
    fn control_addr_defaults_and_reads_env() {
        let config = HivemindConfig::default();
        assert_eq!(config.server.worker_control_http_addr, "127.0.0.1:18080");

        let old_config_path = std::env::var_os("HIVEMIND_CONFIG");
        let old_control_addr = std::env::var_os("WORKER_CONTROL_HTTP_ADDR");
        std::env::remove_var("HIVEMIND_CONFIG");
        std::env::set_var("WORKER_CONTROL_HTTP_ADDR", "127.0.0.1:19090");
        let loaded = HivemindConfig::load().unwrap();
        match old_control_addr {
            Some(value) => std::env::set_var("WORKER_CONTROL_HTTP_ADDR", value),
            None => std::env::remove_var("WORKER_CONTROL_HTTP_ADDR"),
        }
        match old_config_path {
            Some(value) => std::env::set_var("HIVEMIND_CONFIG", value),
            None => std::env::remove_var("HIVEMIND_CONFIG"),
        }

        assert_eq!(loaded.server.worker_control_http_addr, "127.0.0.1:19090");
    }
}

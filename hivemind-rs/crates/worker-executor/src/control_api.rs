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
use crate::WorkerExecutor;

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
                location: self.profile.location.clone(),
                token: token.to_string(),
                interval: std::time::Duration::from_secs(10),
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
    let open_addr = addr.to_string();
    tokio::spawn(async move {
        client_runtime::open_ui_when_ready(&open_addr).await;
    });
    axum::serve(listener, router_with_ui_dir(state, allowed_origins, ui_dir)).await?;
    Ok(())
}

async fn worker_info(State(state): State<ControlApiState>) -> Json<WorkerInfoResponse> {
    let mut profile = state.profile.clone();
    if let Some(worker_id) = state.current_worker_identity() {
        profile.worker_id = worker_id;
    }
    if let Some(session) = client_runtime::current_vpn_session(ClientRole::Worker).await {
        if let Some(ip) = session.overlay_ip.as_deref() {
            let port = profile.ip.rsplit(':').next().unwrap_or("50053");
            profile.ip = format!("{ip}:{port}");
        }
    }
    state.set_worker_addr(profile.ip.clone());
    Json(WorkerInfoResponse {
        success: true,
        profile,
    })
}

async fn bootstrap_vpn(
    State(state): State<ControlApiState>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<VpnBootstrapResponse>) {
    let Some(token) = bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(vpn_bootstrap_response(
                client_runtime::current_vpn_status(ClientRole::Worker),
                Some("missing bearer token".into()),
            )),
        );
    };

    match client_runtime::ensure_user_vpn_for_token(&state.config, ClientRole::Worker, &token).await
    {
        Ok(Some(endpoint)) => {
            state.set_nodepool_addr(endpoint);
            (
                StatusCode::OK,
                Json(vpn_bootstrap_response(
                    client_runtime::current_vpn_status(ClientRole::Worker),
                    None,
                )),
            )
        }
        Ok(None) => (
            StatusCode::OK,
            Json(vpn_bootstrap_response(
                client_runtime::current_vpn_status(ClientRole::Worker),
                None,
            )),
        ),
        Err(err) => {
            tracing::warn!("Worker VPN bootstrap failed: {err}");
            let status = client_runtime::current_vpn_status(ClientRole::Worker);
            let http_status = vpn_bootstrap_http_status(status.state);
            (
                http_status,
                Json(vpn_bootstrap_response(
                    status,
                    Some("VPN/Nodepool bootstrap failed".into()),
                )),
            )
        }
    }
}

async fn vpn_status(headers: axum::http::HeaderMap) -> (StatusCode, Json<VpnBootstrapResponse>) {
    if bearer_token(&headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(vpn_bootstrap_response(
                client_runtime::current_vpn_status(ClientRole::Worker),
                Some("missing bearer token".into()),
            )),
        );
    }
    (
        StatusCode::OK,
        Json(vpn_bootstrap_response(
            client_runtime::current_vpn_status(ClientRole::Worker),
            None,
        )),
    )
}

fn vpn_bootstrap_response(
    status: client_runtime::VpnBootstrapStatus,
    fallback_message: Option<String>,
) -> VpnBootstrapResponse {
    let success = matches!(
        status.state,
        client_runtime::VpnBootstrapState::Ready | client_runtime::VpnBootstrapState::Disabled
    );
    VpnBootstrapResponse {
        success,
        state: status.state.as_str().to_string(),
        endpoint: status.endpoint,
        overlay_ip: status.overlay_ip,
        message: status.message.or(fallback_message),
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
        Ok(None) => None,
        Err(err) => {
            let message = err.to_string();
            tracing::warn!("Worker VPN bootstrap before login failed: {}", message);
            if message.contains("nodepool endpoint")
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
                Ok(None) => {}
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
            Err(error) if error.to_string().contains("enrollment is disabled") => {
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
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?
        .trim();
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

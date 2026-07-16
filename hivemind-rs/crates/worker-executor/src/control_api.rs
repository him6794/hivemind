use anyhow::Result;
use axum::{
    extract::State,
    http::{header, HeaderValue, Method, StatusCode},
    routing::{get, post},
    Json, Router,
};
use hivemind_config::HivemindConfig;
use hivemind_models::ResourceSpec;
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;

use crate::nodepool_client::{self, login_to_nodepool, register_once};

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

#[derive(Debug, Clone)]
pub struct ControlApiState {
    pub profile: WorkerProfile,
    pub nodepool_addr: String,
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
    ip: String,
    cpu_cores: i32,
    memory_gb: i64,
    cpu_score: i32,
    gpu_score: Option<i32>,
    gpu_memory_gb: Option<i64>,
    gpu_name: Option<String>,
    storage_total_gb: Option<i64>,
    storage_available_gb: Option<i64>,
    location: Option<String>,
    token: Option<String>,
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
            profile,
            nodepool_addr: config
                .server
                .nodepool_grpc_endpoint
                .clone()
                .unwrap_or_else(|| config.server.nodepool_grpc_addr.clone()),
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
            profile,
            nodepool_addr: config
                .server
                .nodepool_grpc_endpoint
                .clone()
                .unwrap_or_else(|| config.server.nodepool_grpc_addr.clone()),
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
            profile,
            nodepool_addr: config
                .server
                .nodepool_grpc_endpoint
                .clone()
                .unwrap_or_else(|| config.server.nodepool_grpc_addr.clone()),
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
    axum::serve(listener, router_with_ui_dir(state, allowed_origins, ui_dir)).await?;
    Ok(())
}

async fn worker_info(State(state): State<ControlApiState>) -> Json<WorkerInfoResponse> {
    Json(WorkerInfoResponse {
        success: true,
        profile: state.profile.clone(),
    })
}

async fn login(
    State(state): State<ControlApiState>,
    Json(body): Json<LoginBody>,
) -> (StatusCode, Json<LoginResponse>) {
    match login_to_nodepool(&state.nodepool_addr, &body.username, &body.password).await {
        Ok(token) => (
            StatusCode::OK,
            Json(LoginResponse {
                success: true,
                message: "Login successful".into(),
                token: Some(token),
            }),
        ),
        Err(err) => {
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
    let token = bearer_token(&headers)
        .or_else(|| {
            body.token
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default();
    if token.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(StatusResponse {
                success: false,
                status_message: "missing bearer token".into(),
            }),
        );
    }

    let endpoint = body.ip.trim();
    if endpoint.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "ip is required".into(),
            }),
        );
    }

    let owner = body
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    if owner.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(StatusResponse {
                success: false,
                status_message: "username is required".into(),
            }),
        );
    }

    let worker_id = body
        .worker_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(owner);
    if !is_safe_worker_id(worker_id) {
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
        ip: endpoint.to_string(),
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

    match register_once(
        &state.nodepool_addr,
        &profile.worker_id,
        owner,
        &profile.ip,
        profile.to_resource_spec(),
        &profile.location,
        &token,
    )
    .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(StatusResponse {
                success: true,
                status_message: "OK".into(),
            }),
        ),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(StatusResponse {
                success: false,
                status_message: err.to_string(),
            }),
        ),
    }
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

// Keep nodepool_client import surface used for endpoint normalization in tests/docs.
#[allow(dead_code)]
fn _nodepool_endpoint_helper(addr: &str) -> String {
    nodepool_client::nodepool_endpoint(addr)
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
            nodepool_addr: "127.0.0.1:50051".into(),
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

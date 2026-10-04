use anyhow::Result;
use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use hivemind_client_runtime::{self as client_runtime, ClientRole};
use serde::Serialize;
use std::{net::SocketAddr, path::Path, sync::Arc};
use tokio::{sync::RwLock, task::JoinHandle};
use tower::ServiceExt;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::ServeDir,
};

#[derive(Clone, Serialize)]
struct StartupStatus {
    state: &'static str,
    phase: &'static str,
    code: Option<&'static str>,
}

struct StartupState {
    status: StartupStatus,
    api: Option<Router>,
}

#[derive(Clone)]
struct UiState {
    startup: Arc<RwLock<StartupState>>,
    files: ServeDir,
}

pub(crate) struct LocalUiServer {
    state: UiState,
    bound_addr: SocketAddr,
    ui_available: bool,
    task: JoinHandle<()>,
}

impl LocalUiServer {
    pub(crate) async fn bind(
        addr: &str,
        ui_dir: &str,
        allowed_origins: &[String],
        role: ClientRole,
    ) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let bound_addr = listener.local_addr()?;
        let state = UiState {
            startup: Arc::new(RwLock::new(StartupState {
                status: StartupStatus {
                    state: "initializing",
                    phase: "starting",
                    code: None,
                },
                api: None,
            })),
            files: ServeDir::new(ui_dir).append_index_html_on_directories(true),
        };
        let app = ui_router(state.clone(), allowed_origins, role);
        let failure_state = state.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                let mut startup = failure_state.startup.write().await;
                startup.status.state = "failed";
                startup.status.code = Some("local_server_failed");
                startup.api = None;
                tracing::error!(%error, "Local UI HTTP server failed");
            }
        });
        tracing::info!(role = role.as_str(), %bound_addr, "Local UI is listening before runtime initialization");
        Ok(Self {
            state,
            bound_addr,
            ui_available: Path::new(ui_dir).join("index.html").is_file(),
            task,
        })
    }

    pub(crate) fn open_window(&self, role: ClientRole) {
        let addr = self.bound_addr;
        let available = self.ui_available;
        tokio::spawn(async move {
            match role {
                ClientRole::Master => {
                    client_runtime::open_master_ui_when_ready(addr, available).await
                }
                ClientRole::Worker => {
                    client_runtime::open_worker_ui_when_ready(addr, available).await
                }
            }
        });
    }

    pub(crate) async fn phase(&self, phase: &'static str) {
        let mut startup = self.state.startup.write().await;
        if startup.status.state == "initializing" {
            startup.status.phase = phase;
        }
    }

    pub(crate) async fn publish(&self, api: Router) {
        let mut startup = self.state.startup.write().await;
        if startup.status.state != "initializing" {
            return;
        }
        startup.api = Some(api);
        startup.status = StartupStatus {
            state: "ready",
            phase: "ready",
            code: None,
        };
    }

    pub(crate) async fn fail(&self, code: &'static str) {
        let mut startup = self.state.startup.write().await;
        startup.api = None;
        startup.status.state = "failed";
        startup.status.code = Some(code);
    }

    pub(crate) fn keep_failure_visible(&self) -> bool {
        cfg!(target_os = "windows")
            && self.ui_available
            && !std::env::var("HIVEMIND_DISABLE_OPEN_UI")
                .ok()
                .is_some_and(|value| {
                    matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes" | "on"
                    )
                })
    }
}

impl Drop for LocalUiServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn ui_router(state: UiState, allowed_origins: &[String], role: ClientRole) -> Router {
    let origins = allowed_origins
        .iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();
    let mut methods = vec![Method::GET, Method::POST, Method::OPTIONS];
    if role == ClientRole::Master {
        methods.push(Method::PUT);
    }
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods(methods)
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
    Router::new()
        .route("/api/startup-status", get(startup_status))
        .fallback(dispatch)
        .with_state(state)
        .layer(cors)
}

async fn startup_status(State(state): State<UiState>) -> impl IntoResponse {
    let snapshot = state.startup.read().await.status.clone();
    ([(header::CACHE_CONTROL, "no-store")], Json(snapshot))
}

async fn dispatch(State(state): State<UiState>, request: Request) -> Response {
    let (api, status) = {
        let startup = state.startup.read().await;
        (startup.api.clone(), startup.status.clone())
    };
    if let Some(api) = api {
        // Delegate the original URI/body through the complete role router, including
        // its authentication, CORS and request-size limits.
        return match api.oneshot(request).await {
            Ok(response) => response,
            Err(error) => match error {},
        };
    }
    let path = request.uri().path();
    if path == "/api" || path.starts_with("/api/") || path == "/health" {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CACHE_CONTROL, "no-store"), (header::RETRY_AFTER, "1")],
            Json(serde_json::json!({
                "success": false,
                "message": if status.state == "failed" { "Local service initialization failed" } else { "Local service is starting" },
                "state": status.state,
                "code": status.code,
            })),
        )
            .into_response();
    }
    match state.files.oneshot(request).await {
        Ok(response) => response.into_response(),
        Err(error) => {
            tracing::warn!(%error, "Could not serve local UI assets");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request as HttpRequest, routing::post};

    fn state() -> UiState {
        UiState {
            startup: Arc::new(RwLock::new(StartupState {
                status: StartupStatus {
                    state: "initializing",
                    phase: "resources",
                    code: None,
                },
                api: None,
            })),
            files: ServeDir::new(std::env::temp_dir().join("hivemind-missing-ui-test")),
        }
    }

    async fn body(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn startup_status_is_read_only_and_operations_fail_closed() {
        let app = ui_router(state(), &[], ClientRole::Worker);
        let status = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/startup-status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        assert_eq!(status.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            body(status).await,
            serde_json::json!({ "state": "initializing", "phase": "resources", "code": null })
        );
        for path in ["/api/login", "/api/register-worker", "/health"] {
            let response = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .method("POST")
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
    }

    #[tokio::test]
    async fn published_router_preserves_authentication_and_original_request() {
        let state = state();
        state.startup.write().await.api = Some(Router::new().route(
            "/api/protected",
            post(|request: Request| async move {
                if request.headers().get(header::AUTHORIZATION).is_none() {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                request.uri().to_string().into_response()
            }),
        ));
        let app = ui_router(state, &[], ClientRole::Master);
        let denied = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/protected?view=current")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let allowed = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/protected?view=current")
                    .header(header::AUTHORIZATION, "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(allowed.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"/api/protected?view=current");
    }

    #[tokio::test]
    async fn delegation_keeps_request_limits_and_the_origin_allowlist() {
        let state = state();
        state.startup.write().await.api = Some(
            Router::new()
                .route(
                    "/api/upload",
                    post(|Json(value): Json<serde_json::Value>| async move { Json(value) }),
                )
                .layer(axum::extract::DefaultBodyLimit::max(4)),
        );
        let app = ui_router(
            state,
            &["http://127.0.0.1:18080".into()],
            ClientRole::Worker,
        );
        let oversized = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/upload")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{\"large\":true}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        for (origin, allowed) in [
            ("http://127.0.0.1:18080", true),
            ("https://untrusted.example", false),
        ] {
            let response = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .method("OPTIONS")
                        .uri("/api/upload")
                        .header(header::ORIGIN, origin)
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response
                    .headers()
                    .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .is_some(),
                allowed
            );
        }
    }

    #[tokio::test]
    async fn homepage_is_served_while_initialization_is_pending_or_failed() {
        let dir = std::env::temp_dir().join(format!("hivemind-early-ui-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<h1>Preparing this computer</h1>").unwrap();
        let server = LocalUiServer::bind(
            "127.0.0.1:0",
            dir.to_str().unwrap(),
            &[],
            ClientRole::Worker,
        )
        .await
        .unwrap();
        let app = ui_router(server.state.clone(), &[], ClientRole::Worker);
        for failed in [false, true] {
            if failed {
                server.fail("runtime_initialization_failed").await;
            }
            let response = app
                .clone()
                .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        server.phase("ready").await;
        server.publish(Router::new()).await;
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/startup-status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body(response).await["state"], "failed");
        drop(server);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn binding_an_occupied_port_fails_before_a_server_is_started() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let result = LocalUiServer::bind(
            &listener.local_addr().unwrap().to_string(),
            ".",
            &[],
            ClientRole::Master,
        )
        .await;
        assert!(result.is_err());
    }
}

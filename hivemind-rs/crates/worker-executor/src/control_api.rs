use anyhow::Result;
use axum::{
    http::{header, HeaderValue, Method},
    routing::get,
    Json, Router,
};
use hivemind_config::HivemindConfig;
use hivemind_models::ResourceSpec;
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

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
}

#[derive(Debug, Clone, Serialize)]
struct WorkerInfoResponse {
    success: bool,
    profile: WorkerProfile,
}

pub fn router(profile: WorkerProfile) -> Router {
    let config = HivemindConfig::default();
    router_with_allowed_origins(profile, &config.server.worker_control_cors_allowed_origins)
}

pub fn router_with_allowed_origins(profile: WorkerProfile, allowed_origins: &[String]) -> Router {
    let cors = build_cors_layer(allowed_origins);

    Router::new()
        .route(
            "/api/worker-info",
            get(move || worker_info(profile.clone())),
        )
        .layer(cors)
}

fn build_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins = allowed_origins
        .iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

pub async fn serve(addr: &str, profile: WorkerProfile) -> Result<()> {
    let config = HivemindConfig::default();
    serve_with_allowed_origins(
        addr,
        profile,
        &config.server.worker_control_cors_allowed_origins,
    )
    .await
}

pub async fn serve_with_allowed_origins(
    addr: &str,
    profile: WorkerProfile,
    allowed_origins: &[String],
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        router_with_allowed_origins(profile, allowed_origins),
    )
    .await?;
    Ok(())
}

async fn worker_info(profile: WorkerProfile) -> Json<WorkerInfoResponse> {
    Json(WorkerInfoResponse {
        success: true,
        profile,
    })
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use hivemind_config::HivemindConfig;
    use hivemind_models::ResourceSpec;
    use serde_json::Value;
    use tower::ServiceExt;

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
        let app =
            super::router_with_allowed_origins(profile, &["http://localhost:5174".to_string()]);

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

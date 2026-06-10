use super::handlers::AppState;
use super::middleware as mw;
use axum::{
    http::{header, HeaderValue, Method},
    middleware,
    routing::{get, post, put},
    Router,
};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub fn create_router(state: AppState) -> Router {
    let cors = build_cors_layer(&state.config.server.master_cors_allowed_origins);

    let public = Router::new()
        .route("/health", get(super::handlers::health_check))
        .route("/api/register", post(super::handlers::register))
        .route("/api/login", post(super::handlers::login));

    let protected = Router::new()
        .route("/api/balance", get(super::handlers::get_balance))
        .route(
            "/api/admin/billing/overview",
            get(super::handlers::get_admin_billing_overview),
        )
        .route(
            "/api/admin/artifacts/overview",
            get(super::handlers::get_admin_artifact_overview),
        )
        .route(
            "/api/admin/artifacts/cleanup",
            post(super::handlers::cleanup_admin_artifacts),
        )
        .route(
            "/api/admin/scheduling/cache-metrics",
            get(super::handlers::get_admin_scheduling_cache_metrics),
        )
        .route(
            "/api/admin/scheduling/cache-alert",
            get(super::handlers::get_admin_scheduling_cache_alert),
        )
        .route(
            "/api/admin/scheduling/cache-anomalies",
            get(super::handlers::list_admin_scheduling_cache_anomalies),
        )
        .route(
            "/api/provider/earnings",
            get(super::handlers::get_provider_earnings),
        )
        .route(
            "/api/provider/workers/:worker_id/settings",
            get(super::handlers::get_provider_worker_settings)
                .put(super::handlers::update_provider_worker_settings),
        )
        .route(
            "/api/provider/workers/:worker_id/trust",
            get(super::handlers::get_provider_worker_trust_profile),
        )
        .route(
            "/api/admin/workers/:worker_id/trust-control",
            put(super::handlers::update_worker_trust_control),
        )
        .route(
            "/api/admin/workers/trust",
            get(super::handlers::list_admin_worker_trust),
        )
        .route(
            "/api/admin/audit/logs",
            get(super::handlers::list_admin_audit_logs),
        )
        .route("/api/tasks/quote", post(super::handlers::quote_task))
        .route("/api/tasks", post(super::handlers::create_task))
        .route("/api/tasks/upload", post(super::handlers::upload_task))
        .route("/api/tasks", get(super::handlers::list_tasks))
        .route("/api/workers", get(super::handlers::list_workers))
        .route(
            "/api/register-worker",
            post(super::handlers::register_worker),
        )
        .route("/api/remove-worker", post(super::handlers::remove_worker))
        .route(
            "/api/tasks/:task_id/log",
            get(super::handlers::get_task_log),
        )
        .route(
            "/api/tasks/:task_id/result",
            get(super::handlers::get_task_result),
        )
        .route(
            "/api/tasks/:task_id/artifact/download",
            get(super::handlers::download_task_artifact),
        )
        .route("/api/tasks/:task_id/stop", post(super::handlers::stop_task))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            mw::auth_middleware,
        ));

    Router::new()
        .merge(public)
        .merge(protected)
        .layer(cors)
        .with_state(state)
}

pub fn build_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins = allowed_origins
        .iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn cors_allows_only_configured_origins_without_wildcard() {
        let app = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .layer(super::build_cors_layer(&[
                "http://localhost:5173".to_string()
            ]));

        let allowed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, "http://localhost:5173")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        assert_eq!(
            allowed.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&"http://localhost:5173".parse().unwrap())
        );

        let rejected = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, "http://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::OK);
        assert!(rejected
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }
}

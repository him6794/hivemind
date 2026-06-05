use super::handlers::AppState;
use super::middleware as mw;
use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let public = Router::new()
        .route("/health", get(super::handlers::health_check))
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

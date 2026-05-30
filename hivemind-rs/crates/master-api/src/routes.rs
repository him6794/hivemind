use axum::{
    Router, routing::{get, post},
    middleware,
};
use tower_http::cors::{CorsLayer, Any};
use super::handlers::AppState;
use super::middleware as mw;

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let public = Router::new()
        .route("/health", get(super::handlers::health_check))
        .route("/api/login", post(super::handlers::login));

    let protected = Router::new()
        .route("/api/tasks", post(super::handlers::create_task))
        .route("/api/tasks", get(super::handlers::list_tasks))
        .route("/api/tasks/{task_id}/log", get(super::handlers::get_task_log))
        .route("/api/tasks/{task_id}/stop", post(super::handlers::stop_task))
        .layer(middleware::from_fn_with_state(state.clone(), mw::auth_middleware));

    Router::new()
        .merge(public)
        .merge(protected)
        .layer(cors)
        .with_state(state)
}

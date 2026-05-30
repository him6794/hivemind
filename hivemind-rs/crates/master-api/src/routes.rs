use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use super::handlers::AppState;
use super::handlers;

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(handlers::health_handler))
        .route("/api/login", post(handlers::login_handler))
        .route("/api/balance", get(handlers::get_balance_handler))
        .route("/api/tasks", post(handlers::upload_task_handler))
        .route("/api/tasks", get(handlers::list_tasks_handler))
        .route("/api/tasks/:task_id/result", get(handlers::get_task_result_handler))
        .route("/api/tasks/:task_id/stop", post(handlers::stop_task_handler))
        .layer(cors)
        .with_state(state)
}

use super::handlers::AppState;
use super::middleware as mw;
use axum::{
    http::{header, HeaderValue, Method},
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub fn create_router(state: AppState) -> Router {
    let cors = build_cors_layer(&state.config.server.website_cors_allowed_origins);

    let public = Router::new()
        .route("/health", get(super::handlers::health_check))
        .route("/api/register", post(super::handlers::register))
        .route("/api/login", post(super::handlers::login));

    let protected = Router::new()
        .route("/api/balance", get(super::handlers::get_balance))
        .route("/api/transfer", post(super::handlers::transfer_cpt))
        .route("/api/vpn/config", post(super::handlers::issue_vpn_config))
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

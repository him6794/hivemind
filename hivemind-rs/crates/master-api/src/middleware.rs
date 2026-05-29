use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
    http::StatusCode,
};
use super::handlers::AppState;

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(h) if h.starts_with("Bearer ") => {
            let token = &h[7..];
            state.auth.validate_token(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
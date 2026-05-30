use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
    http::{StatusCode, header},
};
use super::handlers::AppState;
use hivemind_models::Claims;

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(h) if h.starts_with("Bearer ") => {
            let token = &h[7..];
            match state.auth.validate_token(token) {
                Ok(claims) => {
                    // Inject claims into request extensions for handlers to extract
                    request.extensions_mut().insert(claims);
                    Ok(next.run(request).await)
                }
                Err(e) => {
                    tracing::warn!("Auth failed: {}", e);
                    Err(StatusCode::UNAUTHORIZED)
                }
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Extractor that pulls Claims from request extensions
pub struct AuthClaims(pub Claims);

#[axum::async_trait]
impl<S> axum::extract::FromRequestParts<S> for AuthClaims
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<Claims>()
            .cloned()
            .map(AuthClaims)
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

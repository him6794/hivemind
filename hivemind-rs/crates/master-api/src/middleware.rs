use super::handlers::AppState;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use hivemind_models::Claims;
use jsonwebtoken::{decode, DecodingKey, Validation};

/// Wraps the raw JWT token so handlers can forward it via gRPC.
#[derive(Clone)]
pub struct RawToken(pub String);

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    match auth_header {
        Some(h) if h.starts_with("Bearer ") => {
            let token = h[7..].to_string();

            match decode::<Claims>(
                &token,
                &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
                &Validation::default(),
            ) {
                Ok(token_data) => {
                    request.extensions_mut().insert(token_data.claims);
                    request.extensions_mut().insert(RawToken(token));
                    Ok(next.run(request).await)
                }
                Err(e) => {
                    tracing::warn!("JWT validation failed: {}", e);
                    Err(StatusCode::UNAUTHORIZED)
                }
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Combined extractor: both JWT claims and raw token for gRPC forwarding.
pub struct AuthUser {
    pub claims: Claims,
    pub token: String,
}

#[axum::async_trait]
impl<S> axum::extract::FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let claims = parts.extensions.get::<Claims>().cloned();
        let token = parts.extensions.get::<RawToken>().map(|t| t.0.clone());

        match (claims, token) {
            (Some(claims), Some(token)) => Ok(AuthUser { claims, token }),
            _ => Err(StatusCode::UNAUTHORIZED),
        }
    }
}

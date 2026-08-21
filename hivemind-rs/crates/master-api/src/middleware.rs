use super::handlers::AppState;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use hivemind_models::Claims;
use jsonwebtoken::{decode, DecodingKey, Validation};

/// Wraps the raw JWT token so handlers can forward it via gRPC.
#[derive(Clone)]
pub struct RawToken(pub String);

fn vpn_gate_response(state: hivemind_client_runtime::VpnBootstrapState) -> Response {
    let (status, message) = match state {
        hivemind_client_runtime::VpnBootstrapState::ReauthenticationRequired => (
            StatusCode::UNAUTHORIZED,
            "Authentication expired; sign in again.",
        ),
        hivemind_client_runtime::VpnBootstrapState::RetryableFailure => (
            StatusCode::BAD_GATEWAY,
            "VPN/Nodepool is unavailable; retry enrollment.",
        ),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            "VPN/Nodepool is not ready; retry enrollment.",
        ),
    };

    (
        status,
        Json(serde_json::json!({
            "success": false,
            "state": state.as_str(),
            "message": message,
        })),
    )
        .into_response()
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    match auth_header.as_deref().and_then(bearer_token) {
        Some(token) => {
            let token = token.to_string();
            match decode_user_claims(&token) {
                Ok(claims) => {
                    // Master is a user-deployed requestor client: it must not require
                    // the platform JWT signing secret. Local claim extraction is only
                    // for request routing / rate-limiting; nodepool remains the
                    // authority and validates the forwarded bearer token.
                    request.extensions_mut().insert(claims);
                    let gate_token = token.clone();
                    request.extensions_mut().insert(RawToken(token));
                    if !matches!(
                        request.uri().path(),
                        "/api/vpn/bootstrap" | "/api/vpn/status"
                    ) {
                        match hivemind_client_runtime::ensure_user_vpn_for_token(
                            &state.config,
                            hivemind_client_runtime::ClientRole::Master,
                            &gate_token,
                        )
                        .await
                        {
                            Ok(Some(endpoint)) => state.grpc_client.set_endpoint(endpoint).await,
                            Ok(None) => {}
                            Err(err) => {
                                let vpn_status = hivemind_client_runtime::current_vpn_status(
                                    hivemind_client_runtime::ClientRole::Master,
                                );
                                tracing::warn!(
                                    "Master VPN readiness gate failed (state={}): {}",
                                    vpn_status.state.as_str(),
                                    err
                                );
                                return Ok(vpn_gate_response(vpn_status.state));
                            }
                        }
                    }
                    Ok(next.run(request).await)
                }
                Err(e) => {
                    tracing::warn!("JWT claim extraction failed: {}", e);
                    Err(StatusCode::UNAUTHORIZED)
                }
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

fn bearer_token(value: &str) -> Option<&str> {
    let mut parts = value.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if parts.next().is_some() || !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    (!token.is_empty()).then_some(token)
}

/// Decode user claims without the platform signing secret.
///
/// Signature verification intentionally stays with nodepool. Master only needs
/// structural claims (subject / expiry) so it can forward the raw token.
pub fn decode_user_claims(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let mut validation = Validation::default();
    validation.insecure_disable_signature_validation();
    // Keep expiry checks so obviously expired browser tokens fail closed locally.
    validation.validate_exp = true;
    decode::<Claims>(
        token,
        // Key is ignored when signature validation is disabled.
        &DecodingKey::from_secret(&[]),
        &validation,
    )
    .map(|data| data.claims)
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

#[cfg(test)]
mod tests {
    use super::{bearer_token, decode_user_claims, vpn_gate_response};
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use chrono::Utc;
    use hivemind_models::Claims;
    use jsonwebtoken::{encode, EncodingKey, Header};

    fn sample_token(secret: &str, subject: &str, exp_offset_secs: i64) -> String {
        let now = Utc::now().timestamp();
        let claims = Claims {
            sub: subject.into(),
            user_id: "user-1".into(),
            role: None,
            task_id: None,
            worker_id: None,
            exp: (now + exp_offset_secs) as usize,
            iat: now as usize,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn bearer_token_accepts_case_and_whitespace_variants() {
        assert_eq!(bearer_token("Bearer abc"), Some("abc"));
        assert_eq!(bearer_token("bearer   abc"), Some("abc"));
        assert_eq!(bearer_token("BEARER\tabc"), Some("abc"));
        assert_eq!(bearer_token("Basic abc"), None);
        assert_eq!(bearer_token("Bearer abc extra"), None);
        assert_eq!(bearer_token("Bearer"), None);
    }

    #[tokio::test]
    async fn vpn_gate_failures_are_structured_and_non_secret() {
        let response =
            vpn_gate_response(hivemind_client_runtime::VpnBootstrapState::RetryableFailure);
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["success"], false);
        assert_eq!(value["state"], "retryable_failure");
        assert!(value.get("auth_key").is_none());
        assert!(!value.to_string().contains("tskey-auth"));
        assert!(!value.to_string().contains("HEADSCALE_API_KEY"));
    }

    #[test]
    fn master_decodes_claims_without_platform_signing_secret() {
        let token = sample_token(
            "platform-signing-secret-not-shared-with-master",
            "alice",
            3600,
        );
        let claims = decode_user_claims(&token).unwrap();
        assert_eq!(claims.sub, "alice");
        assert_eq!(claims.user_id, "user-1");
    }

    #[test]
    fn master_rejects_expired_claims_even_without_signature_check() {
        // jsonwebtoken default leeway is 60s; expire well beyond that.
        let token = sample_token("any-secret", "bob", -120);
        let err = decode_user_claims(&token).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("expired"));
    }
}

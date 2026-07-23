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
    State(_state): State<AppState>,
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
            match decode_user_claims(&token) {
                Ok(claims) => {
                    // Master is a user-deployed requestor client: it must not require
                    // the platform JWT signing secret. Local claim extraction is only
                    // for request routing / rate-limiting; nodepool remains the
                    // authority and validates the forwarded bearer token.
                    request.extensions_mut().insert(claims);
                    request.extensions_mut().insert(RawToken(token));
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
    use super::decode_user_claims;
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

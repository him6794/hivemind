use anyhow::{Context, Result};
use hivemind_models::{Claims, User};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

// The payload of a sign-in session token. The session id sits beside the claims instead of inside `Claims`,
// so every token keeps the claims it has always had, and an older reader still decodes these tokens.
#[derive(Serialize)]
struct SessionPayload<'a> {
    sub: &'a str,
    user_id: &'a str,
    exp: usize,
    iat: usize,
    sid: &'a str,
}

#[derive(Deserialize)]
struct SessionIdClaims {
    #[serde(default)]
    sid: Option<String>,
}

pub struct JwtService {
    secret: String,
    expiry_hours: i64,
}

impl JwtService {
    pub fn new(secret: &str, expiry_hours: i64) -> Self {
        Self {
            secret: secret.to_string(),
            expiry_hours,
        }
    }

    pub fn generate_token(&self, user: &User) -> Result<hivemind_models::TokenResponse> {
        let now = chrono::Utc::now();
        let expires_at = now + chrono::Duration::hours(self.expiry_hours);

        let claims = Claims {
            sub: user.username.clone(),
            user_id: user.id.to_string(),
            role: None,
            task_id: None,
            worker_id: None,
            exp: expires_at.timestamp() as usize,
            iat: now.timestamp() as usize,
        };

        let token = self.encode_claims(&claims)?;

        Ok(hivemind_models::TokenResponse { token, expires_at })
    }

    pub fn validate(&self, token: &str) -> Result<Claims> {
        self.decode(token)
    }

    pub fn encode_claims(&self, claims: &Claims) -> Result<String> {
        encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .context("Failed to encode JWT token")
    }

    /// A token for a sign-in session. Signing out ends the session, and the token then stops working.
    pub fn generate_session_token(
        &self,
        user: &User,
        session_id: &str,
    ) -> Result<hivemind_models::TokenResponse> {
        let now = chrono::Utc::now();
        let expires_at = now + chrono::Duration::hours(self.expiry_hours);
        let user_id = user.id.to_string();
        let token = self.encode_session_payload(&SessionPayload {
            sub: &user.username,
            user_id: &user_id,
            exp: expires_at.timestamp() as usize,
            iat: now.timestamp() as usize,
            sid: session_id,
        })?;

        Ok(hivemind_models::TokenResponse { token, expires_at })
    }

    /// Encodes the claims of a session token again for a refresh. The session and the expiry stay the same.
    pub fn encode_session_claims(&self, claims: &Claims, session_id: &str) -> Result<String> {
        self.encode_session_payload(&SessionPayload {
            sub: &claims.sub,
            user_id: &claims.user_id,
            exp: claims.exp,
            iat: claims.iat,
            sid: session_id,
        })
    }

    /// The session a token was issued for. Tokens from other flows have none. The signature and expiry are checked.
    pub fn session_id(&self, token: &str) -> Result<Option<String>> {
        let token_data = decode::<SessionIdClaims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &Validation::default(),
        )
        .context("Failed to decode JWT token")?;
        Ok(token_data.claims.sid)
    }

    fn encode_session_payload(&self, payload: &SessionPayload<'_>) -> Result<String> {
        encode(
            &Header::default(),
            payload,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .context("Failed to encode JWT token")
    }

    pub fn decode(&self, token: &str) -> Result<Claims> {
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &Validation::default(),
        )
        .context("Failed to decode JWT token")?;
        Ok(token_data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_token_carries_its_session_and_still_decodes_as_claims() {
        let jwt = JwtService::new("jwt-session-test-secret", 1);
        let now = chrono::Utc::now();
        let claims = Claims {
            sub: "alice".into(),
            user_id: "user-1".into(),
            role: None,
            task_id: None,
            worker_id: None,
            exp: (now + chrono::Duration::hours(1)).timestamp() as usize,
            iat: now.timestamp() as usize,
        };

        let token = jwt.encode_session_claims(&claims, "session-1").unwrap();
        assert_eq!(jwt.validate(&token).unwrap().sub, "alice");
        assert_eq!(
            jwt.session_id(&token).unwrap().as_deref(),
            Some("session-1")
        );

        // A token from another flow has no session to end.
        let plain = jwt.encode_claims(&claims).unwrap();
        assert_eq!(jwt.session_id(&plain).unwrap(), None);

        // The signature still has to match, whichever flow issued the token.
        let other_secret = JwtService::new("another-secret", 1);
        assert!(other_secret.session_id(&token).is_err());
    }
}

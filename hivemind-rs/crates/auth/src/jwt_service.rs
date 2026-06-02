use anyhow::{Context, Result};
use hivemind_models::{Claims, User};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};

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

pub mod jwt_service;
pub mod user_repository;

use anyhow::Result;
use hivemind_database::DatabaseManager;
use jwt_service::JwtService;
use std::sync::Arc;
use user_repository::UserRepository;

pub struct AuthManager {
    jwt: Arc<JwtService>,
    users: Arc<UserRepository>,
}

impl Clone for AuthManager {
    fn clone(&self) -> Self {
        Self {
            jwt: self.jwt.clone(),
            users: self.users.clone(),
        }
    }
}

impl AuthManager {
    pub fn new(db: &DatabaseManager, jwt_secret: &str, token_expiry_hours: i64) -> Self {
        Self {
            jwt: Arc::new(JwtService::new(jwt_secret, token_expiry_hours)),
            users: Arc::new(UserRepository::new(db.pool.clone())),
        }
    }

    /// Authenticate username/password, return JWT token on success
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<Option<String>> {
        let user = match self.users.find_by_username(username).await? {
            Some(u) => u,
            None => return Ok(None),
        };

        let valid = bcrypt::verify(password, &user.password_hash).unwrap_or(false);
        if !valid || !user.is_active {
            return Ok(None);
        }

        let token_response = self.jwt.generate_token(&user)?;
        Ok(Some(token_response.token))
    }

    /// Validate a JWT token and extract claims
    pub fn validate_token(&self, token: &str) -> Result<hivemind_models::Claims> {
        self.jwt.validate(token)
    }

    pub fn jwt_service(&self) -> &JwtService {
        &self.jwt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_config::HivemindConfig;

    async fn setup_test_db() -> Option<DatabaseManager> {
        let config = HivemindConfig::default();
        let db = DatabaseManager::new(&config).await.ok()?;
        db.run_migrations().await.ok()?;
        Some(db)
    }

    #[tokio::test]
    async fn test_authenticate_invalid_user() {
        let db = match setup_test_db().await {
            Some(d) => d,
            None => return,
        };
        let auth = AuthManager::new(&db, "test-secret", 24);
        let token = auth.authenticate("nonexistent", "pass").await.unwrap();
        assert!(token.is_none());
    }

    #[tokio::test]
    async fn test_jwt_token_roundtrip() {
        let jwt = JwtService::new("test-secret", 24);
        let claims = hivemind_models::Claims {
            sub: "testuser".into(),
            user_id: uuid::Uuid::new_v4().to_string(),
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        let token = jwt.encode_claims(&claims).unwrap();
        let decoded = jwt.decode(&token).unwrap();
        assert_eq!(decoded.sub, "testuser");
    }
}

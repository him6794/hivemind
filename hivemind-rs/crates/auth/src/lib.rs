pub mod jwt_service;
pub mod user_repository;

use anyhow::Result;
use hivemind_database::DatabaseManager;
use hivemind_models::{LoginRequest, LoginResponse};
use jwt_service::JwtService;
use user_repository::UserRepository;
use std::sync::Arc;

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

    pub async fn login(&self, req: &LoginRequest) -> Result<LoginResponse> {
        let user = match self.users.find_by_username(&req.username).await? {
            Some(u) => u,
            None => {
                return Ok(LoginResponse {
                    success: false,
                    status_message: "Invalid username or password".into(),
                    token: None,
                });
            }
        };

        let valid = bcrypt::verify(&req.password, &user.password_hash)
            .unwrap_or(false);

        if !valid {
            return Ok(LoginResponse {
                success: false,
                status_message: "Invalid username or password".into(),
                token: None,
            });
        }

        if !user.is_active {
            return Ok(LoginResponse {
                success: false,
                status_message: "Account is deactivated".into(),
                token: None,
            });
        }

        let token_response = self.jwt.generate_token(&user)?;

        Ok(LoginResponse {
            success: true,
            status_message: "Login successful".into(),
            token: Some(token_response.token),
        })
    }

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
    use hivemind_database::DatabaseManager;

    async fn setup_test_db() -> Option<DatabaseManager> {
        let config = HivemindConfig::default();
        let db = DatabaseManager::new(&config).await.ok()?;
        db.run_migrations().await.ok()?;
        Some(db)
    }

    #[tokio::test]
    async fn test_login_invalid_user() {
        let db = setup_test_db().await;
        if db.is_none() {
            eprintln!("Skipping: no database available");
            return;
        }
        let db = db.unwrap();

        let auth = AuthManager::new(&db, "test-secret", 24);
        let resp = auth
            .login(&LoginRequest {
                username: "nonexistent_user".into(),
                password: "pass".into(),
            })
            .await
            .unwrap();

        assert!(!resp.success);
        assert!(resp.token.is_none());
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
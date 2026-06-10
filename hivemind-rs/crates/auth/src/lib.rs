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

    async fn setup_test_db(
        test_name: &str,
    ) -> Option<(
        DatabaseManager,
        hivemind_database::postgres::IsolatedTestPool,
    )> {
        let fixture = hivemind_database::postgres::create_isolated_test_pool(test_name)
            .await
            .ok()?;
        hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .ok()?;
        let db = DatabaseManager {
            pool: fixture.pool.clone(),
        };
        Some((db, fixture))
    }

    #[tokio::test]
    async fn setup_test_db_uses_isolated_schema() {
        let (db, fixture) = match setup_test_db("auth_setup_schema").await {
            Some(parts) => parts,
            None => return,
        };

        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert!(
            schema.starts_with("hm_test_"),
            "expected isolated test schema, got {schema}"
        );
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_authenticate_invalid_user() {
        let (db, fixture) = match setup_test_db("auth_invalid_user").await {
            Some(parts) => parts,
            None => return,
        };
        let auth = AuthManager::new(&db, "test-secret", 24);
        let token = auth.authenticate("nonexistent", "pass").await.unwrap();
        assert!(token.is_none());
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_jwt_token_roundtrip() {
        let jwt = JwtService::new("test-secret", 24);
        let claims = hivemind_models::Claims {
            sub: "example-user".into(),
            user_id: uuid::Uuid::new_v4().to_string(),
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        let token = jwt.encode_claims(&claims).unwrap();
        let decoded = jwt.decode(&token).unwrap();
        assert_eq!(decoded.sub, "example-user");
    }
}

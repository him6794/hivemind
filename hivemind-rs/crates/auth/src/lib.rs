pub mod api_tokens;
pub mod artifact_probe;
pub mod artifact_read;
pub mod enrollment;
pub mod jwt_service;
pub mod session_store;
pub mod user_repository;
pub mod worker_execution;

use anyhow::Result;
use api_tokens::ApiTokenStore;
use hivemind_database::DatabaseManager;
use jwt_service::JwtService;
use session_store::SessionStore;
use std::sync::Arc;
use user_repository::UserRepository;

pub struct AuthManager {
    jwt: Arc<JwtService>,
    users: Arc<UserRepository>,
    sessions: Arc<SessionStore>,
    api_tokens: Arc<ApiTokenStore>,
}

impl Clone for AuthManager {
    fn clone(&self) -> Self {
        Self {
            jwt: self.jwt.clone(),
            users: self.users.clone(),
            sessions: self.sessions.clone(),
            api_tokens: self.api_tokens.clone(),
        }
    }
}

impl AuthManager {
    pub fn new(db: &DatabaseManager, jwt_secret: &str, token_expiry_hours: i64) -> Self {
        Self {
            jwt: Arc::new(JwtService::new(jwt_secret, token_expiry_hours)),
            users: Arc::new(UserRepository::new(db.pool.clone())),
            sessions: Arc::new(SessionStore::new(db.pool.clone())),
            api_tokens: Arc::new(ApiTokenStore::new(db.pool.clone())),
        }
    }

    /// API token management. Callers must check that the request came from a sign-in session.
    pub fn api_tokens(&self) -> &ApiTokenStore {
        &self.api_tokens
    }

    /// Authenticate username/password, return JWT token on success
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<Option<String>> {
        if username == hivemind_models::SYSTEM_ACCOUNT_USERNAME {
            return Ok(None);
        }
        // The account row is locked (shared) from the password check until the session commits. A password
        // change needs the row lock first, so it cannot end the sessions while a sign-in that checked the old
        // password is still open. Either the change waits for this session, or the sign-in reads the new hash.
        let mut tx = self.sessions.begin().await?;
        let user = match self
            .users
            .find_active_for_sign_in(&mut tx, username)
            .await?
        {
            Some(u) => u,
            None => {
                tx.rollback().await?;
                return Ok(None);
            }
        };

        let valid = bcrypt::verify(password, &user.password_hash).unwrap_or(false);
        if !valid || !user.is_active {
            tx.rollback().await?;
            return Ok(None);
        }

        let session_id = uuid::Uuid::new_v4().to_string();
        let token_response = self.jwt.generate_session_token(&user, &session_id)?;
        self.sessions
            .open(
                &mut tx,
                &session_id,
                &user.username,
                token_response.expires_at,
            )
            .await?;
        tx.commit().await?;
        Ok(Some(token_response.token))
    }

    /// Validate a JWT token and extract claims. This checks the signature and expiry only. A token that acts
    /// for the user should go through `validate_session`, which also checks the sign-in session.
    pub fn validate_token(&self, token: &str) -> Result<hivemind_models::Claims> {
        self.jwt.validate(token)
    }

    /// Validate a token. A sign-in session must not have ended. A token with no session must have been issued
    /// after the account's last password change. An API token is valid while it is not revoked, not expired,
    /// and its account is active; a password change revokes it.
    pub async fn validate_session(&self, token: &str) -> Result<hivemind_models::Claims> {
        if api_tokens::is_api_token(token) {
            let owner = self
                .api_tokens
                .validate(token)
                .await?
                .ok_or_else(|| anyhow::anyhow!("API token is not valid"))?;
            return Ok(hivemind_models::Claims {
                sub: owner.username,
                user_id: owner.user_id.to_string(),
                role: None,
                task_id: None,
                worker_id: None,
                exp: owner.expires_at.timestamp().max(0) as usize,
                iat: owner.created_at.timestamp().max(0) as usize,
            });
        }
        let claims = self.jwt.validate(token)?;
        match self.jwt.session_id(token)? {
            Some(session_id) => {
                if !self.sessions.is_active(&session_id, &claims.sub).await? {
                    anyhow::bail!("session has ended");
                }
            }
            None => {
                if !self
                    .sessions
                    .password_unchanged_since(&claims.sub, claims.iat)
                    .await?
                {
                    anyhow::bail!("password changed after this token was issued");
                }
            }
        }
        Ok(claims)
    }

    /// Ends the sign-in session a token belongs to. A token with no session, or one that is not valid, has
    /// nothing to end, and that returns false.
    pub async fn end_session(&self, token: &str) -> Result<bool> {
        let Ok(claims) = self.jwt.validate(token) else {
            return Ok(false);
        };
        match self.jwt.session_id(token)? {
            Some(session_id) => self.sessions.end(&session_id, &claims.sub).await,
            None => Ok(false),
        }
    }

    /// The token for a refresh: the same claims and the same session, once the session has been checked.
    /// An API token has no session to renew; a new token is created instead.
    pub async fn renew_token(&self, token: &str) -> Result<String> {
        if api_tokens::is_api_token(token) {
            anyhow::bail!("API tokens are not refreshed; create a new token instead");
        }
        let claims = self.validate_session(token).await?;
        match self.jwt.session_id(token)? {
            Some(session_id) => self.jwt.encode_session_claims(&claims, &session_id),
            None => self.jwt.encode_claims(&claims),
        }
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
            role: None,
            task_id: None,
            worker_id: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        let token = jwt.encode_claims(&claims).unwrap();
        let decoded = jwt.decode(&token).unwrap();
        assert_eq!(decoded.sub, "example-user");
    }
}

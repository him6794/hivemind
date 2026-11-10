//! API tokens: long-lived bearer credentials for scripts and SDKs.
//!
//! A token is `hmk_` followed by 64 hex characters from the operating system's random source. Nodepool
//! stores only the SHA-256 digest of the token, and returns the plain token once, when it is created.
//! Nodepool is the only place that validates a token, so a Master or a client never holds a verifier.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Prefix that marks an API token. A bearer token with this prefix is never decoded as a JWT.
pub use hivemind_models::API_TOKEN_PREFIX;
/// Lifetime when a request names none.
pub const API_TOKEN_DEFAULT_DAYS: i64 = 90;
/// Longest lifetime a token may be given.
pub const API_TOKEN_MAX_DAYS: i64 = 365;
/// Most tokens an account may hold at once. Expired and revoked tokens do not count.
pub const API_TOKEN_MAX_ACTIVE: i64 = 20;
/// Longest name a token may carry.
pub const API_TOKEN_MAX_NAME_CHARS: usize = 64;
/// Seconds between writes of `last_used_at` for one token.
const LAST_USED_WRITE_INTERVAL_SECS: i64 = 60;

/// Whether a bearer token is an API token. Only the prefix is inspected; the token is not validated here.
pub fn is_api_token(token: &str) -> bool {
    token.starts_with(API_TOKEN_PREFIX)
}

/// A new random token. Its digest is what the database keeps.
pub fn generate_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("{API_TOKEN_PREFIX}{}", hex::encode(bytes))
}

/// The digest a token is stored and looked up by.
pub fn token_digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// The part of a token that listings show: the prefix and eight hex characters. That is enough to tell
/// two tokens apart and far too little to use one.
fn display_prefix(token: &str) -> String {
    token.chars().take(API_TOKEN_PREFIX.len() + 8).collect()
}

/// Why a token request failed. `Rejected` is a rule the caller broke, and its message is safe to show.
/// `Storage` is a database failure, and the caller should report it as a server error.
#[derive(Debug)]
pub enum ApiTokenError {
    Rejected(String),
    Storage(anyhow::Error),
}

impl std::fmt::Display for ApiTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(message) => f.write_str(message),
            Self::Storage(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ApiTokenError {}

impl From<sqlx::Error> for ApiTokenError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(error.into())
    }
}

/// A token just created. `token` is the only copy of the plain value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedApiToken {
    pub id: Uuid,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// A stored token as listings show it. The secret is never part of a record.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ApiTokenRecord {
    pub id: Uuid,
    pub name: String,
    pub token_prefix: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// The account a valid token acts for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenOwner {
    pub username: String,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct ApiTokenStore {
    pool: PgPool,
}

impl ApiTokenStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a token for an active account. Creation locks the account row, so two requests cannot both
    /// pass the count of active tokens.
    pub async fn create(
        &self,
        username: &str,
        name: &str,
        days: i64,
    ) -> Result<CreatedApiToken, ApiTokenError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > API_TOKEN_MAX_NAME_CHARS {
            return Err(ApiTokenError::Rejected(format!(
                "token name must be 1 to {API_TOKEN_MAX_NAME_CHARS} characters"
            )));
        }
        if !(1..=API_TOKEN_MAX_DAYS).contains(&days) {
            return Err(ApiTokenError::Rejected(format!(
                "token lifetime must be 1 to {API_TOKEN_MAX_DAYS} days"
            )));
        }
        let token = generate_token();
        let digest = token_digest(&token);
        let prefix = display_prefix(&token);
        let expires_at = Utc::now() + Duration::days(days);

        let mut tx = self.pool.begin().await?;
        let user_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM users WHERE username = $1 AND is_active = true FOR UPDATE",
        )
        .bind(username)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(user_id) = user_id else {
            return Err(ApiTokenError::Rejected(
                "account not found or inactive".into(),
            ));
        };
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_tokens
             WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > NOW()",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        if active >= API_TOKEN_MAX_ACTIVE {
            return Err(ApiTokenError::Rejected(format!(
                "an account may hold at most {API_TOKEN_MAX_ACTIVE} active API tokens; revoke one first"
            )));
        }
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO api_tokens (user_id, name, token_prefix, token_hash, expires_at)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(user_id)
        .bind(name)
        .bind(&prefix)
        .bind(&digest)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(CreatedApiToken {
            id,
            token,
            expires_at,
        })
    }

    /// The account's tokens, newest first, including revoked and expired ones.
    pub async fn list(&self, username: &str) -> Result<Vec<ApiTokenRecord>> {
        let records = sqlx::query_as::<_, ApiTokenRecord>(
            "SELECT t.id, t.name, t.token_prefix, t.created_at, t.expires_at, t.last_used_at, t.revoked_at
             FROM api_tokens t
             JOIN users u ON u.id = t.user_id
             WHERE u.username = $1
             ORDER BY t.created_at DESC
             LIMIT 100",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await?;
        Ok(records)
    }

    /// Revokes one of the account's tokens. Returns false when it was not found, belongs to someone else,
    /// or was already revoked.
    pub async fn revoke(&self, username: &str, id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE api_tokens SET revoked_at = NOW()
             WHERE id = $1 AND revoked_at IS NULL
               AND user_id = (SELECT id FROM users WHERE username = $2)",
        )
        .bind(id)
        .bind(username)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// The account a token acts for, or `None` when the token is unknown, revoked, expired, or its account
    /// is inactive. A successful check records use, at most once a minute per token.
    pub async fn validate(&self, token: &str) -> Result<Option<TokenOwner>> {
        let digest = token_digest(token);
        let row = sqlx::query_as::<_, (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
            "SELECT t.id, u.id, u.username, t.created_at, t.expires_at
             FROM api_tokens t
             JOIN users u ON u.id = t.user_id
             WHERE t.token_hash = $1
               AND t.revoked_at IS NULL
               AND t.expires_at > NOW()
               AND u.is_active = true",
        )
        .bind(&digest)
        .fetch_optional(&self.pool)
        .await?;
        let Some((id, user_id, username, created_at, expires_at)) = row else {
            return Ok(None);
        };
        sqlx::query(
            "UPDATE api_tokens SET last_used_at = NOW()
             WHERE id = $1
               AND (last_used_at IS NULL OR last_used_at < NOW() - make_interval(secs => $2))",
        )
        .bind(id)
        .bind(LAST_USED_WRITE_INTERVAL_SECS as f64)
        .execute(&self.pool)
        .await?;
        Ok(Some(TokenOwner {
            username,
            user_id,
            created_at,
            expires_at,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_have_the_prefix_and_256_bits_of_hex() {
        let token = generate_token();
        assert!(is_api_token(&token));
        let body = token.strip_prefix(API_TOKEN_PREFIX).unwrap();
        assert_eq!(body.len(), 64);
        assert!(body.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(token, generate_token());
    }

    #[test]
    fn digests_are_stable_and_never_the_token() {
        let token = "hmk_0123456789abcdef";
        let digest = token_digest(token);
        assert_eq!(digest, token_digest(token));
        assert_eq!(digest.len(), 64);
        assert!(!digest.contains("hmk_"));
        assert_ne!(digest, token_digest("hmk_0123456789abcdee"));
    }

    #[test]
    fn only_the_prefix_marks_an_api_token() {
        assert!(is_api_token("hmk_anything"));
        assert!(!is_api_token("eyJhbGciOi.payload.sig"));
        assert!(!is_api_token(" hmk_with_space"));
    }

    #[test]
    fn listings_show_a_prefix_that_cannot_be_used_as_a_token() {
        let token = generate_token();
        let shown = display_prefix(&token);
        assert_eq!(shown.len(), API_TOKEN_PREFIX.len() + 8);
        assert!(token.starts_with(&shown));
        assert_ne!(shown, token);
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::{session_store, AuthManager};
    use hivemind_database::DatabaseManager;

    // Without a database the tests return early, the same as the other database tests in this crate.
    // HIVEMIND_REQUIRE_TEST_DATABASE=1 turns that skip into a failure.
    async fn setup(
        test_name: &str,
    ) -> Option<(PgPool, hivemind_database::postgres::IsolatedTestPool)> {
        let fixture = hivemind_database::postgres::create_isolated_test_pool(test_name)
            .await
            .ok()?;
        hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .ok()?;
        Some((fixture.pool.clone(), fixture))
    }

    async fn account(pool: &PgPool, username: &str) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO users (username, password_hash, balance, is_active)
             VALUES ($1, '!test-only', 0, true) RETURNING id",
        )
        .bind(username)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_created_token_validates_to_its_account_until_it_is_revoked() {
        let Some((pool, fixture)) = setup("api_token_lifecycle").await else {
            return;
        };
        account(&pool, "alice").await;
        let store = ApiTokenStore::new(pool.clone());

        let created = store.create("alice", "nightly sweep", 30).await.unwrap();
        assert!(is_api_token(&created.token));
        let owner = store.validate(&created.token).await.unwrap().unwrap();
        assert_eq!(owner.username, "alice");

        assert!(store.validate(&generate_token()).await.unwrap().is_none());

        let listed = store.list("alice").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "nightly sweep");
        assert_eq!(listed[0].token_prefix, display_prefix(&created.token));
        assert!(listed[0].revoked_at.is_none());

        assert!(store.revoke("alice", created.id).await.unwrap());
        assert!(!store.revoke("alice", created.id).await.unwrap());
        assert!(store.validate(&created.token).await.unwrap().is_none());
        assert!(store.list("alice").await.unwrap()[0].revoked_at.is_some());
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn a_token_cannot_be_revoked_by_another_account() {
        let Some((pool, fixture)) = setup("api_token_ownership").await else {
            return;
        };
        account(&pool, "alice").await;
        account(&pool, "bob").await;
        let store = ApiTokenStore::new(pool.clone());
        let created = store.create("alice", "mine", 30).await.unwrap();
        assert!(!store.revoke("bob", created.id).await.unwrap());
        assert!(store.validate(&created.token).await.unwrap().is_some());
        assert!(store.list("bob").await.unwrap().is_empty());
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn an_expired_token_or_an_inactive_account_does_not_validate() {
        let Some((pool, fixture)) = setup("api_token_expiry").await else {
            return;
        };
        account(&pool, "alice").await;
        let store = ApiTokenStore::new(pool.clone());
        let expired = store.create("alice", "old", 1).await.unwrap();
        sqlx::query("UPDATE api_tokens SET expires_at = NOW() - INTERVAL '1 second' WHERE id = $1")
            .bind(expired.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(store.validate(&expired.token).await.unwrap().is_none());

        let live = store.create("alice", "live", 30).await.unwrap();
        assert!(store.validate(&live.token).await.unwrap().is_some());
        sqlx::query("UPDATE users SET is_active = false WHERE username = 'alice'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(store.validate(&live.token).await.unwrap().is_none());
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn creation_checks_the_name_the_lifetime_the_account_and_the_limit() {
        let Some((pool, fixture)) = setup("api_token_limits").await else {
            return;
        };
        account(&pool, "alice").await;
        let store = ApiTokenStore::new(pool.clone());
        assert!(store.create("alice", "   ", 30).await.is_err());
        assert!(store
            .create("alice", &"n".repeat(API_TOKEN_MAX_NAME_CHARS + 1), 30)
            .await
            .is_err());
        assert!(store.create("alice", "bad lifetime", 0).await.is_err());
        assert!(store
            .create("alice", "too long", API_TOKEN_MAX_DAYS + 1)
            .await
            .is_err());
        assert!(store.create("nobody", "ghost", 30).await.is_err());

        let mut ids = Vec::new();
        for n in 0..API_TOKEN_MAX_ACTIVE {
            ids.push(
                store
                    .create("alice", &format!("t{n}"), 30)
                    .await
                    .unwrap()
                    .id,
            );
        }
        let refused = store.create("alice", "one too many", 30).await.unwrap_err();
        assert!(refused.to_string().contains("at most 20"), "{refused}");

        assert!(store.revoke("alice", ids[0]).await.unwrap());
        assert!(store.create("alice", "replacement", 30).await.is_ok());
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn a_password_change_revokes_the_accounts_api_tokens() {
        let Some((pool, fixture)) = setup("api_token_password_change").await else {
            return;
        };
        let user_id = account(&pool, "alice").await;
        let store = ApiTokenStore::new(pool.clone());
        let created = store.create("alice", "ci", 30).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        session_store::end_account_sessions(&mut tx, user_id, None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(store.validate(&created.token).await.unwrap().is_none());
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn an_api_token_authenticates_requests_but_cannot_renew_itself() {
        let Some((pool, fixture)) = setup("api_token_session").await else {
            return;
        };
        account(&pool, "alice").await;
        let db = DatabaseManager { pool: pool.clone() };
        let auth = AuthManager::new(&db, "test-secret", 24);
        let store = ApiTokenStore::new(pool.clone());
        let created = store.create("alice", "sdk", 30).await.unwrap();

        let claims = auth.validate_session(&created.token).await.unwrap();
        assert_eq!(claims.sub, "alice");
        assert!(claims.exp > claims.iat);
        let renewed = auth.renew_token(&created.token).await.unwrap_err();
        assert!(renewed.to_string().contains("not refreshed"), "{renewed}");
        assert!(auth.validate_session("hmk_not_a_real_token").await.is_err());
        fixture.cleanup().await.ok();
    }
}

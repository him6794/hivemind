use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

// Sign-in sessions. A token whose session has been ended stops working at once, not only when it expires.
pub struct SessionStore {
    pool: PgPool,
}

impl SessionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // Starts the transaction a sign-in runs in. The transaction holds the account's row lock until it ends.
    pub async fn begin(&self) -> Result<Transaction<'static, Postgres>> {
        Ok(self.pool.begin().await?)
    }

    // Opens the session inside the sign-in's transaction, so it commits only with the lock still held.
    pub async fn open(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
        username: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO account_sessions (id, username, expires_at) VALUES ($1, $2, $3)")
            .bind(session_id)
            .bind(username)
            .bind(expires_at)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    // True while the session exists for this user, has not been ended, and has not expired.
    pub async fn is_active(&self, session_id: &str, username: &str) -> Result<bool> {
        let active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM account_sessions
                WHERE id = $1 AND username = $2 AND revoked_at IS NULL AND expires_at > NOW()
            )",
        )
        .bind(session_id)
        .bind(username)
        .fetch_one(&self.pool)
        .await?;
        Ok(active)
    }

    // Returns false when the session was already ended, or does not belong to this user.
    pub async fn end(&self, session_id: &str, username: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE account_sessions SET revoked_at = NOW()
             WHERE id = $1 AND username = $2 AND revoked_at IS NULL",
        )
        .bind(session_id)
        .bind(username)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    // True unless the account's password changed after the token was issued. A token from before sessions
    // existed has no session row, so this check is what ends it when the password changes.
    pub async fn password_unchanged_since(&self, username: &str, issued_at: usize) -> Result<bool> {
        let changed: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM users
                WHERE username = $1 AND password_changed_at > to_timestamp($2::double precision)
            )",
        )
        .bind(username)
        .bind(issued_at as f64)
        .fetch_one(&self.pool)
        .await?;
        Ok(!changed)
    }
}

/// Ends every sign-in session of one account inside the caller's transaction, so a password change commits
/// together with the sessions it ends. `keep` is the session that made the change, if there is one.
/// The account's API tokens are revoked in the same transaction: they are credentials for the same account,
/// and a password change must end them too. Tokens are never kept.
pub async fn end_account_sessions(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    keep: Option<&str>,
) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE account_sessions SET revoked_at = NOW()
         WHERE revoked_at IS NULL
           AND username = (SELECT username FROM users WHERE id = $1)
           AND ($2::text IS NULL OR id <> $2)",
    )
    .bind(user_id)
    .bind(keep)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE api_tokens SET revoked_at = NOW()
         WHERE revoked_at IS NULL AND user_id = $1",
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected())
}

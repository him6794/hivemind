use anyhow::Result;
use hivemind_models::User;
use sqlx::PgPool;
use uuid::Uuid;

pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE username = $1 AND is_active = true",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(user)
    }

    pub async fn get_balance(&self, username: &str) -> Result<i64> {
        let balance: Option<i64> =
            sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
                .bind(username)
                .fetch_optional(&self.pool)
                .await?;
        Ok(balance.unwrap_or(0))
    }

    pub async fn deduct_balance(&self, username: &str, amount: i64) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE users SET balance = balance - $1, updated_at = NOW() WHERE username = $2 AND balance >= $1",
        )
        .bind(amount)
        .bind(username)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

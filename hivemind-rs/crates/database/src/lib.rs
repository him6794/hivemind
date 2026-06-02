use anyhow::Result;
use hivemind_config::HivemindConfig;
use sqlx::PgPool;

pub struct DatabaseManager {
    pub pool: PgPool,
}

impl DatabaseManager {
    pub async fn new(config: &HivemindConfig) -> Result<Self> {
        let pool = postgres::create_pool(config).await?;
        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> Result<()> {
        postgres::run_migrations(&self.pool).await
    }
}

impl Clone for DatabaseManager {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}

pub mod postgres;
pub mod redis_store;

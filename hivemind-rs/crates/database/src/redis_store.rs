use anyhow::{Context, Result};
use deadpool_redis::{Config, Pool, Runtime};
use hivemind_config::HivemindConfig;

pub fn create_pool(config: &HivemindConfig) -> Result<Pool> {
    let cfg = Config::from_url(&config.redis.url);
    let pool = cfg
        .create_pool(Some(Runtime::Tokio1))
        .with_context(|| format!("Failed to create Redis pool from {}", config.redis.url))?;

    tracing::info!("Redis pool created for {}", config.redis.url);
    Ok(pool)
}

/// Redis key helpers
pub const fn task_key(task_id: &str) -> String {
    // Will be formatted; placeholder prefix
    todo!()
}

pub const fn user_tasks_key(owner: &str) -> String {
    todo!()
}

pub const fn active_tasks_key() -> &'static str {
    "tasks:active"
}

pub const fn worker_key(worker_id: &str) -> &'static str {
    todo!()
}
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

// Redis key helpers

pub fn task_key(task_id: &str) -> String {
    format!("task:{}", task_id)
}

pub fn user_tasks_key(owner: &str) -> String {
    format!("tasks:owner:{}", owner)
}

pub fn active_tasks_key() -> &'static str {
    "tasks:active"
}

pub fn worker_key(worker_id: &str) -> String {
    format!("worker:{}", worker_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_key_format() {
        assert_eq!(task_key("abc-123"), "task:abc-123");
    }

    #[test]
    fn test_user_tasks_key_format() {
        assert_eq!(user_tasks_key("testuser"), "tasks:owner:testuser");
    }

    #[test]
    fn test_active_tasks_key() {
        assert_eq!(active_tasks_key(), "tasks:active");
    }

    #[test]
    fn test_worker_key_format() {
        assert_eq!(worker_key("worker-1"), "worker:worker-1");
    }

    #[test]
    fn test_create_pool_default_config() {
        let config = HivemindConfig::default();
        let pool = create_pool(&config);
        assert!(
            pool.is_ok(),
            "Redis pool creation should succeed with default config"
        );
    }
}

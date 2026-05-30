use anyhow::{Context, Result};
use hivemind_config::HivemindConfig;
use sqlx::postgres::{PgPool, PgPoolOptions};

pub async fn create_pool(config: &HivemindConfig) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .min_connections(config.database.min_connections)
        .idle_timeout(std::time::Duration::from_secs(config.database.idle_timeout_secs))
        .acquire_timeout(std::time::Duration::from_secs(config.database.connect_timeout_secs))
        .connect(&config.database.url)
        .await
        .context("Failed to connect to PostgreSQL")?;
    tracing::info!("Connected to PostgreSQL at {}", config.database.url);
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            username VARCHAR(255) NOT NULL UNIQUE,
            password_hash VARCHAR(255) NOT NULL,
            balance BIGINT NOT NULL DEFAULT 0,
            is_active BOOLEAN NOT NULL DEFAULT true,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS worker_nodes (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            worker_id VARCHAR(255) NOT NULL UNIQUE,
            username VARCHAR(255) NOT NULL,
            ip VARCHAR(45) NOT NULL,
            virtual_ip VARCHAR(45),
            hostname VARCHAR(255),
            cpu_cores INTEGER NOT NULL DEFAULT 0,
            memory_gb INTEGER NOT NULL DEFAULT 0,
            cpu_score INTEGER NOT NULL DEFAULT 0,
            gpu_score INTEGER NOT NULL DEFAULT 0,
            gpu_memory_gb INTEGER NOT NULL DEFAULT 0,
            gpu_name VARCHAR(255),
            vram_mb BIGINT NOT NULL DEFAULT 0,
            storage_total_gb BIGINT NOT NULL DEFAULT 0,
            storage_available_gb BIGINT NOT NULL DEFAULT 0,
            location VARCHAR(255) NOT NULL DEFAULT '',
            status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',
            cpu_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
            memory_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
            gpu_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
            gpu_memory_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
            available_memory_gb INTEGER NOT NULL DEFAULT 0,
            queue_capacity INTEGER NOT NULL DEFAULT 0,
            last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tasks (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL UNIQUE,
            owner VARCHAR(255) NOT NULL,
            worker_id VARCHAR(255),
            worker_ip VARCHAR(45),
            status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
            status_message TEXT,
            output TEXT,
            result_torrent TEXT,
            torrent_source TEXT,
            expected_btih VARCHAR(64),
            cpu_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
            memory_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
            gpu_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
            gpu_memory_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
            req_cpu_score INTEGER NOT NULL DEFAULT 0,
            req_gpu_score INTEGER NOT NULL DEFAULT 0,
            req_memory_gb INTEGER NOT NULL DEFAULT 0,
            req_gpu_memory_gb INTEGER NOT NULL DEFAULT 0,
            req_storage_gb BIGINT NOT NULL DEFAULT 0,
            host_count INTEGER NOT NULL DEFAULT 1,
            max_cpt BIGINT NOT NULL DEFAULT 0,
            billing_settled BOOLEAN NOT NULL DEFAULT false,
            billed_amount BIGINT NOT NULL DEFAULT 0,
            retry_count INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3,
            deadline TIMESTAMPTZ,
            deterministic BOOLEAN NOT NULL DEFAULT false,
            side_effects BOOLEAN NOT NULL DEFAULT false,
            priority INTEGER NOT NULL DEFAULT 0,
            cpu_time_ms BIGINT NOT NULL DEFAULT 0,
            wall_time_ms BIGINT NOT NULL DEFAULT 0,
            peak_memory_mb BIGINT NOT NULL DEFAULT 0,
            download_bytes BIGINT NOT NULL DEFAULT 0,
            cache_hits BIGINT NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            last_update TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            completed_at TIMESTAMPTZ
        );",
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS vpn_peers (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            worker_id VARCHAR(255) NOT NULL UNIQUE,
            hostname VARCHAR(255) NOT NULL,
            virtual_ip VARCHAR(45) NOT NULL UNIQUE,
            auth_key VARCHAR(255),
            online BOOLEAN NOT NULL DEFAULT false,
            last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    ).execute(pool).await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_owner ON tasks(owner);").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_worker_id ON tasks(worker_id);").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_worker_nodes_status ON worker_nodes(status);").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_worker_nodes_username ON worker_nodes(username);").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vpn_peers_worker_id ON vpn_peers(worker_id);").execute(pool).await?;

    let _ = sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS gpu_name VARCHAR(255);").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS vram_mb BIGINT NOT NULL DEFAULT 0;").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS storage_total_gb BIGINT NOT NULL DEFAULT 0;").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS storage_available_gb BIGINT NOT NULL DEFAULT 0;").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS req_storage_gb BIGINT NOT NULL DEFAULT 0;").execute(pool).await;

    tracing::info!("Database migrations completed successfully");
    Ok(())
}

pub async fn seed_default_user(pool: &PgPool) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)",
    )
    .bind("testuser")
    .fetch_one(pool)
    .await?;

    if !exists {
        let hash = bcrypt::hash("testpass123", 12)?;
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, $3)",
        )
        .bind("testuser")
        .bind(&hash)
        .bind(1000i64)
        .execute(pool)
        .await?;
        tracing::info!("Seeded default test user: testuser / testpass123");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_migration_idempotent() {
        let db_url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        let pool = match PgPoolOptions::new().max_connections(1).connect(&db_url).await {
            Ok(p) => p,
            Err(_) => { tracing::warn!("Skipping DB test"); return; }
        };
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
        seed_default_user(&pool).await.unwrap();
    }
}

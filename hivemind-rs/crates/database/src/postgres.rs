use anyhow::{Context, Result};
use hivemind_config::HivemindConfig;
#[cfg(any(test, debug_assertions))]
use sqlx::postgres::PgConnectOptions;
use sqlx::postgres::{PgPool, PgPoolOptions};
#[cfg(any(test, debug_assertions))]
use sqlx::AssertSqlSafe;
#[cfg(any(test, debug_assertions))]
use std::str::FromStr;
#[cfg(any(test, debug_assertions))]
use uuid::Uuid;

pub async fn create_pool(config: &HivemindConfig) -> Result<PgPool> {
    let pool = pool_options(config)
        .connect(&config.database.url)
        .await
        .context("Failed to connect to PostgreSQL")?;
    tracing::info!("Connected to PostgreSQL at {}", config.database.url);
    Ok(pool)
}

fn pool_options(config: &HivemindConfig) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .min_connections(config.database.min_connections)
        .idle_timeout(std::time::Duration::from_secs(
            config.database.idle_timeout_secs,
        ))
        .acquire_timeout(std::time::Duration::from_secs(
            config.database.connect_timeout_secs,
        ))
}

#[cfg(any(test, debug_assertions))]
pub struct IsolatedTestPool {
    pub pool: PgPool,
    admin_pool: PgPool,
    schema_name: String,
}

#[cfg(any(test, debug_assertions))]
impl IsolatedTestPool {
    pub fn schema_name(&self) -> &str {
        &self.schema_name
    }

    pub async fn cleanup(self) -> Result<()> {
        self.pool.close().await;
        let sql = format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema_name);
        sqlx::query(AssertSqlSafe(sql))
            .execute(&self.admin_pool)
            .await?;
        self.admin_pool.close().await;
        Ok(())
    }
}

#[cfg(any(test, debug_assertions))]
pub async fn create_isolated_test_pool(test_name: &str) -> Result<IsolatedTestPool> {
    let config = HivemindConfig::for_test();
    create_isolated_test_pool_with_config(&config, test_name).await
}

#[cfg(any(test, debug_assertions))]
pub async fn create_isolated_test_pool_with_config(
    config: &HivemindConfig,
    test_name: &str,
) -> Result<IsolatedTestPool> {
    let schema_name = unique_test_schema_name(test_name);
    let admin_pool = pool_options(config)
        .max_connections(1)
        .min_connections(0)
        .connect(&config.database.url)
        .await
        .context("Failed to connect to PostgreSQL test database")?;

    let create_schema_sql = format!("CREATE SCHEMA {}", schema_name);
    sqlx::query(AssertSqlSafe(create_schema_sql))
        .execute(&admin_pool)
        .await?;

    let connect_options = PgConnectOptions::from_str(&config.database.url)?
        .options([("search_path", format!("{schema_name},public"))]);
    let pool = pool_options(config)
        .max_connections(config.database.max_connections.clamp(1, 5))
        .min_connections(0)
        .connect_with(connect_options)
        .await
        .context("Failed to connect to isolated PostgreSQL test schema")?;

    Ok(IsolatedTestPool {
        pool,
        admin_pool,
        schema_name,
    })
}

#[cfg(any(test, debug_assertions))]
fn unique_test_schema_name(test_name: &str) -> String {
    let label: String = test_name
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch == '_' || ch == '-' {
                Some('_')
            } else {
                None
            }
        })
        .take(18)
        .collect();
    let label = if label.is_empty() { "case" } else { &label };
    format!("hm_test_{}_{}", label, Uuid::new_v4().simple())
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
    )
    .execute(pool)
    .await?;

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
            provider_enabled BOOLEAN NOT NULL DEFAULT true,
            cpu_cores_limit INTEGER NOT NULL DEFAULT 0,
            memory_gb_limit INTEGER NOT NULL DEFAULT 0,
            gpu_memory_gb_limit INTEGER NOT NULL DEFAULT 0,
            storage_gb_limit BIGINT NOT NULL DEFAULT 0,
            min_cpt_per_hour BIGINT NOT NULL DEFAULT 0,
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
    )
    .execute(pool)
    .await?;

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
    )
    .execute(pool)
    .await?;

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
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ledger_entries (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL,
            payer_user VARCHAR(255) NOT NULL,
            provider_worker_id VARCHAR(255),
            provider_user VARCHAR(255),
            kind VARCHAR(64) NOT NULL,
            amount_cpt BIGINT NOT NULL,
            currency VARCHAR(16) NOT NULL DEFAULT 'CPT',
            status VARCHAR(32) NOT NULL,
            idempotency_key VARCHAR(255) NOT NULL UNIQUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS worker_reputation (
            worker_id VARCHAR(255) PRIMARY KEY,
            successful_tasks BIGINT NOT NULL DEFAULT 0,
            failed_tasks BIGINT NOT NULL DEFAULT 0,
            score INTEGER NOT NULL DEFAULT 100,
            banned BOOLEAN NOT NULL DEFAULT false,
            last_attested_at TIMESTAMPTZ,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS task_attestations (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL,
            worker_id VARCHAR(255) NOT NULL,
            verifier_worker_id VARCHAR(255),
            verdict VARCHAR(32) NOT NULL,
            confidence INTEGER NOT NULL DEFAULT 0,
            details TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS artifacts (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL,
            artifact_key VARCHAR(255) NOT NULL UNIQUE,
            checksum_sha1 VARCHAR(64) NOT NULL,
            size_bytes BIGINT NOT NULL DEFAULT 0,
            storage_path TEXT NOT NULL,
            status VARCHAR(32) NOT NULL DEFAULT 'ready',
            resume_supported BOOLEAN NOT NULL DEFAULT true,
            dedup_hit BOOLEAN NOT NULL DEFAULT false,
            expires_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS admin_audit_logs (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            admin_user VARCHAR(255) NOT NULL,
            action VARCHAR(64) NOT NULL,
            target_type VARCHAR(64) NOT NULL,
            target_id VARCHAR(255) NOT NULL DEFAULT '',
            detail JSONB NOT NULL DEFAULT '{}'::jsonb,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cache_alert_anomalies (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            severity VARCHAR(16) NOT NULL,
            cache_hit_rate DOUBLE PRECISION NOT NULL DEFAULT 0,
            low_threshold DOUBLE PRECISION NOT NULL DEFAULT 0,
            high_threshold DOUBLE PRECISION NOT NULL DEFAULT 0,
            message TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_owner ON tasks(owner);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_worker_id ON tasks(worker_id);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_worker_nodes_status ON worker_nodes(status);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_worker_nodes_username ON worker_nodes(username);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vpn_peers_worker_id ON vpn_peers(worker_id);")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ledger_entries_task_id ON ledger_entries(task_id);",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_task_attestations_task_id ON task_attestations(task_id);",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_artifacts_task_id ON artifacts(task_id);")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_admin_audit_logs_created_at ON admin_audit_logs(created_at DESC);",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_cache_alert_anomalies_created_at ON cache_alert_anomalies(created_at DESC);",
    )
    .execute(pool)
    .await?;

    let _ = sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS gpu_name VARCHAR(255);")
        .execute(pool)
        .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS vram_mb BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS storage_total_gb BIGINT NOT NULL DEFAULT 0;").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS storage_available_gb BIGINT NOT NULL DEFAULT 0;").execute(pool).await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS provider_enabled BOOLEAN NOT NULL DEFAULT true;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS cpu_cores_limit INTEGER NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS memory_gb_limit INTEGER NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS gpu_memory_gb_limit INTEGER NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS storage_gb_limit BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS min_cpt_per_hour BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS req_storage_gb BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;

    tracing::info!("Database migrations completed successfully");
    Ok(())
}

pub async fn seed_default_user(pool: &PgPool) -> Result<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
        .bind("testuser")
        .fetch_one(pool)
        .await?;

    if !exists {
        create_user(pool, "testuser", "testpass123", 1000).await?;
        tracing::info!("Seeded default test user: testuser");
    }

    Ok(())
}

pub async fn create_user(
    pool: &PgPool,
    username: &str,
    password: &str,
    balance: i64,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
        .bind(username)
        .fetch_one(pool)
        .await?;
    if exists {
        anyhow::bail!("username already exists");
    }

    let hash = bcrypt::hash(password, 12)?;
    sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, $3)")
        .bind(username)
        .bind(&hash)
        .bind(balance)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_migration_idempotent() {
        let db_url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        let pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn isolated_test_pool_runs_migrations_in_unique_schema() {
        let fixture = match create_isolated_test_pool("database_schema_fixture").await {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };

        run_migrations(&fixture.pool).await.unwrap();

        let users_schema: String = sqlx::query_scalar("SELECT table_schema FROM information_schema.tables WHERE table_name = 'users' AND table_schema = $1")
            .bind(fixture.schema_name())
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
        assert_eq!(users_schema, fixture.schema_name());

        let public_fixture_users_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1
                FROM information_schema.tables
                WHERE table_schema = 'public'
                  AND table_name = 'database_schema_fixture_public_probe'
            )",
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(!public_fixture_users_exists);

        sqlx::query("CREATE TABLE database_schema_fixture_public_probe (id INTEGER)")
            .execute(&fixture.pool)
            .await
            .unwrap();
        let probe_schema: String = sqlx::query_scalar(
            "SELECT table_schema
             FROM information_schema.tables
             WHERE table_schema = $1
               AND table_name = 'database_schema_fixture_public_probe'",
        )
        .bind(fixture.schema_name())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(probe_schema, fixture.schema_name());

        let schema_name = fixture.schema_name().to_string();
        fixture.cleanup().await.unwrap();

        let db_url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .unwrap();
        let schema_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
        )
        .bind(&schema_name)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert!(!schema_exists);
    }

    #[tokio::test]
    async fn test_seed_default_user_inserts_bootstrap_account() {
        let db_url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        let pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };

        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind("testuser")
            .execute(&pool)
            .await
            .unwrap();

        seed_default_user(&pool).await.unwrap();

        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
                .bind("testuser")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(exists);

        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind("testuser")
            .execute(&pool)
            .await
            .ok();
    }
}

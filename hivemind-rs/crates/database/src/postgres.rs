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
    tracing::info!("Connected to PostgreSQL");
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
    let result = create_isolated_test_pool_with_config_inner(config, test_name).await;
    if let Err(error) = &result {
        panic_if_required_test_database(error, "setup");
    }
    result
}

#[cfg(any(test, debug_assertions))]
async fn create_isolated_test_pool_with_config_inner(
    config: &HivemindConfig,
    test_name: &str,
) -> Result<IsolatedTestPool> {
    let schema_name = unique_test_schema_name(test_name);
    let admin_pool = match pool_options(config)
        .max_connections(1)
        .min_connections(0)
        .connect(&config.database.url)
        .await
        .context("Failed to connect to PostgreSQL test database")
    {
        Ok(pool) => pool,
        Err(error) => {
            #[cfg(any(test, debug_assertions))]
            panic_if_required_test_database(&error, "connection");
            return Err(error);
        }
    };

    let create_schema_sql = format!("CREATE SCHEMA {}", schema_name);
    sqlx::query(AssertSqlSafe(create_schema_sql))
        .execute(&admin_pool)
        .await?;

    let connect_options = PgConnectOptions::from_str(&config.database.url)?
        .options([("search_path", format!("{schema_name},public"))]);
    let pool = match pool_options(config)
        .max_connections(config.database.max_connections.clamp(1, 5))
        .min_connections(0)
        .connect_with(connect_options)
        .await
        .context("Failed to connect to isolated PostgreSQL test schema")
    {
        Ok(pool) => pool,
        Err(error) => {
            #[cfg(any(test, debug_assertions))]
            panic_if_required_test_database(&error, "isolated-schema connection");
            return Err(error);
        }
    };

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

const MIGRATION_ADVISORY_LOCK_KEY: i64 = 7_184_239_104;

#[cfg(any(test, debug_assertions))]
fn required_test_database() -> bool {
    matches!(
        std::env::var("HIVEMIND_REQUIRE_TEST_DATABASE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

#[cfg(any(test, debug_assertions))]
fn panic_if_required_test_database(error: impl std::fmt::Display, operation: &str) {
    if required_test_database() {
        panic!("required PostgreSQL test database {operation} failed: {error}");
    }
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    let result = run_migrations_inner(pool).await;
    #[cfg(any(test, debug_assertions))]
    if let Err(error) = &result {
        panic_if_required_test_database(error, "migration");
    }
    result
}

async fn run_migrations_inner(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(MIGRATION_ADVISORY_LOCK_KEY)
        .execute(&mut *tx)
        .await?;
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
    .execute(&mut *tx)
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
            general_compute_capabilities_json TEXT,
            managed_dsl_capabilities_json TEXT,
            admission_mode VARCHAR(32) NOT NULL DEFAULT 'private_static',
            dynamic_capabilities_json TEXT,
            dynamic_capabilities_digest VARCHAR(71),
            dynamic_admission_ready BOOLEAN NOT NULL DEFAULT false,
            dynamic_readiness_reason VARCHAR(255),
            dynamic_observed_at TIMESTAMPTZ,
            last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS enrollment_credentials (
            credential_id UUID PRIMARY KEY,
            owner VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
            role VARCHAR(16) NOT NULL CHECK (role IN ('master', 'worker')),
            client_instance_id VARCHAR(128) NOT NULL,
            token_sha256 VARCHAR(64) NOT NULL UNIQUE,
            issued_at TIMESTAMPTZ NOT NULL,
            expires_at TIMESTAMPTZ NOT NULL,
            redeemed_at TIMESTAMPTZ,
            redeemed_identity_id UUID
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS client_identities (
            identity_id UUID PRIMARY KEY,
            owner VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
            role VARCHAR(16) NOT NULL CHECK (role IN ('master', 'worker')),
            client_instance_id VARCHAR(128) NOT NULL,
            worker_id VARCHAR(255) UNIQUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE (owner, role, client_instance_id)
        );",
    )
    .execute(&mut *tx)
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
            runtime TEXT,
            task_source TEXT,
            general_compute_manifest_json BYTEA,
            managed_gpu_manifest_json BYTEA,
            managed_dsl_backend_id VARCHAR(255),
            managed_dsl_semantics_manifest_sha256 VARCHAR(71),
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
            managed_executed_ops BIGINT NOT NULL DEFAULT 0,
            managed_output_bytes BIGINT NOT NULL DEFAULT 0,
            managed_receipt_json TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0
                CONSTRAINT tasks_retry_count_nonnegative CHECK (retry_count >= 0),
            max_retries INTEGER NOT NULL DEFAULT 3
                CONSTRAINT tasks_max_retries_nonnegative CHECK (max_retries >= 0),
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
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "ALTER TABLE tasks
         ADD COLUMN IF NOT EXISTS worker_id VARCHAR(255),
         ADD COLUMN IF NOT EXISTS worker_ip VARCHAR(45),
         ADD COLUMN IF NOT EXISTS status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
         ADD COLUMN IF NOT EXISTS status_message TEXT,
         ADD COLUMN IF NOT EXISTS output TEXT,
         ADD COLUMN IF NOT EXISTS result_torrent TEXT,
         ADD COLUMN IF NOT EXISTS torrent_source TEXT,
         ADD COLUMN IF NOT EXISTS runtime TEXT,
         ADD COLUMN IF NOT EXISTS task_source TEXT,
         ADD COLUMN IF NOT EXISTS general_compute_manifest_json BYTEA,
         ADD COLUMN IF NOT EXISTS managed_gpu_manifest_json BYTEA,
         ADD COLUMN IF NOT EXISTS managed_dsl_backend_id VARCHAR(255),
         ADD COLUMN IF NOT EXISTS managed_dsl_semantics_manifest_sha256 VARCHAR(71),
         ADD COLUMN IF NOT EXISTS expected_btih VARCHAR(64),
         ADD COLUMN IF NOT EXISTS cpu_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS memory_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS gpu_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS gpu_memory_usage DOUBLE PRECISION NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS req_cpu_score INTEGER NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS req_gpu_score INTEGER NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS req_memory_gb INTEGER NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS req_gpu_memory_gb INTEGER NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS req_storage_gb BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS host_count INTEGER NOT NULL DEFAULT 1,
         ADD COLUMN IF NOT EXISTS max_cpt BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS billing_settled BOOLEAN NOT NULL DEFAULT false,
         ADD COLUMN IF NOT EXISTS billed_amount BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS managed_executed_ops BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS managed_output_bytes BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS managed_receipt_json TEXT,
         ADD COLUMN IF NOT EXISTS retry_count INTEGER NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS max_retries INTEGER NOT NULL DEFAULT 3,
         ADD COLUMN IF NOT EXISTS deadline TIMESTAMPTZ,
         ADD COLUMN IF NOT EXISTS deterministic BOOLEAN NOT NULL DEFAULT false,
         ADD COLUMN IF NOT EXISTS side_effects BOOLEAN NOT NULL DEFAULT false,
         ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS cpu_time_ms BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS wall_time_ms BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS peak_memory_mb BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS download_bytes BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS cache_hits BIGINT NOT NULL DEFAULT 0,
         ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
         ADD COLUMN IF NOT EXISTS last_update TIMESTAMPTZ NOT NULL DEFAULT NOW(),
         ADD COLUMN IF NOT EXISTS completed_at TIMESTAMPTZ;",
    )
    .execute(&mut *tx)
    .await?;

    let invalid_retry_rows: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1
             FROM tasks
             WHERE retry_count IS NULL
                OR retry_count < 0
                OR max_retries IS NULL
                OR max_retries < 0
         )",
    )
    .fetch_one(&mut *tx)
    .await?;
    if invalid_retry_rows {
        anyhow::bail!(
            "tasks contains NULL or negative retry_count/max_retries; remediate the rows before starting"
        );
    }
    sqlx::query(
        "ALTER TABLE tasks
         ALTER COLUMN retry_count SET NOT NULL,
         ALTER COLUMN max_retries SET NOT NULL",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1
                FROM pg_constraint
                WHERE conrelid = 'tasks'::regclass
                  AND conname = 'tasks_retry_count_nonnegative'
            ) THEN
                ALTER TABLE tasks
                    ADD CONSTRAINT tasks_retry_count_nonnegative
                    CHECK (retry_count >= 0);
            END IF;
            IF NOT EXISTS (
                SELECT 1
                FROM pg_constraint
                WHERE conrelid = 'tasks'::regclass
                  AND conname = 'tasks_max_retries_nonnegative'
            ) THEN
                ALTER TABLE tasks
                    ADD CONSTRAINT tasks_max_retries_nonnegative
                    CHECK (max_retries >= 0);
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS general_compute_results (
            task_id VARCHAR(255) PRIMARY KEY,
            worker_id VARCHAR(255) NOT NULL,
            result_json BYTEA NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS general_compute_settlements (
            task_id VARCHAR(255) PRIMARY KEY REFERENCES tasks(task_id) ON DELETE CASCADE,
            worker_id VARCHAR(255) NOT NULL,
            execution_id VARCHAR(255) NOT NULL,
            attempt_id VARCHAR(255) NOT NULL,
            idempotency_key VARCHAR(255) NOT NULL,
            request_digest VARCHAR(71) NOT NULL,
            billing_version VARCHAR(64) NOT NULL,
            cost_model_version VARCHAR(64) NOT NULL,
            usage_claim_json BYTEA NOT NULL,
            evidence_level VARCHAR(32) NOT NULL,
            settlement_basis VARCHAR(64) NOT NULL,
            amount_cpt BIGINT NOT NULL CHECK (amount_cpt >= 0),
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS general_compute_artifact_sources (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            artifact_id VARCHAR(255) NOT NULL,
            sha256 VARCHAR(71) NOT NULL,
            size_bytes BIGINT NOT NULL,
            content BYTEA NOT NULL,
            expires_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE (task_id, artifact_id)
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "ALTER TABLE general_compute_artifact_sources
         ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_artifact_sources_task
         ON general_compute_artifact_sources(task_id);",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS general_compute_artifact_chunks (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            artifact_id VARCHAR(255) NOT NULL,
            offset_bytes BIGINT NOT NULL,
            size_bytes BIGINT NOT NULL,
            sha256 VARCHAR(71) NOT NULL,
            content BYTEA NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE (task_id, artifact_id, offset_bytes)
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_artifact_chunks_task
         ON general_compute_artifact_chunks(task_id, artifact_id, offset_bytes);",
    )
    .execute(&mut *tx)
    .await?;

    // Immutable artifact identity and lifecycle state are separate from the
    // mutable per-attempt request manifest and from uploaded chunk content.
    // This lets retries/workers reuse the same artifact while preventing a
    // later manifest edit from changing the bytes that Nodepool will trust.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS general_compute_artifacts (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            artifact_id VARCHAR(255) NOT NULL,
            sha256 VARCHAR(71) NOT NULL,
            size_bytes BIGINT NOT NULL,
            expected_chunk_count BIGINT NOT NULL,
            availability_status VARCHAR(32) NOT NULL DEFAULT 'pending',
            complete BOOLEAN NOT NULL DEFAULT false,
            expires_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE (task_id, artifact_id)
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS managed_gpu_results (
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            attempt_id VARCHAR(256) NOT NULL,
            attempt_generation BIGINT NOT NULL CHECK (attempt_generation > 0),
            worker_id VARCHAR(255) NOT NULL,
            result_json BYTEA NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (task_id, attempt_generation)
        );",
    )
    .execute(&mut *tx)
    .await?;

    // Bind each managed GPU attempt to the exact operator-owned capability
    // snapshot and device selected at assignment time. Later registration
    // updates must not invalidate terminal handling for an in-flight attempt.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS managed_gpu_attempt_bindings (
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            attempt_generation BIGINT NOT NULL CHECK (attempt_generation > 0),
            worker_id VARCHAR(255) NOT NULL,
            capability_snapshot_json BYTEA NOT NULL,
            selected_gpu_json BYTEA NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (task_id, attempt_generation)
        );",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_managed_gpu_attempt_bindings_worker
         ON managed_gpu_attempt_bindings(worker_id);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE managed_gpu_attempt_bindings
         ALTER COLUMN attempt_generation SET NOT NULL;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1
                FROM pg_constraint
                WHERE conrelid = 'managed_gpu_attempt_bindings'::regclass
                  AND conname = 'managed_gpu_attempt_bindings_attempt_generation_positive'
            ) THEN
                ALTER TABLE managed_gpu_attempt_bindings
                    ADD CONSTRAINT managed_gpu_attempt_bindings_attempt_generation_positive
                    CHECK (attempt_generation > 0);
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;

    // table so stale immutable bindings cannot block task cleanup.
    sqlx::query(
        r#"DO $$
        DECLARE
            constraint_name text;
        BEGIN
            FOR constraint_name IN
                SELECT c.conname
                FROM pg_constraint c
                WHERE c.conrelid = 'managed_gpu_attempt_bindings'::regclass
                  AND c.confrelid = 'tasks'::regclass
                  AND c.contype = 'f'
                  AND c.confdeltype <> 'c'
            LOOP
                EXECUTE format(
                    'ALTER TABLE managed_gpu_attempt_bindings DROP CONSTRAINT %I',
                    constraint_name
                );
            END LOOP;

            IF NOT EXISTS (
                SELECT 1
                FROM pg_constraint c
                WHERE c.conrelid = 'managed_gpu_attempt_bindings'::regclass
                  AND c.confrelid = 'tasks'::regclass
                  AND c.contype = 'f'
                  AND c.confdeltype = 'c'
            ) THEN
                ALTER TABLE managed_gpu_attempt_bindings
                    ADD CONSTRAINT managed_gpu_attempt_bindings_task_id_fkey
                    FOREIGN KEY (task_id) REFERENCES tasks(task_id) ON DELETE CASCADE;
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;

    // Active GPU assignments from before immutable binding support cannot be
    // reconstructed safely. Quarantine them instead of inventing a device or
    // capability snapshot that Nodepool never authorized.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS managed_gpu_attempt_quarantines (
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            attempt_generation BIGINT NOT NULL CHECK (attempt_generation > 0),
            worker_id VARCHAR(255),
            reason VARCHAR(255) NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (task_id, attempt_generation)
        );",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_managed_gpu_attempt_quarantines_worker
         ON managed_gpu_attempt_quarantines(worker_id);",
    )
    .execute(&mut *tx)
    .await?;
    // Existing active assignments are reconciled after all legacy task columns
    // have been upgraded below.

    // Older installations created one GPU result row per task. Upgrade that
    // table to retain one immutable typed result for each attempt so a failed
    // attempt can be retried without losing its audit record.
    sqlx::query(
        "ALTER TABLE managed_gpu_results
         ADD COLUMN IF NOT EXISTS attempt_id VARCHAR(256);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE managed_gpu_results
         ADD COLUMN IF NOT EXISTS attempt_generation BIGINT;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE managed_gpu_results
         SET attempt_id = CASE
                 WHEN NULLIF(BTRIM(attempt_id), '') IS NULL THEN
                     CASE
                         WHEN OCTET_LENGTH('legacy-' || task_id) <= 256
                             THEN 'legacy-' || task_id
                         ELSE 'legacy-' || md5(task_id)
                     END
                 ELSE attempt_id
             END,
             attempt_generation = COALESCE(attempt_generation, 1)
         WHERE attempt_id IS NULL
            OR BTRIM(attempt_id) = ''
            OR attempt_generation IS NULL;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"DO $$
        BEGIN
            IF EXISTS (
                SELECT 1
                FROM managed_gpu_results
                WHERE OCTET_LENGTH(attempt_id) > 256
            ) THEN
                RAISE EXCEPTION 'managed_gpu_results.attempt_id exceeds 256 bytes';
            END IF;
            IF EXISTS (
                SELECT 1
                FROM managed_gpu_results
                WHERE attempt_generation IS NULL OR attempt_generation <= 0
            ) THEN
                RAISE EXCEPTION 'managed_gpu_results.attempt_generation must be positive';
            END IF;
            IF EXISTS (
                SELECT 1
                FROM managed_gpu_results
                GROUP BY task_id, attempt_generation
                HAVING COUNT(*) > 1
            ) THEN
                RAISE EXCEPTION 'managed_gpu_results contains duplicate task/generation rows';
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE managed_gpu_results
         ALTER COLUMN attempt_id TYPE VARCHAR(256)
             USING attempt_id::text;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE managed_gpu_results
         ALTER COLUMN attempt_id SET NOT NULL,
         ALTER COLUMN attempt_generation SET NOT NULL;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"DO $$
        DECLARE
            current_pk_name text;
            current_pk_definition text;
        BEGIN
            SELECT c.conname, pg_get_constraintdef(c.oid)
              INTO current_pk_name, current_pk_definition
            FROM pg_constraint c
            WHERE c.conrelid = 'managed_gpu_results'::regclass
              AND c.contype = 'p';

            IF current_pk_name IS NULL THEN
                ALTER TABLE managed_gpu_results
                    ADD CONSTRAINT managed_gpu_results_pkey
                    PRIMARY KEY (task_id, attempt_generation);
            ELSIF current_pk_definition <> 'PRIMARY KEY (task_id, attempt_generation)' THEN
                EXECUTE format(
                    'ALTER TABLE managed_gpu_results DROP CONSTRAINT %I',
                    current_pk_name
                );
                ALTER TABLE managed_gpu_results
                    ADD CONSTRAINT managed_gpu_results_pkey
                    PRIMARY KEY (task_id, attempt_generation);
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1
                FROM pg_constraint
                WHERE conrelid = 'managed_gpu_results'::regclass
                  AND conname = 'managed_gpu_results_attempt_generation_positive'
            ) THEN
                ALTER TABLE managed_gpu_results
                    ADD CONSTRAINT managed_gpu_results_attempt_generation_positive
                    CHECK (attempt_generation > 0);
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_managed_gpu_results_task_attempt
         ON managed_gpu_results(task_id, attempt_id);",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS managed_gpu_settlements (
            task_id VARCHAR(255) PRIMARY KEY REFERENCES tasks(task_id) ON DELETE CASCADE,
            worker_id VARCHAR(255) NOT NULL,
            execution_id VARCHAR(256) NOT NULL,
            attempt_id VARCHAR(256) NOT NULL,
            idempotency_key VARCHAR(256) NOT NULL,
            request_digest VARCHAR(71) NOT NULL,
            attempt_generation BIGINT NOT NULL CHECK (attempt_generation > 0),
            billing_version VARCHAR(64) NOT NULL,
            cost_model_version VARCHAR(64) NOT NULL,
            usage_claim_json BYTEA NOT NULL,
            evidence_level VARCHAR(32) NOT NULL,
            settlement_basis VARCHAR(64) NOT NULL,
            amount_cpt BIGINT NOT NULL CHECK (amount_cpt >= 0),
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "ALTER TABLE managed_gpu_settlements
         ADD COLUMN IF NOT EXISTS attempt_generation BIGINT NOT NULL DEFAULT 1",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "ALTER TABLE managed_gpu_settlements
         ALTER COLUMN execution_id TYPE VARCHAR(256)
             USING execution_id::text,
         ALTER COLUMN attempt_id TYPE VARCHAR(256)
             USING attempt_id::text,
         ALTER COLUMN idempotency_key TYPE VARCHAR(256)
             USING idempotency_key::text;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE managed_gpu_settlements
         SET attempt_generation = 1
         WHERE attempt_generation IS NULL;",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"DO $$
        BEGIN
            IF EXISTS (
                SELECT 1
                FROM managed_gpu_settlements
                WHERE NULLIF(BTRIM(execution_id), '') IS NULL
                   OR NULLIF(BTRIM(attempt_id), '') IS NULL
                   OR NULLIF(BTRIM(idempotency_key), '') IS NULL
            ) THEN
                RAISE EXCEPTION 'managed_gpu_settlements contains incomplete immutable identity';
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE managed_gpu_settlements
         ALTER COLUMN execution_id SET NOT NULL,
         ALTER COLUMN attempt_id SET NOT NULL,
         ALTER COLUMN idempotency_key SET NOT NULL;",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"DO $$
        BEGIN
            IF EXISTS (
                SELECT 1
                FROM managed_gpu_settlements
                WHERE OCTET_LENGTH(execution_id) > 256
                   OR OCTET_LENGTH(attempt_id) > 256
                   OR OCTET_LENGTH(idempotency_key) > 256
            ) THEN
                RAISE EXCEPTION 'managed_gpu_settlements identity exceeds 256 bytes';
            END IF;
            IF EXISTS (
                SELECT 1
                FROM managed_gpu_settlements
                WHERE attempt_generation IS NULL OR attempt_generation <= 0
            ) THEN
                RAISE EXCEPTION 'managed_gpu_settlements.attempt_generation must be positive';
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE managed_gpu_settlements
         ALTER COLUMN attempt_generation SET DEFAULT 1,
         ALTER COLUMN attempt_generation SET NOT NULL;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1
                FROM pg_constraint
                WHERE conrelid = 'managed_gpu_settlements'::regclass
                  AND conname = 'managed_gpu_settlements_attempt_generation_positive'
            ) THEN
                ALTER TABLE managed_gpu_settlements
                    ADD CONSTRAINT managed_gpu_settlements_attempt_generation_positive
                    CHECK (attempt_generation > 0);
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_artifacts_task
         ON general_compute_artifacts(task_id, artifact_id);",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS general_compute_artifact_manifest_chunks (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            artifact_id VARCHAR(255) NOT NULL,
            offset_bytes BIGINT NOT NULL,
            size_bytes BIGINT NOT NULL,
            sha256 VARCHAR(71) NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE (task_id, artifact_id, offset_bytes),
            FOREIGN KEY (task_id, artifact_id)
                REFERENCES general_compute_artifacts(task_id, artifact_id)
                ON DELETE CASCADE
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_artifact_manifest_chunks
         ON general_compute_artifact_manifest_chunks(task_id, artifact_id, offset_bytes);",
    )
    .execute(&mut *tx)
    .await?;

    // Nodepool-owned cross-worker transfer coordination. A lease is scoped to
    // one immutable execution attempt and one Worker; reassignment revokes the
    // previous active row and allocates the next monotonically increasing
    // generation. Worker-local CAS state is never treated as shared storage.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS general_compute_transfer_leases (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            execution_id VARCHAR(255) NOT NULL,
            attempt_id VARCHAR(255) NOT NULL,
            worker_id VARCHAR(255) NOT NULL,
            generation BIGINT NOT NULL CHECK (generation > 0),
            state VARCHAR(16) NOT NULL DEFAULT 'active',
            expires_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE (task_id, execution_id, attempt_id, worker_id, generation),
            CONSTRAINT general_compute_transfer_leases_state_check
                CHECK (state IN ('active', 'revoked', 'expired'))
        );",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_general_compute_transfer_leases_active_task
         ON general_compute_transfer_leases(task_id)
         WHERE state = 'active';",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_transfer_leases_identity
         ON general_compute_transfer_leases(task_id, execution_id, attempt_id, worker_id, generation);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE general_compute_transfer_leases
         ALTER COLUMN generation SET NOT NULL;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1
                FROM pg_constraint
                WHERE conrelid = 'general_compute_transfer_leases'::regclass
                  AND conname = 'general_compute_transfer_leases_generation_positive'
            ) THEN
                ALTER TABLE general_compute_transfer_leases
                    ADD CONSTRAINT general_compute_transfer_leases_generation_positive
                    CHECK (generation > 0);
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;

    // Nodepool-owned, metadata-only authorization evidence for managed proof
    // attempts. Source, input, receipt bytes, private keys, and bearer tokens
    // are deliberately absent from this table.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS managed_proof_authorizations (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
            protocol_version INTEGER,
            proof_task_id VARCHAR(255),
            owner VARCHAR(255) NOT NULL,
            worker_id VARCHAR(255) NOT NULL,
            execution_id VARCHAR(255) NOT NULL,
            attempt_id VARCHAR(255) NOT NULL,
            idempotency_key VARCHAR(255) NOT NULL,
            request_digest VARCHAR(71) NOT NULL,
            lease_generation BIGINT NOT NULL CHECK (lease_generation > 0),
            runtime VARCHAR(64) NOT NULL,
            backend_id VARCHAR(255) NOT NULL DEFAULT '',
            semantics_manifest_sha256 VARCHAR(71) NOT NULL DEFAULT '',
            proof_scheme VARCHAR(64) NOT NULL,
            image_id_json TEXT NOT NULL,
            deadline_unix_ms BIGINT NOT NULL CHECK (deadline_unix_ms > 0),
            token_jti VARCHAR(255) NOT NULL,
            token_iat BIGINT,
            token_exp BIGINT,
            token_sha256 VARCHAR(71) NOT NULL,
            state VARCHAR(32) NOT NULL DEFAULT 'issued',
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE (task_id, lease_generation, attempt_id)
        );",
    )
    .execute(&mut *tx)
    .await?;

    // Upgrade authorization rows created before proof-task and issuance
    // metadata became part of the immutable binding. Legacy rows with NULL
    // timestamps are deliberately not regenerated by the scheduler.
    sqlx::query(
        "ALTER TABLE managed_proof_authorizations
         ADD COLUMN IF NOT EXISTS protocol_version INTEGER,
         ADD COLUMN IF NOT EXISTS proof_task_id VARCHAR(255),
         ADD COLUMN IF NOT EXISTS token_iat BIGINT,
         ADD COLUMN IF NOT EXISTS token_exp BIGINT",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE managed_proof_authorizations
         ALTER COLUMN lease_generation SET NOT NULL;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1
                FROM pg_constraint
                WHERE conrelid = 'managed_proof_authorizations'::regclass
                  AND conname = 'managed_proof_authorizations_lease_generation_positive'
            ) THEN
                ALTER TABLE managed_proof_authorizations
                    ADD CONSTRAINT managed_proof_authorizations_lease_generation_positive
                    CHECK (lease_generation > 0);
            END IF;
        END $$;"#,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_managed_proof_authorizations_identity
         ON managed_proof_authorizations(task_id, worker_id, execution_id, attempt_id, lease_generation);",
    )
    .execute(&mut *tx)
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
    .execute(&mut *tx)
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
            idempotency_key TEXT NOT NULL UNIQUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE ledger_entries
         ALTER COLUMN idempotency_key TYPE TEXT
             USING idempotency_key::text;",
    )
    .execute(&mut *tx)
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
    .execute(&mut *tx)
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
    .execute(&mut *tx)
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
    .execute(&mut *tx)
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
    .execute(&mut *tx)
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
    .execute(&mut *tx)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_owner ON tasks(owner);")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_owner_created_at ON tasks(owner, created_at DESC);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_pending_priority_created_at
         ON tasks(priority DESC, created_at ASC)
         WHERE status IN ('PENDING', 'QUEUED');",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_assigned_timeout
         ON tasks(last_update, priority DESC, created_at ASC)
         WHERE status = 'ASSIGNED';",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_worker_id ON tasks(worker_id);")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_results_worker_id
         ON general_compute_results(worker_id);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_torrent_source_worker_completed
         ON tasks(torrent_source, worker_id, completed_at DESC)
         WHERE status = 'COMPLETED' AND torrent_source IS NOT NULL AND worker_id IS NOT NULL;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_worker_nodes_status ON worker_nodes(status);")
        .execute(&mut *tx)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_worker_nodes_username ON worker_nodes(username);")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_enrollment_credentials_owner
         ON enrollment_credentials(owner, expires_at);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_client_identities_owner
         ON client_identities(owner, role);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vpn_peers_worker_id ON vpn_peers(worker_id);")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ledger_entries_task_id ON ledger_entries(task_id);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_task_attestations_task_id ON task_attestations(task_id);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_artifacts_task_id ON artifacts(task_id);")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_admin_audit_logs_created_at ON admin_audit_logs(created_at DESC);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_cache_alert_anomalies_created_at ON cache_alert_anomalies(created_at DESC);",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS gpu_name VARCHAR(255);")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS vram_mb BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS storage_total_gb BIGINT NOT NULL DEFAULT 0;")
        .execute(&mut *tx)
        .await?;
    sqlx::query("ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS storage_available_gb BIGINT NOT NULL DEFAULT 0;")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS provider_enabled BOOLEAN NOT NULL DEFAULT true;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS cpu_cores_limit INTEGER NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS memory_gb_limit INTEGER NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS gpu_memory_gb_limit INTEGER NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS storage_gb_limit BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS min_cpt_per_hour BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS general_compute_capabilities_json TEXT;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS managed_dsl_capabilities_json TEXT;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS admission_mode VARCHAR(32) NOT NULL DEFAULT 'private_static';",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_capabilities_json TEXT;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_capabilities_digest VARCHAR(71);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_admission_ready BOOLEAN NOT NULL DEFAULT false;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_readiness_reason VARCHAR(255);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_observed_at TIMESTAMPTZ;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS req_storage_gb BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS runtime TEXT;")
        .execute(&mut *tx)
        .await?;
    sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS task_source TEXT;")
        .execute(&mut *tx)
        .await?;
    sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS general_compute_manifest_json BYTEA;")
        .execute(&mut *tx)
        .await?;
    sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_dsl_backend_id VARCHAR(255);")
        .execute(&mut *tx)
        .await?;
    sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_gpu_manifest_json BYTEA;")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_dsl_semantics_manifest_sha256 VARCHAR(71);",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_executed_ops BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_output_bytes BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_receipt_json TEXT;")
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO managed_gpu_attempt_quarantines (
             task_id, attempt_generation, worker_id, reason
         )
         SELECT t.task_id,
                GREATEST(COALESCE(t.retry_count::BIGINT, 0) + 1, 1),
                t.worker_id,
                CASE
                    WHEN COALESCE(OCTET_LENGTH(t.managed_gpu_manifest_json), 0) = 0
                        THEN 'active managed GPU task has no persisted request manifest'
                    WHEN t.worker_id IS NULL
                        THEN 'active managed GPU task has no assigned Worker identity'
                    ELSE 'active managed GPU task has no matching immutable attempt binding'
                END
         FROM tasks t
         WHERE UPPER(BTRIM(COALESCE(t.runtime, ''))) = 'MANAGED-FUNCTION-GPU-V1'
           AND UPPER(t.status) IN ('ASSIGNED', 'RUNNING')
           AND (
               COALESCE(OCTET_LENGTH(t.managed_gpu_manifest_json), 0) = 0
               OR t.worker_id IS NULL
               OR NOT EXISTS (
                   SELECT 1
                   FROM managed_gpu_attempt_bindings b
                   WHERE b.task_id = t.task_id
                     AND b.attempt_generation = GREATEST(COALESCE(t.retry_count::BIGINT, 0) + 1, 1)
                     AND b.worker_id = t.worker_id
               )
           )
         ON CONFLICT (task_id, attempt_generation) DO NOTHING;",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE tasks t
         SET status = 'FAILED',
             status_message = CASE
                 WHEN COALESCE(OCTET_LENGTH(t.managed_gpu_manifest_json), 0) = 0
                     THEN 'Managed GPU attempt quarantined: request manifest is unavailable'
                 WHEN t.worker_id IS NULL
                     THEN 'Managed GPU attempt quarantined: assigned Worker identity is unavailable'
                 ELSE 'Managed GPU attempt quarantined: immutable attempt binding is unavailable'
             END,
             output = NULL,
             result_torrent = NULL,
             torrent_source = NULL,
             expected_btih = NULL,
             billing_settled = FALSE,
             billed_amount = 0,
             managed_executed_ops = 0,
             managed_output_bytes = 0,
             managed_receipt_json = NULL,
             cpu_usage = 0,
             memory_usage = 0,
             gpu_usage = 0,
             gpu_memory_usage = 0,
             cpu_time_ms = 0,
             wall_time_ms = 0,
             peak_memory_mb = 0,
             download_bytes = 0,
             cache_hits = 0,
             worker_id = NULL,
             worker_ip = NULL,
             last_update = NOW(),
             completed_at = COALESCE(t.completed_at, NOW())
         WHERE UPPER(BTRIM(COALESCE(t.runtime, ''))) = 'MANAGED-FUNCTION-GPU-V1'
           AND UPPER(t.status) IN ('ASSIGNED', 'RUNNING')
           AND (
               COALESCE(OCTET_LENGTH(t.managed_gpu_manifest_json), 0) = 0
               OR t.worker_id IS NULL
               OR NOT EXISTS (
                   SELECT 1
                   FROM managed_gpu_attempt_bindings b
                   WHERE b.task_id = t.task_id
                     AND b.attempt_generation = GREATEST(COALESCE(t.retry_count::BIGINT, 0) + 1, 1)
                     AND b.worker_id = t.worker_id
               )
           );",
    )
    .execute(&mut *tx)
    .await?;

    tracing::info!("Database migrations completed successfully");
    tx.commit().await?;
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
        let db_url = std::env::var("HIVEMIND_TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://hivemind:replace-with-a-test-password@localhost:5432/hivemind_test".into()
        });
        let pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(error) => {
                panic_if_required_test_database(&error, "connection");
                tracing::warn!("Skipping DB test");
                return;
            }
        };
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn managed_gpu_migrations_enforce_identity_width_and_generation_constraints() {
        let fixture = match create_isolated_test_pool("database_gpu_invariants").await {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };

        run_migrations(&fixture.pool).await.unwrap();

        for column in ["execution_id", "attempt_id", "idempotency_key"] {
            let length: Option<i32> = sqlx::query_scalar(
                "SELECT character_maximum_length
                 FROM information_schema.columns
                 WHERE table_schema = current_schema()
                   AND table_name = 'managed_gpu_settlements'
                   AND column_name = $1",
            )
            .bind(column)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
            assert_eq!(length, Some(256), "managed GPU column {column} width");
        }
        let result_attempt_length: Option<i32> = sqlx::query_scalar(
            "SELECT character_maximum_length
             FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name = 'managed_gpu_results'
               AND column_name = 'attempt_id'",
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(result_attempt_length, Some(256));

        let ledger_type: String = sqlx::query_scalar(
            "SELECT data_type
             FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name = 'ledger_entries'
               AND column_name = 'idempotency_key'",
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(ledger_type, "text");

        for (table, constraint) in [
            (
                "managed_gpu_results",
                "managed_gpu_results_attempt_generation_positive",
            ),
            (
                "managed_gpu_settlements",
                "managed_gpu_settlements_attempt_generation_positive",
            ),
            (
                "managed_gpu_attempt_bindings",
                "managed_gpu_attempt_bindings_attempt_generation_positive",
            ),
        ] {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1
                    FROM pg_constraint c
                    JOIN pg_class t ON t.oid = c.conrelid
                    JOIN pg_namespace n ON n.oid = t.relnamespace
                    WHERE n.nspname = current_schema()
                      AND t.relname = $1
                      AND c.conname = $2
                )",
            )
            .bind(table)
            .bind(constraint)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
            assert!(exists, "missing generation constraint {table}.{constraint}");
        }

        sqlx::query("INSERT INTO tasks (task_id, owner) VALUES ($1, $2)")
            .bind("gpu-boundary-task")
            .bind("boundary-owner")
            .execute(&fixture.pool)
            .await
            .unwrap();
        let attempt_id = "a".repeat(256);
        sqlx::query(
            "INSERT INTO managed_gpu_results
                (task_id, attempt_id, attempt_generation, worker_id, result_json)
             VALUES ($1, $2, 1, $3, $4)",
        )
        .bind("gpu-boundary-task")
        .bind(&attempt_id)
        .bind("gpu-worker")
        .bind(Vec::<u8>::new())
        .execute(&fixture.pool)
        .await
        .unwrap();

        let invalid_generation = sqlx::query(
            "INSERT INTO managed_gpu_results
                (task_id, attempt_id, attempt_generation, worker_id, result_json)
             VALUES ($1, $2, 0, $3, $4)",
        )
        .bind("gpu-boundary-task")
        .bind("invalid-generation")
        .bind("gpu-worker")
        .bind(Vec::<u8>::new())
        .execute(&fixture.pool)
        .await;
        assert!(invalid_generation.is_err());

        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn migration_rolls_back_on_incomplete_managed_gpu_identity() {
        let fixture = match create_isolated_test_pool("database_migration_rollback").await {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };

        sqlx::query(
            "CREATE TABLE tasks (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                task_id VARCHAR(255) NOT NULL UNIQUE,
                owner VARCHAR(255) NOT NULL
            );",
        )
        .execute(&fixture.pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE managed_gpu_settlements (
                task_id VARCHAR(255) PRIMARY KEY REFERENCES tasks(task_id),
                worker_id VARCHAR(255) NOT NULL,
                execution_id VARCHAR(255),
                attempt_id VARCHAR(255),
                idempotency_key VARCHAR(255),
                request_digest VARCHAR(71) NOT NULL,
                billing_version VARCHAR(64) NOT NULL,
                cost_model_version VARCHAR(64) NOT NULL,
                usage_claim_json BYTEA NOT NULL,
                evidence_level VARCHAR(32) NOT NULL,
                settlement_basis VARCHAR(64) NOT NULL,
                amount_cpt BIGINT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );",
        )
        .execute(&fixture.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO tasks (task_id, owner) VALUES ('rollback-task', 'owner')")
            .execute(&fixture.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO managed_gpu_settlements
                (task_id, worker_id, execution_id, attempt_id, idempotency_key,
                 request_digest, billing_version, cost_model_version,
                 usage_claim_json, evidence_level, settlement_basis, amount_cpt)
             VALUES ('rollback-task', 'worker', NULL, 'attempt', 'idempotency',
                     'sha256:request', 'billing-v1', 'cost-v1', $1,
                     'unverified', 'fixed-reservation', 10)",
        )
        .bind(Vec::<u8>::new())
        .execute(&fixture.pool)
        .await
        .unwrap();

        assert!(run_migrations(&fixture.pool).await.is_err());

        let quarantine_table_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1
                FROM information_schema.tables
                WHERE table_schema = current_schema()
                  AND table_name = 'managed_gpu_attempt_quarantines'
            )",
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(
            !quarantine_table_exists,
            "failed migration must roll back all DDL"
        );
        let generation_column_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = current_schema()
                  AND table_name = 'managed_gpu_settlements'
                  AND column_name = 'attempt_generation'
            )",
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(
            !generation_column_exists,
            "failed migration must roll back ALTER TABLE"
        );

        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn managed_gpu_legacy_upgrade_is_bounded_and_quarantines_unbound_tasks() {
        let fixture = match create_isolated_test_pool("database_gpu_legacy_upgrade").await {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };
        let legacy_task_id = "t".repeat(255);

        sqlx::query(
            "CREATE TABLE tasks (
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
                runtime TEXT,
                task_source TEXT,
                general_compute_manifest_json BYTEA,
                managed_gpu_manifest_json BYTEA,
                managed_dsl_backend_id VARCHAR(255),
                managed_dsl_semantics_manifest_sha256 VARCHAR(71),
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
                managed_executed_ops BIGINT NOT NULL DEFAULT 0,
                managed_output_bytes BIGINT NOT NULL DEFAULT 0,
                managed_receipt_json TEXT,
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
        .execute(&fixture.pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE managed_gpu_results (
                task_id VARCHAR(255) PRIMARY KEY REFERENCES tasks(task_id) ON DELETE CASCADE,
                worker_id VARCHAR(255) NOT NULL,
                result_json BYTEA NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );",
        )
        .execute(&fixture.pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE managed_gpu_attempt_bindings (
                task_id VARCHAR(255) NOT NULL REFERENCES tasks(task_id),
                attempt_generation BIGINT NOT NULL,
                worker_id VARCHAR(255) NOT NULL,
                capability_snapshot_json BYTEA NOT NULL,
                selected_gpu_json BYTEA NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (task_id, attempt_generation)
            );",
        )
        .execute(&fixture.pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE ledger_entries (
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
        .execute(&fixture.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO tasks
                (task_id, owner, worker_id, worker_ip, status, runtime,
                 output, result_torrent, torrent_source, expected_btih,
                 billing_settled, billed_amount, managed_executed_ops,
                 managed_output_bytes, managed_receipt_json, gpu_usage,
                 gpu_memory_usage, wall_time_ms)
             VALUES ($1, 'legacy-owner', 'legacy-worker', '10.0.0.2', 'RUNNING',
                     ' managed-function-gpu-v1 ', 'stale output', 'stale torrent',
                     'legacy source', 'stale-btih', true, 77, 9, 123,
                     'stale receipt', 4.5, 128, 900)",
        )
        .bind(&legacy_task_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO managed_gpu_results (task_id, worker_id, result_json)
             VALUES ($1, 'legacy-worker', $2)",
        )
        .bind(&legacy_task_id)
        .bind(Vec::<u8>::new())
        .execute(&fixture.pool)
        .await
        .unwrap();

        let zero_manifest_task_id = "gpu-zero-manifest";
        sqlx::query(
            "INSERT INTO tasks
                (task_id, owner, worker_id, status, runtime, managed_gpu_manifest_json)
             VALUES ($1, 'gpu-owner', 'zero-worker', 'ASSIGNED',
                     'MANAGED-FUNCTION-GPU-V1', $2)",
        )
        .bind(zero_manifest_task_id)
        .bind(Vec::<u8>::new())
        .execute(&fixture.pool)
        .await
        .unwrap();

        let mismatched_binding_task_id = "gpu-mismatched-binding";
        sqlx::query(
            "INSERT INTO tasks
                (task_id, owner, worker_id, status, runtime, managed_gpu_manifest_json)
             VALUES ($1, 'gpu-owner', 'current-worker', 'RUNNING',
                     '  MANAGED-FUNCTION-GPU-V1  ', $2)",
        )
        .bind(mismatched_binding_task_id)
        .bind(vec![1_u8, 2, 3])
        .execute(&fixture.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO managed_gpu_attempt_bindings
                (task_id, attempt_generation, worker_id,
                 capability_snapshot_json, selected_gpu_json)
             VALUES ($1, 1, 'stale-worker', $2, $3)",
        )
        .bind(mismatched_binding_task_id)
        .bind(vec![4_u8])
        .bind(vec![5_u8])
        .execute(&fixture.pool)
        .await
        .unwrap();

        run_migrations(&fixture.pool).await.unwrap();

        let (attempt_id, generation): (String, i64) = sqlx::query_as(
            "SELECT attempt_id, attempt_generation
             FROM managed_gpu_results
             WHERE task_id = $1",
        )
        .bind(&legacy_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(attempt_id.starts_with("legacy-"));
        assert_eq!(attempt_id.len(), 39, "long legacy IDs use the hash form");
        assert_eq!(generation, 1);

        type LegacyTaskState = (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            bool,
            i64,
            i64,
            i64,
            Option<String>,
            f64,
            f64,
            i64,
        );
        let (
            status,
            worker_id,
            output,
            result_torrent,
            torrent_source,
            expected_btih,
            billing_settled,
            billed_amount,
            managed_executed_ops,
            managed_output_bytes,
            managed_receipt_json,
            gpu_usage,
            gpu_memory_usage,
            wall_time_ms,
        ): LegacyTaskState = sqlx::query_as(
            "SELECT status, worker_id, output, result_torrent, torrent_source,
                    expected_btih, billing_settled, billed_amount,
                    managed_executed_ops, managed_output_bytes,
                    managed_receipt_json, gpu_usage, gpu_memory_usage, wall_time_ms
             FROM tasks
             WHERE task_id = $1",
        )
        .bind(&legacy_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(status, "FAILED");
        assert!(worker_id.is_none());
        assert!(output.is_none());
        assert!(result_torrent.is_none());
        assert!(torrent_source.is_none());
        assert!(expected_btih.is_none());
        assert!(!billing_settled);
        assert_eq!(billed_amount, 0);
        assert_eq!(managed_executed_ops, 0);
        assert_eq!(managed_output_bytes, 0);
        assert!(managed_receipt_json.is_none());
        assert_eq!(gpu_usage, 0.0);
        assert_eq!(gpu_memory_usage, 0.0);
        assert_eq!(wall_time_ms, 0);

        let quarantine_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM managed_gpu_attempt_quarantines
             WHERE task_id = $1 AND attempt_generation = 1",
        )
        .bind(&legacy_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(quarantine_count, 1);

        let (zero_status, zero_worker_id, zero_status_message): (
            String,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT status, worker_id, status_message
             FROM tasks
             WHERE task_id = $1",
        )
        .bind(zero_manifest_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(zero_status, "FAILED");
        assert!(zero_worker_id.is_none());
        assert_eq!(
            zero_status_message.as_deref(),
            Some("Managed GPU attempt quarantined: request manifest is unavailable")
        );
        let zero_reason: String = sqlx::query_scalar(
            "SELECT reason
             FROM managed_gpu_attempt_quarantines
             WHERE task_id = $1 AND attempt_generation = 1",
        )
        .bind(zero_manifest_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(
            zero_reason,
            "active managed GPU task has no persisted request manifest"
        );

        let (mismatched_status, mismatched_worker_id): (String, Option<String>) = sqlx::query_as(
            "SELECT status, worker_id
                 FROM tasks
                 WHERE task_id = $1",
        )
        .bind(mismatched_binding_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(mismatched_status, "FAILED");
        assert!(mismatched_worker_id.is_none());
        let mismatch_reason: String = sqlx::query_scalar(
            "SELECT reason
             FROM managed_gpu_attempt_quarantines
             WHERE task_id = $1 AND attempt_generation = 1",
        )
        .bind(mismatched_binding_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(mismatch_reason.contains("matching immutable attempt binding"));

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(mismatched_binding_task_id)
            .execute(&fixture.pool)
            .await
            .unwrap();
        let binding_count_after_task_delete: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM managed_gpu_attempt_bindings
             WHERE task_id = $1",
        )
        .bind(mismatched_binding_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(binding_count_after_task_delete, 0);

        run_migrations(&fixture.pool).await.unwrap();
        let quarantine_count_after_retry: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM managed_gpu_attempt_quarantines
             WHERE task_id = $1 AND attempt_generation = 1",
        )
        .bind(&legacy_task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(quarantine_count_after_retry, 1);

        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_migrations_are_serialized_and_idempotent() {
        let fixture = match create_isolated_test_pool("database_concurrent_migrations").await {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };

        let first_pool = fixture.pool.clone();
        let second_pool = fixture.pool.clone();
        let (first, second) =
            tokio::join!(run_migrations(&first_pool), run_migrations(&second_pool),);
        first.unwrap();
        second.unwrap();

        let users_table_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1
                FROM information_schema.tables
                WHERE table_schema = current_schema()
                  AND table_name = 'users'
            )",
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(users_table_exists);

        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn task_migrations_add_columns_needed_by_task_model() {
        let fixture = match create_isolated_test_pool("database_task_columns").await {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };

        sqlx::query(
            "CREATE TABLE tasks (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                task_id VARCHAR(255) NOT NULL UNIQUE,
                owner VARCHAR(255) NOT NULL
            );",
        )
        .execute(&fixture.pool)
        .await
        .unwrap();

        run_migrations(&fixture.pool).await.unwrap();

        for column in [
            "status",
            "worker_id",
            "output",
            "result_torrent",
            "runtime",
            "managed_gpu_manifest_json",
            "cpu_usage",
            "gpu_usage",
            "req_gpu_score",
            "billing_settled",
            "managed_output_bytes",
            "retry_count",
            "priority",
            "wall_time_ms",
            "created_at",
            "last_update",
        ] {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = current_schema()
                      AND table_name = 'tasks'
                      AND column_name = $1
                )",
            )
            .bind(column)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
            assert!(exists, "missing migrated task column {column}");
        }

        sqlx::query("INSERT INTO tasks (task_id, owner) VALUES ('minimal-task', 'owner')")
            .execute(&fixture.pool)
            .await
            .unwrap();
        let (status, cpu_usage): (String, f64) =
            sqlx::query_as("SELECT status, cpu_usage FROM tasks WHERE task_id = 'minimal-task'")
                .fetch_one(&fixture.pool)
                .await
                .unwrap();
        assert_eq!(status, "PENDING");
        assert_eq!(cpu_usage, 0.0);

        fixture.cleanup().await.unwrap();
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

        for table_name in ["enrollment_credentials", "client_identities"] {
            let table_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM information_schema.tables
                    WHERE table_schema = $1 AND table_name = $2
                )",
            )
            .bind(fixture.schema_name())
            .bind(table_name)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
            assert!(table_exists, "missing enrollment table {table_name}");
        }
        let enrollment_columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name
             FROM information_schema.columns
             WHERE table_schema = $1 AND table_name = 'enrollment_credentials'",
        )
        .bind(fixture.schema_name())
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
        for column in [
            "credential_id",
            "owner",
            "role",
            "client_instance_id",
            "token_sha256",
            "expires_at",
            "redeemed_at",
            "redeemed_identity_id",
        ] {
            assert!(enrollment_columns.iter().any(|value| value == column));
        }
        let enrollment_indexes: Vec<String> = sqlx::query_scalar(
            "SELECT indexname FROM pg_indexes
             WHERE schemaname = $1 AND tablename = 'enrollment_credentials'",
        )
        .bind(fixture.schema_name())
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
        assert!(enrollment_indexes
            .iter()
            .any(|value| value == "idx_enrollment_credentials_owner"));
        let identity_indexes: Vec<String> = sqlx::query_scalar(
            "SELECT indexname FROM pg_indexes
             WHERE schemaname = $1 AND tablename = 'client_identities'",
        )
        .bind(fixture.schema_name())
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
        assert!(identity_indexes
            .iter()
            .any(|value| value == "idx_client_identities_owner"));

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

        let db_url = std::env::var("HIVEMIND_TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://hivemind:replace-with-a-test-password@localhost:5432/hivemind_test".into()
        });
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
    async fn task_migrations_create_hot_path_indexes() {
        let fixture = match create_isolated_test_pool("database_task_indexes").await {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };

        run_migrations(&fixture.pool).await.unwrap();

        let indexes: Vec<String> = sqlx::query_scalar(
            "SELECT indexname
             FROM pg_indexes
             WHERE schemaname = $1
               AND tablename = 'tasks'",
        )
        .bind(fixture.schema_name())
        .fetch_all(&fixture.pool)
        .await
        .unwrap();

        assert!(indexes.contains(&"idx_tasks_owner_created_at".to_string()));
        assert!(indexes.contains(&"idx_tasks_pending_priority_created_at".to_string()));
        assert!(indexes.contains(&"idx_tasks_assigned_timeout".to_string()));
        assert!(indexes.contains(&"idx_tasks_torrent_source_worker_completed".to_string()));

        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn task_migrations_create_general_compute_artifact_source_table() {
        let fixture =
            match create_isolated_test_pool("database_general_compute_artifact_sources").await {
                Ok(fixture) => fixture,
                Err(_) => {
                    tracing::warn!("Skipping DB test");
                    return;
                }
            };
        run_migrations(&fixture.pool).await.unwrap();
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = $1 AND table_name = 'general_compute_artifact_sources'
            )",
        )
        .bind(fixture.schema_name())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(exists);
        let lifecycle_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = $1 AND table_name = 'general_compute_artifacts'
            )",
        )
        .bind(fixture.schema_name())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(lifecycle_exists);
        let manifest_chunks_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = $1 AND table_name = 'general_compute_artifact_manifest_chunks'
            )",
        )
        .bind(fixture.schema_name())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(manifest_chunks_exists);
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn task_migrations_create_general_compute_transfer_lease_table() {
        let fixture =
            match create_isolated_test_pool("database_general_compute_transfer_leases").await {
                Ok(fixture) => fixture,
                Err(_) => {
                    tracing::warn!("Skipping DB test");
                    return;
                }
            };
        run_migrations(&fixture.pool).await.unwrap();

        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = $1 AND table_name = 'general_compute_transfer_leases'
            )",
        )
        .bind(fixture.schema_name())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(exists, "Nodepool must persist transfer lease state");

        let indexes: Vec<String> = sqlx::query_scalar(
            "SELECT indexname FROM pg_indexes
             WHERE schemaname = $1 AND tablename = 'general_compute_transfer_leases'",
        )
        .bind(fixture.schema_name())
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
        assert!(indexes.contains(&"idx_general_compute_transfer_leases_active_task".into()));
        assert!(indexes.contains(&"idx_general_compute_transfer_leases_identity".into()));

        let state_check: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1
                FROM pg_constraint c
                JOIN pg_class t ON t.oid = c.conrelid
                JOIN pg_namespace n ON n.oid = t.relnamespace
                WHERE n.nspname = $1
                  AND t.relname = 'general_compute_transfer_leases'
                  AND c.conname = 'general_compute_transfer_leases_state_check'
            )",
        )
        .bind(fixture.schema_name())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(state_check, "lease state values must be constrained");

        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn task_migrations_create_general_compute_settlement_table() {
        let fixture = match create_isolated_test_pool("database_general_compute_settlements").await
        {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };
        run_migrations(&fixture.pool).await.unwrap();

        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = $1 AND table_name = 'general_compute_settlements'
            )",
        )
        .bind(fixture.schema_name())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert!(
            exists,
            "Nodepool must persist general-compute settlement provenance"
        );

        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn test_seed_default_user_inserts_bootstrap_account() {
        // Owns its schema and migrations rather than borrowing whatever another
        // test left in `public`: against a fresh database the shared-pool
        // version failed with `relation "users" does not exist`, and against a
        // dirty one it only passed when it happened to run second.
        let fixture = match create_isolated_test_pool("database_seed_default_user").await {
            Ok(fixture) => fixture,
            Err(_) => {
                tracing::warn!("Skipping DB test");
                return;
            }
        };
        run_migrations(&fixture.pool).await.unwrap();

        seed_default_user(&fixture.pool).await.unwrap();

        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
                .bind("testuser")
                .fetch_one(&fixture.pool)
                .await
                .unwrap();
        assert!(exists);

        fixture.cleanup().await.unwrap();
    }
}

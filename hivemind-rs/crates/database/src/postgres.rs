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
            runtime TEXT,
            task_source TEXT,
            general_compute_manifest_json BYTEA,
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
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS general_compute_results (
            task_id VARCHAR(255) PRIMARY KEY,
            worker_id VARCHAR(255) NOT NULL,
            result_json BYTEA NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )
    .execute(pool)
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
    .execute(pool)
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
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE general_compute_artifact_sources
         ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_artifact_sources_task
         ON general_compute_artifact_sources(task_id);",
    )
    .execute(pool)
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
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_artifact_chunks_task
         ON general_compute_artifact_chunks(task_id, artifact_id, offset_bytes);",
    )
    .execute(pool)
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
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_artifacts_task
         ON general_compute_artifacts(task_id, artifact_id);",
    )
    .execute(pool)
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
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_artifact_manifest_chunks
         ON general_compute_artifact_manifest_chunks(task_id, artifact_id, offset_bytes);",
    )
    .execute(pool)
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
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_general_compute_transfer_leases_active_task
         ON general_compute_transfer_leases(task_id)
         WHERE state = 'active';",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_transfer_leases_identity
         ON general_compute_transfer_leases(task_id, execution_id, attempt_id, worker_id, generation);",
    )
    .execute(pool)
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
    .execute(pool)
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
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_managed_proof_authorizations_identity
         ON managed_proof_authorizations(task_id, worker_id, execution_id, attempt_id, lease_generation);",
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
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_owner_created_at ON tasks(owner, created_at DESC);",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_pending_priority_created_at
         ON tasks(priority DESC, created_at ASC)
         WHERE status IN ('PENDING', 'QUEUED');",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_assigned_timeout
         ON tasks(last_update, priority DESC, created_at ASC)
         WHERE status = 'ASSIGNED';",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_worker_id ON tasks(worker_id);")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_general_compute_results_worker_id
         ON general_compute_results(worker_id);",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_torrent_source_worker_completed
         ON tasks(torrent_source, worker_id, completed_at DESC)
         WHERE status = 'COMPLETED' AND torrent_source IS NOT NULL AND worker_id IS NOT NULL;",
    )
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
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS general_compute_capabilities_json TEXT;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS managed_dsl_capabilities_json TEXT;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS admission_mode VARCHAR(32) NOT NULL DEFAULT 'private_static';",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_capabilities_json TEXT;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_capabilities_digest VARCHAR(71);",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_admission_ready BOOLEAN NOT NULL DEFAULT false;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_readiness_reason VARCHAR(255);",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE worker_nodes ADD COLUMN IF NOT EXISTS dynamic_observed_at TIMESTAMPTZ;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS req_storage_gb BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS runtime TEXT;")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS task_source TEXT;")
        .execute(pool)
        .await;
    let _ = sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS general_compute_manifest_json BYTEA;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_dsl_backend_id VARCHAR(255);",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_dsl_semantics_manifest_sha256 VARCHAR(71);",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_executed_ops BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_output_bytes BIGINT NOT NULL DEFAULT 0;",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS managed_receipt_json TEXT;")
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
        let db_url = std::env::var("HIVEMIND_TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://hivemind:replace-with-a-test-password@localhost:5432/hivemind_test".into()
        });
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

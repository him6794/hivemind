use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HivemindConfig {
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub torrent: TorrentConfig,
    pub vpn: VpnConfig,
    pub executor: ExecutorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub idle_timeout_secs: u64,
    pub connect_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub pool_size: usize,
    pub connect_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub nodepool_grpc_addr: String,
    #[serde(default)]
    pub nodepool_grpc_endpoint: Option<String>,
    pub master_http_addr: String,
    pub worker_grpc_addr: String,
    pub worker_grpc_port: u16,
    #[serde(default = "default_worker_control_http_addr")]
    pub worker_control_http_addr: String,
    #[serde(default = "default_master_ui_dir")]
    pub master_ui_dir: String,
    #[serde(default = "default_worker_ui_dir")]
    pub worker_ui_dir: String,
    #[serde(default)]
    pub worker_advertise_addr: Option<String>,
    #[serde(default)]
    pub worker_nodepool_token: Option<String>,
    #[serde(default)]
    pub worker_nodepool_username: Option<String>,
    #[serde(default)]
    pub worker_nodepool_password: Option<String>,
    #[serde(default = "default_master_cors_allowed_origins")]
    pub master_cors_allowed_origins: Vec<String>,
    #[serde(default = "default_worker_control_cors_allowed_origins")]
    pub worker_control_cors_allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret: String,
    #[serde(default = "default_worker_execution_secret")]
    pub worker_execution_secret: String,
    pub token_expiry_hours: i64,
    pub refresh_expiry_hours: i64,
    pub bcrypt_cost: u32,
}

impl AuthConfig {
    pub fn validate_jwt_secret(&self) -> anyhow::Result<()> {
        validate_secret(
            &self.jwt_secret,
            "JWT_SECRET",
            &["CHANGE_ME_IN_PRODUCTION", "change-me-in-production"],
        )
    }

    pub fn validate_worker_execution_secret(&self) -> anyhow::Result<()> {
        validate_secret(
            &self.worker_execution_secret,
            "WORKER_EXECUTION_SECRET",
            &[
                "CHANGE_ME_WORKER_EXECUTION_SECRET",
                "change-me-worker-execution-secret",
                "CHANGE_ME_IN_PRODUCTION",
                "change-me-in-production",
            ],
        )
    }
}

fn validate_secret(secret: &str, name: &str, placeholders: &[&str]) -> anyhow::Result<()> {
    let secret = secret.trim();
    if secret.is_empty()
        || placeholders
            .iter()
            .any(|placeholder| secret.eq_ignore_ascii_case(placeholder))
    {
        anyhow::bail!("{name} must be set to a non-default value");
    }
    if secret.len() < 32 {
        anyhow::bail!("{name} must contain at least 32 bytes");
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentConfig {
    pub api_dir: String,
    pub bt_dir: String,
    #[serde(default = "default_torrent_announce_url")]
    pub announce_url: String,
    #[serde(default = "default_torrent_tracker_listen_addr")]
    pub tracker_listen_addr: String,
    #[serde(default = "default_torrent_seed_listen_addr")]
    pub seed_listen_addr: String,
    #[serde(default)]
    pub seed_advertise_host: Option<String>,
    #[serde(default)]
    pub allow_local_task_artifacts: bool,
    #[serde(default)]
    pub task_artifact_base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnConfig {
    pub headscale_url: String,
    pub headscale_api_key: String,
    pub base_virtual_ip: String,
    pub vpn_network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorConfig {
    pub monty_executable: String,
    pub sandbox_dir: String,
    pub max_cpu_percent: f64,
    pub max_memory_mb: u64,
    pub task_timeout_secs: u64,
    pub max_concurrent_tasks: usize,
    #[serde(default = "default_sandbox_mode")]
    pub sandbox_mode: String,
    #[serde(default = "default_network_egress_enabled")]
    pub network_egress_enabled: bool,
    #[serde(default = "default_network_egress_mode")]
    pub network_egress_mode: String,
    #[serde(default)]
    pub network_egress_targets: Vec<String>,
}

impl Default for HivemindConfig {
    fn default() -> Self {
        Self {
            database: DatabaseConfig {
                url: "postgres://hivemind:hivemind@localhost:5432/hivemind".into(),
                max_connections: 20,
                min_connections: 2,
                idle_timeout_secs: 300,
                connect_timeout_secs: 10,
            },
            redis: RedisConfig {
                url: "redis://localhost:6379".into(),
                pool_size: 16,
                connect_timeout_secs: 5,
            },
            server: ServerConfig {
                nodepool_grpc_addr: "0.0.0.0:50051".into(),
                nodepool_grpc_endpoint: None,
                master_http_addr: "0.0.0.0:8082".into(),
                worker_grpc_addr: "0.0.0.0:50053".into(),
                worker_grpc_port: 50053,
                worker_control_http_addr: default_worker_control_http_addr(),
                master_ui_dir: default_master_ui_dir(),
                worker_ui_dir: default_worker_ui_dir(),
                worker_advertise_addr: None,
                worker_nodepool_token: None,
                worker_nodepool_username: None,
                worker_nodepool_password: None,
                master_cors_allowed_origins: default_master_cors_allowed_origins(),
                worker_control_cors_allowed_origins: default_worker_control_cors_allowed_origins(),
            },
            auth: AuthConfig {
                jwt_secret: "CHANGE_ME_IN_PRODUCTION".into(),
                worker_execution_secret: default_worker_execution_secret(),
                token_expiry_hours: 24,
                refresh_expiry_hours: 168,
                bcrypt_cost: 12,
            },
            torrent: TorrentConfig {
                api_dir: "./api/torrents".into(),
                bt_dir: "./bt_torrents".into(),
                announce_url: "http://localhost:6969/announce".into(),
                tracker_listen_addr: default_torrent_tracker_listen_addr(),
                seed_listen_addr: default_torrent_seed_listen_addr(),
                seed_advertise_host: None,
                allow_local_task_artifacts: false,
                task_artifact_base_url: None,
            },
            vpn: VpnConfig {
                headscale_url: "http://localhost:8080".into(),
                headscale_api_key: "".into(),
                base_virtual_ip: "100.64.0.0".into(),
                vpn_network: "100.64.0.0/10".into(),
            },
            executor: ExecutorConfig {
                monty_executable: "monty.exe".into(),
                sandbox_dir: "./sandbox".into(),
                max_cpu_percent: 80.0,
                max_memory_mb: 4096,
                task_timeout_secs: 3600,
                max_concurrent_tasks: 4,
                sandbox_mode: default_sandbox_mode(),
                network_egress_enabled: default_network_egress_enabled(),
                network_egress_mode: default_network_egress_mode(),
                network_egress_targets: vec![],
            },
        }
    }
}

impl HivemindConfig {
    pub fn for_test() -> Self {
        let mut config = Self::default();
        config.torrent.allow_local_task_artifacts = true;
        config.database.url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());
        config
    }

    /// Loads JSON/default configuration first, then applies process environment overrides.
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let mut config = match std::env::var("HIVEMIND_CONFIG") {
            Ok(path) => {
                let contents = std::fs::read_to_string(&path)?;
                serde_json::from_str(&contents)?
            }
            Err(_) => {
                let mut config = Self::default();
                config.executor.sandbox_mode = "production".into();
                config
            }
        };
        config.apply_env_overrides()?;
        Ok(config)
    }

    #[cfg(test)]
    fn load_from_env() -> Self {
        let mut config = Self::default();
        config.executor.sandbox_mode = "production".into();
        config
            .apply_env_overrides()
            .expect("test environment overrides must be valid");
        config
    }

    fn apply_env_overrides(&mut self) -> anyhow::Result<()> {
        if let Ok(url) = std::env::var("DATABASE_URL") {
            self.database.url = url;
        }
        if let Ok(url) = std::env::var("REDIS_URL") {
            self.redis.url = url;
        }
        if let Ok(addr) = std::env::var("NODEPOOL_GRPC_ADDR") {
            self.server.nodepool_grpc_addr = addr;
        }
        if let Ok(endpoint) = std::env::var("NODEPOOL_GRPC_ENDPOINT") {
            self.server.nodepool_grpc_endpoint = Some(endpoint);
        }
        if let Ok(addr) = std::env::var("MASTER_HTTP_ADDR") {
            self.server.master_http_addr = addr;
        }
        if let Ok(dir) = std::env::var("MASTER_UI_DIR") {
            self.server.master_ui_dir = dir;
        }
        if let Ok(addr) = std::env::var("WORKER_GRPC_ADDR") {
            self.server.worker_grpc_addr = addr;
        }
        if let Ok(dir) = std::env::var("WORKER_UI_DIR") {
            self.server.worker_ui_dir = dir;
        }
        if let Ok(addr) = std::env::var("WORKER_CONTROL_HTTP_ADDR") {
            self.server.worker_control_http_addr = addr;
        }
        if let Ok(addr) = std::env::var("WORKER_ADVERTISE_ADDR") {
            self.server.worker_advertise_addr = Some(addr);
        }
        if let Ok(token) = std::env::var("WORKER_NODEPOOL_TOKEN") {
            self.server.worker_nodepool_token = Some(token);
        }
        if let Ok(username) = std::env::var("WORKER_NODEPOOL_USERNAME") {
            self.server.worker_nodepool_username = Some(username);
        }
        if let Ok(password) = std::env::var("WORKER_NODEPOOL_PASSWORD") {
            self.server.worker_nodepool_password = Some(password);
        }
        if self.server.worker_nodepool_username.is_none() {
            if let Ok(username) = std::env::var("WORKER_USERNAME") {
                self.server.worker_nodepool_username = Some(username);
            }
        }
        if self.server.worker_nodepool_password.is_none() {
            if let Ok(password) = std::env::var("WORKER_PASSWORD") {
                self.server.worker_nodepool_password = Some(password);
            }
        }
        if let Ok(origins) = std::env::var("MASTER_CORS_ALLOWED_ORIGINS") {
            self.server.master_cors_allowed_origins = parse_csv(&origins);
        }
        if let Ok(origins) = std::env::var("WORKER_CONTROL_CORS_ALLOWED_ORIGINS") {
            self.server.worker_control_cors_allowed_origins = parse_csv(&origins);
        }
        if let Ok(secret) = std::env::var("JWT_SECRET") {
            self.auth.jwt_secret = secret;
        }
        if let Ok(secret) = std::env::var("WORKER_EXECUTION_SECRET") {
            self.auth.worker_execution_secret = secret;
        }
        if let Ok(exec) = std::env::var("MONTY_EXECUTABLE") {
            self.executor.monty_executable = exec;
        }
        if let Ok(dir) = std::env::var("EXECUTOR_SANDBOX_DIR") {
            self.executor.sandbox_dir = dir;
        }
        if let Ok(value) = std::env::var("EXECUTOR_MAX_CPU_PERCENT") {
            self.executor.max_cpu_percent = parse_env("EXECUTOR_MAX_CPU_PERCENT", &value)?;
        }
        if let Ok(value) = std::env::var("EXECUTOR_MAX_MEMORY_MB") {
            self.executor.max_memory_mb = parse_env("EXECUTOR_MAX_MEMORY_MB", &value)?;
        }
        if let Ok(value) = std::env::var("EXECUTOR_TASK_TIMEOUT_SECS") {
            self.executor.task_timeout_secs = parse_env("EXECUTOR_TASK_TIMEOUT_SECS", &value)?;
        }
        if let Ok(value) = std::env::var("EXECUTOR_MAX_CONCURRENT_TASKS") {
            self.executor.max_concurrent_tasks =
                parse_env("EXECUTOR_MAX_CONCURRENT_TASKS", &value)?;
        }
        if let Ok(mode) = std::env::var("EXECUTOR_SANDBOX_MODE") {
            self.executor.sandbox_mode = mode;
        }
        if let Ok(enabled) = std::env::var("EXECUTOR_NETWORK_EGRESS_ENABLED") {
            self.executor.network_egress_enabled =
                parse_env("EXECUTOR_NETWORK_EGRESS_ENABLED", &enabled)?;
        }
        if let Ok(mode) = std::env::var("EXECUTOR_NETWORK_EGRESS_MODE") {
            self.executor.network_egress_mode = mode;
        }
        if let Ok(targets) = std::env::var("EXECUTOR_NETWORK_EGRESS_TARGETS") {
            self.executor.network_egress_targets = targets
                .split(',')
                .map(|target| target.trim())
                .filter(|target| !target.is_empty())
                .map(ToString::to_string)
                .collect();
        }
        if let Ok(dir) = std::env::var("TORRENT_API_DIR") {
            self.torrent.api_dir = dir;
        }
        if let Ok(dir) = std::env::var("TORRENT_BT_DIR") {
            self.torrent.bt_dir = dir;
        }
        if let Ok(url) = std::env::var("TORRENT_ANNOUNCE_URL") {
            self.torrent.announce_url = url;
        }
        if let Ok(addr) = std::env::var("TORRENT_TRACKER_LISTEN_ADDR") {
            self.torrent.tracker_listen_addr = addr;
        }
        if let Ok(addr) = std::env::var("TORRENT_SEED_LISTEN_ADDR") {
            self.torrent.seed_listen_addr = addr;
        }
        if let Ok(host) = std::env::var("TORRENT_SEED_ADVERTISE_HOST") {
            let host = host.trim().to_string();
            if !host.is_empty() {
                self.torrent.seed_advertise_host = Some(host);
            }
        }
        if let Ok(enabled) = std::env::var("TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS") {
            self.torrent.allow_local_task_artifacts =
                parse_env("TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS", &enabled)?;
        }
        if let Ok(url) = std::env::var("TORRENT_TASK_ARTIFACT_BASE_URL") {
            let url = url.trim().trim_end_matches('/').to_string();
            if !url.is_empty() {
                self.torrent.task_artifact_base_url = Some(url);
            }
        }
        if let Ok(url) = std::env::var("HEADSCALE_URL") {
            self.vpn.headscale_url = url;
        }
        if let Ok(key) = std::env::var("HEADSCALE_API_KEY") {
            self.vpn.headscale_api_key = key;
        }
        if let Ok(ip) = std::env::var("VPN_BASE_VIRTUAL_IP") {
            self.vpn.base_virtual_ip = ip;
        }
        if let Ok(network) = std::env::var("VPN_NETWORK") {
            self.vpn.vpn_network = network;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test_config_tests {
    use super::*;

    #[test]
    fn test_config_uses_dedicated_test_database_url() {
        let config = HivemindConfig::for_test();
        assert!(
            config.database.url.ends_with("/hivemind_test"),
            "test database URL should not default to the production/dev database: {}",
            config.database.url
        );
    }
}

fn default_torrent_announce_url() -> String {
    "http://localhost:6969/announce".into()
}

fn default_torrent_tracker_listen_addr() -> String {
    "0.0.0.0:6969".into()
}

fn default_torrent_seed_listen_addr() -> String {
    "0.0.0.0:6881".into()
}

fn default_sandbox_mode() -> String {
    "dev".into()
}

fn default_network_egress_enabled() -> bool {
    false
}

fn default_network_egress_mode() -> String {
    "denylist".into()
}

fn default_master_ui_dir() -> String {
    "./frontend/master-ui/dist".into()
}
fn default_worker_ui_dir() -> String {
    "./frontend/worker-ui/dist".into()
}

fn default_worker_control_http_addr() -> String {
    "127.0.0.1:18080".into()
}

fn default_worker_execution_secret() -> String {
    "CHANGE_ME_WORKER_EXECUTION_SECRET".into()
}

fn default_master_cors_allowed_origins() -> Vec<String> {
    local_ui_origins(&[5173, 3000, 3001])
}

fn default_worker_control_cors_allowed_origins() -> Vec<String> {
    local_ui_origins(&[5173, 3000, 3001])
}

fn local_ui_origins(ports: &[u16]) -> Vec<String> {
    ports
        .iter()
        .flat_map(|port| {
            [
                format!("http://localhost:{port}"),
                format!("http://127.0.0.1:{port}"),
            ]
        })
        .collect()
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_env<T>(name: &str, value: &str) -> anyhow::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|error| anyhow::anyhow!("{name} must be valid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_optional_fields_keep_sensible_defaults() {
        let config: HivemindConfig = serde_json::from_value(serde_json::json!({
            "database": {
                "url": "postgres://example",
                "max_connections": 10,
                "min_connections": 1,
                "idle_timeout_secs": 30,
                "connect_timeout_secs": 5
            },
            "redis": {
                "url": "redis://example",
                "pool_size": 4,
                "connect_timeout_secs": 5
            },
            "server": {
                "nodepool_grpc_addr": "0.0.0.0:50051",
                "nodepool_grpc_endpoint": "nodepool.example:50051",
                "master_http_addr": "0.0.0.0:8082",
                "worker_grpc_addr": "0.0.0.0:50053",
                "worker_grpc_port": 50053
            },
            "auth": {
                "jwt_secret": "secret",
                "token_expiry_hours": 24,
                "refresh_expiry_hours": 168,
                "bcrypt_cost": 12
            },
            "torrent": {
                "api_dir": "./api/torrents",
                "bt_dir": "./bt_torrents"
            },
            "vpn": {
                "headscale_url": "http://localhost:8080",
                "headscale_api_key": "",
                "base_virtual_ip": "100.64.0.0",
                "vpn_network": "100.64.0.0/10"
            },
            "executor": {
                "monty_executable": "monty.exe",
                "sandbox_dir": "./sandbox",
                "max_cpu_percent": 80.0,
                "max_memory_mb": 4096,
                "task_timeout_secs": 3600,
                "max_concurrent_tasks": 4
            }
        }))
        .unwrap();

        assert_eq!(config.server.worker_control_http_addr, "127.0.0.1:18080");
        assert!(config
            .server
            .master_cors_allowed_origins
            .contains(&"http://localhost:5173".to_string()));
        assert!(config
            .server
            .worker_control_cors_allowed_origins
            .contains(&"http://localhost:3001".to_string()));
        assert_eq!(
            config.torrent.announce_url,
            "http://localhost:6969/announce"
        );
        assert!(!config.torrent.allow_local_task_artifacts);
        assert_eq!(
            config.server.nodepool_grpc_endpoint.as_deref(),
            Some("nodepool.example:50051")
        );
        assert_eq!(
            config.auth.worker_execution_secret,
            "CHANGE_ME_WORKER_EXECUTION_SECRET"
        );
    }

    #[test]
    fn auth_config_deserializes_worker_execution_secret() {
        // Given: a JSON auth boundary with distinct control-plane and worker secrets.
        let auth: AuthConfig = serde_json::from_value(serde_json::json!({
            "jwt_secret": "control-plane-secret-at-least-32-bytes",
            "worker_execution_secret": "worker-execution-secret-at-least-32-bytes",
            "token_expiry_hours": 24,
            "refresh_expiry_hours": 168,
            "bcrypt_cost": 12
        }))
        .unwrap();

        // When/Then: deserialization preserves the worker trust secret separately.
        assert_eq!(
            auth.worker_execution_secret,
            "worker-execution-secret-at-least-32-bytes"
        );
    }

    #[test]
    fn env_loading_keeps_defaults_for_unspecified_values() {
        let old_config = std::env::var_os("HIVEMIND_CONFIG");
        let old_database_url = std::env::var_os("DATABASE_URL");
        let old_redis_url = std::env::var_os("REDIS_URL");
        let old_jwt_secret = std::env::var_os("JWT_SECRET");
        let old_master_cors = std::env::var_os("MASTER_CORS_ALLOWED_ORIGINS");
        let old_nodepool_endpoint = std::env::var_os("NODEPOOL_GRPC_ENDPOINT");
        let old_local_artifacts = std::env::var_os("TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS");
        std::env::remove_var("HIVEMIND_CONFIG");
        std::env::set_var("DATABASE_URL", "postgres://example");
        std::env::remove_var("REDIS_URL");
        std::env::remove_var("JWT_SECRET");
        std::env::set_var("NODEPOOL_GRPC_ENDPOINT", "nodepool.internal:50051");
        std::env::set_var("TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS", "true");
        std::env::set_var(
            "MASTER_CORS_ALLOWED_ORIGINS",
            "http://app.example, http://admin.example",
        );

        let loaded = HivemindConfig::load_from_env();

        match old_database_url {
            Some(value) => std::env::set_var("DATABASE_URL", value),
            None => std::env::remove_var("DATABASE_URL"),
        }
        match old_redis_url {
            Some(value) => std::env::set_var("REDIS_URL", value),
            None => std::env::remove_var("REDIS_URL"),
        }
        match old_jwt_secret {
            Some(value) => std::env::set_var("JWT_SECRET", value),
            None => std::env::remove_var("JWT_SECRET"),
        }
        match old_master_cors {
            Some(value) => std::env::set_var("MASTER_CORS_ALLOWED_ORIGINS", value),
            None => std::env::remove_var("MASTER_CORS_ALLOWED_ORIGINS"),
        }
        match old_nodepool_endpoint {
            Some(value) => std::env::set_var("NODEPOOL_GRPC_ENDPOINT", value),
            None => std::env::remove_var("NODEPOOL_GRPC_ENDPOINT"),
        }
        match old_local_artifacts {
            Some(value) => std::env::set_var("TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS", value),
            None => std::env::remove_var("TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS"),
        }
        match old_config {
            Some(value) => std::env::set_var("HIVEMIND_CONFIG", value),
            None => std::env::remove_var("HIVEMIND_CONFIG"),
        }

        assert_eq!(loaded.database.url, "postgres://example");
        assert_eq!(loaded.redis.url, "redis://localhost:6379");
        assert_eq!(loaded.auth.jwt_secret, "CHANGE_ME_IN_PRODUCTION");
        assert_eq!(
            loaded.torrent.announce_url,
            "http://localhost:6969/announce"
        );
        assert!(loaded.torrent.allow_local_task_artifacts);
        assert_eq!(loaded.server.worker_control_http_addr, "127.0.0.1:18080");
        assert_eq!(
            loaded.server.nodepool_grpc_endpoint.as_deref(),
            Some("nodepool.internal:50051")
        );
        assert_eq!(
            loaded.server.master_cors_allowed_origins,
            vec![
                "http://app.example".to_string(),
                "http://admin.example".to_string()
            ]
        );
    }

    #[test]
    fn env_loading_applies_ui_directory_overrides_without_json_config() {
        let old_config = std::env::var_os("HIVEMIND_CONFIG");
        let old_master_ui_dir = std::env::var_os("MASTER_UI_DIR");
        let old_worker_ui_dir = std::env::var_os("WORKER_UI_DIR");
        std::env::remove_var("HIVEMIND_CONFIG");
        std::env::set_var("MASTER_UI_DIR", "./release/master-ui");
        std::env::set_var("WORKER_UI_DIR", "./release/worker-ui");

        let loaded = HivemindConfig::load_from_env();

        match old_config {
            Some(value) => std::env::set_var("HIVEMIND_CONFIG", value),
            None => std::env::remove_var("HIVEMIND_CONFIG"),
        }
        match old_master_ui_dir {
            Some(value) => std::env::set_var("MASTER_UI_DIR", value),
            None => std::env::remove_var("MASTER_UI_DIR"),
        }
        match old_worker_ui_dir {
            Some(value) => std::env::set_var("WORKER_UI_DIR", value),
            None => std::env::remove_var("WORKER_UI_DIR"),
        }

        assert_eq!(loaded.server.master_ui_dir, "./release/master-ui");
        assert_eq!(loaded.server.worker_ui_dir, "./release/worker-ui");
    }

    #[test]
    fn env_loading_reads_worker_execution_secret_without_json_config() {
        // Given: no JSON config and an explicit worker-execution environment secret.
        let old_config = std::env::var_os("HIVEMIND_CONFIG");
        let old_secret = std::env::var_os("WORKER_EXECUTION_SECRET");
        std::env::remove_var("HIVEMIND_CONFIG");
        std::env::set_var(
            "WORKER_EXECUTION_SECRET",
            "worker-execution-env-secret-at-least-32-bytes",
        );

        // When: configuration is loaded from the environment.
        let loaded = HivemindConfig::load_from_env();

        match old_secret {
            Some(value) => std::env::set_var("WORKER_EXECUTION_SECRET", value),
            None => std::env::remove_var("WORKER_EXECUTION_SECRET"),
        }
        match old_config {
            Some(value) => std::env::set_var("HIVEMIND_CONFIG", value),
            None => std::env::remove_var("HIVEMIND_CONFIG"),
        }

        // Then: the worker secret is independent of JWT_SECRET.
        assert_eq!(
            loaded.auth.worker_execution_secret,
            "worker-execution-env-secret-at-least-32-bytes"
        );
    }

    #[test]
    fn json_config_applies_environment_overrides_after_file_defaults() {
        // Given: a JSON configuration with file defaults and distinct launcher overrides.
        let old_env = [
            ("HIVEMIND_CONFIG", std::env::var_os("HIVEMIND_CONFIG")),
            ("JWT_SECRET", std::env::var_os("JWT_SECRET")),
            (
                "WORKER_EXECUTION_SECRET",
                std::env::var_os("WORKER_EXECUTION_SECRET"),
            ),
            (
                "NODEPOOL_GRPC_ENDPOINT",
                std::env::var_os("NODEPOOL_GRPC_ENDPOINT"),
            ),
            ("MASTER_HTTP_ADDR", std::env::var_os("MASTER_HTTP_ADDR")),
            ("MASTER_UI_DIR", std::env::var_os("MASTER_UI_DIR")),
            ("WORKER_UI_DIR", std::env::var_os("WORKER_UI_DIR")),
            (
                "WORKER_ADVERTISE_ADDR",
                std::env::var_os("WORKER_ADVERTISE_ADDR"),
            ),
            (
                "EXECUTOR_SANDBOX_MODE",
                std::env::var_os("EXECUTOR_SANDBOX_MODE"),
            ),
        ];
        let path = std::env::temp_dir().join(format!(
            "hivemind-config-json-overrides-{}.json",
            std::process::id()
        ));
        let mut file_config = HivemindConfig::default();
        file_config.auth.jwt_secret = "file-jwt-secret-at-least-32-bytes".into();
        file_config.auth.worker_execution_secret = "file-worker-secret-at-least-32-bytes".into();
        file_config.server.nodepool_grpc_endpoint = Some("file-nodepool:50051".into());
        file_config.server.master_http_addr = "file-master:8082".into();
        file_config.server.master_ui_dir = "./file/master-ui".into();
        file_config.server.worker_ui_dir = "./file/worker-ui".into();
        file_config.server.worker_advertise_addr = Some("file-worker:50053".into());
        file_config.executor.sandbox_mode = "file-sandbox".into();
        std::fs::write(&path, serde_json::to_string(&file_config).unwrap()).unwrap();

        std::env::set_var("HIVEMIND_CONFIG", path.as_os_str());
        std::env::set_var("JWT_SECRET", "env-jwt-secret-at-least-32-bytes");
        std::env::set_var(
            "WORKER_EXECUTION_SECRET",
            "env-worker-secret-at-least-32-bytes",
        );
        std::env::set_var("NODEPOOL_GRPC_ENDPOINT", "env-nodepool:50051");
        std::env::set_var("MASTER_HTTP_ADDR", "env-master:8082");
        std::env::set_var("MASTER_UI_DIR", "./env/master-ui");
        std::env::set_var("WORKER_UI_DIR", "./env/worker-ui");
        std::env::remove_var("WORKER_ADVERTISE_ADDR");
        std::env::remove_var("EXECUTOR_SANDBOX_MODE");

        // When: the runtime loads the configuration through its public entry point.
        let loaded = HivemindConfig::load();

        for (name, value) in old_env {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        std::fs::remove_file(&path).unwrap();

        // Then: environment values override secrets/endpoints/UI paths while file defaults survive.
        let loaded = loaded.unwrap();
        assert_eq!(loaded.auth.jwt_secret, "env-jwt-secret-at-least-32-bytes");
        assert_eq!(
            loaded.auth.worker_execution_secret,
            "env-worker-secret-at-least-32-bytes"
        );
        assert_eq!(
            loaded.server.nodepool_grpc_endpoint.as_deref(),
            Some("env-nodepool:50051")
        );
        assert_eq!(loaded.server.master_http_addr, "env-master:8082");
        assert_eq!(loaded.server.master_ui_dir, "./env/master-ui");
        assert_eq!(loaded.server.worker_ui_dir, "./env/worker-ui");
        assert_eq!(
            loaded.server.worker_advertise_addr.as_deref(),
            Some("file-worker:50053")
        );
        assert_eq!(loaded.executor.sandbox_mode, "file-sandbox");
    }

    #[test]
    fn invalid_numeric_environment_value_returns_configuration_error() {
        // Given: a malformed value at the shared environment parsing seam.
        let error = parse_env::<u64>("EXECUTOR_MAX_MEMORY_MB", "not-a-memory-limit")
            .expect_err("malformed numeric override must fail configuration loading");

        // Then: the invalid variable is named instead of silently falling back.
        assert!(error.to_string().contains("EXECUTOR_MAX_MEMORY_MB"));
    }

    #[test]
    fn jwt_secret_validation_rejects_missing_or_known_placeholders() {
        for secret in [
            "",
            "   ",
            "CHANGE_ME_IN_PRODUCTION",
            "change-me-in-production",
        ] {
            let mut auth = AuthConfig {
                jwt_secret: secret.into(),
                worker_execution_secret: default_worker_execution_secret(),
                token_expiry_hours: 24,
                refresh_expiry_hours: 168,
                bcrypt_cost: 12,
            };

            let error = auth.validate_jwt_secret().unwrap_err().to_string();

            assert!(
                error.contains("JWT_SECRET"),
                "unexpected validation error for {secret:?}: {error}"
            );

            auth.jwt_secret = "unit-test-secret-with-at-least-32-bytes".into();
            auth.validate_jwt_secret().unwrap();
        }
    }

    #[test]
    fn jwt_secret_validation_accepts_non_default_secret() {
        let auth = AuthConfig {
            jwt_secret: "unit-test-secret-with-at-least-32-bytes".into(),
            worker_execution_secret: default_worker_execution_secret(),
            token_expiry_hours: 24,
            refresh_expiry_hours: 168,
            bcrypt_cost: 12,
        };

        auth.validate_jwt_secret().unwrap();
    }

    #[test]
    fn worker_execution_secret_validation_rejects_defaults_and_short_values() {
        // Given: each invalid worker trust-secret class.
        for secret in [
            "",
            "   ",
            "CHANGE_ME_WORKER_EXECUTION_SECRET",
            "change-me-worker-execution-secret",
            "CHANGE_ME_IN_PRODUCTION",
            "short",
        ] {
            let mut auth = HivemindConfig::default().auth;
            auth.worker_execution_secret = secret.into();

            // When: the worker-execution boundary validates the secret.
            let error = auth
                .validate_worker_execution_secret()
                .unwrap_err()
                .to_string();

            // Then: startup receives a precise WORKER_EXECUTION_SECRET failure.
            assert!(
                error.contains("WORKER_EXECUTION_SECRET"),
                "unexpected validation error for {secret:?}: {error}"
            );
        }
    }

    #[test]
    fn worker_execution_secret_validation_accepts_32_bytes() {
        // Given: a non-default worker trust secret at the minimum byte length.
        let mut auth = HivemindConfig::default().auth;
        auth.worker_execution_secret = "12345678901234567890123456789012".into();

        // When/Then: the boundary accepts exactly 32 bytes.
        auth.validate_worker_execution_secret().unwrap();
    }
}

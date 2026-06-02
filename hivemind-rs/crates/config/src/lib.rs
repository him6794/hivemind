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
    pub master_http_addr: String,
    pub worker_grpc_addr: String,
    pub worker_grpc_port: u16,
    #[serde(default = "default_worker_control_http_addr")]
    pub worker_control_http_addr: String,
    #[serde(default)]
    pub worker_advertise_addr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub token_expiry_hours: i64,
    pub refresh_expiry_hours: i64,
    pub bcrypt_cost: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentConfig {
    pub api_dir: String,
    pub bt_dir: String,
    #[serde(default = "default_torrent_announce_url")]
    pub announce_url: String,
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
                master_http_addr: "0.0.0.0:8082".into(),
                worker_grpc_addr: "0.0.0.0:50053".into(),
                worker_grpc_port: 50053,
                worker_control_http_addr: default_worker_control_http_addr(),
                worker_advertise_addr: None,
            },
            auth: AuthConfig {
                jwt_secret: "CHANGE_ME_IN_PRODUCTION".into(),
                token_expiry_hours: 24,
                refresh_expiry_hours: 168,
                bcrypt_cost: 12,
            },
            torrent: TorrentConfig {
                api_dir: "./api/torrents".into(),
                bt_dir: "./bt_torrents".into(),
                announce_url: "http://localhost:6969/announce".into(),
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
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let config = match std::env::var("HIVEMIND_CONFIG") {
            Ok(path) => {
                let contents = std::fs::read_to_string(&path)?;
                serde_json::from_str(&contents)?
            }
            Err(_) => Self::load_from_env(),
        };
        Ok(config)
    }

    fn load_from_env() -> Self {
        let mut config = Self::default();
        if let Ok(url) = std::env::var("DATABASE_URL") {
            config.database.url = url;
        }
        if let Ok(url) = std::env::var("REDIS_URL") {
            config.redis.url = url;
        }
        if let Ok(addr) = std::env::var("NODEPOOL_GRPC_ADDR") {
            config.server.nodepool_grpc_addr = addr;
        }
        if let Ok(addr) = std::env::var("MASTER_HTTP_ADDR") {
            config.server.master_http_addr = addr;
        }
        if let Ok(addr) = std::env::var("WORKER_GRPC_ADDR") {
            config.server.worker_grpc_addr = addr;
        }
        if let Ok(addr) = std::env::var("WORKER_CONTROL_HTTP_ADDR") {
            config.server.worker_control_http_addr = addr;
        }
        if let Ok(addr) = std::env::var("WORKER_ADVERTISE_ADDR") {
            config.server.worker_advertise_addr = Some(addr);
        }
        if let Ok(secret) = std::env::var("JWT_SECRET") {
            config.auth.jwt_secret = secret;
        }
        if let Ok(exec) = std::env::var("MONTY_EXECUTABLE") {
            config.executor.monty_executable = exec;
        }
        if let Ok(dir) = std::env::var("EXECUTOR_SANDBOX_DIR") {
            config.executor.sandbox_dir = dir;
        }
        if let Ok(value) = std::env::var("EXECUTOR_MAX_CPU_PERCENT") {
            if let Ok(parsed) = value.parse() {
                config.executor.max_cpu_percent = parsed;
            }
        }
        if let Ok(value) = std::env::var("EXECUTOR_MAX_MEMORY_MB") {
            if let Ok(parsed) = value.parse() {
                config.executor.max_memory_mb = parsed;
            }
        }
        if let Ok(value) = std::env::var("EXECUTOR_TASK_TIMEOUT_SECS") {
            if let Ok(parsed) = value.parse() {
                config.executor.task_timeout_secs = parsed;
            }
        }
        if let Ok(value) = std::env::var("EXECUTOR_MAX_CONCURRENT_TASKS") {
            if let Ok(parsed) = value.parse() {
                config.executor.max_concurrent_tasks = parsed;
            }
        }
        if let Ok(mode) = std::env::var("EXECUTOR_SANDBOX_MODE") {
            config.executor.sandbox_mode = mode;
        }
        if let Ok(enabled) = std::env::var("EXECUTOR_NETWORK_EGRESS_ENABLED") {
            if let Ok(parsed) = enabled.parse() {
                config.executor.network_egress_enabled = parsed;
            }
        }
        if let Ok(mode) = std::env::var("EXECUTOR_NETWORK_EGRESS_MODE") {
            config.executor.network_egress_mode = mode;
        }
        if let Ok(targets) = std::env::var("EXECUTOR_NETWORK_EGRESS_TARGETS") {
            config.executor.network_egress_targets = targets
                .split(',')
                .map(|target| target.trim())
                .filter(|target| !target.is_empty())
                .map(ToString::to_string)
                .collect();
        }
        if let Ok(dir) = std::env::var("TORRENT_API_DIR") {
            config.torrent.api_dir = dir;
        }
        if let Ok(dir) = std::env::var("TORRENT_BT_DIR") {
            config.torrent.bt_dir = dir;
        }
        if let Ok(url) = std::env::var("TORRENT_ANNOUNCE_URL") {
            config.torrent.announce_url = url;
        }
        if let Ok(url) = std::env::var("HEADSCALE_URL") {
            config.vpn.headscale_url = url;
        }
        if let Ok(key) = std::env::var("HEADSCALE_API_KEY") {
            config.vpn.headscale_api_key = key;
        }
        if let Ok(ip) = std::env::var("VPN_BASE_VIRTUAL_IP") {
            config.vpn.base_virtual_ip = ip;
        }
        if let Ok(network) = std::env::var("VPN_NETWORK") {
            config.vpn.vpn_network = network;
        }
        config
    }
}

fn default_torrent_announce_url() -> String {
    "http://localhost:6969/announce".into()
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

fn default_worker_control_http_addr() -> String {
    "127.0.0.1:18080".into()
}

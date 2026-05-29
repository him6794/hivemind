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
        if let Ok(secret) = std::env::var("JWT_SECRET") {
            config.auth.jwt_secret = secret;
        }
        if let Ok(exec) = std::env::var("MONTY_EXECUTABLE") {
            config.executor.monty_executable = exec;
        }
        config
    }
}
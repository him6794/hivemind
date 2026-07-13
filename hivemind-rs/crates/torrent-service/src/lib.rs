//! Torrent Service: converts task ZIPs into BitTorrent metainfo files
//! for P2P distribution among workers, then tracks P2P swarm state.

pub mod metainfo;
pub mod p2p_swarm;
pub mod tracker;
pub mod transfer;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use transfer::SeedStore;

/// Manages torrent creation, seeding, and P2P task distribution
pub struct TorrentService {
    api_dir: PathBuf,
    bt_dir: PathBuf,
    seed_store: SeedStore,
}

impl TorrentService {
    pub fn new(config: &HivemindConfig) -> Self {
        Self {
            api_dir: PathBuf::from(&config.torrent.api_dir),
            bt_dir: PathBuf::from(&config.torrent.bt_dir),
            seed_store: SeedStore::new(),
        }
    }

    pub fn with_seed_store(config: &HivemindConfig, seed_store: SeedStore) -> Self {
        Self {
            api_dir: PathBuf::from(&config.torrent.api_dir),
            bt_dir: PathBuf::from(&config.torrent.bt_dir),
            seed_store,
        }
    }

    pub fn with_dirs(api_dir: PathBuf, bt_dir: PathBuf, seed_store: SeedStore) -> Self {
        Self {
            api_dir,
            bt_dir,
            seed_store,
        }
    }

    pub fn seed_store(&self) -> SeedStore {
        self.seed_store.clone()
    }

    /// Convert a task ZIP file into a .torrent metainfo file.
    pub async fn zip_to_torrent(&self, zip_path: &Path, announce: &str) -> Result<TorrentInfo> {
        let data =
            std::fs::read(zip_path).map_err(|e| anyhow::anyhow!("Failed to read ZIP: {}", e))?;
        self.package_bytes_to_torrent(&data, zip_path, announce)
            .await
    }

    /// Convert package bytes into a seeded torrent and magnet URI.
    pub async fn package_bytes_to_torrent(
        &self,
        package_data: &[u8],
        package_path: &Path,
        announce: &str,
    ) -> Result<TorrentInfo> {
        let file_name = package_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("task.zip");
        let seeded = transfer::create_and_store_seed(
            &self.seed_store,
            package_data,
            package_path,
            announce,
            &self.api_dir,
            &self.bt_dir,
            String::new(),
        )
        .await?;
        let magnet = self.magnet_uri(&seeded.info_hash, file_name, announce);
        let mut seeded = seeded;
        seeded.magnet_uri = magnet.clone();
        self.seed_store.insert(seeded.clone()).await;

        tracing::info!(
            "Created torrent {} (BTIH: {}) for {}",
            seeded.torrent_path.display(),
            seeded.info_hash,
            package_path.display()
        );

        Ok(TorrentInfo {
            info_hash: seeded.info_hash,
            torrent_path: seeded.torrent_path,
            seed_path: seeded.seed_path,
            magnet_uri: magnet,
            data_size: package_data.len() as u64,
            piece_count: seeded.piece_hashes.len(),
            piece_size: seeded.piece_length as u64,
        })
    }

    /// Generate a magnet URI for a torrent info-hash, including announce URL.
    pub fn magnet_uri(&self, info_hash: &str, name: &str, announce: &str) -> String {
        format!(
            "magnet:?xt=urn:btih:{}&dn={}&tr={}",
            info_hash,
            urlencoding(name),
            urlencoding(announce)
        )
    }
}

/// Shared nodepool distribution runtime (tracker + seed store).
#[derive(Clone)]
pub struct DistributionRuntime {
    pub tracker: Arc<tracker::Tracker>,
    pub seed_store: SeedStore,
    pub announce_url: String,
    pub api_dir: PathBuf,
    pub bt_dir: PathBuf,
    pub seed_addr: std::net::SocketAddr,
    pub tracker_addr: std::net::SocketAddr,
    pub seed_advertise_host: String,
}

impl DistributionRuntime {
    pub async fn start(
        config: &HivemindConfig,
    ) -> Result<(Self, Vec<tokio::task::JoinHandle<()>>)> {
        let tracker = Arc::new(tracker::Tracker::new(300));
        let seed_store = SeedStore::new();

        let tracker_addr = parse_socket_addr(
            &config.torrent.tracker_listen_addr,
            "torrent tracker listen address",
        )?;
        let seed_addr = parse_socket_addr(
            &config.torrent.seed_listen_addr,
            "torrent seed listen address",
        )?;

        let mut handles = Vec::new();
        handles.push(transfer::start_http_tracker(tracker_addr, tracker.clone()).await?);
        handles.push(transfer::start_seed_listener(seed_addr, seed_store.clone()).await?);

        let seed_advertise_host = config
            .torrent
            .seed_advertise_host
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| advertised_ip(&seed_addr));

        Ok((
            Self {
                tracker,
                seed_store,
                announce_url: config.torrent.announce_url.clone(),
                api_dir: PathBuf::from(&config.torrent.api_dir),
                bt_dir: PathBuf::from(&config.torrent.bt_dir),
                seed_addr,
                tracker_addr,
                seed_advertise_host,
            },
            handles,
        ))
    }

    pub fn torrent_service(&self) -> TorrentService {
        TorrentService::with_dirs(
            self.api_dir.clone(),
            self.bt_dir.clone(),
            self.seed_store.clone(),
        )
    }

    pub async fn register_local_seeder(&self, info_hash: &str, total_size: u64) -> Result<()> {
        let _ = self
            .tracker
            .announce(
                info_hash,
                tracker::PeerEntry {
                    peer_id: format!("nodepool-seeder-{}", &info_hash[..8.min(info_hash.len())]),
                    ip: self.seed_advertise_host.clone(),
                    port: self.seed_addr.port(),
                    uploaded: 0,
                    downloaded: total_size,
                    left: 0,
                    last_announce: 0,
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(())
    }
}

fn parse_socket_addr(value: &str, label: &str) -> Result<std::net::SocketAddr> {
    value
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid {label} '{value}': {e}"))
}

fn advertised_ip(addr: &std::net::SocketAddr) -> String {
    match addr.ip() {
        std::net::IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".into(),
        std::net::IpAddr::V6(ip) if ip.is_unspecified() => "::1".into(),
        other => other.to_string(),
    }
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TorrentInfo {
    pub info_hash: String,
    pub torrent_path: PathBuf,
    pub seed_path: PathBuf,
    pub magnet_uri: String,
    pub data_size: u64,
    pub piece_count: usize,
    pub piece_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_zip_to_torrent_creates_torrent_file() {
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("task.zip");
        std::fs::write(&zip_path, b"example-task-data-for-torrent-generation").unwrap();

        let api_dir = tmp.path().join("api");
        let bt_dir = tmp.path().join("bt");
        std::fs::create_dir_all(&bt_dir).unwrap();

        let svc = TorrentService {
            api_dir,
            bt_dir,
            seed_store: SeedStore::new(),
        };

        let info = svc
            .zip_to_torrent(&zip_path, "http://tracker:6969/announce")
            .await
            .unwrap();
        assert!(!info.info_hash.is_empty(), "info_hash should be non-empty");
        assert!(info.torrent_path.exists(), "torrent file should exist");
        assert!(info.seed_path.exists(), "seed package should exist");
        assert!(info.data_size > 0);
        assert!(info.piece_count > 0);
        assert!(info.magnet_uri.contains("tr=http"));
    }

    #[test]
    fn test_magnet_uri_format_includes_announce() {
        let svc = TorrentService {
            api_dir: PathBuf::from("./api"),
            bt_dir: PathBuf::from("./bt"),
            seed_store: SeedStore::new(),
        };
        let uri = svc.magnet_uri(
            "abc123def456",
            "task_name",
            "http://127.0.0.1:6969/announce",
        );
        assert!(uri.starts_with("magnet:?xt=urn:btih:abc123def456"));
        assert!(uri.contains("dn=task_name"));
        assert!(uri.contains("tr=http%3A%2F%2F127.0.0.1%3A6969%2Fannounce"));
    }
}

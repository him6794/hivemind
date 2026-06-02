//! Torrent Service: converts task ZIPs into BitTorrent metainfo files
//! for P2P distribution among workers, then tracks P2P swarm state.

pub mod metainfo;
pub mod p2p_swarm;
pub mod tracker;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use std::path::{Path, PathBuf};

/// Manages torrent creation, seeding, and P2P task distribution
pub struct TorrentService {
    api_dir: PathBuf,
    bt_dir: PathBuf,
}

impl TorrentService {
    pub fn new(config: &HivemindConfig) -> Self {
        Self {
            api_dir: PathBuf::from(&config.torrent.api_dir),
            bt_dir: PathBuf::from(&config.torrent.bt_dir),
        }
    }

    /// Convert a task ZIP file into a .torrent metainfo file.
    /// Returns the torrent file path and its info-hash (BTIH).
    pub async fn zip_to_torrent(&self, zip_path: &Path, announce: &str) -> Result<TorrentInfo> {
        std::fs::create_dir_all(&self.bt_dir)?;

        let data =
            std::fs::read(zip_path).map_err(|e| anyhow::anyhow!("Failed to read ZIP: {}", e))?;

        let meta = metainfo::create_metainfo(&data, zip_path, announce)?;

        let torrent_path = self.bt_dir.join(format!("{}.torrent", &meta.info_hash));
        let torrent_bytes = bendy::serde::to_bytes(&meta.metainfo)
            .map_err(|e| anyhow::anyhow!("Failed to serialize torrent: {}", e))?;
        std::fs::write(&torrent_path, &torrent_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to write torrent file: {}", e))?;

        // Also copy the data to the seed directory
        let seed_path = self.api_dir.join(
            zip_path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("task.zip")),
        );
        std::fs::create_dir_all(&self.api_dir)?;
        std::fs::copy(zip_path, &seed_path)
            .map_err(|e| anyhow::anyhow!("Failed to copy task to seed dir: {}", e))?;

        tracing::info!(
            "Created torrent {} (BTIH: {}) for {}",
            torrent_path.display(),
            meta.info_hash,
            zip_path.display()
        );

        Ok(TorrentInfo {
            info_hash: meta.info_hash,
            torrent_path,
            data_size: data.len() as u64,
            piece_count: meta.piece_count,
            piece_size: meta.piece_size,
        })
    }

    /// Generate a magnet URI for a torrent info-hash
    pub fn magnet_uri(&self, info_hash: &str, name: &str) -> String {
        format!(
            "magnet:?xt=urn:btih:{}&dn={}&tr=",
            info_hash,
            urlencoding(name)
        )
    }
}

fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace('#', "%23")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TorrentInfo {
    pub info_hash: String,
    pub torrent_path: PathBuf,
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
        std::fs::write(&zip_path, b"fake-task-data-for-torrent-generation").unwrap();

        let api_dir = tmp.path().join("api");
        let bt_dir = tmp.path().join("bt");
        std::fs::create_dir_all(&bt_dir).unwrap();

        let svc = TorrentService { api_dir, bt_dir };

        let info = svc
            .zip_to_torrent(&zip_path, "http://tracker:6969/announce")
            .await
            .unwrap();
        assert!(!info.info_hash.is_empty(), "info_hash should be non-empty");
        assert!(info.torrent_path.exists(), "torrent file should exist");
        assert!(info.data_size > 0);
        assert!(info.piece_count > 0);
    }

    #[test]
    fn test_magnet_uri_format() {
        let svc = TorrentService {
            api_dir: PathBuf::from("./api"),
            bt_dir: PathBuf::from("./bt"),
        };
        let uri = svc.magnet_uri("abc123def456", "task_name");
        assert!(uri.starts_with("magnet:?xt=urn:btih:abc123def456"));
        assert!(uri.contains("dn=task_name"));
    }
}

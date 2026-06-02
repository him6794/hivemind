//! P2P Swarm coordinator: manages the lifecycle of P2P task distribution.
//! Uses the BitTorrent tracker to coordinate peer discovery,
//! then workers exchange data via the VPN overlay.

use crate::tracker::{PeerEntry, Tracker};
use anyhow::Result;
use std::sync::Arc;
use tracing::info;

/// Coordinates P2P distribution of a task across worker peers
pub struct P2PSwarm {
    tracker: Arc<Tracker>,
}

impl P2PSwarm {
    pub fn new(tracker: Arc<Tracker>) -> Self {
        Self { tracker }
    }

    /// Seed a new torrent: register the master as the initial seeder
    pub async fn start_seeding(
        &self,
        info_hash: &str,
        master_ip: &str,
        master_port: u16,
        total_size: u64,
    ) -> Result<()> {
        let seeder = PeerEntry {
            peer_id: format!("master-seeder-{}", &info_hash[..8]),
            ip: master_ip.to_string(),
            port: master_port,
            uploaded: 0,
            downloaded: 0,
            left: 0, // seeder has all data
            last_announce: 0,
        };

        let _ = self
            .tracker
            .announce(info_hash, seeder)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to register seeder: {}", e))?;

        info!(
            "Started seeding {} ({} MB) on {}:{}",
            &info_hash[..8],
            total_size / (1024 * 1024),
            master_ip,
            master_port
        );
        Ok(())
    }

    /// A worker joins the swarm to download a task
    pub async fn worker_join(
        &self,
        info_hash: &str,
        worker_id: &str,
        worker_ip: &str,
        worker_port: u16,
        total_size: u64,
    ) -> Result<Vec<PeerEntry>> {
        let peer = PeerEntry {
            peer_id: worker_id.to_string(),
            ip: worker_ip.to_string(),
            port: worker_port,
            uploaded: 0,
            downloaded: 0,
            left: total_size,
            last_announce: 0,
        };

        let peers = self
            .tracker
            .announce(info_hash, peer)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to join swarm: {}", e))?;

        info!(
            "Worker {} joined swarm {} - {} peers available",
            worker_id,
            &info_hash[..8],
            peers.len()
        );

        Ok(peers)
    }

    /// Worker reports completion of download to the tracker
    pub async fn worker_completed(
        &self,
        info_hash: &str,
        worker_id: &str,
        worker_ip: &str,
        worker_port: u16,
        downloaded: u64,
    ) -> Result<()> {
        let peer = PeerEntry {
            peer_id: worker_id.to_string(),
            ip: worker_ip.to_string(),
            port: worker_port,
            uploaded: 0,
            downloaded,
            left: 0, // fully downloaded
            last_announce: 0,
        };

        let _ = self
            .tracker
            .announce(info_hash, peer)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to report completion: {}", e))?;

        info!(
            "Worker {} completed download for {}",
            worker_id,
            &info_hash[..8]
        );
        Ok(())
    }

    /// Get current peer count for a swarm
    pub async fn peer_count(&self, info_hash: &str) -> usize {
        self.tracker.swarm_size(info_hash).await
    }

    /// Remove a torrent swarm (task done/cancelled)
    pub async fn remove_swarm(&self, info_hash: &str) {
        self.tracker.remove_swarm(info_hash).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_p2p_swarm_lifecycle() {
        let tracker = Arc::new(Tracker::new(3600));
        let swarm = P2PSwarm::new(tracker);

        let info_hash = "deadbeefcafebabe000000000000000000000000";

        // Master starts seeding
        swarm
            .start_seeding(info_hash, "10.0.0.1", 6881, 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(swarm.peer_count(info_hash).await, 1);

        // Worker joins
        let peers = swarm
            .worker_join(info_hash, "worker-1", "100.64.0.2", 6881, 1024 * 1024)
            .await
            .unwrap();
        assert!(!peers.is_empty(), "should get master seeder as peer");
        assert_eq!(
            peers[0].peer_id,
            format!("master-seeder-{}", &info_hash[..8])
        );

        // Worker completes
        swarm
            .worker_completed(info_hash, "worker-1", "100.64.0.2", 6881, 1024 * 1024)
            .await
            .unwrap();

        // Remove swarm
        swarm.remove_swarm(info_hash).await;
        assert_eq!(swarm.peer_count(info_hash).await, 0);
    }
}

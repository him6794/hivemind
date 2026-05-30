//! BitTorrent tracker: lightweight HTTP announce tracker for the P2P swarm.
//! Workers announce themselves to get peer lists for torrent distribution.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Represents a peer in the tracker's swarm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEntry {
    pub peer_id: String,
    pub ip: String,
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub last_announce: i64,
}

/// Lightweight in-memory tracker for P2P task distribution
pub struct Tracker {
    swarms: Arc<RwLock<HashMap<String, Vec<PeerEntry>>>>,
    peer_timeout_secs: i64,
}

impl Tracker {
    pub fn new(peer_timeout_secs: i64) -> Self {
        Self { swarms: Arc::new(RwLock::new(HashMap::new())), peer_timeout_secs }
    }

    /// Register or update a peer in a torrent swarm.
    /// Returns all OTHER peers in the swarm.
    pub async fn announce(&self, info_hash: &str, peer: PeerEntry) -> Result<Vec<PeerEntry>, String> {
        let peer_id = peer.peer_id.clone(); // capture before moving
        let mut swarms = self.swarms.write().await;
        let now = chrono::Utc::now().timestamp();

        let swarm = swarms.entry(info_hash.to_string()).or_default();
        swarm.retain(|p| now - p.last_announce < self.peer_timeout_secs);

        // Upsert the announcing peer
        let mut updated = false;
        for existing in swarm.iter_mut() {
            if existing.peer_id == peer_id {
                existing.ip.clone_from(&peer.ip);
                existing.port = peer.port;
                existing.uploaded = peer.uploaded;
                existing.downloaded = peer.downloaded;
                existing.left = peer.left;
                existing.last_announce = now;
                updated = true;
                break;
            }
        }
        if !updated {
            let mut new_peer = peer;
            new_peer.last_announce = now;
            swarm.push(new_peer);
        }

        info!("Tracker announce: {} peers for info_hash {}...", swarm.len(), &info_hash[..usize::min(8, info_hash.len())]);

        // Return all peers EXCEPT the announcing peer
        Ok(swarm.iter().filter(|p| p.peer_id != peer_id).cloned().collect())
    }

    pub async fn swarm_size(&self, info_hash: &str) -> usize {
        self.swarms.read().await.get(info_hash).map(|s| s.len()).unwrap_or(0)
    }

    pub async fn remove_swarm(&self, info_hash: &str) {
        self.swarms.write().await.remove(info_hash);
        info!("Tracker swarm removed for {}...", &info_hash[..usize::min(8, info_hash.len())]);
    }

    pub async fn cleanup_stale(&self) {
        let mut swarms = self.swarms.write().await;
        let now = chrono::Utc::now().timestamp();
        let before: usize = swarms.values().map(|s| s.len()).sum();
        for swarm in swarms.values_mut() {
            swarm.retain(|p| now - p.last_announce < self.peer_timeout_secs);
        }
        swarms.retain(|_, s| !s.is_empty());
        let after: usize = swarms.values().map(|s| s.len()).sum();
        if before != after { info!("Tracker cleanup: {} peers removed ({} remaining)", before - after, after); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str) -> PeerEntry {
        PeerEntry { peer_id: id.into(), ip: "10.0.0.1".into(), port: 6881, uploaded: 0, downloaded: 0, left: 4096, last_announce: 0 }
    }

    #[tokio::test]
    async fn test_tracker_announce_and_peer_list() {
        let tracker = Tracker::new(60);
        let p1 = peer("worker-1");
        let peers = tracker.announce("abc123", p1).await.unwrap();
        assert_eq!(peers.len(), 0);

        let p2 = peer("worker-2");
        let peers = tracker.announce("abc123", p2).await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].peer_id, "worker-1");
    }

    #[tokio::test]
    async fn test_tracker_swarm_size_and_remove() {
        let tracker = Tracker::new(60);
        let _ = tracker.announce("hash1", peer("w1")).await;
        assert_eq!(tracker.swarm_size("hash1").await, 1);
        tracker.remove_swarm("hash1").await;
        assert_eq!(tracker.swarm_size("hash1").await, 0);
    }
}

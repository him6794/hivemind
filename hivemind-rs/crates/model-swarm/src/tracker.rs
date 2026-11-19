//! In-memory peer registry for one private swarm.
//!
//! Nodepool owns an instance and fills it only with enrolled, online nodes.
//! The registry does not authenticate callers, so the caller must check identity
//! before it calls `announce`. Entries expire after `ttl` without a new announce.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Entries expire this long after their last announce.
pub const DEFAULT_TTL: Duration = Duration::from_secs(30 * 60);
/// Largest number of distinct infohashes tracked at once.
pub const MAX_INFOHASHES: usize = 4096;
/// Largest number of peers tracked for one infohash.
pub const MAX_PEERS_PER_INFOHASH: usize = 4096;

/// Refusals from the registry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("registry has no room for another swarm or peer")]
    Full,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    addr: SocketAddr,
    seed: bool,
    seen: Instant,
}

/// Peers known for each infohash.
#[derive(Debug)]
pub struct Registry {
    ttl: Duration,
    swarms: HashMap<[u8; 20], HashMap<[u8; 20], Entry>>,
}

impl Registry {
    /// Creates an empty registry whose entries expire after `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            swarms: HashMap::new(),
        }
    }

    /// Records an announce. A `left` of zero means the peer has the whole file
    /// and counts as a seed. A peer that announces again replaces its entry.
    pub fn announce(
        &mut self,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
        addr: SocketAddr,
        left: u64,
        now: Instant,
    ) -> Result<(), RegistryError> {
        if !self.swarms.contains_key(&info_hash) && self.swarms.len() >= MAX_INFOHASHES {
            return Err(RegistryError::Full);
        }
        let peers = self.swarms.entry(info_hash).or_default();
        if !peers.contains_key(&peer_id) && peers.len() >= MAX_PEERS_PER_INFOHASH {
            return Err(RegistryError::Full);
        }
        peers.insert(
            peer_id,
            Entry {
                addr,
                seed: left == 0,
                seen: now,
            },
        );
        Ok(())
    }

    /// Live seeds for `info_hash`, leaving out `requester`, at most `limit` addresses.
    pub fn seeds(
        &self,
        info_hash: &[u8; 20],
        requester: Option<&[u8; 20]>,
        now: Instant,
        limit: usize,
    ) -> Vec<SocketAddr> {
        self.live(info_hash, requester, now, limit, true)
    }

    /// Live peers of any kind for `info_hash`, leaving out `requester`.
    pub fn peers(
        &self,
        info_hash: &[u8; 20],
        requester: Option<&[u8; 20]>,
        now: Instant,
        limit: usize,
    ) -> Vec<SocketAddr> {
        self.live(info_hash, requester, now, limit, false)
    }

    /// Drops expired entries and swarms that became empty.
    pub fn expire(&mut self, now: Instant) {
        let ttl = self.ttl;
        self.swarms.retain(|_, peers| {
            peers.retain(|_, entry| now.saturating_duration_since(entry.seen) <= ttl);
            !peers.is_empty()
        });
    }

    /// Number of infohashes with at least one entry, expired or not.
    pub fn swarm_count(&self) -> usize {
        self.swarms.len()
    }

    fn live(
        &self,
        info_hash: &[u8; 20],
        requester: Option<&[u8; 20]>,
        now: Instant,
        limit: usize,
        seeds_only: bool,
    ) -> Vec<SocketAddr> {
        let Some(peers) = self.swarms.get(info_hash) else {
            return Vec::new();
        };
        peers
            .iter()
            .filter(|(peer_id, entry)| {
                requester != Some(*peer_id)
                    && now.saturating_duration_since(entry.seen) <= self.ttl
                    && (!seeds_only || entry.seed)
            })
            .map(|(_, entry)| entry.addr)
            .take(limit)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([100, 64, 0, 1], port))
    }

    #[test]
    fn a_leecher_becomes_a_seed_when_it_reports_nothing_left() {
        let mut registry = Registry::new(DEFAULT_TTL);
        let now = Instant::now();
        let hash = [1u8; 20];
        registry
            .announce(hash, [10; 20], addr(1), 500, now)
            .expect("announce");
        assert!(registry.seeds(&hash, None, now, 10).is_empty());
        assert_eq!(registry.peers(&hash, None, now, 10), vec![addr(1)]);

        registry
            .announce(hash, [10; 20], addr(1), 0, now)
            .expect("announce");
        assert_eq!(registry.seeds(&hash, None, now, 10), vec![addr(1)]);
    }

    #[test]
    fn the_requester_is_never_returned_to_itself() {
        let mut registry = Registry::new(DEFAULT_TTL);
        let now = Instant::now();
        let hash = [1u8; 20];
        registry
            .announce(hash, [10; 20], addr(1), 0, now)
            .expect("announce");
        registry
            .announce(hash, [11; 20], addr(2), 0, now)
            .expect("announce");
        let others = registry.seeds(&hash, Some(&[10; 20]), now, 10);
        assert_eq!(others, vec![addr(2)]);
    }

    #[test]
    fn stale_entries_are_not_returned_and_expire_removes_them() {
        let mut registry = Registry::new(Duration::from_secs(60));
        let start = Instant::now();
        let hash = [2u8; 20];
        registry
            .announce(hash, [1; 20], addr(1), 0, start)
            .expect("announce");
        let later = start + Duration::from_secs(61);
        assert!(registry.seeds(&hash, None, later, 10).is_empty());
        registry.expire(later);
        assert_eq!(registry.swarm_count(), 0);
    }

    #[test]
    fn the_limit_caps_the_answer() {
        let mut registry = Registry::new(DEFAULT_TTL);
        let now = Instant::now();
        let hash = [3u8; 20];
        for port in 1..=5u16 {
            registry
                .announce(hash, [port as u8; 20], addr(port), 0, now)
                .expect("announce");
        }
        assert_eq!(registry.seeds(&hash, None, now, 3).len(), 3);
    }

    #[test]
    fn a_full_registry_refuses_new_swarms_but_still_updates_known_ones() {
        let mut registry = Registry::new(DEFAULT_TTL);
        let now = Instant::now();
        for index in 0..MAX_INFOHASHES {
            let mut hash = [0u8; 20];
            hash[..8].copy_from_slice(&(index as u64).to_be_bytes());
            registry
                .announce(hash, [1; 20], addr(1), 0, now)
                .expect("announce");
        }
        let mut first = [0u8; 20];
        first[..8].copy_from_slice(&0u64.to_be_bytes());
        assert!(registry.announce(first, [2; 20], addr(2), 0, now).is_ok());
        let fresh = [0xffu8; 20];
        assert_eq!(
            registry.announce(fresh, [1; 20], addr(1), 0, now),
            Err(RegistryError::Full)
        );
    }
}

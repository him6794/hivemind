//! Stateful distributed KV-Cache session manager.
//!
//! Tracks per-session KV-cache tensors hosted on a partial-layer worker with:
//! - hard memory quota enforcement across all active sessions
//! - idle TTL reclamation
//! - concurrent micro-batch admission caps

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KvCacheQuotaConfig {
    /// Total KV-cache byte budget across all concurrent sessions on this worker.
    pub max_total_bytes: u64,
    /// Maximum number of concurrent active inference sessions.
    pub max_concurrent_sessions: usize,
    /// Maximum tokens stored per session (context window bound).
    pub max_tokens_per_session: u32,
    /// Evict sessions that have been idle longer than this TTL.
    pub idle_ttl: Duration,
}

impl Default for KvCacheQuotaConfig {
    fn default() -> Self {
        Self {
            max_total_bytes: 2 << 30, // 2 GiB
            max_concurrent_sessions: 32,
            max_tokens_per_session: 8192,
            idle_ttl: Duration::from_secs(120),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KvCacheError {
    #[error("KV-cache memory quota exceeded: requested {requested} B, available {available} B")]
    MemoryQuotaExceeded { requested: u64, available: u64 },
    #[error("maximum concurrent KV-cache sessions ({0}) reached")]
    ConcurrencyLimitReached(usize),
    #[error("session `{0}` not found")]
    SessionNotFound(String),
    #[error("session `{session_id}` exceeded max context tokens ({limit})")]
    ContextLimitExceeded { session_id: String, limit: u32 },
}

#[derive(Debug, Clone)]
pub struct KvSession {
    pub session_id: String,
    pub model_id: String,
    pub start_layer: u32,
    pub end_layer: u32,
    pub bytes_per_token: u64,
    pub token_count: u32,
    /// Compact per-step KV summary state (key/value vectors per step).
    pub kv_history: Vec<Vec<f32>>,
    pub allocated_bytes: u64,
    pub last_active: Instant,
}

#[derive(Debug)]
pub struct KvCacheManager {
    config: KvCacheQuotaConfig,
    sessions: HashMap<String, KvSession>,
    total_allocated_bytes: u64,
}

impl KvCacheManager {
    #[must_use]
    pub fn new(config: KvCacheQuotaConfig) -> Self {
        Self {
            config,
            sessions: HashMap::new(),
            total_allocated_bytes: 0,
        }
    }

    #[must_use]
    pub fn total_allocated_bytes(&self) -> u64 {
        self.total_allocated_bytes
    }

    #[must_use]
    pub fn active_session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Evicts any session idle longer than `idle_ttl` relative to `now`.
    pub fn evict_expired(&mut self, now: Instant) -> Vec<String> {
        let ttl = self.config.idle_ttl;
        let expired: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| now.saturating_duration_since(s.last_active) >= ttl)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &expired {
            if let Some(removed) = self.sessions.remove(id) {
                self.total_allocated_bytes = self
                    .total_allocated_bytes
                    .saturating_sub(removed.allocated_bytes);
            }
        }
        expired
    }

    /// Opens or re-uses a stateful session, reserving `initial_tokens` worth of KV bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn open_session(
        &mut self,
        session_id: impl Into<String>,
        model_id: impl Into<String>,
        start_layer: u32,
        end_layer: u32,
        bytes_per_layer_token: u64,
        initial_tokens: u32,
        now: Instant,
    ) -> Result<(), KvCacheError> {
        self.evict_expired(now);
        let session_id = session_id.into();
        if self.sessions.contains_key(&session_id) {
            return Ok(());
        }
        if self.sessions.len() >= self.config.max_concurrent_sessions {
            return Err(KvCacheError::ConcurrencyLimitReached(
                self.config.max_concurrent_sessions,
            ));
        }
        if initial_tokens > self.config.max_tokens_per_session {
            return Err(KvCacheError::ContextLimitExceeded {
                session_id,
                limit: self.config.max_tokens_per_session,
            });
        }
        let layers = u64::from(end_layer.saturating_sub(start_layer).max(1));
        let bytes_per_token = bytes_per_layer_token.saturating_mul(layers);
        let initial_bytes = bytes_per_token.saturating_mul(u64::from(initial_tokens));
        let available = self
            .config
            .max_total_bytes
            .saturating_sub(self.total_allocated_bytes);
        if initial_bytes > available {
            return Err(KvCacheError::MemoryQuotaExceeded {
                requested: initial_bytes,
                available,
            });
        }
        self.total_allocated_bytes = self.total_allocated_bytes.saturating_add(initial_bytes);
        self.sessions.insert(
            session_id.clone(),
            KvSession {
                session_id,
                model_id: model_id.into(),
                start_layer,
                end_layer,
                bytes_per_token,
                token_count: 0,
                kv_history: Vec::new(),
                allocated_bytes: initial_bytes,
                last_active: now,
            },
        );
        Ok(())
    }

    /// Appends one decoded token's KV entry to an open session, enforcing memory
    /// and context window caps.
    pub fn append_step(
        &mut self,
        session_id: &str,
        kv_state: Vec<f32>,
        now: Instant,
    ) -> Result<u32, KvCacheError> {
        let max_total = self.config.max_total_bytes;
        let max_tokens = self.config.max_tokens_per_session;
        let current_total = self.total_allocated_bytes;

        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| KvCacheError::SessionNotFound(session_id.to_owned()))?;
        let next_tokens = session.token_count.saturating_add(1);
        if next_tokens > max_tokens {
            return Err(KvCacheError::ContextLimitExceeded {
                session_id: session_id.to_owned(),
                limit: max_tokens,
            });
        }
        let needed_total_for_session = session
            .bytes_per_token
            .saturating_mul(u64::from(next_tokens));
        let delta = needed_total_for_session.saturating_sub(session.allocated_bytes);
        let available = max_total.saturating_sub(current_total);
        if delta > available {
            return Err(KvCacheError::MemoryQuotaExceeded {
                requested: delta,
                available,
            });
        }
        session.allocated_bytes = session.allocated_bytes.saturating_add(delta);
        self.total_allocated_bytes = self.total_allocated_bytes.saturating_add(delta);
        session.token_count = next_tokens;
        session.kv_history.push(kv_state);
        session.last_active = now;
        Ok(next_tokens)
    }

    #[must_use]
    pub fn get_session(&self, session_id: &str) -> Option<&KvSession> {
        self.sessions.get(session_id)
    }

    pub fn close_session(&mut self, session_id: &str) -> bool {
        if let Some(removed) = self.sessions.remove(session_id) {
            self.total_allocated_bytes = self
                .total_allocated_bytes
                .saturating_sub(removed.allocated_bytes);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_cache_enforces_memory_quota_concurrency_and_idle_ttl() {
        let mut mgr = KvCacheManager::new(KvCacheQuotaConfig {
            max_total_bytes: 10_000,
            max_concurrent_sessions: 2,
            max_tokens_per_session: 4,
            idle_ttl: Duration::from_secs(30),
        });
        let t0 = Instant::now();

        // 2 layers * 500 B/layer/tok = 1000 B/tok, initial 2 tokens = 2000 B
        mgr.open_session("s1", "m1", 0, 2, 500, 2, t0).unwrap();
        mgr.open_session("s2", "m1", 0, 2, 500, 2, t0).unwrap();
        assert_eq!(mgr.total_allocated_bytes(), 4_000);

        // Concurrency cap blocks 3rd session
        assert_eq!(
            mgr.open_session("s3", "m1", 0, 2, 500, 1, t0),
            Err(KvCacheError::ConcurrencyLimitReached(2))
        );

        // Appending steps updates token count and memory
        assert_eq!(mgr.append_step("s1", vec![1.0, 2.0], t0).unwrap(), 1);
        assert_eq!(mgr.append_step("s1", vec![3.0, 4.0], t0).unwrap(), 2);
        assert_eq!(mgr.append_step("s1", vec![5.0, 6.0], t0).unwrap(), 3);
        assert_eq!(mgr.total_allocated_bytes(), 5_000);

        // Idle eviction reclaims expired sessions
        let t_later = t0 + Duration::from_secs(31);
        let evicted = mgr.evict_expired(t_later);
        assert_eq!(evicted.len(), 2);
        assert_eq!(mgr.total_allocated_bytes(), 0);
        assert_eq!(mgr.active_session_count(), 0);
    }
}

//! Swarm Layer Coverage Registry, scarcity-first layer assignment, and
//! lowest-latency inference chain pathfinding.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerTransportMode {
    LanDirect,
    StunDirect,
    RelayFallback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedLayerRange {
    pub worker_id: String,
    pub owner: String,
    pub model_id: String,
    /// Inclusive starting layer index.
    pub start_layer: u32,
    /// Exclusive ending layer index.
    pub end_layer: u32,
    pub rtt_ms: f64,
    pub tokens_per_sec: f64,
    pub active_sessions: u32,
    pub max_sessions: u32,
    pub trust_tier: u8,
    pub transport_mode: PeerTransportMode,
    pub endpoint: String,
    pub healthy: bool,
}

impl HostedLayerRange {
    #[must_use]
    pub fn layer_count(&self) -> u32 {
        self.end_layer.saturating_sub(self.start_layer)
    }

    #[must_use]
    pub fn has_capacity(&self) -> bool {
        self.healthy && self.active_sessions < self.max_sessions
    }

    /// Lower score is better for pathfinding.
    #[must_use]
    pub fn hop_cost(&self) -> f64 {
        let load_penalty = if self.max_sessions == 0 {
            100.0
        } else {
            (f64::from(self.active_sessions) / f64::from(self.max_sessions)) * 40.0
        };
        let transport_penalty = match self.transport_mode {
            PeerTransportMode::LanDirect => 0.0,
            PeerTransportMode::StunDirect => 10.0,
            PeerTransportMode::RelayFallback => 35.0,
        };
        let speed_penalty = if self.tokens_per_sec > 0.0 {
            1000.0 / self.tokens_per_sec
        } else {
            500.0
        };
        self.rtt_ms.max(0.1) + load_penalty + transport_penalty + speed_penalty
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerCoverageSummary {
    pub model_id: String,
    pub total_layers: u32,
    /// Number of healthy replicas covering each layer `0..total_layers`.
    pub per_layer_replicas: Vec<u32>,
    pub min_replicas: u32,
    pub complete: bool,
    pub online_workers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoverageError {
    #[error("model `{0}` has no registered layer topology")]
    UnknownModel(String),
    #[error("no complete inference chain can cover layers [0..{total_layers}) for `{model_id}`")]
    IncompleteChain { model_id: String, total_layers: u32 },
    #[error("manual worker selection `{0}` is unavailable or does not cover required layers")]
    InvalidManualSelection(String),
}

#[derive(Debug, Clone, Default)]
pub struct ChainSelectionFilter {
    /// If `Some(owner)`, restrict all hops strictly to devices owned by `owner` (Tier 0 private).
    pub own_devices_owner: Option<String>,
    /// Maximum allowed trust tier (0 = private only, 1 = private or public).
    pub max_trust_tier: u8,
    /// Exclude specific worker IDs (e.g. failed workers during mid-stream failover).
    pub exclude_workers: HashSet<String>,
    /// Optional explicit ordered list of worker IDs for Manual distributed mode.
    pub manual_worker_ids: Vec<String>,
}

/// Real-time Swarm Layer Coverage Registry tracking every model's hosted layer ranges.
#[derive(Debug, Default)]
pub struct LayerCoverageRegistry {
    model_total_layers: HashMap<String, u32>,
    /// `model_id -> (worker_id -> HostedLayerRange)`
    hosted: HashMap<String, HashMap<String, HostedLayerRange>>,
}

impl LayerCoverageRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_model(&mut self, model_id: impl Into<String>, total_layers: u32) {
        self.model_total_layers
            .insert(model_id.into(), total_layers);
    }

    pub fn upsert_worker_range(&mut self, range: HostedLayerRange) {
        self.hosted
            .entry(range.model_id.clone())
            .or_default()
            .insert(range.worker_id.clone(), range);
    }

    pub fn remove_worker(&mut self, model_id: &str, worker_id: &str) -> bool {
        self.hosted
            .get_mut(model_id)
            .and_then(|workers| workers.remove(worker_id))
            .is_some()
    }

    pub fn mark_worker_health(&mut self, model_id: &str, worker_id: &str, healthy: bool) {
        if let Some(entry) = self
            .hosted
            .get_mut(model_id)
            .and_then(|workers| workers.get_mut(worker_id))
        {
            entry.healthy = healthy;
        }
    }

    pub fn coverage_summary(&self, model_id: &str) -> Result<LayerCoverageSummary, CoverageError> {
        let &total_layers = self
            .model_total_layers
            .get(model_id)
            .ok_or_else(|| CoverageError::UnknownModel(model_id.to_owned()))?;
        let mut counts = vec![0u32; total_layers as usize];
        let mut online_workers = 0usize;
        if let Some(workers) = self.hosted.get(model_id) {
            for w in workers.values().filter(|w| w.healthy) {
                online_workers += 1;
                let end = w.end_layer.min(total_layers);
                for layer in w.start_layer..end {
                    counts[layer as usize] = counts[layer as usize].saturating_add(1);
                }
            }
        }
        let min_replicas = counts.iter().copied().min().unwrap_or(0);
        Ok(LayerCoverageSummary {
            model_id: model_id.to_owned(),
            total_layers,
            per_layer_replicas: counts,
            min_replicas,
            complete: min_replicas >= 1,
            online_workers,
        })
    }

    /// Scarcity-first auto-balancing planner: picks the contiguous range
    /// `[start..end)` of length `min(hostable_layers, total_layers)` that covers
    /// the scarcest layers in the swarm so heterogeneous low-memory devices
    /// automatically assemble complete coverage of large models.
    pub fn suggest_layer_range(
        &self,
        model_id: &str,
        hostable_layers: u32,
    ) -> Result<(u32, u32), CoverageError> {
        let summary = self.coverage_summary(model_id)?;
        let total = summary.total_layers;
        let span = hostable_layers.clamp(1, total) as usize;
        let counts = &summary.per_layer_replicas;

        let mut best_start = 0usize;
        let mut best_key = (u32::MAX, u64::MAX);
        for start in 0..=(total as usize - span) {
            let window = &counts[start..start + span];
            let window_min = window.iter().copied().min().unwrap_or(0);
            let window_sum: u64 = window.iter().map(|&c| u64::from(c)).sum();
            let key = (window_min, window_sum);
            if key < best_key {
                best_key = key;
                best_start = start;
            }
        }
        Ok((best_start as u32, (best_start + span) as u32))
    }

    /// Builds the lowest-cost inference chain covering `[0..total_layers)`.
    /// Supports both automatic DP pathfinding and explicit manual worker chains.
    pub fn build_inference_chain(
        &self,
        model_id: &str,
        filter: &ChainSelectionFilter,
    ) -> Result<Vec<HostedLayerRange>, CoverageError> {
        let &total_layers = self
            .model_total_layers
            .get(model_id)
            .ok_or_else(|| CoverageError::UnknownModel(model_id.to_owned()))?;
        let workers = self
            .hosted
            .get(model_id)
            .ok_or_else(|| CoverageError::IncompleteChain {
                model_id: model_id.to_owned(),
                total_layers,
            })?;

        if !filter.manual_worker_ids.is_empty() {
            let mut chain = Vec::with_capacity(filter.manual_worker_ids.len());
            let mut cursor = 0u32;
            for wid in &filter.manual_worker_ids {
                let w = workers
                    .get(wid)
                    .filter(|w| w.has_capacity() && !filter.exclude_workers.contains(&w.worker_id))
                    .ok_or_else(|| CoverageError::InvalidManualSelection(wid.clone()))?;
                if w.start_layer > cursor || w.end_layer <= cursor {
                    return Err(CoverageError::InvalidManualSelection(wid.clone()));
                }
                let mut hop = w.clone();
                hop.start_layer = cursor;
                hop.end_layer = hop.end_layer.min(total_layers);
                cursor = hop.end_layer;
                chain.push(hop);
            }
            if cursor < total_layers {
                return Err(CoverageError::IncompleteChain {
                    model_id: model_id.to_owned(),
                    total_layers,
                });
            }
            return Ok(chain);
        }

        let eligible: Vec<&HostedLayerRange> = workers
            .values()
            .filter(|w| {
                w.has_capacity()
                    && !filter.exclude_workers.contains(&w.worker_id)
                    && w.trust_tier <= filter.max_trust_tier
                    && filter
                        .own_devices_owner
                        .as_deref()
                        .is_none_or(|owner| w.owner == owner)
            })
            .collect();

        // Shortest-path DP over layers 0..=total_layers.
        let n = total_layers as usize;
        let mut dist = vec![f64::INFINITY; n + 1];
        let mut prev: Vec<Option<(usize, HostedLayerRange)>> = vec![None; n + 1];
        dist[0] = 0.0;

        for pos in 0..n {
            if !dist[pos].is_finite() {
                continue;
            }
            let pos_u32 = pos as u32;
            for w in &eligible {
                if w.start_layer <= pos_u32 && w.end_layer > pos_u32 {
                    let next = (w.end_layer.min(total_layers)) as usize;
                    // Small per-hop overhead (5 ms) encourages fewer hops when latency is equal.
                    let cand = dist[pos] + w.hop_cost() + 5.0;
                    if cand < dist[next] {
                        dist[next] = cand;
                        let mut sliced = (*w).clone();
                        sliced.start_layer = pos_u32;
                        sliced.end_layer = next as u32;
                        prev[next] = Some((pos, sliced));
                    }
                }
            }
        }

        if !dist[n].is_finite() {
            return Err(CoverageError::IncompleteChain {
                model_id: model_id.to_owned(),
                total_layers,
            });
        }

        let mut chain = Vec::new();
        let mut cur = n;
        while cur > 0 {
            let (p, hop) = prev[cur]
                .clone()
                .ok_or_else(|| CoverageError::IncompleteChain {
                    model_id: model_id.to_owned(),
                    total_layers,
                })?;
            chain.push(hop);
            cur = p;
        }
        chain.reverse();
        Ok(chain)
    }

    /// Finds the best replacement worker covering `[start_layer..end_layer)`
    /// during mid-stream failover (excluding failed workers).
    pub fn find_replacement_hop(
        &self,
        model_id: &str,
        start_layer: u32,
        end_layer: u32,
        filter: &ChainSelectionFilter,
    ) -> Option<HostedLayerRange> {
        let workers = self.hosted.get(model_id)?;
        workers
            .values()
            .filter(|w| {
                w.has_capacity()
                    && !filter.exclude_workers.contains(&w.worker_id)
                    && w.start_layer <= start_layer
                    && w.end_layer >= end_layer
                    && w.trust_tier <= filter.max_trust_tier
                    && filter
                        .own_devices_owner
                        .as_deref()
                        .is_none_or(|owner| w.owner == owner)
            })
            .min_by(|a, b| a.hop_cost().total_cmp(&b.hop_cost()))
            .map(|w| {
                let mut hop = w.clone();
                hop.start_layer = start_layer;
                hop.end_layer = end_layer;
                hop
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(id: &str, owner: &str, start: u32, end: u32, rtt_ms: f64) -> HostedLayerRange {
        HostedLayerRange {
            worker_id: id.into(),
            owner: owner.into(),
            model_id: "llama-70b".into(),
            start_layer: start,
            end_layer: end,
            rtt_ms,
            tokens_per_sec: 40.0,
            active_sessions: 0,
            max_sessions: 8,
            trust_tier: 0,
            transport_mode: PeerTransportMode::LanDirect,
            endpoint: format!("10.0.0.1:{start}"),
            healthy: true,
        }
    }

    #[test]
    fn scarcity_planner_assembles_complete_model_from_small_devices() {
        let mut reg = LayerCoverageRegistry::new();
        reg.register_model("llama-70b", 12);

        // Device 1 can hold 4 layers -> assigned [0..4)
        let r1 = reg.suggest_layer_range("llama-70b", 4).unwrap();
        assert_eq!(r1, (0, 4));
        reg.upsert_worker_range(worker("w1", "alice", r1.0, r1.1, 5.0));

        // Device 2 can hold 4 layers -> assigned [4..8)
        let r2 = reg.suggest_layer_range("llama-70b", 4).unwrap();
        assert_eq!(r2, (4, 8));
        reg.upsert_worker_range(worker("w2", "alice", r2.0, r2.1, 5.0));

        // Device 3 can hold 4 layers -> assigned [8..12)
        let r3 = reg.suggest_layer_range("llama-70b", 4).unwrap();
        assert_eq!(r3, (8, 12));
        reg.upsert_worker_range(worker("w3", "alice", r3.0, r3.1, 5.0));

        let summary = reg.coverage_summary("llama-70b").unwrap();
        assert!(summary.complete);
        assert_eq!(summary.min_replicas, 1);

        let chain = reg
            .build_inference_chain("llama-70b", &ChainSelectionFilter::default())
            .unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(
            chain
                .iter()
                .map(|h| h.worker_id.as_str())
                .collect::<Vec<_>>(),
            vec!["w1", "w2", "w3"]
        );
    }
}

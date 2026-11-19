//! Node quality & reputation scoring with automatic down-ranking and quarantine.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReputationPolicy {
    pub ema_alpha: f64,
    /// Quarantine node if verification pass rate falls below this after `min_verifications`.
    pub min_verify_pass_rate: f64,
    pub min_verifications_before_quarantine: u64,
    /// Quarantine node if drop rate exceeds this after `min_requests_before_quarantine`.
    pub max_drop_rate: f64,
    pub min_requests_before_quarantine: u64,
}

impl Default for ReputationPolicy {
    fn default() -> Self {
        Self {
            ema_alpha: 0.2,
            min_verify_pass_rate: 0.85,
            min_verifications_before_quarantine: 5,
            max_drop_rate: 0.35,
            min_requests_before_quarantine: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeReputation {
    pub worker_id: String,
    pub ttft_ema_ms: f64,
    pub tokens_per_sec_ema: f64,
    pub completed_requests: u64,
    pub dropped_requests: u64,
    pub verify_passed: u64,
    pub verify_failed: u64,
    pub reputation_score: f64,
    pub quarantined: bool,
    pub quarantine_reason: Option<String>,
}

impl NodeReputation {
    #[must_use]
    pub fn new(worker_id: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            ttft_ema_ms: 250.0,
            tokens_per_sec_ema: 20.0,
            completed_requests: 0,
            dropped_requests: 0,
            verify_passed: 0,
            verify_failed: 0,
            reputation_score: 0.75,
            quarantined: false,
            quarantine_reason: None,
        }
    }

    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        let total = self.completed_requests + self.dropped_requests;
        if total == 0 {
            0.0
        } else {
            (self.dropped_requests as f64) / (total as f64)
        }
    }

    #[must_use]
    pub fn verify_pass_rate(&self) -> f64 {
        let total = self.verify_passed + self.verify_failed;
        if total == 0 {
            1.0
        } else {
            (self.verify_passed as f64) / (total as f64)
        }
    }

    fn recompute(&mut self, policy: &ReputationPolicy) {
        let reliability = (1.0 - self.drop_rate()).clamp(0.0, 1.0);
        let accuracy = self.verify_pass_rate().clamp(0.0, 1.0);
        // Latency factor: 100ms -> ~0.91, 1000ms -> 0.50
        let latency_factor = 1.0 / (1.0 + (self.ttft_ema_ms.max(1.0) / 1000.0));
        // Throughput factor saturates around 60 tok/s
        let speed_factor = (self.tokens_per_sec_ema / 60.0).clamp(0.1, 1.0);
        self.reputation_score =
            (0.40 * accuracy + 0.30 * reliability + 0.20 * latency_factor + 0.10 * speed_factor)
                .clamp(0.0, 1.0);

        let verify_total = self.verify_passed + self.verify_failed;
        let req_total = self.completed_requests + self.dropped_requests;

        if verify_total >= policy.min_verifications_before_quarantine
            && accuracy < policy.min_verify_pass_rate
        {
            self.quarantined = true;
            self.quarantine_reason = Some(format!(
                "verification pass rate {:.1}% below threshold {:.1}%",
                accuracy * 100.0,
                policy.min_verify_pass_rate * 100.0
            ));
        } else if req_total >= policy.min_requests_before_quarantine
            && self.drop_rate() > policy.max_drop_rate
        {
            self.quarantined = true;
            self.quarantine_reason = Some(format!(
                "drop rate {:.1}% above threshold {:.1}%",
                self.drop_rate() * 100.0,
                policy.max_drop_rate * 100.0
            ));
        }
    }
}

#[derive(Debug, Default)]
pub struct NodeReputationTracker {
    policy: ReputationPolicy,
    nodes: HashMap<String, NodeReputation>,
}

impl NodeReputationTracker {
    #[must_use]
    pub fn new(policy: ReputationPolicy) -> Self {
        Self {
            policy,
            nodes: HashMap::new(),
        }
    }

    pub fn record_completion(&mut self, worker_id: &str, ttft_ms: f64, tokens_per_sec: f64) {
        let entry = self
            .nodes
            .entry(worker_id.to_owned())
            .or_insert_with(|| NodeReputation::new(worker_id));
        let alpha = self.policy.ema_alpha;
        if entry.completed_requests == 0 {
            entry.ttft_ema_ms = ttft_ms.max(1.0);
            entry.tokens_per_sec_ema = tokens_per_sec.max(0.1);
        } else {
            entry.ttft_ema_ms = (1.0 - alpha) * entry.ttft_ema_ms + alpha * ttft_ms.max(1.0);
            entry.tokens_per_sec_ema =
                (1.0 - alpha) * entry.tokens_per_sec_ema + alpha * tokens_per_sec.max(0.1);
        }
        entry.completed_requests += 1;
        entry.recompute(&self.policy);
    }

    pub fn record_drop(&mut self, worker_id: &str) {
        let entry = self
            .nodes
            .entry(worker_id.to_owned())
            .or_insert_with(|| NodeReputation::new(worker_id));
        entry.dropped_requests += 1;
        entry.recompute(&self.policy);
    }

    pub fn record_verification(&mut self, worker_id: &str, passed: bool) {
        let entry = self
            .nodes
            .entry(worker_id.to_owned())
            .or_insert_with(|| NodeReputation::new(worker_id));
        if passed {
            entry.verify_passed += 1;
        } else {
            entry.verify_failed += 1;
        }
        entry.recompute(&self.policy);
    }

    #[must_use]
    pub fn get(&self, worker_id: &str) -> Option<&NodeReputation> {
        self.nodes.get(worker_id)
    }

    /// Returns a multiplier in `[0.0, 1.0]` for router scoring (`0.0` = quarantined).
    #[must_use]
    pub fn routing_weight(&self, worker_id: &str) -> f64 {
        match self.nodes.get(worker_id) {
            Some(rep) if rep.quarantined => 0.0,
            Some(rep) => rep.reputation_score,
            None => 0.75,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malicious_and_flaky_nodes_are_downranked_and_quarantined() {
        let mut tracker = NodeReputationTracker::new(ReputationPolicy::default());
        for _ in 0..10 {
            tracker.record_completion("good", 80.0, 55.0);
            tracker.record_verification("good", true);
        }
        for _ in 0..6 {
            tracker.record_completion("cheater", 80.0, 55.0);
            tracker.record_verification("cheater", false);
        }
        assert!(tracker.routing_weight("good") > 0.9);
        assert_eq!(tracker.routing_weight("cheater"), 0.0);
        assert!(tracker.get("cheater").unwrap().quarantined);
    }
}

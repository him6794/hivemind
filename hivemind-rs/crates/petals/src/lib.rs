//! Petals-style multi-node distributed LLM inference for Hivemind.
//!
//! Provides:
//! - Layer-sharded model manifests (`manifest`)
//! - FP32 / FP16 / BF16 / INT8 dynamic activation compression (`compression`)
//! - Stateful KV-cache session management with quotas & idle TTL (`kv_cache`)
//! - Swarm Layer Coverage Registry & scarcity-first auto-balancing (`coverage`)
//! - Partial-layer inference engine & mid-stream failover with KV-cache
//!   reconstruction (`engine`, `pipeline`)
//! - Proportional per-layer CPT settlement & failure refund (`pipeline`)

pub mod compression;
pub mod coverage;
pub mod engine;
pub mod kv_cache;
pub mod manifest;
pub mod pipeline;

#[cfg(test)]
mod e2e_tests {
    use super::compression::ActivationCodec;
    use super::coverage::{
        ChainSelectionFilter, HostedLayerRange, LayerCoverageRegistry, PeerTransportMode,
    };
    use super::engine::{PartialLayerEngine, ReferencePartialWorker};
    use super::kv_cache::KvCacheQuotaConfig;
    use super::pipeline::{calculate_layer_cpt_split, run_distributed_generation};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn make_range(id: &str, owner: &str, start: u32, end: u32, rtt_ms: f64) -> HostedLayerRange {
        HostedLayerRange {
            worker_id: id.into(),
            owner: owner.into(),
            model_id: "llama-70b-q4".into(),
            start_layer: start,
            end_layer: end,
            rtt_ms,
            tokens_per_sec: 35.0,
            active_sessions: 0,
            max_sessions: 4,
            trust_tier: 0,
            transport_mode: PeerTransportMode::LanDirect,
            endpoint: format!("192.168.1.10:{}", 9000 + start),
            healthy: true,
        }
    }

    #[tokio::test]
    async fn three_node_chain_recovers_mid_stream_via_kv_replay_and_matches_healthy_output() {
        // Baseline: 3 healthy nodes [0..4), [4..8), [8..12)
        let mut baseline_reg = LayerCoverageRegistry::new();
        baseline_reg.register_model("llama-70b-q4", 12);
        baseline_reg.upsert_worker_range(make_range("w1", "alice", 0, 4, 4.0));
        baseline_reg.upsert_worker_range(make_range("w2", "bob", 4, 8, 5.0));
        baseline_reg.upsert_worker_range(make_range("w3", "carol", 8, 12, 6.0));

        let mut baseline_engines: HashMap<String, Arc<dyn PartialLayerEngine>> = HashMap::new();
        baseline_engines.insert(
            "w1".into(),
            Arc::new(ReferencePartialWorker::new("w1", 0, 4, KvCacheQuotaConfig::default())),
        );
        baseline_engines.insert(
            "w2".into(),
            Arc::new(ReferencePartialWorker::new("w2", 4, 8, KvCacheQuotaConfig::default())),
        );
        baseline_engines.insert(
            "w3".into(),
            Arc::new(ReferencePartialWorker::new("w3", 8, 12, KvCacheQuotaConfig::default())),
        );

        let baseline = run_distributed_generation(
            "sess-baseline".into(),
            "llama-70b-q4",
            16,
            6,
            ActivationCodec::Fp16,
            &mut baseline_reg,
            &baseline_engines,
            ChainSelectionFilter::default(),
            None,
        )
        .await
        .expect("baseline 3-node run should succeed");
        assert_eq!(baseline.tokens.len(), 6);
        assert_eq!(baseline.failover_recoveries, 0);

        // Failover run: w2 crashes at step 3; backup node w2-backup covering [4..8)
        // seamlessly reconstructs KV-cache from cached hidden states and completes!
        let mut failover_reg = LayerCoverageRegistry::new();
        failover_reg.register_model("llama-70b-q4", 12);
        failover_reg.upsert_worker_range(make_range("w1", "alice", 0, 4, 4.0));
        failover_reg.upsert_worker_range(make_range("w2", "bob", 4, 8, 5.0));
        failover_reg.upsert_worker_range(make_range("w2-backup", "dave", 4, 8, 12.0));
        failover_reg.upsert_worker_range(make_range("w3", "carol", 8, 12, 6.0));

        let w2_flaky = Arc::new(ReferencePartialWorker::new(
            "w2",
            4,
            8,
            KvCacheQuotaConfig::default(),
        ));
        w2_flaky.inject_fail_at_step(3);

        let mut failover_engines: HashMap<String, Arc<dyn PartialLayerEngine>> = HashMap::new();
        failover_engines.insert(
            "w1".into(),
            Arc::new(ReferencePartialWorker::new("w1", 0, 4, KvCacheQuotaConfig::default())),
        );
        failover_engines.insert("w2".into(), w2_flaky);
        failover_engines.insert(
            "w2-backup".into(),
            Arc::new(ReferencePartialWorker::new(
                "w2-backup",
                4,
                8,
                KvCacheQuotaConfig::default(),
            )),
        );
        failover_engines.insert(
            "w3".into(),
            Arc::new(ReferencePartialWorker::new("w3", 8, 12, KvCacheQuotaConfig::default())),
        );

        let (tx, mut rx) = mpsc::channel(16);
        let recovered = run_distributed_generation(
            "sess-failover".into(),
            "llama-70b-q4",
            16,
            6,
            ActivationCodec::Fp16,
            &mut failover_reg,
            &failover_engines,
            ChainSelectionFilter::default(),
            Some(tx),
        )
        .await
        .expect("mid-stream failover should transparently recover");

        assert_eq!(recovered.failover_recoveries, 1);
        assert_eq!(recovered.final_chain, vec!["w1", "w2-backup", "w3"]);
        assert_eq!(recovered.tokens.len(), 6);

        // Streamed token IDs MUST match the unbroken baseline token-for-token!
        let baseline_ids: Vec<u32> = baseline.tokens.iter().map(|t| t.token_id).collect();
        let recovered_ids: Vec<u32> = recovered.tokens.iter().map(|t| t.token_id).collect();
        assert_eq!(recovered_ids, baseline_ids);

        let mut streamed_count = 0;
        while rx.try_recv().is_ok() {
            streamed_count += 1;
        }
        assert_eq!(streamed_count, 6);

        // Verify exact CPT settlement conservation across all 4 participating workers
        // (w1: 6 steps, w2: 3 steps, w2-backup: 3 steps, w3: 6 steps, each 4 layers).
        let settlement = calculate_layer_cpt_split(1000, 500, &recovered.contributions);
        assert_eq!(settlement.platform_fee_cpt, 50); // 5%
        let sum_shares: i64 = settlement.node_shares.iter().map(|s| s.cpt_earned).sum();
        assert_eq!(sum_shares + settlement.platform_fee_cpt, 1000);
        // w2 and w2-backup served 3 steps each (half of w1/w3's 6 steps).
        let w1_share = settlement
            .node_shares
            .iter()
            .find(|s| s.worker_id == "w1")
            .unwrap()
            .cpt_earned;
        let w2_share = settlement
            .node_shares
            .iter()
            .find(|s| s.worker_id == "w2")
            .unwrap()
            .cpt_earned;
        assert!(w1_share > w2_share);
    }

    #[tokio::test]
    async fn manual_chain_selection_and_int8_compression_work_end_to_end() {
        let mut reg = LayerCoverageRegistry::new();
        reg.register_model("llama-70b-q4", 8);
        reg.upsert_worker_range(make_range("home-pc", "alice", 0, 5, 2.0));
        reg.upsert_worker_range(make_range("home-tablet", "alice", 5, 8, 8.0));

        let mut engines: HashMap<String, Arc<dyn PartialLayerEngine>> = HashMap::new();
        engines.insert(
            "home-pc".into(),
            Arc::new(ReferencePartialWorker::new("home-pc", 0, 5, KvCacheQuotaConfig::default())),
        );
        engines.insert(
            "home-tablet".into(),
            Arc::new(ReferencePartialWorker::new(
                "home-tablet",
                5,
                8,
                KvCacheQuotaConfig::default(),
            )),
        );

        let filter = ChainSelectionFilter {
            own_devices_owner: Some("alice".into()),
            max_trust_tier: 0,
            manual_worker_ids: vec!["home-pc".into(), "home-tablet".into()],
            ..Default::default()
        };

        let report = run_distributed_generation(
            "sess-manual".into(),
            "llama-70b-q4",
            16,
            4,
            ActivationCodec::Int8Dynamic,
            &mut reg,
            &engines,
            filter,
            None,
        )
        .await
        .expect("manual 2-device chain should succeed");

        assert_eq!(report.final_chain, vec!["home-pc", "home-tablet"]);
        assert_eq!(report.tokens.len(), 4);
    }
}

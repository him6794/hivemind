//! Multi-hop distributed LLM inference pipeline with mid-stream failover,
//! KV-cache reconstruction from cached hidden states, and proportional CPT split.

use crate::compression::{ActivationCodec, CompressedActivation};
use crate::coverage::{
    ChainSelectionFilter, CoverageError, HostedLayerRange, LayerCoverageRegistry,
};
use crate::engine::{EngineError, ForwardStepRequest, PartialLayerEngine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCptShare {
    pub worker_id: String,
    pub owner: String,
    pub layers_served: u32,
    pub steps_served: u32,
    pub cpt_earned: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistributedSettlement {
    pub total_billed_cpt: i64,
    pub platform_fee_cpt: i64,
    pub refunded_cpt: i64,
    pub node_shares: Vec<NodeCptShare>,
}

/// Splits `total_billed_cpt` (minus `platform_fee_bps` basis points) across all
/// participating nodes proportionally to `layers_served * steps_served`.
/// Guarantees exact integer conservation (no CPT created or destroyed).
#[must_use]
pub fn calculate_layer_cpt_split(
    total_billed_cpt: i64,
    platform_fee_bps: u32,
    contributions: &[(String, String, u32, u32)], // (worker_id, owner, layers, steps)
) -> DistributedSettlement {
    if total_billed_cpt <= 0 || contributions.is_empty() {
        return DistributedSettlement {
            total_billed_cpt: 0,
            platform_fee_cpt: 0,
            refunded_cpt: total_billed_cpt.max(0),
            node_shares: Vec::new(),
        };
    }
    let fee_bps = i128::from(platform_fee_bps.min(10_000));
    let total_i128 = i128::from(total_billed_cpt);
    let platform_fee_cpt = ((total_i128 * fee_bps) / 10_000) as i64;
    let distributable = total_billed_cpt.saturating_sub(platform_fee_cpt);

    let total_weight: u128 = contributions
        .iter()
        .map(|(_, _, layers, steps)| u128::from(*layers) * u128::from(*steps))
        .sum();
    if total_weight == 0 {
        return DistributedSettlement {
            total_billed_cpt: 0,
            platform_fee_cpt: 0,
            refunded_cpt: total_billed_cpt,
            node_shares: Vec::new(),
        };
    }

    let mut shares = Vec::with_capacity(contributions.len());
    let mut allocated = 0i64;
    for (wid, owner, layers, steps) in contributions {
        let weight = u128::from(*layers) * u128::from(*steps);
        let share = ((i128::from(distributable) * (weight as i128)) / (total_weight as i128)) as i64;
        allocated = allocated.saturating_add(share);
        shares.push(NodeCptShare {
            worker_id: wid.clone(),
            owner: owner.clone(),
            layers_served: *layers,
            steps_served: *steps,
            cpt_earned: share,
        });
    }
    // Distribute any integer rounding remainder deterministically to the largest contributor.
    let remainder = distributable.saturating_sub(allocated);
    if remainder > 0 {
        if let Some(top) = shares
            .iter_mut()
            .max_by_key(|s| u128::from(s.layers_served) * u128::from(s.steps_served))
        {
            top.cpt_earned = top.cpt_earned.saturating_add(remainder);
        }
    }

    DistributedSettlement {
        total_billed_cpt,
        platform_fee_cpt,
        refunded_cpt: 0,
        node_shares: shares,
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedStepToken {
    pub step_index: u32,
    pub token_id: u32,
    pub hidden_norm: f32,
    pub active_workers: Vec<String>,
    pub failover_events: usize,
}

#[derive(Debug, Clone)]
pub struct DistributedGenerationReport {
    pub session_id: String,
    pub tokens: Vec<GeneratedStepToken>,
    pub failover_recoveries: usize,
    pub final_chain: Vec<String>,
    /// `(worker_id, owner, layers, steps)` contributions for CPT split settlement.
    pub contributions: Vec<(String, String, u32, u32)>,
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("coverage error: {0}")]
    Coverage(#[from] CoverageError),
    #[error("engine error: {0}")]
    Engine(#[from] EngineError),
    #[error("unrecoverable failover at hop [{start_layer}..{end_layer}): no healthy replacement worker")]
    NoFailoverReplacement { start_layer: u32, end_layer: u32 },
    #[error("worker engine `{0}` is not connected")]
    WorkerNotConnected(String),
}

/// Runs a multi-hop distributed generation session with transparent mid-stream
/// failover and historical hidden-state KV-cache reconstruction.
#[allow(clippy::too_many_arguments)]
pub async fn run_distributed_generation(
    session_id: String,
    model_id: &str,
    hidden_dim: u32,
    max_new_tokens: u32,
    codec: ActivationCodec,
    registry: &mut LayerCoverageRegistry,
    engines: &HashMap<String, Arc<dyn PartialLayerEngine>>,
    mut filter: ChainSelectionFilter,
    token_tx: Option<mpsc::Sender<GeneratedStepToken>>,
) -> Result<DistributedGenerationReport, PipelineError> {
    let mut chain: Vec<HostedLayerRange> = registry.build_inference_chain(model_id, &filter)?;

    // Cache the exact input activation sent to each hop index at every prior step,
    // enabling instant KV-cache reconstruction if that hop dies mid-stream!
    let mut hop_input_history: Vec<Vec<CompressedActivation>> = vec![Vec::new(); chain.len()];

    // Track per-worker contribution `(owner, layers, steps)` for CPT split.
    let mut contrib_map: HashMap<String, (String, u32, u32)> = HashMap::new();
    let mut tokens = Vec::with_capacity(max_new_tokens as usize);
    let mut failover_recoveries = 0usize;

    // Initial embedding activation vector for step 0.
    let mut prev_token_id: u32 = 1;

    for step_idx in 0..max_new_tokens {
        let embed: Vec<f32> = (0..hidden_dim)
            .map(|d| (((prev_token_id + step_idx + d) % 17) as f32) * 0.05 - 0.4)
            .collect();
        let mut current_act =
            CompressedActivation::encode(codec, vec![1, hidden_dim], &embed)
                .map_err(EngineError::from)?;

        for hop_idx in 0..chain.len() {
            let hop_input = current_act.clone();

            let resp = loop {
                let hop = chain[hop_idx].clone();
                let engine = engines
                    .get(&hop.worker_id)
                    .ok_or_else(|| PipelineError::WorkerNotConnected(hop.worker_id.clone()))?;
                let req = ForwardStepRequest {
                    session_id: session_id.clone(),
                    model_id: model_id.to_owned(),
                    start_layer: hop.start_layer,
                    end_layer: hop.end_layer,
                    step_index: step_idx,
                    activation: hop_input.clone(),
                    output_codec: codec,
                };
                match engine.forward_step(req).await {
                    Ok(ok) => break ok,
                    Err(EngineError::NodeFailed(failed_wid)) => {
                        // Mark failed worker unhealthy and exclude it.
                        registry.mark_worker_health(model_id, &failed_wid, false);
                        filter.exclude_workers.insert(failed_wid);
                        // Find replacement covering [hop.start_layer..hop.end_layer).
                        let replacement = registry
                            .find_replacement_hop(model_id, hop.start_layer, hop.end_layer, &filter)
                            .ok_or(PipelineError::NoFailoverReplacement {
                                start_layer: hop.start_layer,
                                end_layer: hop.end_layer,
                            })?;
                        let repl_engine = engines
                            .get(&replacement.worker_id)
                            .ok_or_else(|| {
                                PipelineError::WorkerNotConnected(replacement.worker_id.clone())
                            })?;
                        // Reconstruct KV-cache on the replacement node from cached hidden states!
                        if !hop_input_history[hop_idx].is_empty() {
                            repl_engine
                                .replay_kv_cache(
                                    session_id.clone(),
                                    model_id.to_owned(),
                                    replacement.start_layer,
                                    replacement.end_layer,
                                    hop_input_history[hop_idx].clone(),
                                    codec,
                                )
                                .await?;
                        }
                        chain[hop_idx] = replacement;
                        failover_recoveries += 1;
                    }
                    Err(other) => return Err(PipelineError::Engine(other)),
                }
            };

            hop_input_history[hop_idx].push(hop_input);
            let hop = &chain[hop_idx];
            let entry = contrib_map
                .entry(hop.worker_id.clone())
                .or_insert_with(|| (hop.owner.clone(), hop.layer_count(), 0));
            entry.2 += 1;
            current_act = resp.activation;
        }

        // LM head projection (deterministic argmax/hash over final hidden state).
        let final_hidden = current_act.decode().map_err(EngineError::from)?;
        let norm: f32 = final_hidden.iter().map(|v| v * v).sum::<f32>().sqrt();
        let token_id = ((norm * 1000.0).round() as u32) % 32_000;
        prev_token_id = token_id;

        let step_token = GeneratedStepToken {
            step_index: step_idx,
            token_id,
            hidden_norm: norm,
            active_workers: chain.iter().map(|h| h.worker_id.clone()).collect(),
            failover_events: failover_recoveries,
        };
        if let Some(tx) = &token_tx {
            let _ = tx.send(step_token.clone()).await;
        }
        tokens.push(step_token);
    }

    let mut contributions: Vec<(String, String, u32, u32)> = contrib_map
        .into_iter()
        .map(|(wid, (owner, layers, steps))| (wid, owner, layers, steps))
        .collect();
    contributions.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(DistributedGenerationReport {
        session_id,
        tokens,
        failover_recoveries,
        final_chain: chain.into_iter().map(|h| h.worker_id).collect(),
        contributions,
    })
}

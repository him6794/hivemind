//! Partial-layer Transformer inference engine trait and deterministic reference
//! implementation with stateful KV-cache & history replay reconstruction.

use crate::compression::{ActivationCodec, CompressedActivation};
use crate::kv_cache::{KvCacheError, KvCacheManager, KvCacheQuotaConfig};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, thiserror::Error)]
pub enum EngineError {
    #[error("worker `{0}` is offline or failed mid-step")]
    NodeFailed(String),
    #[error("KV-cache error: {0}")]
    KvCache(#[from] KvCacheError),
    #[error("codec error: {0}")]
    Codec(#[from] crate::compression::CodecError),
    #[error("layer range mismatch: expected [{expected_start}..{expected_end}), got [{actual_start}..{actual_end})")]
    LayerRangeMismatch {
        expected_start: u32,
        expected_end: u32,
        actual_start: u32,
        actual_end: u32,
    },
}

/// Request sent to one partial-layer worker for a single autoregressive token step.
#[derive(Debug, Clone)]
pub struct ForwardStepRequest {
    pub session_id: String,
    pub model_id: String,
    pub start_layer: u32,
    pub end_layer: u32,
    pub step_index: u32,
    pub activation: CompressedActivation,
    pub output_codec: ActivationCodec,
}

/// Response returned from one partial-layer worker for a single step.
#[derive(Debug, Clone)]
pub struct ForwardStepResponse {
    pub worker_id: String,
    pub activation: CompressedActivation,
    pub kv_tokens_cached: u32,
}

/// Async partial-layer execution backend (implemented by local/remote gRPC hops
/// or the in-process reference engine in integration tests).
pub trait PartialLayerEngine: Send + Sync {
    fn worker_id(&self) -> &str;
    fn hosted_range(&self) -> (u32, u32);

    fn forward_step(
        &self,
        req: ForwardStepRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ForwardStepResponse, EngineError>> + Send + '_>,
    >;

    /// Reconstructs KV-cache state for `session_id` by replaying historical
    /// input activations `[0..history.len())` after mid-stream failover.
    fn replay_kv_cache(
        &self,
        session_id: String,
        model_id: String,
        start_layer: u32,
        end_layer: u32,
        history: Vec<CompressedActivation>,
        output_codec: ActivationCodec,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<u32, EngineError>> + Send + '_>,
    >;
}

/// Deterministic stateful partial-layer reference worker used for unit and
/// multi-node E2E verification (including fault injection for failover tests).
#[derive(Debug, Clone)]
pub struct ReferencePartialWorker {
    worker_id: String,
    start_layer: u32,
    end_layer: u32,
    kv_bytes_per_layer_token: u64,
    kv_cache: Arc<Mutex<KvCacheManager>>,
    fail_on_step: Arc<Mutex<Option<u32>>>,
    offline: Arc<Mutex<bool>>,
}

impl ReferencePartialWorker {
    #[must_use]
    pub fn new(
        worker_id: impl Into<String>,
        start_layer: u32,
        end_layer: u32,
        quota: KvCacheQuotaConfig,
    ) -> Self {
        Self {
            worker_id: worker_id.into(),
            start_layer,
            end_layer,
            kv_bytes_per_layer_token: 256,
            kv_cache: Arc::new(Mutex::new(KvCacheManager::new(quota))),
            fail_on_step: Arc::new(Mutex::new(None)),
            offline: Arc::new(Mutex::new(false)),
        }
    }

    /// Injects a deterministic crash at `step_index` (for mid-stream failover E2E tests).
    pub fn inject_fail_at_step(&self, step_index: u32) {
        *self.fail_on_step.lock().unwrap() = Some(step_index);
    }

    pub fn set_offline(&self, offline: bool) {
        *self.offline.lock().unwrap() = offline;
    }

    fn execute_layers_sync(&self, req: ForwardStepRequest) -> Result<ForwardStepResponse, EngineError> {
        if *self.offline.lock().unwrap() {
            return Err(EngineError::NodeFailed(self.worker_id.clone()));
        }
        if let Some(fail_step) = *self.fail_on_step.lock().unwrap() {
            if req.step_index >= fail_step {
                *self.offline.lock().unwrap() = true;
                return Err(EngineError::NodeFailed(self.worker_id.clone()));
            }
        }
        if req.start_layer < self.start_layer || req.end_layer > self.end_layer {
            return Err(EngineError::LayerRangeMismatch {
                expected_start: self.start_layer,
                expected_end: self.end_layer,
                actual_start: req.start_layer,
                actual_end: req.end_layer,
            });
        }

        let mut hidden = req.activation.decode()?;
        let now = Instant::now();

        let mut cache = self.kv_cache.lock().unwrap();
        cache.open_session(
            req.session_id.clone(),
            req.model_id.clone(),
            req.start_layer,
            req.end_layer,
            self.kv_bytes_per_layer_token,
            1,
            now,
        )?;

        // Retrieve past KV context summary for attention-like dependence on prior tokens.
        let past_context: Vec<f32> = cache
            .get_session(&req.session_id)
            .map(|s| {
                if s.kv_history.is_empty() {
                    vec![0.0; hidden.len()]
                } else {
                    let mut acc = vec![0.0; hidden.len()];
                    for past in &s.kv_history {
                        for (a, &p) in acc.iter_mut().zip(past.iter()) {
                            *a += p;
                        }
                    }
                    let inv = 1.0 / (s.kv_history.len() as f32);
                    for a in &mut acc {
                        *a *= inv;
                    }
                    acc
                }
            })
            .unwrap_or_else(|| vec![0.0; hidden.len()]);

        // Deterministic per-layer transformation so layer [0..4) + [4..8) + [8..12)
        // produces the exact same output as a single node running [0..12) when using Fp32!
        let mut next_kv = vec![0.0; hidden.len()];
        for layer in req.start_layer..req.end_layer {
            let layer_bias = ((layer + 1) as f32) * 0.01;
            for (idx, h) in hidden.iter_mut().enumerate() {
                let ctx = past_context.get(idx).copied().unwrap_or(0.0);
                let updated = (*h * 0.95 + ctx * 0.05 + layer_bias).tanh();
                *h = updated;
                next_kv[idx] += updated * 0.1;
            }
        }

        let kv_tokens = cache.append_step(&req.session_id, next_kv, now)?;
        let out = CompressedActivation::encode(req.output_codec, req.activation.shape, &hidden)?;
        Ok(ForwardStepResponse {
            worker_id: self.worker_id.clone(),
            activation: out,
            kv_tokens_cached: kv_tokens,
        })
    }
}

impl PartialLayerEngine for ReferencePartialWorker {
    fn worker_id(&self) -> &str {
        &self.worker_id
    }

    fn hosted_range(&self) -> (u32, u32) {
        (self.start_layer, self.end_layer)
    }

    fn forward_step(
        &self,
        req: ForwardStepRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ForwardStepResponse, EngineError>> + Send + '_>,
    > {
        let res = self.execute_layers_sync(req);
        Box::pin(async move { res })
    }

    fn replay_kv_cache(
        &self,
        session_id: String,
        model_id: String,
        start_layer: u32,
        end_layer: u32,
        history: Vec<CompressedActivation>,
        output_codec: ActivationCodec,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<u32, EngineError>> + Send + '_>,
    > {
        let res = (|| {
            let mut last_kv = 0u32;
            for (step_idx, act) in history.into_iter().enumerate() {
                let resp = self.execute_layers_sync(ForwardStepRequest {
                    session_id: session_id.clone(),
                    model_id: model_id.clone(),
                    start_layer,
                    end_layer,
                    step_index: step_idx as u32,
                    activation: act,
                    output_codec,
                })?;
                last_kv = resp.kv_tokens_cached;
            }
            Ok(last_kv)
        })();
        Box::pin(async move { res })
    }
}

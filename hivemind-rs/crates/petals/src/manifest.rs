//! Layer-sharded model manifest for Petals-style distributed LLM inference.
//!
//! A single logical model is split into contiguous Transformer layer shards
//! (`[start_layer..end_layer)`), plus explicit `embedding` and `lm_head` roles.
//! Each device downloads and verifies only the shard range it hosts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const LAYER_MANIFEST_SCHEMA: &str = "hivemind-petals-manifest/v1";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("invalid manifest field `{field}`: {reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("shard range [{start}..{end}) is out of bounds for {total_layers} layers")]
    RangeOutOfBounds {
        start: u32,
        end: u32,
        total_layers: u32,
    },
    #[error("layers are not completely covered: missing layer {0}")]
    IncompleteCoverage(u32),
    #[error("shard sha256 mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
}

/// Metadata for one downloadable weight shard (either a single layer or a
/// contiguous layer block).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerShardSpec {
    pub shard_id: String,
    /// Inclusive starting Transformer layer index.
    pub start_layer: u32,
    /// Exclusive ending Transformer layer index.
    pub end_layer: u32,
    pub includes_embedding: bool,
    pub includes_lm_head: bool,
    pub size_bytes: u64,
    pub ram_bytes: u64,
    pub sha256: String,
}

impl LayerShardSpec {
    #[must_use]
    pub fn layer_count(&self) -> u32 {
        self.end_layer.saturating_sub(self.start_layer)
    }

    pub fn verify_bytes(&self, bytes: &[u8]) -> Result<(), ManifestError> {
        let actual = hex::encode(Sha256::digest(bytes));
        if actual != self.sha256 {
            return Err(ManifestError::HashMismatch {
                expected: self.sha256.clone(),
                actual,
            });
        }
        Ok(())
    }
}

/// Complete layer-sharded manifest for a distributed LLM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerShardManifest {
    pub schema: String,
    pub model_id: String,
    pub display_name: String,
    pub total_layers: u32,
    pub hidden_dim: u32,
    pub num_kv_heads: u32,
    pub head_dim: u32,
    pub context_length: u32,
    pub quantization: String,
    /// Estimated KV-cache bytes per layer per token (K + V).
    pub kv_bytes_per_layer_token: u64,
    /// Estimated weight memory per layer in bytes.
    pub bytes_per_layer: u64,
    pub shards: Vec<LayerShardSpec>,
}

impl LayerShardManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema != LAYER_MANIFEST_SCHEMA {
            return Err(ManifestError::Invalid {
                field: "schema",
                reason: format!("expected `{LAYER_MANIFEST_SCHEMA}`"),
            });
        }
        if self.model_id.trim().is_empty() {
            return Err(ManifestError::Invalid {
                field: "model_id",
                reason: "must not be empty".into(),
            });
        }
        if self.total_layers == 0 {
            return Err(ManifestError::Invalid {
                field: "total_layers",
                reason: "must be >= 1".into(),
            });
        }
        if self.hidden_dim == 0 || self.context_length == 0 {
            return Err(ManifestError::Invalid {
                field: "hidden_dim/context_length",
                reason: "must be >= 1".into(),
            });
        }
        if self.shards.is_empty() {
            return Err(ManifestError::Invalid {
                field: "shards",
                reason: "must contain at least one shard".into(),
            });
        }

        let mut covered = vec![false; self.total_layers as usize];
        let mut has_embed = false;
        let mut has_lm_head = false;
        for shard in &self.shards {
            if shard.start_layer >= shard.end_layer || shard.end_layer > self.total_layers {
                return Err(ManifestError::RangeOutOfBounds {
                    start: shard.start_layer,
                    end: shard.end_layer,
                    total_layers: self.total_layers,
                });
            }
            if shard.sha256.len() != 64 || !shard.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(ManifestError::Invalid {
                    field: "shards[].sha256",
                    reason: "must be 64 hex characters".into(),
                });
            }
            if shard.includes_embedding {
                has_embed = true;
            }
            if shard.includes_lm_head {
                has_lm_head = true;
            }
            for layer in shard.start_layer..shard.end_layer {
                covered[layer as usize] = true;
            }
        }
        if let Some((missing, _)) = covered.iter().enumerate().find(|(_, c)| !**c) {
            return Err(ManifestError::IncompleteCoverage(missing as u32));
        }
        if !has_embed {
            return Err(ManifestError::Invalid {
                field: "shards",
                reason: "at least one shard must include embedding weights".into(),
            });
        }
        if !has_lm_head {
            return Err(ManifestError::Invalid {
                field: "shards",
                reason: "at least one shard must include lm_head weights".into(),
            });
        }
        Ok(())
    }

    /// Computes how many contiguous layers a host with `available_memory_bytes`
    /// can safely host (reserving 25% headroom for KV-cache and runtime overhead).
    #[must_use]
    pub fn max_hostable_layers(&self, available_memory_bytes: u64) -> u32 {
        if self.bytes_per_layer == 0 {
            return 0;
        }
        let usable = available_memory_bytes.saturating_mul(3) / 4;
        let layers = usable / self.bytes_per_layer;
        u32::try_from(layers)
            .unwrap_or(self.total_layers)
            .min(self.total_layers)
    }

    /// Returns the shard specs needed for a device assigned `[start_layer..end_layer)`.
    #[must_use]
    pub fn shards_for_range(&self, start_layer: u32, end_layer: u32) -> Vec<&LayerShardSpec> {
        self.shards
            .iter()
            .filter(|s| s.end_layer > start_layer && s.start_layer < end_layer)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest(total_layers: u32) -> LayerShardManifest {
        let shards = (0..total_layers)
            .map(|i| LayerShardSpec {
                shard_id: format!("layer-{i:03}"),
                start_layer: i,
                end_layer: i + 1,
                includes_embedding: i == 0,
                includes_lm_head: i + 1 == total_layers,
                size_bytes: 512 << 20,
                ram_bytes: 600 << 20,
                sha256: "a".repeat(64),
            })
            .collect();
        LayerShardManifest {
            schema: LAYER_MANIFEST_SCHEMA.to_owned(),
            model_id: "llama-70b-q4".to_owned(),
            display_name: "Llama 70B Q4".to_owned(),
            total_layers,
            hidden_dim: 8192,
            num_kv_heads: 8,
            head_dim: 128,
            context_length: 8192,
            quantization: "Q4_K_M".to_owned(),
            kv_bytes_per_layer_token: 4096,
            bytes_per_layer: 512 << 20,
            shards,
        }
    }

    #[test]
    fn valid_manifest_passes_and_calculates_hostable_layers() {
        let m = sample_manifest(80);
        m.validate().expect("manifest should validate");
        // 4 GiB available -> 3 GiB usable -> 6 layers of 512 MiB
        assert_eq!(m.max_hostable_layers(4 << 30), 6);
        assert_eq!(m.shards_for_range(10, 16).len(), 6);
    }

    #[test]
    fn missing_layer_is_rejected() {
        let mut m = sample_manifest(4);
        m.shards.remove(2);
        assert_eq!(m.validate(), Err(ManifestError::IncompleteCoverage(2)));
    }

    #[test]
    fn shard_bytes_hash_verification() {
        let payload = b"layer-weights-bytes";
        let digest = hex::encode(Sha256::digest(payload));
        let shard = LayerShardSpec {
            shard_id: "layer-000".into(),
            start_layer: 0,
            end_layer: 1,
            includes_embedding: true,
            includes_lm_head: true,
            size_bytes: payload.len() as u64,
            ram_bytes: 1024,
            sha256: digest,
        };
        shard.verify_bytes(payload).expect("matching hash");
        assert!(matches!(
            shard.verify_bytes(b"corrupted"),
            Err(ManifestError::HashMismatch { .. })
        ));
    }
}

//! Task-bound transport envelope for CPU hcode tensor inputs.
//!
//! The complete envelope remains the consensus input. Only `input` is passed to
//! the interpreter; artifact bytes are uploaded separately to Nodepool.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ArtifactManifest, ArtifactRole};

pub const ENVELOPE_KEY: &str = "$hcode_tensor_inputs";
pub const MAX_ARTIFACTS: usize = 16;
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub const CHUNK_BYTES: usize = 1024 * 1024;
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const OWNER_STORAGE_QUOTA_BYTES: i64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TensorInputEnvelope {
    #[serde(rename = "$hcode_tensor_inputs")]
    pub protocol: u8,
    pub input: Value,
    pub artifacts: Vec<ArtifactManifest>,
}

impl TensorInputEnvelope {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != 1 || self.artifacts.is_empty() || self.artifacts.len() > MAX_ARTIFACTS {
            return Err("invalid hcode tensor input protocol or artifact count".into());
        }
        let mut names = std::collections::BTreeSet::new();
        let mut total = 0_u64;
        for artifact in &self.artifacts {
            artifact.validate()?;
            if !names.insert(&artifact.artifact_id)
                || artifact.role != ArtifactRole::Input
                || artifact.mime_type != "application/x-safetensors"
                || artifact.inline_bytes.is_some()
                || artifact.size_bytes < 10
                || artifact.size_bytes > MAX_FILE_BYTES
                || artifact.chunks.is_empty()
                || artifact.chunks.len() as u64 > MAX_FILE_BYTES / CHUNK_BYTES as u64
                || artifact
                    .chunks
                    .iter()
                    .any(|chunk| chunk.size_bytes > CHUNK_BYTES as u64)
            {
                return Err("invalid hcode tensor artifact manifest".into());
            }
            total = total
                .checked_add(artifact.size_bytes)
                .ok_or("hcode tensor artifact total overflows")?;
        }
        if total > MAX_TOTAL_BYTES {
            return Err("hcode tensor artifacts exceed the total byte limit".into());
        }
        Ok(())
    }

    pub fn to_input_json(&self) -> Result<String, String> {
        self.validate()?;
        let json = serde_json::to_string(self).map_err(|error| error.to_string())?;
        if json.len() > MAX_INPUT_BYTES {
            return Err("hcode tensor input envelope exceeds the JSON byte limit".into());
        }
        Ok(json)
    }
}

/// Ordinary JSON inputs are unchanged. Presence of the reserved envelope key
/// opts in; malformed envelopes are never reinterpreted as ordinary user input.
pub fn parse_input(input: &str) -> Result<Option<TensorInputEnvelope>, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("hcode JSON input exceeds the byte limit".into());
    }
    if input.trim().is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(input).map_err(|error| error.to_string())?;
    if value.get(ENVELOPE_KEY).is_none() {
        return Ok(None);
    }
    // Parse the original bytes, not Value, so duplicate envelope fields fail.
    let envelope: TensorInputEnvelope = serde_json::from_str(input)
        .map_err(|error| format!("invalid hcode tensor input envelope: {error}"))?;
    envelope.validate()?;
    Ok(Some(envelope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArtifactChunk, sha256_digest};

    fn envelope() -> TensorInputEnvelope {
        TensorInputEnvelope {
            protocol: 1,
            input: serde_json::json!([1, 2]),
            artifacts: vec![ArtifactManifest {
                artifact_id: "model".into(),
                role: ArtifactRole::Input,
                size_bytes: 10,
                mime_type: "application/x-safetensors".into(),
                sha256: sha256_digest(&[0; 10]),
                chunks: vec![ArtifactChunk {
                    offset: 0,
                    size_bytes: 10,
                    sha256: sha256_digest(&[0; 10]),
                }],
                inline_bytes: None,
            }],
        }
    }

    #[test]
    fn ordinary_input_is_not_reinterpreted() {
        for json in ["", "  ", "null", "[1,2]", r#"{"artifacts":[]}"#] {
            assert!(parse_input(json).unwrap().is_none());
        }
        let json = envelope().to_input_json().unwrap();
        assert_eq!(
            parse_input(&json).unwrap().unwrap().input,
            serde_json::json!([1, 2])
        );
        assert!(parse_input(r#"{"$hcode_tensor_inputs":2}"#).is_err());
        assert!(parse_input(&json.replacen('{', r#"{"input":null,"#, 1)).is_err());
    }

    #[test]
    fn invalid_or_unbounded_manifests_fail_closed() {
        let good = envelope();
        for change in 0..8 {
            let mut bad = good.clone();
            match change {
                0 => bad.artifacts[0].artifact_id = "../secret".into(),
                1 => bad.artifacts.push(bad.artifacts[0].clone()),
                2 => bad.artifacts[0].inline_bytes = Some(vec![0; 10]),
                3 => bad.artifacts[0].chunks.clear(),
                4 => bad.artifacts[0].chunks[0].sha256 = "unknown".into(),
                5 => bad.artifacts[0].role = ArtifactRole::Output,
                6 => bad.artifacts[0].size_bytes = MAX_FILE_BYTES + 1,
                _ => bad.artifacts[0].mime_type = "application/octet-stream".into(),
            }
            assert!(bad.validate().is_err(), "case {change}");
        }
    }
}

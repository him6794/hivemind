//! Versioned contracts for the `general-compute-v1` runtime.
//!
//! This crate deliberately owns data contracts only.  It has no dependency on
//! the Hivemind database or scheduler, so a worker supervisor and the trusted
//! control plane can share the same validation and serialization rules.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const GENERAL_COMPUTE_RUNTIME_VERSION: &str = "general-compute-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralComputeRequest {
    pub runtime_version: String,
    pub guest_image_digest: String,
    pub backend_id: String,
    pub entrypoint: String,
    pub source_artifact: ArtifactManifest,
    pub input_artifacts: Vec<ArtifactManifest>,
    pub execution_policy: ExecutionPolicy,
    pub determinism: DeterminismPolicy,
    pub billing_version: String,
    pub cost_model_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralComputeResult {
    pub status: ResultStatus,
    pub exit_code: Option<i32>,
    pub error_code: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub output_artifacts: Vec<ArtifactManifest>,
    pub usage: UsageClaim,
    pub runtime_version: String,
    pub backend_id: String,
    pub guest_image_digest: String,
    pub input_sha256: String,
    pub determinism: DeterminismPolicy,
    pub capability_summary: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    ResourceExhausted,
    BackendUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPolicy {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub wall_time_ms: u64,
    pub max_processes: u32,
    pub max_threads: u32,
    pub scratch_bytes: u64,
    pub output_bytes: u64,
    pub network_allowed: bool,
    pub filesystem_read_only: bool,
    pub gpu_required: bool,
    pub cancellation_deadline_ms: Option<u64>,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            cpu_millis: 60_000,
            memory_bytes: 512 * 1024 * 1024,
            wall_time_ms: 120_000,
            max_processes: 1,
            max_threads: 1,
            scratch_bytes: 256 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            network_allowed: false,
            filesystem_read_only: true,
            gpu_required: false,
            cancellation_deadline_ms: Some(5_000),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeterminismPolicy {
    pub seed: Option<u64>,
    pub thread_count: u32,
    pub cpu_feature_set: String,
    pub reproducibility: Reproducibility,
}

impl Default for DeterminismPolicy {
    fn default() -> Self {
        Self {
            seed: Some(0),
            thread_count: 1,
            cpu_feature_set: "baseline".into(),
            reproducibility: Reproducibility::Reproducible,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reproducibility {
    Reproducible,
    BestEffort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UsageClaim {
    pub cpu_time_ms: u64,
    pub wall_time_ms: u64,
    pub peak_memory_bytes: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub gpu_time_ms: u64,
    pub gpu_memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    Source,
    Input,
    Output,
    Stderr,
    Stdout,
    Checkpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactChunk {
    pub offset: u64,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub artifact_id: String,
    pub role: ArtifactRole,
    pub size_bytes: u64,
    pub mime_type: String,
    pub sha256: String,
    pub chunks: Vec<ArtifactChunk>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_bytes: Option<Vec<u8>>,
}

impl ArtifactManifest {
    pub fn inline_json(artifact_id: impl Into<String>, role: ArtifactRole, bytes: &[u8]) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            role,
            size_bytes: bytes.len() as u64,
            mime_type: "application/json".into(),
            sha256: sha256_digest(bytes),
            chunks: Vec::new(),
            inline_bytes: Some(bytes.to_vec()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.artifact_id.trim().is_empty() {
            return Err("artifact id must not be empty".into());
        }

        if let Some(bytes) = &self.inline_bytes {
            if self.size_bytes != bytes.len() as u64 {
                return Err("artifact size does not match bytes".into());
            }
            if self.sha256 != sha256_digest(bytes) {
                return Err("artifact checksum does not match bytes".into());
            }
        }

        let mut previous_end = 0u64;
        for chunk in &self.chunks {
            if chunk.size_bytes == 0 {
                return Err("artifact chunk size must be positive".into());
            }
            if chunk.offset < previous_end {
                return Err("artifact chunks overlap or are out of order".into());
            }
            let end = chunk
                .offset
                .checked_add(chunk.size_bytes)
                .ok_or_else(|| "artifact chunk range overflows".to_string())?;
            if end > self.size_bytes {
                return Err("artifact chunk exceeds artifact size".into());
            }
            if !is_sha256_digest(&chunk.sha256) {
                return Err("artifact chunk checksum is invalid".into());
            }
            previous_end = end;
        }

        if !is_sha256_digest(&self.sha256) {
            return Err("artifact checksum is invalid".into());
        }
        Ok(())
    }
}

fn sha256_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

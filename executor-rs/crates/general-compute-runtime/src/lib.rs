//! Versioned contracts for the `general-compute-v1alpha1` runtime.
//!
//! This crate deliberately owns data contracts only.  It has no dependency on
//! the Hivemind database or scheduler, so a worker supervisor and the trusted
//! control plane can share the same validation and serialization rules.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod cp_python;
pub mod differential;
pub mod reference;
pub mod supervisor;

pub const GENERAL_COMPUTE_RUNTIME_VERSION: &str = "general-compute-v1alpha1";
pub const MAX_CPU_MILLIS: u64 = 24 * 60 * 60 * 1000;
pub const MAX_MEMORY_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const MAX_WALL_TIME_MS: u64 = 7 * 24 * 60 * 60 * 1000;
pub const MAX_PROCESSES: u32 = 256;
pub const MAX_THREADS: u32 = 4096;
pub const MAX_SCRATCH_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_PROTOCOL_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    PayloadTooLarge,
    Truncated,
    InvalidJson,
}

pub fn encode_frame<T: Serialize>(value: &T, max_payload_bytes: usize) -> Result<Vec<u8>, ProtocolError> {
    let payload = serde_json::to_vec(value).map_err(|_| ProtocolError::InvalidJson)?;
    if payload.len() > max_payload_bytes || payload.len() > u32::MAX as usize {
        return Err(ProtocolError::PayloadTooLarge);
    }

    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame<T: for<'de> Deserialize<'de>>(
    input: &[u8],
    max_payload_bytes: usize,
) -> Result<(T, usize), ProtocolError> {
    if input.len() < 4 {
        return Err(ProtocolError::Truncated);
    }

    let mut length_bytes = [0u8; 4];
    length_bytes.copy_from_slice(&input[..4]);
    let payload_len = u32::from_be_bytes(length_bytes) as usize;
    if payload_len > max_payload_bytes {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let frame_len = 4usize.checked_add(payload_len).ok_or(ProtocolError::PayloadTooLarge)?;
    if input.len() < frame_len {
        return Err(ProtocolError::Truncated);
    }

    let value = serde_json::from_slice(&input[4..frame_len]).map_err(|_| ProtocolError::InvalidJson)?;
    Ok((value, frame_len))
}

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

impl GeneralComputeRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.runtime_version != GENERAL_COMPUTE_RUNTIME_VERSION {
            return Err(ValidationError::new(
                ValidationErrorCode::RuntimeVersionMismatch,
                "unsupported general-compute runtime version",
            ));
        }
        if self.backend_id.trim().is_empty()
            || self.entrypoint.trim().is_empty()
            || !is_sha256_digest(&self.guest_image_digest)
        {
            return Err(ValidationError::new(
                ValidationErrorCode::RequestInvalid,
                "runtime, backend, entrypoint, and guest image digest are required",
            ));
        }

        self.execution_policy.validate()?;
        if !self.execution_policy.filesystem_read_only {
            return Err(ValidationError::new(
                ValidationErrorCode::FilesystemPolicyViolation,
                "general-compute requests require a read-only host filesystem",
            ));
        }

        if self.source_artifact.role != ArtifactRole::Source {
            return Err(ValidationError::new(
                ValidationErrorCode::ArtifactInvalid,
                "source artifact has the wrong role",
            ));
        }
        if let Err(message) = self.source_artifact.validate() {
            return Err(ValidationError::new(ValidationErrorCode::ArtifactInvalid, message));
        }
        for artifact in &self.input_artifacts {
            if artifact.role != ArtifactRole::Input {
                return Err(ValidationError::new(
                    ValidationErrorCode::ArtifactInvalid,
                    "input artifact has the wrong role",
                ));
            }
            if let Err(message) = artifact.validate() {
                return Err(ValidationError::new(ValidationErrorCode::ArtifactInvalid, message));
            }
        }

        if self.billing_version.trim().is_empty() || self.cost_model_version.trim().is_empty() {
            return Err(ValidationError::new(
                ValidationErrorCode::RequestInvalid,
                "billing and cost model versions are required",
            ));
        }
        Ok(())
    }
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

impl ExecutionPolicy {
    fn validate(&self) -> Result<(), ValidationError> {
        let finite_and_positive = [
            (self.cpu_millis, MAX_CPU_MILLIS, "cpu quota"),
            (self.memory_bytes, MAX_MEMORY_BYTES, "memory limit"),
            (self.wall_time_ms, MAX_WALL_TIME_MS, "wall-time limit"),
            (self.scratch_bytes, MAX_SCRATCH_BYTES, "scratch limit"),
            (self.output_bytes, MAX_OUTPUT_BYTES, "output limit"),
        ];
        if finite_and_positive
            .iter()
            .any(|(value, maximum, _)| *value == 0 || *value > *maximum)
        {
            return Err(ValidationError::new(
                ValidationErrorCode::PolicyInvalid,
                "execution quotas must be finite, positive, and within the runtime limits",
            ));
        }
        if self.max_processes == 0
            || self.max_processes > MAX_PROCESSES
            || self.max_threads == 0
            || self.max_threads > MAX_THREADS
        {
            return Err(ValidationError::new(
                ValidationErrorCode::PolicyInvalid,
                "process and thread limits must be finite and within the runtime limits",
            ));
        }
        if self
            .cancellation_deadline_ms
            .is_some_and(|deadline| deadline == 0 || deadline > self.wall_time_ms)
        {
            return Err(ValidationError::new(
                ValidationErrorCode::PolicyInvalid,
                "cancellation deadline must be positive and no later than wall time",
            ));
        }
        Ok(())
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationErrorCode {
    RequestInvalid,
    RuntimeVersionMismatch,
    PolicyInvalid,
    FilesystemPolicyViolation,
    ArtifactInvalid,
    BackendUnavailable,
    GuestImageMismatch,
    NetworkDenied,
    GpuUnavailable,
    CapabilityMissing,
    ThreadLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: ValidationErrorCode,
    pub message: String,
}

impl ValidationError {
    fn new(code: ValidationErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendRegistration {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub capabilities: Vec<String>,
    pub max_threads: u32,
    pub network_allowed: bool,
    pub filesystem_read_only: bool,
    pub gpu_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCapabilities {
    pub guest_image_digests: Vec<String>,
    pub capabilities: Vec<String>,
    pub max_threads: u32,
    pub gpu_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CapabilityMatrix {
    pub backends: Vec<BackendRegistration>,
}

impl CapabilityMatrix {
    pub fn new(backends: Vec<BackendRegistration>) -> Self {
        Self { backends }
    }

    pub fn validate_request(
        &self,
        request: &GeneralComputeRequest,
        worker: &WorkerCapabilities,
    ) -> Result<(), ValidationError> {
        request.validate()?;

        let Some(backend) = self
            .backends
            .iter()
            .find(|backend| backend.backend_id == request.backend_id)
        else {
            return Err(ValidationError::new(
                ValidationErrorCode::BackendUnavailable,
                "requested backend is not registered",
            ));
        };

        if backend.guest_image_digest != request.guest_image_digest
            || !worker
                .guest_image_digests
                .iter()
                .any(|digest| digest == &request.guest_image_digest)
        {
            return Err(ValidationError::new(
                ValidationErrorCode::GuestImageMismatch,
                "requested guest image is not registered for this backend and worker",
            ));
        }
        if request.execution_policy.network_allowed && !backend.network_allowed {
            return Err(ValidationError::new(
                ValidationErrorCode::NetworkDenied,
                "backend registration denies network access",
            ));
        }
        if request.execution_policy.gpu_required && (!backend.gpu_allowed || !worker.gpu_available) {
            return Err(ValidationError::new(
                ValidationErrorCode::GpuUnavailable,
                "requested GPU capability is unavailable",
            ));
        }
        if request.execution_policy.max_threads > backend.max_threads
            || request.execution_policy.max_threads > worker.max_threads
        {
            return Err(ValidationError::new(
                ValidationErrorCode::ThreadLimitExceeded,
                "requested thread limit exceeds registered capability",
            ));
        }
        for capability in &backend.capabilities {
            if !worker.capabilities.iter().any(|available| available == capability) {
                return Err(ValidationError::new(
                    ValidationErrorCode::CapabilityMissing,
                    format!("worker does not provide capability {capability}"),
                ));
            }
        }
        Ok(())
    }
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
            if chunk.offset != previous_end {
                return Err("artifact chunks do not cover artifact bytes".into());
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

        if !self.chunks.is_empty() && previous_end != self.size_bytes {
            return Err("artifact chunks do not cover artifact bytes".into());
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

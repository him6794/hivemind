//! Independent request/result contracts for the Rust-owned managed GPU DSL.
//!
//! This module intentionally has no dependency on the general-compute GPU ABI.
//! The managed GPU route has its own capability, requirement, billing, result,
//! and evidence identities and must never enter the v0 proof or result-torrent
//! completion paths.

use crate::{
    TrustedWorkerCapabilityRegistration, ValidationError, ValidationErrorCode, sha256_digest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MANAGED_GPU_REQUEST_PROTOCOL_VERSION: &str = "managed-function-gpu-request-v1";
pub const MANAGED_GPU_RESULT_PROTOCOL_VERSION: &str = "managed-function-gpu-result-v1";
pub const MANAGED_GPU_CAPABILITY_PROTOCOL_VERSION: &str = "managed-function-gpu-capability-v1";
pub const MANAGED_GPU_REQUIREMENT_PROTOCOL_VERSION: &str = "managed-function-gpu-requirement-v1";
pub const MANAGED_GPU_RUNTIME_VERSION: &str = "managed-function-gpu-v1";
/// This is the raw digest of the canonical managed GPU semantics manifest. It
/// deliberately uses the same representation as the interpreter crate.
pub const MANAGED_GPU_SEMANTICS_MANIFEST_SHA256: &str =
    "4b5230145a43f05df6e8e09a4fa682e3babcfe43aa980883f72dd95d74d8cb13";
pub const MANAGED_GPU_OPERATION_REGISTRY_VERSION: &str = "managed-function-gpu-ops-v1";
pub const MANAGED_GPU_BILLING_VERSION: &str = "managed-function-gpu-billing-v1";
pub const MANAGED_GPU_COST_MODEL_VERSION: &str = "managed-function-gpu-metering-v1";
pub const MANAGED_GPU_SETTLEMENT_BASIS: &str = "fixed-operation-reservation";
pub const MANAGED_GPU_OPERATION_COST_UNITS: u64 = 10;
pub const MANAGED_GPU_MAX_RESERVATION_CPT: u64 = 1_000_000_000_000;
pub const MANAGED_GPU_MAX_SOURCE_BYTES: usize = 256 * 1024;
pub const MANAGED_GPU_MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
pub const MANAGED_GPU_MAX_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
pub const MANAGED_GPU_MAX_OPERATIONS: u64 = 100_000_000;
pub const MANAGED_GPU_MAX_VALUE_BYTES: u64 = 16 * 1024 * 1024;
pub const MANAGED_GPU_MAX_COLLECTION_ITEMS: u64 = 1_000_000;
pub const MANAGED_GPU_MAX_VALUE_DEPTH: u64 = 64;
pub const MANAGED_GPU_MAX_MATERIALIZATION_BYTES: u64 = 64 * 1024 * 1024;
pub const MANAGED_GPU_MAX_WALL_TIME_MS: u64 = 7 * 24 * 60 * 60 * 1000;
pub const MANAGED_GPU_MAX_CUDA_UUID_LENGTH: usize = 128;
pub const MANAGED_GPU_MAX_TOKEN_LENGTH: usize = 256;
pub const MANAGED_GPU_MAX_VRAM_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const MANAGED_GPU_MAX_STREAMS: u32 = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedGpuVendor {
    Nvidia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedGpuRuntime {
    Cuda,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuRequirement {
    pub protocol_version: String,
    pub vendor: ManagedGpuVendor,
    pub compute_capability: String,
    pub runtime: ManagedGpuRuntime,
    pub runtime_version: String,
    pub driver_abi: String,
    pub min_vram_bytes: u64,
    pub min_streams: u32,
    pub image_digest: String,
    pub allow_cpu_fallback: bool,
}

impl ManagedGpuRequirement {
    pub fn new(
        compute_capability: impl Into<String>,
        runtime_version: impl Into<String>,
        driver_abi: impl Into<String>,
        min_vram_bytes: u64,
        min_streams: u32,
        image_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let requirement = Self {
            protocol_version: MANAGED_GPU_REQUIREMENT_PROTOCOL_VERSION.into(),
            vendor: ManagedGpuVendor::Nvidia,
            compute_capability: compute_capability.into(),
            runtime: ManagedGpuRuntime::Cuda,
            runtime_version: runtime_version.into(),
            driver_abi: driver_abi.into(),
            min_vram_bytes,
            min_streams,
            image_digest: image_digest.into(),
            allow_cpu_fallback: false,
        };
        requirement.validate()?;
        Ok(requirement)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version != MANAGED_GPU_REQUIREMENT_PROTOCOL_VERSION {
            return Err("managed GPU requirement protocol is unsupported".into());
        }
        validate_token(&self.compute_capability, "compute_capability")?;
        validate_token(&self.runtime_version, "runtime_version")?;
        validate_token(&self.driver_abi, "driver_abi")?;
        if self.min_vram_bytes == 0 || self.min_vram_bytes > MANAGED_GPU_MAX_VRAM_BYTES {
            return Err("managed GPU minimum VRAM is outside the bounded range".into());
        }
        if self.min_streams == 0 || self.min_streams > MANAGED_GPU_MAX_STREAMS {
            return Err("managed GPU minimum stream count is outside the bounded range".into());
        }
        if !is_sha256_digest(&self.image_digest) {
            return Err("managed GPU image digest is invalid".into());
        }
        if self.allow_cpu_fallback {
            return Err("managed GPU-v1 does not permit CPU fallback".into());
        }
        Ok(())
    }
}

/// The only device identity that may cross the managed GPU admission boundary.
/// The CUDA UUID and ordinal are both operator-resolved and required; a task
/// cannot select either value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuCapability {
    pub protocol_version: String,
    pub vendor: ManagedGpuVendor,
    pub device_id: String,
    pub compute_capability: String,
    pub runtime: ManagedGpuRuntime,
    pub runtime_version: String,
    pub driver_abi: String,
    pub vram_bytes: u64,
    pub max_streams: u32,
    pub image_digest: String,
    pub cuda_device_ordinal: i32,
    pub cuda_uuid: String,
}

impl ManagedGpuCapability {
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        device_id: impl Into<String>,
        compute_capability: impl Into<String>,
        runtime_version: impl Into<String>,
        driver_abi: impl Into<String>,
        vram_bytes: u64,
        max_streams: u32,
        image_digest: impl Into<String>,
        cuda_device_ordinal: i32,
        cuda_uuid: impl Into<String>,
    ) -> Result<Self, String> {
        let capability = Self {
            protocol_version: MANAGED_GPU_CAPABILITY_PROTOCOL_VERSION.into(),
            vendor: ManagedGpuVendor::Nvidia,
            device_id: device_id.into(),
            compute_capability: compute_capability.into(),
            runtime: ManagedGpuRuntime::Cuda,
            runtime_version: runtime_version.into(),
            driver_abi: driver_abi.into(),
            vram_bytes,
            max_streams,
            image_digest: image_digest.into(),
            cuda_device_ordinal,
            cuda_uuid: cuda_uuid.into(),
        };
        capability.validate()?;
        Ok(capability)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version != MANAGED_GPU_CAPABILITY_PROTOCOL_VERSION {
            return Err("managed GPU capability protocol is unsupported".into());
        }
        for (field, value) in [
            ("device_id", self.device_id.as_str()),
            ("compute_capability", self.compute_capability.as_str()),
            ("runtime_version", self.runtime_version.as_str()),
            ("driver_abi", self.driver_abi.as_str()),
        ] {
            validate_token(value, field)?;
        }
        if self.vram_bytes == 0 || self.vram_bytes > MANAGED_GPU_MAX_VRAM_BYTES {
            return Err("managed GPU VRAM is outside the bounded range".into());
        }
        if self.max_streams == 0 || self.max_streams > MANAGED_GPU_MAX_STREAMS {
            return Err("managed GPU stream count is outside the bounded range".into());
        }
        if !is_sha256_digest(&self.image_digest) {
            return Err("managed GPU image digest is invalid".into());
        }
        if self.cuda_device_ordinal < 0 {
            return Err("managed GPU CUDA ordinal is invalid".into());
        }
        if !valid_cuda_uuid(&self.cuda_uuid) {
            return Err("managed GPU CUDA UUID is invalid".into());
        }
        Ok(())
    }

    fn satisfies(&self, requirement: &ManagedGpuRequirement, image_digest: &str) -> bool {
        self.vendor == requirement.vendor
            && self.compute_capability == requirement.compute_capability
            && self.runtime == requirement.runtime
            && self.runtime_version == requirement.runtime_version
            && self.driver_abi == requirement.driver_abi
            && self.vram_bytes >= requirement.min_vram_bytes
            && self.max_streams >= requirement.min_streams
            && self.image_digest == requirement.image_digest
            && self.image_digest == image_digest
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedGpuStatus {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    ResourceExhausted,
    BackendUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedGpuEvidenceLevel {
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuEvidence {
    pub level: ManagedGpuEvidenceLevel,
    pub payload_sha256: Option<String>,
}

impl Default for ManagedGpuEvidence {
    fn default() -> Self {
        Self {
            level: ManagedGpuEvidenceLevel::Unverified,
            payload_sha256: None,
        }
    }
}

/// Operator-owned backend registration for the managed GPU route.
///
/// There are no executable paths, library names, commands, mounts, device
/// handles, kernel sources, PTX strings, or task-controlled selectors here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuBackendRegistration {
    pub backend_id: String,
    pub runtime_version: String,
    pub semantics_manifest_sha256: String,
    pub operation_registry_version: String,
    pub guest_image_digest: String,
    pub billing_version: String,
    pub cost_model_version: String,
    pub reservation_cpt: u64,
    pub max_source_bytes: u64,
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_operations: u64,
    pub max_gpu_time_ms: u64,
    pub capabilities: Vec<ManagedGpuCapability>,
}

impl ManagedGpuBackendRegistration {
    pub fn validate(&self) -> Result<(), String> {
        validate_token(&self.backend_id, "backend_id")?;
        if self.runtime_version != MANAGED_GPU_RUNTIME_VERSION {
            return Err("managed GPU backend runtime identity is unsupported".into());
        }
        if self.semantics_manifest_sha256 != MANAGED_GPU_SEMANTICS_MANIFEST_SHA256 {
            return Err("managed GPU backend semantics digest is unsupported".into());
        }
        if self.operation_registry_version != MANAGED_GPU_OPERATION_REGISTRY_VERSION {
            return Err("managed GPU operation registry identity is unsupported".into());
        }
        if self.billing_version != MANAGED_GPU_BILLING_VERSION {
            return Err("managed GPU billing identity is unsupported".into());
        }
        if self.cost_model_version != MANAGED_GPU_COST_MODEL_VERSION {
            return Err("managed GPU cost-model identity is unsupported".into());
        }
        if !is_sha256_digest(&self.guest_image_digest) {
            return Err("managed GPU guest image digest is invalid".into());
        }
        if self.reservation_cpt == 0 || self.reservation_cpt > MANAGED_GPU_MAX_RESERVATION_CPT {
            return Err("managed GPU reservation is outside the bounded range".into());
        }
        if self.max_source_bytes == 0
            || self.max_source_bytes > MANAGED_GPU_MAX_SOURCE_BYTES as u64
            || self.max_input_bytes == 0
            || self.max_input_bytes > MANAGED_GPU_MAX_INPUT_BYTES as u64
            || self.max_output_bytes == 0
            || self.max_output_bytes > MANAGED_GPU_MAX_OUTPUT_BYTES
            || self.max_operations == 0
            || self.max_operations > MANAGED_GPU_MAX_OPERATIONS
            || self.max_gpu_time_ms == 0
            || self.max_gpu_time_ms > MANAGED_GPU_MAX_WALL_TIME_MS
        {
            return Err("managed GPU backend limits are outside the bounded range".into());
        }
        if self.capabilities.is_empty() {
            return Err("managed GPU backend must register a concrete CUDA device".into());
        }
        let mut device_ids = BTreeSet::new();
        let mut cuda_ordinals = BTreeSet::new();
        let mut cuda_uuids = BTreeSet::new();
        for capability in &self.capabilities {
            capability.validate()?;
            if !device_ids.insert(capability.device_id.as_str()) {
                return Err("managed GPU device IDs must be unique".into());
            }
            if !cuda_ordinals.insert(capability.cuda_device_ordinal) {
                return Err("managed GPU CUDA ordinals must be unique".into());
            }
            if !cuda_uuids.insert(capability.cuda_uuid.as_str()) {
                return Err("managed GPU CUDA UUIDs must be unique".into());
            }
            if capability.image_digest != self.guest_image_digest {
                return Err("managed GPU capability image does not match backend image".into());
            }
        }
        Ok(())
    }

    pub fn validate_request(&self, request: &ManagedGpuRequest) -> Result<(), String> {
        self.validate()?;
        if self.backend_id != request.backend_id
            || self.runtime_version != request.runtime_version
            || self.semantics_manifest_sha256 != request.semantics_manifest_sha256
            || self.operation_registry_version != request.operation_registry_version
            || self.guest_image_digest != request.guest_image_digest
            || self.billing_version != request.billing_version
            || self.cost_model_version != request.cost_model_version
            || self.reservation_cpt != request.reservation_cpt
        {
            return Err(
                "managed GPU request identity does not match the registered backend".into(),
            );
        }
        if request.source.len() as u64 > self.max_source_bytes
            || request.input_json.len() as u64 > self.max_input_bytes
            || request.limits.max_output_bytes > self.max_output_bytes
            || request.limits.max_operations > self.max_operations
            || request.limits.max_gpu_time_ms > self.max_gpu_time_ms
        {
            return Err("managed GPU request exceeds the registered backend limits".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuLimits {
    pub max_operations: u64,
    pub max_output_bytes: u64,
    pub max_value_bytes: u64,
    pub max_collection_items: u64,
    pub max_value_depth: u64,
    pub max_value_materialization_bytes: u64,
    pub max_wall_time_ms: u64,
    pub max_gpu_time_ms: u64,
}

impl Default for ManagedGpuLimits {
    fn default() -> Self {
        Self {
            max_operations: 1_000_000,
            max_output_bytes: MANAGED_GPU_MAX_OUTPUT_BYTES,
            max_value_bytes: MANAGED_GPU_MAX_VALUE_BYTES,
            max_collection_items: MANAGED_GPU_MAX_COLLECTION_ITEMS,
            max_value_depth: MANAGED_GPU_MAX_VALUE_DEPTH,
            max_value_materialization_bytes: MANAGED_GPU_MAX_MATERIALIZATION_BYTES,
            max_wall_time_ms: 120_000,
            max_gpu_time_ms: 120_000,
        }
    }
}

impl ManagedGpuLimits {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_operations == 0 || self.max_operations > MANAGED_GPU_MAX_OPERATIONS {
            return Err("managed GPU operation budget is outside the bounded range".into());
        }
        if self.max_output_bytes == 0 || self.max_output_bytes > MANAGED_GPU_MAX_OUTPUT_BYTES {
            return Err("managed GPU output limit is outside the bounded range".into());
        }
        if self.max_value_bytes == 0 || self.max_value_bytes > MANAGED_GPU_MAX_VALUE_BYTES {
            return Err("managed GPU value limit is outside the bounded range".into());
        }
        if self.max_collection_items == 0
            || self.max_collection_items > MANAGED_GPU_MAX_COLLECTION_ITEMS
        {
            return Err("managed GPU collection limit is outside the bounded range".into());
        }
        if self.max_value_depth == 0 || self.max_value_depth > MANAGED_GPU_MAX_VALUE_DEPTH {
            return Err("managed GPU value depth is outside the bounded range".into());
        }
        if self.max_value_materialization_bytes == 0
            || self.max_value_materialization_bytes > MANAGED_GPU_MAX_MATERIALIZATION_BYTES
        {
            return Err("managed GPU materialization limit is outside the bounded range".into());
        }
        if self.max_wall_time_ms == 0 || self.max_wall_time_ms > MANAGED_GPU_MAX_WALL_TIME_MS {
            return Err("managed GPU wall-time limit is outside the bounded range".into());
        }
        if self.max_gpu_time_ms == 0 || self.max_gpu_time_ms > self.max_wall_time_ms {
            return Err("managed GPU time limit is outside the wall-time limit".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuRequest {
    pub protocol_version: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub runtime_version: String,
    pub semantics_manifest_sha256: String,
    pub operation_registry_version: String,
    pub backend_id: String,
    pub guest_image_digest: String,
    pub source: String,
    pub input_json: String,
    pub gpu_requirement: ManagedGpuRequirement,
    pub limits: ManagedGpuLimits,
    pub reservation_cpt: u64,
    pub billing_version: String,
    pub cost_model_version: String,
    pub settlement_basis: String,
    pub proof_policy: ManagedGpuProofPolicy,
}

/// GPU-v1 has no proof-bearing result variant. Keeping the policy explicit in
/// the request makes the absence of a proof a versioned protocol decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedGpuProofPolicy {
    None,
}

impl ManagedGpuRequest {
    ///
    /// # Panics
    ///
    /// Panics only if the canonical request representation cannot be serialized;
    /// all fields are serde-compatible values, so this indicates a programming
    /// error rather than malformed task input.
    #[must_use]
    pub fn canonical_request_digest(&self) -> String {
        let canonical = CanonicalManagedGpuRequest::from(self);
        let bytes = serde_json::to_vec(&canonical)
            .expect("managed GPU request canonicalization is infallible");
        sha256_digest(&bytes)
    }

    #[must_use]
    pub fn source_sha256(&self) -> String {
        sha256_digest(self.source.as_bytes())
    }

    #[must_use]
    pub fn input_sha256(&self) -> String {
        sha256_digest(self.input_json.as_bytes())
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != MANAGED_GPU_REQUEST_PROTOCOL_VERSION {
            return Err(invalid(
                ValidationErrorCode::RequestInvalid,
                "managed GPU request protocol is unsupported",
            ));
        }
        for (name, value) in [
            ("execution_id", &self.execution_id),
            ("attempt_id", &self.attempt_id),
            ("idempotency_key", &self.idempotency_key),
            ("backend_id", &self.backend_id),
        ] {
            validate_token(value, name)
                .map_err(|message| invalid(ValidationErrorCode::RequestInvalid, message))?;
        }
        if !is_sha256_digest(&self.request_digest) {
            return Err(invalid(
                ValidationErrorCode::RequestDigestInvalid,
                "managed GPU request digest must be a SHA-256 digest",
            ));
        }
        if self.request_digest != self.canonical_request_digest() {
            return Err(invalid(
                ValidationErrorCode::RequestDigestMismatch,
                "managed GPU request digest does not match the canonical request",
            ));
        }
        if self.runtime_version != MANAGED_GPU_RUNTIME_VERSION {
            return Err(invalid(
                ValidationErrorCode::RuntimeVersionMismatch,
                "managed GPU runtime identity is unsupported",
            ));
        }
        if !is_raw_sha256_digest(&self.semantics_manifest_sha256)
            || self.semantics_manifest_sha256 != MANAGED_GPU_SEMANTICS_MANIFEST_SHA256
        {
            return Err(invalid(
                ValidationErrorCode::RequestBindingMismatch,
                "managed GPU semantics digest is unsupported",
            ));
        }
        if self.operation_registry_version != MANAGED_GPU_OPERATION_REGISTRY_VERSION {
            return Err(invalid(
                ValidationErrorCode::RequestBindingMismatch,
                "managed GPU operation registry identity is unsupported",
            ));
        }
        if !is_sha256_digest(&self.guest_image_digest) {
            return Err(invalid(
                ValidationErrorCode::GuestImageMismatch,
                "managed GPU guest image digest is invalid",
            ));
        }
        if self.source.is_empty() || self.source.len() > MANAGED_GPU_MAX_SOURCE_BYTES {
            return Err(invalid(
                ValidationErrorCode::RequestInvalid,
                "managed GPU source exceeds the bounded limit",
            ));
        }
        if self.input_json.is_empty() || self.input_json.len() > MANAGED_GPU_MAX_INPUT_BYTES {
            return Err(invalid(
                ValidationErrorCode::RequestInvalid,
                "managed GPU JSON input exceeds the bounded limit",
            ));
        }
        serde_json::from_str::<serde_json::Value>(&self.input_json).map_err(|error| {
            invalid(
                ValidationErrorCode::RequestInvalid,
                format!("managed GPU input is not valid JSON: {error}"),
            )
        })?;
        self.gpu_requirement.validate().map_err(|error| {
            invalid(
                ValidationErrorCode::GpuUnavailable,
                format!("managed GPU requirement is invalid: {error}"),
            )
        })?;
        if self.gpu_requirement.image_digest != self.guest_image_digest {
            return Err(invalid(
                ValidationErrorCode::RequestBindingMismatch,
                "managed GPU requirement image does not match the guest image",
            ));
        }
        self.limits
            .validate()
            .map_err(|message| invalid(ValidationErrorCode::PolicyInvalid, message))?;
        if self.reservation_cpt == 0 || self.reservation_cpt > MANAGED_GPU_MAX_RESERVATION_CPT {
            return Err(invalid(
                ValidationErrorCode::PolicyInvalid,
                "managed GPU reservation is outside the bounded range",
            ));
        }
        if self.billing_version != MANAGED_GPU_BILLING_VERSION
            || self.cost_model_version != MANAGED_GPU_COST_MODEL_VERSION
        {
            return Err(invalid(
                ValidationErrorCode::RequestBindingMismatch,
                "managed GPU billing identity is unsupported",
            ));
        }
        if self.settlement_basis != MANAGED_GPU_SETTLEMENT_BASIS {
            return Err(invalid(
                ValidationErrorCode::RequestBindingMismatch,
                "managed GPU settlement basis is unsupported",
            ));
        }
        if self.proof_policy != ManagedGpuProofPolicy::None {
            return Err(invalid(
                ValidationErrorCode::EvidenceInvalid,
                "managed GPU-v1 does not support proof-bearing requests",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct CanonicalManagedGpuRequest<'a> {
    protocol_version: &'a str,
    execution_id: &'a str,
    attempt_id: &'a str,
    idempotency_key: &'a str,
    runtime_version: &'a str,
    semantics_manifest_sha256: &'a str,
    operation_registry_version: &'a str,
    backend_id: &'a str,
    guest_image_digest: &'a str,
    source: &'a str,
    input_json: &'a str,
    gpu_requirement: &'a ManagedGpuRequirement,
    limits: &'a ManagedGpuLimits,
    reservation_cpt: u64,
    billing_version: &'a str,
    cost_model_version: &'a str,
    settlement_basis: &'a str,
    proof_policy: ManagedGpuProofPolicy,
}

impl<'a> From<&'a ManagedGpuRequest> for CanonicalManagedGpuRequest<'a> {
    fn from(request: &'a ManagedGpuRequest) -> Self {
        Self {
            protocol_version: &request.protocol_version,
            execution_id: &request.execution_id,
            attempt_id: &request.attempt_id,
            idempotency_key: &request.idempotency_key,
            runtime_version: &request.runtime_version,
            semantics_manifest_sha256: &request.semantics_manifest_sha256,
            operation_registry_version: &request.operation_registry_version,
            backend_id: &request.backend_id,
            guest_image_digest: &request.guest_image_digest,
            source: &request.source,
            input_json: &request.input_json,
            gpu_requirement: &request.gpu_requirement,
            limits: &request.limits,
            reservation_cpt: request.reservation_cpt,
            billing_version: &request.billing_version,
            cost_model_version: &request.cost_model_version,
            settlement_basis: &request.settlement_basis,
            proof_policy: request.proof_policy,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuUsage {
    pub source_bytes: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub executed_operations: u64,
    pub operation_cost_units: u64,
    pub wall_time_ms: u64,
    pub gpu_time_ms: u64,
    pub gpu_memory_bytes: u64,
}

impl ManagedGpuUsage {
    fn validate_for(
        &self,
        request: &ManagedGpuRequest,
        output_bytes: u64,
        gpu: &ManagedGpuCapability,
    ) -> Result<(), ValidationError> {
        let source_bytes = request.source.len() as u64;
        let input_bytes = request.input_json.len() as u64;
        let expected_operation_cost = self
            .executed_operations
            .checked_mul(MANAGED_GPU_OPERATION_COST_UNITS)
            .ok_or_else(|| {
                invalid(
                    ValidationErrorCode::UsageExceedsPolicy,
                    "managed GPU operation accounting overflowed",
                )
            })?;
        if self.source_bytes != source_bytes
            || self.input_bytes != input_bytes
            || self.output_bytes != output_bytes
            || self.operation_cost_units != expected_operation_cost
            || self.executed_operations > request.limits.max_operations
            || self.output_bytes > request.limits.max_output_bytes
            || self.wall_time_ms > request.limits.max_wall_time_ms
            || self.gpu_time_ms > request.limits.max_gpu_time_ms
            || self.gpu_memory_bytes > gpu.vram_bytes
        {
            return Err(invalid(
                ValidationErrorCode::UsageExceedsPolicy,
                "managed GPU usage claim exceeds the request policy",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuResult {
    pub protocol_version: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub runtime_version: String,
    pub semantics_manifest_sha256: String,
    pub operation_registry_version: String,
    pub backend_id: String,
    pub guest_image_digest: String,
    pub source_sha256: String,
    pub input_sha256: String,
    pub reservation_cpt: u64,
    pub status: ManagedGpuStatus,
    pub exit_code: Option<i32>,
    pub error_code: Option<String>,
    pub output: String,
    pub output_sha256: String,
    pub selected_gpu: ManagedGpuCapability,
    pub usage: ManagedGpuUsage,
    pub evidence: ManagedGpuEvidence,
}

impl ManagedGpuResult {
    pub fn validate_against(
        &self,
        request: &ManagedGpuRequest,
        registration: &TrustedWorkerCapabilityRegistration,
    ) -> Result<(), ValidationError> {
        request.validate()?;
        let expected_gpu = select_trusted_gpu(registration, request)?;
        self.validate_identity(request)?;
        self.selected_gpu.validate().map_err(|error| {
            invalid(
                ValidationErrorCode::GpuUnavailable,
                format!("managed GPU result selection is invalid: {error}"),
            )
        })?;
        if self.selected_gpu != expected_gpu {
            return Err(invalid(
                ValidationErrorCode::GpuUnavailable,
                "managed GPU result selected a device other than the trusted admission device",
            ));
        }
        if self.source_sha256 != request.source_sha256()
            || self.input_sha256 != request.input_sha256()
        {
            return Err(invalid(
                ValidationErrorCode::ResultBindingMismatch,
                "managed GPU result input/source digest does not match the request",
            ));
        }
        if self.output.len() as u64 > request.limits.max_output_bytes {
            return Err(invalid(
                ValidationErrorCode::UsageExceedsPolicy,
                "managed GPU output exceeds the request limit",
            ));
        }
        if self.output_sha256 != sha256_digest(self.output.as_bytes()) {
            return Err(invalid(
                ValidationErrorCode::ResultBindingMismatch,
                "managed GPU output digest does not match output bytes",
            ));
        }
        self.usage
            .validate_for(request, self.output.len() as u64, &self.selected_gpu)?;
        validate_status(self.status, self.exit_code, self.error_code.as_deref())?;
        if self.evidence.level != ManagedGpuEvidenceLevel::Unverified {
            return Err(invalid(
                ValidationErrorCode::EvidenceInvalid,
                "managed GPU worker results may only claim unverified evidence",
            ));
        }
        if self
            .evidence
            .payload_sha256
            .as_deref()
            .is_some_and(|digest| !is_sha256_digest(digest))
        {
            return Err(invalid(
                ValidationErrorCode::EvidenceInvalid,
                "managed GPU evidence payload digest is invalid",
            ));
        }
        Ok(())
    }

    fn validate_identity(&self, request: &ManagedGpuRequest) -> Result<(), ValidationError> {
        if self.protocol_version != MANAGED_GPU_RESULT_PROTOCOL_VERSION
            || self.execution_id != request.execution_id
            || self.attempt_id != request.attempt_id
            || self.idempotency_key != request.idempotency_key
            || self.request_digest != request.request_digest
            || self.runtime_version != request.runtime_version
            || self.semantics_manifest_sha256 != request.semantics_manifest_sha256
            || self.operation_registry_version != request.operation_registry_version
            || self.backend_id != request.backend_id
            || self.guest_image_digest != request.guest_image_digest
            || self.reservation_cpt != request.reservation_cpt
        {
            return Err(invalid(
                ValidationErrorCode::ResultBindingMismatch,
                "managed GPU result identity does not match the request",
            ));
        }
        Ok(())
    }
}

fn validate_status(
    status: ManagedGpuStatus,
    exit_code: Option<i32>,
    error_code: Option<&str>,
) -> Result<(), ValidationError> {
    let valid = match status {
        ManagedGpuStatus::Completed => exit_code == Some(0) && error_code.is_none(),
        ManagedGpuStatus::Failed => {
            exit_code != Some(0) && error_code.is_some_and(|code| !code.trim().is_empty())
        }
        ManagedGpuStatus::Cancelled
        | ManagedGpuStatus::TimedOut
        | ManagedGpuStatus::ResourceExhausted
        | ManagedGpuStatus::BackendUnavailable => {
            exit_code.is_none() && error_code.is_some_and(|code| !code.trim().is_empty())
        }
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(
            ValidationErrorCode::ResultStatusInvalid,
            "managed GPU result status and exit/error code combination is invalid",
        ))
    }
}

fn validate_token(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MANAGED_GPU_MAX_TOKEN_LENGTH
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+' | b':')
        })
    {
        return Err(format!("managed GPU {field} is invalid"));
    }
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_raw_sha256_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_cuda_uuid(value: &str) -> bool {
    value.len() >= 8
        && value.len() <= MANAGED_GPU_MAX_CUDA_UUID_LENGTH
        && value.starts_with("GPU-")
        && value[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

fn invalid(code: ValidationErrorCode, message: impl Into<String>) -> ValidationError {
    ValidationError::new(code, message)
}

/// Select a concrete operator-approved CUDA device for managed GPU-v1. This
/// path never returns a CPU fallback and only reads the private registration.
pub(crate) fn select_trusted_gpu(
    registration: &TrustedWorkerCapabilityRegistration,
    request: &ManagedGpuRequest,
) -> Result<ManagedGpuCapability, ValidationError> {
    request.validate()?;
    let matching_backends: Vec<_> = registration
        .managed_gpu_backends
        .iter()
        .filter(|backend| backend.backend_id == request.backend_id)
        .collect();
    if matching_backends.len() != 1 {
        return Err(invalid(
            ValidationErrorCode::BackendUnavailable,
            "managed GPU backend is missing or duplicated",
        ));
    }
    let backend = matching_backends[0];
    backend
        .validate_request(request)
        .map_err(|message| invalid(ValidationErrorCode::BackendUnavailable, message))?;
    if !registration
        .worker
        .guest_image_digests
        .iter()
        .any(|digest| digest == &request.guest_image_digest)
    {
        return Err(invalid(
            ValidationErrorCode::GuestImageMismatch,
            "managed GPU guest image is not registered for this worker",
        ));
    }
    let mut compatible: Vec<_> = backend
        .capabilities
        .iter()
        .filter(|capability| {
            capability.satisfies(&request.gpu_requirement, &request.guest_image_digest)
        })
        .cloned()
        .collect();
    compatible.sort_by(|left, right| {
        left.device_id
            .cmp(&right.device_id)
            .then_with(|| left.cuda_uuid.cmp(&right.cuda_uuid))
            .then_with(|| left.cuda_device_ordinal.cmp(&right.cuda_device_ordinal))
    });
    compatible.into_iter().next().ok_or_else(|| {
        invalid(
            ValidationErrorCode::GpuUnavailable,
            "no compatible trusted managed GPU device is registered",
        )
    })
}

//! Typed GPU capability negotiation for the alpha general-compute contract.
//!
//! The control plane must negotiate a concrete vendor/runtime/driver/image
//! tuple before a GPU task is dispatched.  A worker's boolean
//! `gpu_available` flag is not sufficient evidence for that decision, so this
//! module keeps the bounded, serializable identity separate from execution.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const GPU_CAPABILITY_PROTOCOL_VERSION: &str = "general-compute-gpu-v1alpha1";
pub const MAX_GPU_TOKEN_LENGTH: usize = 128;
pub const MAX_GPU_STREAMS: u32 = 65_536;
pub const MAX_GPU_VRAM_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Nvidia,
    Amd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuRuntime {
    Cuda,
    Rocm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuContractError {
    InvalidField(&'static str),
    InvalidVendorRuntimePair,
    InvalidVram,
    InvalidStreamCount,
    InvalidImageDigest,
}

impl fmt::Display for GpuContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField(field) => write!(formatter, "GPU field {field} is invalid"),
            Self::InvalidVendorRuntimePair => {
                formatter.write_str("GPU vendor and runtime are incompatible")
            }
            Self::InvalidVram => formatter.write_str("GPU VRAM must be within the bounded range"),
            Self::InvalidStreamCount => {
                formatter.write_str("GPU stream count must be within the bounded range")
            }
            Self::InvalidImageDigest => {
                formatter.write_str("GPU image digest must be a SHA-256 digest")
            }
        }
    }
}

impl std::error::Error for GpuContractError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuCapability {
    pub protocol_version: String,
    pub vendor: GpuVendor,
    pub device_id: String,
    pub compute_capability: String,
    pub runtime: GpuRuntime,
    pub runtime_version: String,
    pub driver_abi: String,
    pub vram_bytes: u64,
    pub max_streams: u32,
    pub image_digest: String,
}

impl GpuCapability {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        vendor: GpuVendor,
        device_id: impl Into<String>,
        compute_capability: impl Into<String>,
        runtime: GpuRuntime,
        runtime_version: impl Into<String>,
        driver_abi: impl Into<String>,
        vram_bytes: u64,
        max_streams: u32,
        image_digest: impl Into<String>,
    ) -> Result<Self, GpuContractError> {
        let capability = Self {
            protocol_version: GPU_CAPABILITY_PROTOCOL_VERSION.into(),
            vendor,
            device_id: device_id.into(),
            compute_capability: compute_capability.into(),
            runtime,
            runtime_version: runtime_version.into(),
            driver_abi: driver_abi.into(),
            vram_bytes,
            max_streams,
            image_digest: image_digest.into(),
        };
        capability.validate()?;
        Ok(capability)
    }

    pub fn validate(&self) -> Result<(), GpuContractError> {
        if self.protocol_version != GPU_CAPABILITY_PROTOCOL_VERSION {
            return Err(GpuContractError::InvalidField("protocol_version"));
        }
        for (field, value) in [
            ("device_id", self.device_id.as_str()),
            ("compute_capability", self.compute_capability.as_str()),
            ("runtime_version", self.runtime_version.as_str()),
            ("driver_abi", self.driver_abi.as_str()),
        ] {
            validate_token(value, field)?;
        }
        if !vendor_runtime_matches(self.vendor, self.runtime) {
            return Err(GpuContractError::InvalidVendorRuntimePair);
        }
        if self.vram_bytes == 0 || self.vram_bytes > MAX_GPU_VRAM_BYTES {
            return Err(GpuContractError::InvalidVram);
        }
        if self.max_streams == 0 || self.max_streams > MAX_GPU_STREAMS {
            return Err(GpuContractError::InvalidStreamCount);
        }
        if !is_sha256_digest(&self.image_digest) {
            return Err(GpuContractError::InvalidImageDigest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuRequirement {
    pub protocol_version: String,
    pub vendor: GpuVendor,
    pub compute_capability: String,
    pub runtime: GpuRuntime,
    pub driver_abi: String,
    pub min_vram_bytes: u64,
    pub min_streams: u32,
    pub image_digest: String,
    pub allow_cpu_fallback: bool,
}

impl GpuRequirement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        vendor: GpuVendor,
        compute_capability: impl Into<String>,
        runtime: GpuRuntime,
        driver_abi: impl Into<String>,
        min_vram_bytes: u64,
        min_streams: u32,
        image_digest: impl Into<String>,
        allow_cpu_fallback: bool,
    ) -> Result<Self, GpuContractError> {
        let requirement = Self {
            protocol_version: GPU_CAPABILITY_PROTOCOL_VERSION.into(),
            vendor,
            compute_capability: compute_capability.into(),
            runtime,
            driver_abi: driver_abi.into(),
            min_vram_bytes,
            min_streams,
            image_digest: image_digest.into(),
            allow_cpu_fallback,
        };
        requirement.validate()?;
        Ok(requirement)
    }

    pub fn validate(&self) -> Result<(), GpuContractError> {
        if self.protocol_version != GPU_CAPABILITY_PROTOCOL_VERSION {
            return Err(GpuContractError::InvalidField("protocol_version"));
        }
        validate_token(&self.compute_capability, "compute_capability")?;
        validate_token(&self.driver_abi, "driver_abi")?;
        if !vendor_runtime_matches(self.vendor, self.runtime) {
            return Err(GpuContractError::InvalidVendorRuntimePair);
        }
        if self.min_vram_bytes == 0 || self.min_vram_bytes > MAX_GPU_VRAM_BYTES {
            return Err(GpuContractError::InvalidVram);
        }
        if self.min_streams == 0 || self.min_streams > MAX_GPU_STREAMS {
            return Err(GpuContractError::InvalidStreamCount);
        }
        if !is_sha256_digest(&self.image_digest) {
            return Err(GpuContractError::InvalidImageDigest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuFallbackReason {
    NoCompatibleDevice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum GpuSelection {
    Gpu(GpuCapability),
    CpuFallback { reason: GpuFallbackReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuNegotiationError {
    InvalidRequirement(GpuContractError),
    InvalidCapability(GpuContractError),
    NoCompatibleDevice,
}

impl fmt::Display for GpuNegotiationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequirement(error) => {
                write!(formatter, "invalid GPU requirement: {error}")
            }
            Self::InvalidCapability(error) => {
                write!(formatter, "invalid registered GPU capability: {error}")
            }
            Self::NoCompatibleDevice => {
                formatter.write_str("no compatible GPU capability is registered")
            }
        }
    }
}

impl std::error::Error for GpuNegotiationError {}

/// Select a deterministic compatible device or an explicitly requested CPU
/// fallback.  Capability rows are validated before matching so malformed
/// operator state cannot be silently ignored.
pub fn negotiate_gpu(
    requirement: &GpuRequirement,
    capabilities: &[GpuCapability],
) -> Result<GpuSelection, GpuNegotiationError> {
    requirement
        .validate()
        .map_err(GpuNegotiationError::InvalidRequirement)?;

    let mut compatible = Vec::new();
    for capability in capabilities {
        capability
            .validate()
            .map_err(GpuNegotiationError::InvalidCapability)?;
        if capability.vendor == requirement.vendor
            && capability.compute_capability == requirement.compute_capability
            && capability.runtime == requirement.runtime
            && capability.driver_abi == requirement.driver_abi
            && capability.vram_bytes >= requirement.min_vram_bytes
            && capability.max_streams >= requirement.min_streams
            && capability.image_digest == requirement.image_digest
        {
            compatible.push(capability.clone());
        }
    }

    compatible.sort_by(|left, right| {
        left.device_id
            .cmp(&right.device_id)
            .then_with(|| left.image_digest.cmp(&right.image_digest))
    });
    if let Some(capability) = compatible.into_iter().next() {
        return Ok(GpuSelection::Gpu(capability));
    }
    if requirement.allow_cpu_fallback {
        Ok(GpuSelection::CpuFallback {
            reason: GpuFallbackReason::NoCompatibleDevice,
        })
    } else {
        Err(GpuNegotiationError::NoCompatibleDevice)
    }
}

fn validate_token(value: &str, field: &'static str) -> Result<(), GpuContractError> {
    if value.is_empty()
        || value.len() > MAX_GPU_TOKEN_LENGTH
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
    {
        return Err(GpuContractError::InvalidField(field));
    }
    Ok(())
}

fn vendor_runtime_matches(vendor: GpuVendor, runtime: GpuRuntime) -> bool {
    matches!(
        (vendor, runtime),
        (GpuVendor::Nvidia, GpuRuntime::Cuda) | (GpuVendor::Amd, GpuRuntime::Rocm)
    )
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

//! Device-bound tensor manifests for GPU-resident buffers.
//!
//! A plain [`TensorManifest`] proves checksum/size/dtype/shape integrity but
//! says nothing about which device it must live on or whether it fits that
//! device's VRAM budget. [`GpuTensorManifest`] binds a tensor to the
//! `device_id` of a negotiated [`GpuCapability`] and fails closed instead of
//! silently overflowing device memory.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::gpu::GpuCapability;
use crate::tensor::TensorManifest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuTensorManifest {
    pub tensor: TensorManifest,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuTensorError {
    Tensor(String),
    DeviceIdMismatch,
    ExceedsDeviceVram { size_bytes: u64, vram_bytes: u64 },
}

impl fmt::Display for GpuTensorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tensor(error) => write!(formatter, "invalid GPU tensor: {error}"),
            Self::DeviceIdMismatch => {
                formatter.write_str("GPU tensor device id does not match the negotiated device")
            }
            Self::ExceedsDeviceVram {
                size_bytes,
                vram_bytes,
            } => write!(
                formatter,
                "GPU tensor of {size_bytes} bytes exceeds the device VRAM budget of {vram_bytes} bytes"
            ),
        }
    }
}

impl std::error::Error for GpuTensorError {}

impl GpuTensorManifest {
    /// Validate tensor metadata and bind it to a negotiated device, without
    /// requiring the materialized bytes to be present yet.
    pub fn validate_for_device(&self, capability: &GpuCapability) -> Result<(), GpuTensorError> {
        self.tensor.validate().map_err(GpuTensorError::Tensor)?;
        if self.device_id != capability.device_id {
            return Err(GpuTensorError::DeviceIdMismatch);
        }
        if self.tensor.data_artifact.size_bytes > capability.vram_bytes {
            return Err(GpuTensorError::ExceedsDeviceVram {
                size_bytes: self.tensor.data_artifact.size_bytes,
                vram_bytes: capability.vram_bytes,
            });
        }
        Ok(())
    }

    /// Validate metadata, device binding, VRAM budget, and the materialized
    /// bytes against the declared checksum in one bounded call.
    pub fn validate_bytes_for_device(
        &self,
        bytes: &[u8],
        capability: &GpuCapability,
    ) -> Result<(), GpuTensorError> {
        self.validate_for_device(capability)?;
        self.tensor
            .validate_bytes(bytes)
            .map_err(GpuTensorError::Tensor)
    }
}

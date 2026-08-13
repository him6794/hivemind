use crate::{sha256_digest, ArtifactManifest, ArtifactRole};
use serde::{Deserialize, Serialize};

pub const TENSOR_ABI_VERSION: &str = "tensor-v1alpha1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TensorDType {
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float32,
    Float64,
    Complex64,
    Complex128,
    BigInt,
}

impl TensorDType {
    fn byte_width(self) -> Option<u64> {
        match self {
            Self::Int8 | Self::Uint8 => Some(1),
            Self::Int16 | Self::Uint16 => Some(2),
            Self::Int32 | Self::Uint32 | Self::Float32 => Some(4),
            Self::Int64 | Self::Uint64 | Self::Float64 | Self::Complex64 => Some(8),
            Self::Complex128 => Some(16),
            Self::BigInt => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ByteOrder {
    Little,
    Big,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TensorLayout {
    C,
    F,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TensorManifest {
    pub abi_version: String,
    pub dtype: TensorDType,
    pub shape: Vec<u64>,
    pub byte_order: ByteOrder,
    pub layout: TensorLayout,
    pub data_artifact: ArtifactManifest,
    pub logical_sha256: String,
}

impl TensorManifest {
    pub fn canonical_logical_sha256(&self) -> String {
        let canonical = CanonicalTensorManifest {
            abi_version: &self.abi_version,
            dtype: self.dtype,
            shape: &self.shape,
            byte_order: self.byte_order,
            layout: self.layout,
            data_sha256: &self.data_artifact.sha256,
        };
        let bytes = serde_json::to_vec(&canonical).expect("tensor canonical serialization is infallible");
        sha256_digest(&bytes)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.abi_version != TENSOR_ABI_VERSION {
            return Err("unsupported tensor ABI version".into());
        }
        let element_count = self
            .shape
            .iter()
            .try_fold(1u64, |count, dimension| count.checked_mul(*dimension))
            .ok_or_else(|| "tensor shape element count overflows".to_string())?;
        if !matches!(self.data_artifact.role, ArtifactRole::Input | ArtifactRole::Output) {
            return Err("tensor data artifact must be input or output".into());
        }
        if self.data_artifact.mime_type != "application/octet-stream" {
            return Err("tensor data must use a binary tensor data MIME type".into());
        }
        if self.data_artifact.inline_bytes.is_none() && self.data_artifact.chunks.is_empty() {
            return Err("tensor data artifact must provide inline bytes or chunks".into());
        }
        self.data_artifact.validate()?;

        if let Some(byte_width) = self.dtype.byte_width() {
            let expected_bytes = element_count
                .checked_mul(byte_width)
                .ok_or_else(|| "tensor byte length overflows".to_string())?;
            if self.data_artifact.size_bytes != expected_bytes {
                return Err("tensor data size does not match dtype and shape".into());
            }
        } else if !self.shape.is_empty() {
            return Err("BigInt tensors are limited to scalar values in this ABI".into());
        } else if self.data_artifact.size_bytes == 0 {
            return Err("BigInt scalar payload must not be empty".into());
        }

        if self.logical_sha256 != self.canonical_logical_sha256() {
            return Err("tensor logical hash does not match canonical metadata".into());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct CanonicalTensorManifest<'a> {
    abi_version: &'a str,
    dtype: TensorDType,
    shape: &'a [u64],
    byte_order: ByteOrder,
    layout: TensorLayout,
    data_sha256: &'a str,
}

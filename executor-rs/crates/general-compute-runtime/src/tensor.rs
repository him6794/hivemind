use crate::{ArtifactManifest, ArtifactRole, sha256_digest};
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

    fn component_byte_width(self) -> Option<usize> {
        match self {
            Self::Complex64 => Some(4),
            Self::Complex128 => Some(8),
            dtype => dtype.byte_width().map(|width| width as usize),
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
        let bytes =
            serde_json::to_vec(&canonical).expect("tensor canonical serialization is infallible");
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
        if !matches!(
            self.data_artifact.role,
            ArtifactRole::Input | ArtifactRole::Output
        ) {
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

    /// Validate bytes after an artifact has been materialized by the trusted
    /// artifact layer. Metadata-only validation cannot prove that a CAS object
    /// still contains the bytes described by the manifest, so callers must
    /// use this boundary before decoding tensor values.
    pub fn validate_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        self.validate()?;
        if self.data_artifact.size_bytes != bytes.len() as u64 {
            return Err("tensor materialized bytes have the wrong size".into());
        }
        if self.data_artifact.sha256 != sha256_digest(bytes) {
            return Err("tensor materialized bytes checksum does not match".into());
        }
        if let Some(inline) = self.data_artifact.inline_bytes.as_deref()
            && inline != bytes
        {
            return Err("tensor materialized bytes differ from inline bytes".into());
        }
        if self.dtype == TensorDType::BigInt {
            validate_bigint_sign_magnitude(bytes, self.byte_order)?;
        }
        Ok(())
    }

    /// Return a canonical little-endian representation without changing the
    /// logical element order or IEEE-754 bit patterns. Complex values reverse
    /// each real/imaginary component independently, preserving signed zero,
    /// NaN, infinity, and subnormal payloads exactly.
    pub fn canonical_little_endian_bytes(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        self.validate_bytes(bytes)?;
        if self.byte_order == ByteOrder::Little {
            return Ok(bytes.to_vec());
        }

        if self.dtype == TensorDType::BigInt {
            let mut canonical = bytes.to_vec();
            canonical[1..].reverse();
            return Ok(canonical);
        }

        let element_width = self
            .dtype
            .byte_width()
            .ok_or_else(|| "tensor dtype has no fixed-width byte representation".to_string())?
            as usize;
        let component_width = self
            .dtype
            .component_byte_width()
            .ok_or_else(|| "tensor dtype has no component byte representation".to_string())?;
        let mut canonical = bytes.to_vec();
        for element in canonical.chunks_exact_mut(element_width) {
            if matches!(self.dtype, TensorDType::Complex64 | TensorDType::Complex128) {
                for component in element.chunks_exact_mut(component_width) {
                    component.reverse();
                }
            } else {
                element.reverse();
            }
        }
        Ok(canonical)
    }
}

fn validate_bigint_sign_magnitude(bytes: &[u8], byte_order: ByteOrder) -> Result<(), String> {
    if bytes.len() < 2 || !matches!(bytes[0], 0 | 1) {
        return Err("BigInt payload must use canonical sign-magnitude encoding".into());
    }
    let magnitude = &bytes[1..];
    if magnitude.len() > 1 {
        let redundant_zero = match byte_order {
            ByteOrder::Big => magnitude.first() == Some(&0),
            ByteOrder::Little => magnitude.last() == Some(&0),
        };
        if redundant_zero {
            return Err("BigInt payload must use canonical sign-magnitude encoding".into());
        }
    }
    Ok(())
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

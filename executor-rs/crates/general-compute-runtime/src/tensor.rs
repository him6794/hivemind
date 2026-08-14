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

/// Version for the sparse matrix metadata envelope. Sparse values use the
/// same binary artifact boundary as dense tensors, but their structural index
/// arrays have format-specific invariants.
pub const SPARSE_TENSOR_ABI_VERSION: &str = "sparse-tensor-v1alpha1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SparseFormat {
    Csr,
    Csc,
    Coo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SparseIndexDType {
    Int32,
    Int64,
    Uint32,
    Uint64,
}

impl SparseIndexDType {
    fn byte_width(self) -> u64 {
        match self {
            Self::Int32 | Self::Uint32 => 4,
            Self::Int64 | Self::Uint64 => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparseTensorManifest {
    pub abi_version: String,
    pub format: SparseFormat,
    pub shape: Vec<u64>,
    pub index_dtype: SparseIndexDType,
    pub byte_order: ByteOrder,
    pub index_base: u8,
    pub sorted_indices: bool,
    pub allow_duplicates: bool,
    pub indptr_artifact: Option<ArtifactManifest>,
    pub indices_artifact: ArtifactManifest,
    pub data_artifact: ArtifactManifest,
    pub data_dtype: TensorDType,
    pub logical_sha256: String,
}

impl SparseTensorManifest {
    pub fn canonical_logical_sha256(&self) -> String {
        let canonical = CanonicalSparseTensorManifest {
            abi_version: &self.abi_version,
            format: self.format,
            shape: &self.shape,
            index_dtype: self.index_dtype,
            byte_order: self.byte_order,
            index_base: self.index_base,
            sorted_indices: self.sorted_indices,
            allow_duplicates: self.allow_duplicates,
            indptr_sha256: self
                .indptr_artifact
                .as_ref()
                .map(|artifact| &artifact.sha256),
            indices_sha256: &self.indices_artifact.sha256,
            data_sha256: &self.data_artifact.sha256,
            data_dtype: self.data_dtype,
        };
        let bytes = serde_json::to_vec(&canonical)
            .expect("sparse tensor canonical serialization is infallible");
        sha256_digest(&bytes)
    }

    /// Validate sparse metadata and artifact sizes before any index/data
    /// bytes are decoded. Structural bounds, sorting, and duplicate checks are
    /// applied by the materialized-byte boundary added in the next ABI slice.
    pub fn validate(&self) -> Result<(), String> {
        if self.abi_version != SPARSE_TENSOR_ABI_VERSION {
            return Err("unsupported sparse tensor ABI version".into());
        }
        if self.shape.len() != 2 {
            return Err("sparse tensor shape must have exactly two dimensions".into());
        }
        if self.index_base > 1 {
            return Err("sparse tensor index base must be zero or one".into());
        }
        validate_sparse_binary_artifact("indices", &self.indices_artifact)?;
        validate_sparse_binary_artifact("data", &self.data_artifact)?;
        let index_width = self.index_dtype.byte_width();
        let index_size = self.indices_artifact.size_bytes;
        let nnz =
            match self.format {
                SparseFormat::Csr => {
                    let indptr = self.indptr_artifact.as_ref().ok_or_else(|| {
                        "CSR sparse tensor requires an indptr artifact".to_string()
                    })?;
                    validate_sparse_binary_artifact("indptr", indptr)?;
                    let expected = self.shape[0]
                        .checked_add(1)
                        .and_then(|count| count.checked_mul(index_width))
                        .ok_or_else(|| "CSR indptr size overflows".to_string())?;
                    if indptr.size_bytes != expected {
                        return Err("CSR indptr artifact size does not match shape".into());
                    }
                    if index_size % index_width != 0 {
                        return Err("CSR indices artifact is not aligned to index dtype".into());
                    }
                    index_size / index_width
                }
                SparseFormat::Csc => {
                    let indptr = self.indptr_artifact.as_ref().ok_or_else(|| {
                        "CSC sparse tensor requires an indptr artifact".to_string()
                    })?;
                    validate_sparse_binary_artifact("indptr", indptr)?;
                    let expected = self.shape[1]
                        .checked_add(1)
                        .and_then(|count| count.checked_mul(index_width))
                        .ok_or_else(|| "CSC indptr size overflows".to_string())?;
                    if indptr.size_bytes != expected {
                        return Err("CSC indptr artifact size does not match shape".into());
                    }
                    if index_size % index_width != 0 {
                        return Err("CSC indices artifact is not aligned to index dtype".into());
                    }
                    index_size / index_width
                }
                SparseFormat::Coo => {
                    if self.indptr_artifact.is_some() {
                        return Err("COO sparse tensor must not carry an indptr artifact".into());
                    }
                    let coordinate_width = index_width
                        .checked_mul(2)
                        .ok_or_else(|| "COO coordinate width overflows".to_string())?;
                    if index_size % coordinate_width != 0 {
                        return Err("COO indices artifact must contain coordinate pairs".into());
                    }
                    index_size / coordinate_width
                }
            };
        let data_width = self.data_dtype.byte_width().ok_or_else(|| {
            "sparse data dtype must have a fixed-width representation".to_string()
        })?;
        let expected_data = nnz
            .checked_mul(data_width)
            .ok_or_else(|| "sparse data size overflows".to_string())?;
        if self.data_artifact.size_bytes != expected_data {
            return Err("sparse data size does not match nonzero count and dtype".into());
        }
        if self.logical_sha256 != self.canonical_logical_sha256() {
            return Err("sparse tensor logical hash does not match canonical metadata".into());
        }
        Ok(())
    }
}

fn validate_sparse_binary_artifact(name: &str, artifact: &ArtifactManifest) -> Result<(), String> {
    if artifact.mime_type != "application/octet-stream" {
        return Err(format!(
            "sparse {name} artifact must use a binary tensor data MIME type"
        ));
    }
    artifact
        .validate()
        .map_err(|error| format!("sparse {name} artifact is invalid: {error}"))
}

#[derive(Debug, Serialize)]
struct CanonicalSparseTensorManifest<'a> {
    abi_version: &'a str,
    format: SparseFormat,
    shape: &'a [u64],
    index_dtype: SparseIndexDType,
    byte_order: ByteOrder,
    index_base: u8,
    sorted_indices: bool,
    allow_duplicates: bool,
    indptr_sha256: Option<&'a String>,
    indices_sha256: &'a String,
    data_sha256: &'a String,
    data_dtype: TensorDType,
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

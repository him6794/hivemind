//! Bounded sparse matrix reference kernels over the validated sparse ABI.
//!
//! This module deliberately consumes already materialized, manifest-bound
//! bytes. It normalizes CSR, CSC, and COO coordinates into a deterministic
//! entry list and provides one f64 matrix-vector product; optimized sparse
//! backends remain a separate capability and deployment gate.

use std::fmt;

use crate::tensor::{ByteOrder, SparseFormat, SparseIndexDType, SparseTensorManifest, TensorDType};

pub const MAX_REFERENCE_SPARSE_DIM: usize = 1_000_000;
pub const MAX_REFERENCE_SPARSE_NNZ: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SparseNumericError {
    ManifestInvalid(String),
    UnsupportedDataType(TensorDType),
    ShapeExceeded { dimension: u64, max: usize },
    NnzExceeded { requested: usize, max: usize },
    VectorLengthMismatch { expected: usize, actual: usize },
    NonFiniteValue,
}

impl fmt::Display for SparseNumericError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestInvalid(message) => {
                write!(formatter, "sparse manifest is invalid: {message}")
            }
            Self::UnsupportedDataType(dtype) => {
                write!(
                    formatter,
                    "sparse f64 reference does not support data dtype {dtype:?}"
                )
            }
            Self::ShapeExceeded { dimension, max } => {
                write!(
                    formatter,
                    "sparse dimension {dimension} exceeds reference limit {max}"
                )
            }
            Self::NnzExceeded { requested, max } => {
                write!(
                    formatter,
                    "sparse nonzero count {requested} exceeds reference limit {max}"
                )
            }
            Self::VectorLengthMismatch { expected, actual } => {
                write!(
                    formatter,
                    "sparse matvec expects vector length {expected}, got {actual}"
                )
            }
            Self::NonFiniteValue => {
                formatter.write_str("sparse matvec encountered a non-finite value")
            }
        }
    }
}

impl std::error::Error for SparseNumericError {}

#[derive(Debug, Clone, PartialEq)]
pub struct SparseF64Matrix {
    shape: [u64; 2],
    entries: Vec<(usize, usize, f64)>,
}

impl SparseF64Matrix {
    /// Construct a sparse f64 matrix only after validating every materialized
    /// byte against the manifest's checksums, shape, index policy, and format.
    pub fn from_materialized(
        manifest: &SparseTensorManifest,
        indptr_bytes: Option<&[u8]>,
        indices_bytes: &[u8],
        data_bytes: &[u8],
    ) -> Result<Self, SparseNumericError> {
        let (rows, columns) = checked_shape(manifest)?;
        if manifest.data_dtype != TensorDType::Float64 {
            return Err(SparseNumericError::UnsupportedDataType(manifest.data_dtype));
        }
        let index_width = manifest.index_dtype.byte_width_for_reference();
        let nnz = match manifest.format {
            SparseFormat::Coo => indices_bytes.len() / (index_width * 2),
            SparseFormat::Csr | SparseFormat::Csc => indices_bytes.len() / index_width,
        };
        if nnz > MAX_REFERENCE_SPARSE_NNZ {
            return Err(SparseNumericError::NnzExceeded {
                requested: nnz,
                max: MAX_REFERENCE_SPARSE_NNZ,
            });
        }
        manifest
            .validate_bytes(indptr_bytes, indices_bytes, data_bytes)
            .map_err(SparseNumericError::ManifestInvalid)?;

        let data = decode_values(data_bytes, manifest.byte_order);
        if data.iter().any(|value| !value.is_finite()) {
            return Err(SparseNumericError::NonFiniteValue);
        }
        let entries = match manifest.format {
            SparseFormat::Csr => decode_csr(
                manifest,
                indptr_bytes.ok_or_else(|| {
                    SparseNumericError::ManifestInvalid("CSR indptr bytes are missing".into())
                })?,
                indices_bytes,
                &data,
                rows,
                columns,
            )?,
            SparseFormat::Csc => decode_csc(
                manifest,
                indptr_bytes.ok_or_else(|| {
                    SparseNumericError::ManifestInvalid("CSC indptr bytes are missing".into())
                })?,
                indices_bytes,
                &data,
                rows,
                columns,
            )?,
            SparseFormat::Coo => decode_coo(manifest, indices_bytes, &data, rows, columns)?,
        };

        Ok(Self {
            shape: [manifest.shape[0], manifest.shape[1]],
            entries,
        })
    }

    #[must_use]
    pub const fn shape(&self) -> [u64; 2] {
        self.shape
    }

    #[must_use]
    pub fn nnz(&self) -> usize {
        self.entries.len()
    }

    pub fn matvec(&self, vector: &[f64]) -> Result<Vec<f64>, SparseNumericError> {
        let rows =
            usize::try_from(self.shape[0]).map_err(|_| SparseNumericError::ShapeExceeded {
                dimension: self.shape[0],
                max: MAX_REFERENCE_SPARSE_DIM,
            })?;
        let columns =
            usize::try_from(self.shape[1]).map_err(|_| SparseNumericError::ShapeExceeded {
                dimension: self.shape[1],
                max: MAX_REFERENCE_SPARSE_DIM,
            })?;
        if vector.len() != columns {
            return Err(SparseNumericError::VectorLengthMismatch {
                expected: columns,
                actual: vector.len(),
            });
        }
        if vector.iter().any(|value| !value.is_finite()) {
            return Err(SparseNumericError::NonFiniteValue);
        }
        let mut result = vec![0.0; rows];
        for &(row, column, value) in &self.entries {
            result[row] += value * vector[column];
            if !result[row].is_finite() {
                return Err(SparseNumericError::NonFiniteValue);
            }
        }
        Ok(result)
    }
}

fn checked_shape(manifest: &SparseTensorManifest) -> Result<(usize, usize), SparseNumericError> {
    if manifest.shape.len() != 2 {
        return Err(SparseNumericError::ManifestInvalid(
            "sparse tensor shape must have exactly two dimensions".into(),
        ));
    }
    let rows = checked_dimension(manifest.shape[0])?;
    let columns = checked_dimension(manifest.shape[1])?;
    Ok((rows, columns))
}

fn checked_dimension(dimension: u64) -> Result<usize, SparseNumericError> {
    let value = usize::try_from(dimension).map_err(|_| SparseNumericError::ShapeExceeded {
        dimension,
        max: MAX_REFERENCE_SPARSE_DIM,
    })?;
    if value > MAX_REFERENCE_SPARSE_DIM {
        return Err(SparseNumericError::ShapeExceeded {
            dimension,
            max: MAX_REFERENCE_SPARSE_DIM,
        });
    }
    Ok(value)
}

fn decode_values(bytes: &[u8], byte_order: ByteOrder) -> Vec<f64> {
    let mut values = Vec::with_capacity(bytes.len() / 8);
    for chunk in bytes.chunks_exact(8) {
        let raw: [u8; 8] = chunk.try_into().expect("chunks_exact produces eight bytes");
        values.push(match byte_order {
            ByteOrder::Little => f64::from_le_bytes(raw),
            ByteOrder::Big => f64::from_be_bytes(raw),
        });
    }
    values
}

fn decode_csr(
    manifest: &SparseTensorManifest,
    indptr_bytes: &[u8],
    indices_bytes: &[u8],
    data: &[f64],
    rows: usize,
    columns: usize,
) -> Result<Vec<(usize, usize, f64)>, SparseNumericError> {
    let mut entries = Vec::with_capacity(data.len());
    for row in 0..rows {
        let start = pointer_at(manifest, indptr_bytes, row, data.len())?;
        let end = pointer_at(manifest, indptr_bytes, row + 1, data.len())?;
        let segment = data.get(start..end).ok_or_else(|| {
            SparseNumericError::ManifestInvalid("CSR data segment is out of bounds".into())
        })?;
        for (offset, &value) in segment.iter().enumerate() {
            let position = start + offset;
            let column = coordinate_at(manifest, indices_bytes, position, columns)?;
            entries.push((row, column, value));
        }
    }
    Ok(entries)
}

fn decode_csc(
    manifest: &SparseTensorManifest,
    indptr_bytes: &[u8],
    indices_bytes: &[u8],
    data: &[f64],
    rows: usize,
    columns: usize,
) -> Result<Vec<(usize, usize, f64)>, SparseNumericError> {
    let mut entries = Vec::with_capacity(data.len());
    for column in 0..columns {
        let start = pointer_at(manifest, indptr_bytes, column, data.len())?;
        let end = pointer_at(manifest, indptr_bytes, column + 1, data.len())?;
        let segment = data.get(start..end).ok_or_else(|| {
            SparseNumericError::ManifestInvalid("CSC data segment is out of bounds".into())
        })?;
        for (offset, &value) in segment.iter().enumerate() {
            let position = start + offset;
            let row = coordinate_at(manifest, indices_bytes, position, rows)?;
            entries.push((row, column, value));
        }
    }
    Ok(entries)
}

fn decode_coo(
    manifest: &SparseTensorManifest,
    indices_bytes: &[u8],
    data: &[f64],
    rows: usize,
    columns: usize,
) -> Result<Vec<(usize, usize, f64)>, SparseNumericError> {
    let mut entries = Vec::with_capacity(data.len());
    for (position, &value) in data.iter().enumerate() {
        let row = coordinate_at(manifest, indices_bytes, position * 2, rows)?;
        let column = coordinate_at(manifest, indices_bytes, position * 2 + 1, columns)?;
        entries.push((row, column, value));
    }
    Ok(entries)
}

fn pointer_at(
    manifest: &SparseTensorManifest,
    bytes: &[u8],
    ordinal: usize,
    nnz: usize,
) -> Result<usize, SparseNumericError> {
    let raw = decode_index(bytes, ordinal, manifest.index_dtype, manifest.byte_order)?;
    normalize_pointer(raw, manifest.index_base, nnz)
}

fn coordinate_at(
    manifest: &SparseTensorManifest,
    bytes: &[u8],
    ordinal: usize,
    dimension: usize,
) -> Result<usize, SparseNumericError> {
    let raw = decode_index(bytes, ordinal, manifest.index_dtype, manifest.byte_order)?;
    normalize_index(raw, manifest.index_base, dimension)
}

fn normalize_index(raw: i128, index_base: u8, bound: usize) -> Result<usize, SparseNumericError> {
    let base = i128::from(index_base);
    let logical = raw.checked_sub(base).ok_or_else(|| {
        SparseNumericError::ManifestInvalid("sparse index is below its index base".into())
    })?;
    let value = usize::try_from(logical).map_err(|_| {
        SparseNumericError::ManifestInvalid("sparse index is negative or overflows usize".into())
    })?;
    if value >= bound {
        return Err(SparseNumericError::ManifestInvalid(
            "sparse index is out of bounds".into(),
        ));
    }
    Ok(value)
}

fn normalize_pointer(raw: i128, index_base: u8, bound: usize) -> Result<usize, SparseNumericError> {
    let base = i128::from(index_base);
    let logical = raw.checked_sub(base).ok_or_else(|| {
        SparseNumericError::ManifestInvalid("sparse pointer is below its index base".into())
    })?;
    let value = usize::try_from(logical).map_err(|_| {
        SparseNumericError::ManifestInvalid("sparse pointer is negative or overflows usize".into())
    })?;
    if value > bound {
        return Err(SparseNumericError::ManifestInvalid(
            "sparse pointer is out of bounds".into(),
        ));
    }
    Ok(value)
}

fn decode_index(
    bytes: &[u8],
    ordinal: usize,
    dtype: SparseIndexDType,
    byte_order: ByteOrder,
) -> Result<i128, SparseNumericError> {
    let width = dtype.byte_width_for_reference();
    let start = ordinal.checked_mul(width).ok_or_else(|| {
        SparseNumericError::ManifestInvalid("sparse index offset overflows".into())
    })?;
    let end = start.checked_add(width).ok_or_else(|| {
        SparseNumericError::ManifestInvalid("sparse index offset overflows".into())
    })?;
    let raw = bytes.get(start..end).ok_or_else(|| {
        SparseNumericError::ManifestInvalid("sparse index bytes are truncated".into())
    })?;
    Ok(match (dtype, byte_order) {
        (SparseIndexDType::Int32, ByteOrder::Little) => {
            i128::from(i32::from_le_bytes(raw.try_into().expect("index width")))
        }
        (SparseIndexDType::Int32, ByteOrder::Big) => {
            i128::from(i32::from_be_bytes(raw.try_into().expect("index width")))
        }
        (SparseIndexDType::Int64, ByteOrder::Little) => {
            i128::from(i64::from_le_bytes(raw.try_into().expect("index width")))
        }
        (SparseIndexDType::Int64, ByteOrder::Big) => {
            i128::from(i64::from_be_bytes(raw.try_into().expect("index width")))
        }
        (SparseIndexDType::Uint32, ByteOrder::Little) => {
            i128::from(u32::from_le_bytes(raw.try_into().expect("index width")))
        }
        (SparseIndexDType::Uint32, ByteOrder::Big) => {
            i128::from(u32::from_be_bytes(raw.try_into().expect("index width")))
        }
        (SparseIndexDType::Uint64, ByteOrder::Little) => {
            i128::from(u64::from_le_bytes(raw.try_into().expect("index width")))
        }
        (SparseIndexDType::Uint64, ByteOrder::Big) => {
            i128::from(u64::from_be_bytes(raw.try_into().expect("index width")))
        }
    })
}

trait SparseIndexWidth {
    fn byte_width_for_reference(self) -> usize;
}

impl SparseIndexWidth for SparseIndexDType {
    fn byte_width_for_reference(self) -> usize {
        match self {
            Self::Int32 | Self::Uint32 => 4,
            Self::Int64 | Self::Uint64 => 8,
        }
    }
}

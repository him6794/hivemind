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
pub const MAX_REFERENCE_SPARSE_SOLVE_DIM: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SparseNumericError {
    ManifestInvalid(String),
    UnsupportedDataType(TensorDType),
    ShapeExceeded { dimension: u64, max: usize },
    NnzExceeded { requested: usize, max: usize },
    VectorLengthMismatch { expected: usize, actual: usize },
    ResidualLengthMismatch { expected: usize, actual: usize },
    InvalidResidualTolerance,
    ResidualExceeded,
    SolveNotSquare { rows: usize, columns: usize },
    SolveDimensionExceeded { requested: usize, max: usize },
    SolveRhsLengthMismatch { expected: usize, actual: usize },
    SingularMatrix,
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
            Self::ResidualLengthMismatch { expected, actual } => {
                write!(
                    formatter,
                    "sparse residual expects RHS length {expected}, got {actual}"
                )
            }
            Self::InvalidResidualTolerance => {
                formatter.write_str("sparse residual tolerance must be finite and non-negative")
            }
            Self::ResidualExceeded => {
                formatter.write_str("sparse matvec residual exceeds the requested tolerance")
            }
            Self::SolveNotSquare { rows, columns } => {
                write!(
                    formatter,
                    "sparse solve requires a square matrix, got {rows}x{columns}"
                )
            }
            Self::SolveDimensionExceeded { requested, max } => {
                write!(
                    formatter,
                    "sparse solve dimension {requested} exceeds reference limit {max}"
                )
            }
            Self::SolveRhsLengthMismatch { expected, actual } => {
                write!(
                    formatter,
                    "sparse solve expects RHS length {expected}, got {actual}"
                )
            }
            Self::SingularMatrix => formatter.write_str("sparse solve matrix is singular"),
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

#[derive(Debug, Clone, PartialEq)]
pub struct CsrF64Matrix {
    pub shape: [u64; 2],
    pub indptr: Vec<u64>,
    pub indices: Vec<u64>,
    pub data: Vec<f64>,
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

    /// Compute the sequential infinity norm of `self * vector - rhs`.
    pub fn residual_inf_norm(
        &self,
        vector: &[f64],
        rhs: &[f64],
    ) -> Result<f64, SparseNumericError> {
        let result = self.matvec(vector)?;
        residual_inf_norm_from_result(&result, rhs)
    }

    /// Compute a sparse matvec and reject it when its infinity-norm residual
    /// exceeds a finite, non-negative tolerance.
    pub fn matvec_with_residual_tolerance(
        &self,
        vector: &[f64],
        rhs: &[f64],
        tolerance: f64,
    ) -> Result<Vec<f64>, SparseNumericError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(SparseNumericError::InvalidResidualTolerance);
        }
        let result = self.matvec(vector)?;
        if residual_inf_norm_from_result(&result, rhs)? > tolerance {
            return Err(SparseNumericError::ResidualExceeded);
        }
        Ok(result)
    }

    /// Reduce entries by row in deterministic source order.
    pub fn row_sums(&self) -> Result<Vec<f64>, SparseNumericError> {
        let (rows, _) = self.dimensions()?;
        let mut sums = vec![0.0; rows];
        for &(row, _, value) in &self.entries {
            sums[row] += value;
            if !sums[row].is_finite() {
                return Err(SparseNumericError::NonFiniteValue);
            }
        }
        Ok(sums)
    }

    /// Reduce entries by column in deterministic source order.
    pub fn column_sums(&self) -> Result<Vec<f64>, SparseNumericError> {
        let (_, columns) = self.dimensions()?;
        let mut sums = vec![0.0; columns];
        for &(_, column, value) in &self.entries {
            sums[column] += value;
            if !sums[column].is_finite() {
                return Err(SparseNumericError::NonFiniteValue);
            }
        }
        Ok(sums)
    }

    /// Convert CSR/CSC/COO entries into a deterministic row-major CSR form.
    pub fn to_csr(&self) -> Result<CsrF64Matrix, SparseNumericError> {
        let (rows, _) = self.dimensions()?;
        let mut sorted = self.entries.clone();
        sorted.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

        let mut indptr = vec![0u64; rows + 1];
        for &(row, _, _) in &sorted {
            indptr[row + 1] =
                indptr[row + 1]
                    .checked_add(1)
                    .ok_or(SparseNumericError::NnzExceeded {
                        requested: sorted.len(),
                        max: MAX_REFERENCE_SPARSE_NNZ,
                    })?;
        }
        for row in 1..=rows {
            indptr[row] = indptr[row].checked_add(indptr[row - 1]).ok_or(
                SparseNumericError::NnzExceeded {
                    requested: sorted.len(),
                    max: MAX_REFERENCE_SPARSE_NNZ,
                },
            )?;
        }

        let mut indices = Vec::with_capacity(sorted.len());
        let mut data = Vec::with_capacity(sorted.len());
        for &(_, column, value) in &sorted {
            indices.push(
                u64::try_from(column).map_err(|_| SparseNumericError::NnzExceeded {
                    requested: sorted.len(),
                    max: MAX_REFERENCE_SPARSE_NNZ,
                })?,
            );
            data.push(value);
        }
        Ok(CsrF64Matrix {
            shape: self.shape,
            indptr,
            indices,
            data,
        })
    }

    /// Solve a bounded square sparse system with deterministic dense
    /// partial-pivot elimination over the validated entry list.
    pub fn solve(&self, rhs: &[f64]) -> Result<Vec<f64>, SparseNumericError> {
        let (rows, columns) = self.dimensions()?;
        if rows != columns {
            return Err(SparseNumericError::SolveNotSquare { rows, columns });
        }
        if rows > MAX_REFERENCE_SPARSE_SOLVE_DIM {
            return Err(SparseNumericError::SolveDimensionExceeded {
                requested: rows,
                max: MAX_REFERENCE_SPARSE_SOLVE_DIM,
            });
        }
        if rhs.len() != rows {
            return Err(SparseNumericError::SolveRhsLengthMismatch {
                expected: rows,
                actual: rhs.len(),
            });
        }
        if rhs.iter().any(|value| !value.is_finite()) {
            return Err(SparseNumericError::NonFiniteValue);
        }

        let matrix_len =
            rows.checked_mul(rows)
                .ok_or(SparseNumericError::SolveDimensionExceeded {
                    requested: rows,
                    max: MAX_REFERENCE_SPARSE_SOLVE_DIM,
                })?;
        let mut matrix = vec![0.0; matrix_len];
        for &(row, column, value) in &self.entries {
            let index = row * rows + column;
            matrix[index] += value;
            if !matrix[index].is_finite() {
                return Err(SparseNumericError::NonFiniteValue);
            }
        }
        let mut rhs = rhs.to_vec();

        for column in 0..rows {
            let mut pivot = column;
            let mut pivot_abs = matrix[column * rows + column].abs();
            for row in (column + 1)..rows {
                let candidate = matrix[row * rows + column].abs();
                if candidate > pivot_abs {
                    pivot = row;
                    pivot_abs = candidate;
                }
            }
            if !pivot_abs.is_finite() {
                return Err(SparseNumericError::NonFiniteValue);
            }
            if pivot_abs <= f64::EPSILON {
                return Err(SparseNumericError::SingularMatrix);
            }
            if pivot != column {
                for offset in column..rows {
                    matrix.swap(column * rows + offset, pivot * rows + offset);
                }
                rhs.swap(column, pivot);
            }

            let diagonal = matrix[column * rows + column];
            for row in (column + 1)..rows {
                let factor = matrix[row * rows + column] / diagonal;
                if !factor.is_finite() {
                    return Err(SparseNumericError::NonFiniteValue);
                }
                matrix[row * rows + column] = 0.0;
                for offset in (column + 1)..rows {
                    let index = row * rows + offset;
                    matrix[index] -= factor * matrix[column * rows + offset];
                    if !matrix[index].is_finite() {
                        return Err(SparseNumericError::NonFiniteValue);
                    }
                }
                rhs[row] -= factor * rhs[column];
                if !rhs[row].is_finite() {
                    return Err(SparseNumericError::NonFiniteValue);
                }
            }
        }

        let mut solution = vec![0.0; rows];
        for row in (0..rows).rev() {
            let diagonal = matrix[row * rows + row];
            if !diagonal.is_finite() {
                return Err(SparseNumericError::NonFiniteValue);
            }
            if diagonal.abs() <= f64::EPSILON {
                return Err(SparseNumericError::SingularMatrix);
            }
            let mut remainder = rhs[row];
            for column in (row + 1)..rows {
                remainder -= matrix[row * rows + column] * solution[column];
            }
            let value = remainder / diagonal;
            if !value.is_finite() {
                return Err(SparseNumericError::NonFiniteValue);
            }
            solution[row] = value;
        }
        Ok(solution)
    }

    fn dimensions(&self) -> Result<(usize, usize), SparseNumericError> {
        Ok((
            checked_dimension(self.shape[0])?,
            checked_dimension(self.shape[1])?,
        ))
    }
}

fn residual_inf_norm_from_result(result: &[f64], rhs: &[f64]) -> Result<f64, SparseNumericError> {
    if rhs.len() != result.len() {
        return Err(SparseNumericError::ResidualLengthMismatch {
            expected: result.len(),
            actual: rhs.len(),
        });
    }
    if rhs.iter().any(|value| !value.is_finite()) {
        return Err(SparseNumericError::NonFiniteValue);
    }

    let mut maximum = 0.0_f64;
    for (&actual, &expected) in result.iter().zip(rhs) {
        let residual = (actual - expected).abs();
        if !residual.is_finite() {
            return Err(SparseNumericError::NonFiniteValue);
        }
        maximum = maximum.max(residual);
    }
    Ok(maximum)
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

//! Deterministic CPU reference kernels for the first S2 numerical slice.
//!
//! The kernels operate on validated scalar values in row-major dense tensors.
//! They intentionally stay independent of the binary tensor artifact layer so
//! callers can use them as a small reference implementation before wiring a
//! scientific backend image.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Sub};

pub const MAX_REFERENCE_FFT_LEN: usize = 4096;
pub const MAX_REFERENCE_LU_DIM: usize = 1024;
pub const MAX_REFERENCE_QR_DIM: usize = 1024;
pub const MAX_REFERENCE_SVD_DIM: usize = 1024;

type SvdTallFactors = (Vec<f64>, Vec<f64>, Vec<f64>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumericError {
    ShapeOverflow,
    ValueCountMismatch {
        expected: usize,
        actual: usize,
    },
    BroadcastIncompatible {
        axis: usize,
        lhs: u64,
        rhs: u64,
    },
    AxisOutOfBounds {
        axis: usize,
        rank: usize,
    },
    DuplicateAxis {
        axis: usize,
    },
    DotRequiresOneDimension,
    DotLengthMismatch {
        lhs: u64,
        rhs: u64,
    },
    BatchedMatmulRequiresThreeDimensions,
    BatchedMatmulBatchMismatch {
        lhs: u64,
        rhs: u64,
    },
    BatchedMatmulInnerDimensionMismatch {
        lhs: u64,
        rhs: u64,
    },
    SolveRequiresSquareMatrix,
    SolveDimensionMismatch {
        matrix: u64,
        rhs: u64,
    },
    LuRequiresSquareMatrix,
    LuDimensionExceeded {
        dimension: usize,
        max: usize,
    },
    QrRequiresTwoDimensions,
    QrRequiresAtLeastAsManyRows {
        rows: u64,
        columns: u64,
    },
    QrDimensionExceeded {
        rows: usize,
        columns: usize,
        max: usize,
    },
    QrDimensionMismatch {
        expected_rows: u64,
        expected_columns: u64,
        actual: Vec<u64>,
    },
    SvdRequiresTwoDimensions,
    SvdDimensionExceeded {
        rows: usize,
        columns: usize,
        max: usize,
    },
    SvdDimensionMismatch {
        expected_rows: u64,
        expected_columns: u64,
        actual: Vec<u64>,
    },
    SingularMatrix,
    InvalidResidualTolerance,
    ResidualExceeded,
    InvalidQrTolerance,
    QrErrorExceeded,
    InvalidSvdTolerance,
    SvdErrorExceeded,
    SvdNoConvergence,
    NonFiniteValue,
    FftRequiresOneDimension,
    RealFftRequiresOneDimension,
    RealFftSpectrumNotConjugateSymmetric,
    FftLengthExceeded {
        length: usize,
        max: usize,
    },
    InvalidFftTolerance,
    FftErrorExceeded,
    InvalidRealFftTolerance,
    RealFftErrorExceeded,
    MatmulRequiresTwoDimensions,
    MatmulInnerDimensionMismatch {
        lhs: u64,
        rhs: u64,
    },
}

impl fmt::Display for NumericError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShapeOverflow => formatter.write_str("tensor shape or stride overflows"),
            Self::ValueCountMismatch { expected, actual } => {
                write!(formatter, "tensor expects {expected} values, got {actual}")
            }
            Self::BroadcastIncompatible { axis, lhs, rhs } => {
                write!(
                    formatter,
                    "dimensions at axis {axis} cannot broadcast: {lhs} and {rhs}"
                )
            }
            Self::AxisOutOfBounds { axis, rank } => {
                write!(formatter, "axis {axis} is out of bounds for rank {rank}")
            }
            Self::DuplicateAxis { axis } => write!(formatter, "axis {axis} was specified twice"),
            Self::DotRequiresOneDimension => {
                formatter.write_str("dot requires one-dimensional tensors")
            }
            Self::DotLengthMismatch { lhs, rhs } => {
                write!(formatter, "dot dimensions do not match: {lhs} and {rhs}")
            }
            Self::BatchedMatmulRequiresThreeDimensions => {
                formatter.write_str("batched matmul requires three-dimensional tensors")
            }
            Self::BatchedMatmulBatchMismatch { lhs, rhs } => {
                write!(
                    formatter,
                    "batched matmul batch dimensions do not broadcast: {lhs} and {rhs}"
                )
            }
            Self::BatchedMatmulInnerDimensionMismatch { lhs, rhs } => {
                write!(
                    formatter,
                    "batched matmul inner dimensions do not match: {lhs} and {rhs}"
                )
            }
            Self::SolveRequiresSquareMatrix => {
                formatter.write_str("solve requires a square two-dimensional matrix")
            }
            Self::SolveDimensionMismatch { matrix, rhs } => {
                write!(
                    formatter,
                    "solve right-hand side dimension does not match matrix: {matrix} and {rhs}"
                )
            }
            Self::LuRequiresSquareMatrix => {
                formatter.write_str("LU factorization requires a square two-dimensional matrix")
            }
            Self::LuDimensionExceeded { dimension, max } => {
                write!(
                    formatter,
                    "LU dimension {dimension} exceeds reference limit {max}"
                )
            }
            Self::QrRequiresTwoDimensions => {
                formatter.write_str("QR factorization requires a two-dimensional matrix")
            }
            Self::QrRequiresAtLeastAsManyRows { rows, columns } => {
                write!(
                    formatter,
                    "QR factorization requires at least as many rows as columns: {rows} and {columns}"
                )
            }
            Self::QrDimensionExceeded { rows, columns, max } => {
                write!(
                    formatter,
                    "QR dimensions {rows}x{columns} exceed reference limit {max}"
                )
            }
            Self::QrDimensionMismatch {
                expected_rows,
                expected_columns,
                actual,
            } => {
                write!(
                    formatter,
                    "QR reconstruction expects shape {expected_rows}x{expected_columns}, got {actual:?}"
                )
            }
            Self::SvdRequiresTwoDimensions => {
                formatter.write_str("SVD factorization requires a two-dimensional matrix")
            }
            Self::SvdDimensionExceeded { rows, columns, max } => {
                write!(
                    formatter,
                    "SVD dimensions {rows}x{columns} exceed reference limit {max}"
                )
            }
            Self::SvdDimensionMismatch {
                expected_rows,
                expected_columns,
                actual,
            } => {
                write!(
                    formatter,
                    "SVD reconstruction expects shape {expected_rows}x{expected_columns}, got {actual:?}"
                )
            }
            Self::SingularMatrix => formatter.write_str("solve matrix is singular"),
            Self::InvalidResidualTolerance => {
                formatter.write_str("solve residual tolerance must be finite and non-negative")
            }
            Self::ResidualExceeded => {
                formatter.write_str("solve residual exceeds the requested tolerance")
            }
            Self::InvalidQrTolerance => {
                formatter.write_str("QR tolerance must be finite and non-negative")
            }
            Self::QrErrorExceeded => {
                formatter.write_str("QR reconstruction or orthogonality error exceeds tolerance")
            }
            Self::InvalidSvdTolerance => {
                formatter.write_str("SVD tolerance must be finite and non-negative")
            }
            Self::SvdErrorExceeded => {
                formatter.write_str("SVD reconstruction or orthogonality error exceeds tolerance")
            }
            Self::SvdNoConvergence => {
                formatter.write_str("SVD reference iteration did not converge")
            }
            Self::NonFiniteValue => {
                formatter.write_str("numeric operation produced a non-finite value")
            }
            Self::FftRequiresOneDimension => {
                formatter.write_str("FFT requires a one-dimensional tensor")
            }
            Self::RealFftRequiresOneDimension => {
                formatter.write_str("real FFT requires a one-dimensional tensor")
            }
            Self::RealFftSpectrumNotConjugateSymmetric => {
                formatter.write_str("real FFT spectrum is not conjugate symmetric")
            }
            Self::FftLengthExceeded { length, max } => {
                write!(
                    formatter,
                    "FFT length {length} exceeds reference limit {max}"
                )
            }
            Self::InvalidFftTolerance => {
                formatter.write_str("FFT round-trip tolerance must be finite and non-negative")
            }
            Self::FftErrorExceeded => {
                formatter.write_str("FFT round-trip error exceeds the requested tolerance")
            }
            Self::InvalidRealFftTolerance => {
                formatter.write_str("real FFT round-trip tolerance must be finite and non-negative")
            }
            Self::RealFftErrorExceeded => {
                formatter.write_str("real FFT round-trip error exceeds the requested tolerance")
            }
            Self::MatmulRequiresTwoDimensions => {
                formatter.write_str("matmul requires two-dimensional tensors")
            }
            Self::MatmulInnerDimensionMismatch { lhs, rhs } => {
                write!(
                    formatter,
                    "matmul inner dimensions do not match: {lhs} and {rhs}"
                )
            }
        }
    }
}

impl std::error::Error for NumericError {}

#[derive(Debug, Clone, PartialEq)]
pub struct DenseTensor<T> {
    shape: Vec<u64>,
    values: Vec<T>,
}

pub type F32Tensor = DenseTensor<f32>;
pub type F64Tensor = DenseTensor<f64>;
pub type Complex64Tensor = DenseTensor<Complex64>;
pub type Complex128Tensor = DenseTensor<Complex128>;

#[derive(Debug, Clone, PartialEq)]
pub struct LuFactorization {
    lower: F64Tensor,
    upper: F64Tensor,
    permutation: Vec<usize>,
}

impl LuFactorization {
    #[must_use]
    pub fn lower(&self) -> &F64Tensor {
        &self.lower
    }

    #[must_use]
    pub fn upper(&self) -> &F64Tensor {
        &self.upper
    }

    #[must_use]
    pub fn permutation(&self) -> &[usize] {
        &self.permutation
    }

    pub fn solve(&self, rhs: &F64Tensor) -> Result<F64Tensor, NumericError> {
        let size = self.permutation.len();
        if rhs.shape.len() != 1 || rhs.shape[0] != size as u64 {
            return Err(NumericError::SolveDimensionMismatch {
                matrix: size as u64,
                rhs: rhs.shape.first().copied().unwrap_or(0),
            });
        }
        if rhs.values.iter().any(|value| !value.is_finite()) {
            return Err(NumericError::NonFiniteValue);
        }

        let mut forward = vec![0.0; size];
        for row in 0..size {
            let mut value = rhs.values[self.permutation[row]];
            for (column, forward_value) in forward.iter().enumerate().take(row) {
                value -= self.lower.values[row * size + column] * forward_value;
            }
            if !value.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            forward[row] = value;
        }

        let mut solution = vec![0.0; size];
        for row in (0..size).rev() {
            let mut value = forward[row];
            for (column, solution_value) in solution.iter().enumerate().skip(row + 1) {
                value -= self.upper.values[row * size + column] * solution_value;
            }
            let pivot = self.upper.values[row * size + row];
            if pivot == 0.0 || !pivot.is_finite() {
                return Err(NumericError::SingularMatrix);
            }
            let solved = value / pivot;
            if !solved.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            solution[row] = solved;
        }

        F64Tensor::new(vec![size as u64], solution)
    }

    pub fn reconstruct_permuted(&self) -> Result<F64Tensor, NumericError> {
        let size = self.permutation.len();
        let mut product = vec![0.0; size * size];
        for row in 0..size {
            for column in 0..size {
                let mut value = 0.0;
                for inner in 0..size {
                    value += self.lower.values[row * size + inner]
                        * self.upper.values[inner * size + column];
                }
                if !value.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                product[row * size + column] = value;
            }
        }
        F64Tensor::new(vec![size as u64, size as u64], product)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QrFactorization {
    orthogonal: F64Tensor,
    upper: F64Tensor,
}

impl QrFactorization {
    #[must_use]
    pub fn q(&self) -> &F64Tensor {
        &self.orthogonal
    }

    #[must_use]
    pub fn r(&self) -> &F64Tensor {
        &self.upper
    }

    #[must_use]
    pub fn orthogonal(&self) -> &F64Tensor {
        &self.orthogonal
    }

    #[must_use]
    pub fn upper(&self) -> &F64Tensor {
        &self.upper
    }

    pub fn reconstruct(&self) -> Result<F64Tensor, NumericError> {
        let rows =
            usize::try_from(self.orthogonal.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        let columns =
            usize::try_from(self.orthogonal.shape[1]).map_err(|_| NumericError::ShapeOverflow)?;
        let mut values = vec![
            0.0;
            rows.checked_mul(columns)
                .ok_or(NumericError::ShapeOverflow)?
        ];
        for row in 0..rows {
            for column in 0..columns {
                let mut value = 0.0;
                for inner in 0..columns {
                    value += self.orthogonal.values[row * columns + inner]
                        * self.upper.values[inner * columns + column];
                    if !value.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                }
                values[row * columns + column] = value;
            }
        }
        F64Tensor::new(
            vec![
                u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
                u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
            ],
            values,
        )
    }

    /// Compute the component-wise infinity norm of `QᵀQ - I`.
    pub fn orthogonality_inf_norm(&self) -> Result<f64, NumericError> {
        let rows =
            usize::try_from(self.orthogonal.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        let columns =
            usize::try_from(self.orthogonal.shape[1]).map_err(|_| NumericError::ShapeOverflow)?;
        let mut maximum = 0.0_f64;
        for left in 0..columns {
            for right in 0..columns {
                let mut value = 0.0;
                for row in 0..rows {
                    value += self.orthogonal.values[row * columns + left]
                        * self.orthogonal.values[row * columns + right];
                    if !value.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                }
                let expected = if left == right { 1.0 } else { 0.0 };
                let error = (value - expected).abs();
                if !error.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                maximum = maximum.max(error);
            }
        }
        Ok(maximum)
    }

    /// Compute the infinity norm of `Q*R - original`.
    pub fn reconstruction_inf_norm(&self, original: &F64Tensor) -> Result<f64, NumericError> {
        let expected_rows = self.orthogonal.shape[0];
        let expected_columns = self.orthogonal.shape[1];
        if original.shape != [expected_rows, expected_columns] {
            return Err(NumericError::QrDimensionMismatch {
                expected_rows,
                expected_columns,
                actual: original.shape.clone(),
            });
        }
        if original.values.iter().any(|value| !value.is_finite()) {
            return Err(NumericError::NonFiniteValue);
        }

        let reconstructed = self.reconstruct()?;
        let mut maximum = 0.0_f64;
        for (actual, expected) in reconstructed.values.iter().zip(original.values.iter()) {
            let error = (actual - expected).abs();
            if !error.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            maximum = maximum.max(error);
        }
        Ok(maximum)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SvdFactorization {
    left: F64Tensor,
    singular_values: F64Tensor,
    right_transpose: F64Tensor,
}

impl SvdFactorization {
    #[must_use]
    pub fn u(&self) -> &F64Tensor {
        &self.left
    }

    #[must_use]
    pub fn left(&self) -> &F64Tensor {
        &self.left
    }

    #[must_use]
    pub fn singular_values(&self) -> &F64Tensor {
        &self.singular_values
    }

    #[must_use]
    pub fn s(&self) -> &F64Tensor {
        &self.singular_values
    }

    #[must_use]
    pub fn vt(&self) -> &F64Tensor {
        &self.right_transpose
    }

    #[must_use]
    pub fn right_transpose(&self) -> &F64Tensor {
        &self.right_transpose
    }

    pub fn reconstruct(&self) -> Result<F64Tensor, NumericError> {
        if self.left.shape.len() != 2
            || self.singular_values.shape.len() != 1
            || self.right_transpose.shape.len() != 2
        {
            return Err(NumericError::SvdRequiresTwoDimensions);
        }
        let rows = usize::try_from(self.left.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        let rank = usize::try_from(self.left.shape[1]).map_err(|_| NumericError::ShapeOverflow)?;
        let columns = usize::try_from(self.right_transpose.shape[1])
            .map_err(|_| NumericError::ShapeOverflow)?;
        if self.singular_values.shape[0] != self.left.shape[1]
            || self.right_transpose.shape[0] != self.left.shape[1]
        {
            return Err(NumericError::SvdDimensionMismatch {
                expected_rows: self.left.shape[0],
                expected_columns: self.right_transpose.shape[1],
                actual: self.singular_values.shape.clone(),
            });
        }

        let output_len = rows
            .checked_mul(columns)
            .ok_or(NumericError::ShapeOverflow)?;
        let mut output = vec![0.0; output_len];
        for row in 0..rows {
            for column in 0..columns {
                let mut value = 0.0;
                for component in 0..rank {
                    value += self.left.values[row * rank + component]
                        * self.singular_values.values[component]
                        * self.right_transpose.values[component * columns + column];
                    if !value.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                }
                output[row * columns + column] = value;
            }
        }
        F64Tensor::new(
            vec![
                u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
                u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
            ],
            output,
        )
    }

    /// Compute the maximum component-wise error in `UᵀU - I` and `VᵀV - I`.
    pub fn orthogonality_inf_norm(&self) -> Result<f64, NumericError> {
        if self.left.shape.len() != 2
            || self.singular_values.shape.len() != 1
            || self.right_transpose.shape.len() != 2
        {
            return Err(NumericError::SvdRequiresTwoDimensions);
        }
        let rows = usize::try_from(self.left.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        let rank = usize::try_from(self.left.shape[1]).map_err(|_| NumericError::ShapeOverflow)?;
        let columns = usize::try_from(self.right_transpose.shape[1])
            .map_err(|_| NumericError::ShapeOverflow)?;
        if self.singular_values.shape[0] != self.left.shape[1]
            || self.right_transpose.shape[0] != self.left.shape[1]
        {
            return Err(NumericError::SvdDimensionMismatch {
                expected_rows: self.left.shape[0],
                expected_columns: self.right_transpose.shape[1],
                actual: self.singular_values.shape.clone(),
            });
        }

        let mut maximum = 0.0_f64;
        for left_component in 0..rank {
            for right_component in 0..rank {
                let mut left_dot = 0.0;
                for row in 0..rows {
                    left_dot += self.left.values[row * rank + left_component]
                        * self.left.values[row * rank + right_component];
                    if !left_dot.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                }
                let expected = if left_component == right_component {
                    1.0
                } else {
                    0.0
                };
                let left_error = (left_dot - expected).abs();
                if !left_error.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                maximum = maximum.max(left_error);

                let mut right_dot = 0.0;
                for column in 0..columns {
                    right_dot += self.right_transpose.values[left_component * columns + column]
                        * self.right_transpose.values[right_component * columns + column];
                    if !right_dot.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                }
                let right_error = (right_dot - expected).abs();
                if !right_error.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                maximum = maximum.max(right_error);
            }
        }
        Ok(maximum)
    }

    /// Compute the infinity norm of `U*S*Vᵀ - original`.
    pub fn reconstruction_inf_norm(&self, original: &F64Tensor) -> Result<f64, NumericError> {
        let expected_rows = self.left.shape.first().copied().unwrap_or(0);
        let expected_columns = self.right_transpose.shape.get(1).copied().unwrap_or(0);
        if original.shape != [expected_rows, expected_columns] {
            return Err(NumericError::SvdDimensionMismatch {
                expected_rows,
                expected_columns,
                actual: original.shape.clone(),
            });
        }
        if original.values.iter().any(|value| !value.is_finite()) {
            return Err(NumericError::NonFiniteValue);
        }
        let reconstructed = self.reconstruct()?;
        let mut maximum = 0.0_f64;
        for (actual, expected) in reconstructed.values.iter().zip(original.values.iter()) {
            let error = (actual - expected).abs();
            if !error.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            maximum = maximum.max(error);
        }
        Ok(maximum)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Complex64 {
    pub re: f32,
    pub im: f32,
}

impl Complex64 {
    #[must_use]
    pub const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }
}

impl Add for Complex64 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.re + rhs.re, self.im + rhs.im)
    }
}

impl Sub for Complex64 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.re - rhs.re, self.im - rhs.im)
    }
}

impl Mul for Complex64 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self::new(
            self.re * rhs.re - self.im * rhs.im,
            self.re * rhs.im + self.im * rhs.re,
        )
    }
}

impl Div for Complex64 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        let denominator = rhs.re * rhs.re + rhs.im * rhs.im;
        Self::new(
            (self.re * rhs.re + self.im * rhs.im) / denominator,
            (self.im * rhs.re - self.re * rhs.im) / denominator,
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Complex128 {
    pub re: f64,
    pub im: f64,
}

impl Complex128 {
    #[must_use]
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
}

impl Add for Complex128 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.re + rhs.re, self.im + rhs.im)
    }
}

impl Sub for Complex128 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.re - rhs.re, self.im - rhs.im)
    }
}

impl Mul for Complex128 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self::new(
            self.re * rhs.re - self.im * rhs.im,
            self.re * rhs.im + self.im * rhs.re,
        )
    }
}

impl Div for Complex128 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        let denominator = rhs.re * rhs.re + rhs.im * rhs.im;
        Self::new(
            (self.re * rhs.re + self.im * rhs.im) / denominator,
            (self.im * rhs.re - self.re * rhs.im) / denominator,
        )
    }
}

impl<T> DenseTensor<T>
where
    T: Copy + Default + Add<Output = T> + Sub<Output = T> + Mul<Output = T> + Div<Output = T>,
{
    pub fn new(shape: Vec<u64>, values: Vec<T>) -> Result<Self, NumericError> {
        let expected = element_count(&shape)?;
        if values.len() != expected {
            return Err(NumericError::ValueCountMismatch {
                expected,
                actual: values.len(),
            });
        }
        Ok(Self { shape, values })
    }

    #[must_use]
    pub fn shape(&self) -> &[u64] {
        &self.shape
    }

    #[must_use]
    pub fn values(&self) -> &[T] {
        &self.values
    }

    pub fn elementwise_binary(
        &self,
        rhs: &Self,
        operation: BinaryOp,
    ) -> Result<Self, NumericError> {
        let output_shape = broadcast_shape(&self.shape, &rhs.shape)?;
        let output_len = element_count(&output_shape)?;
        let lhs_strides = row_major_strides(&self.shape)?;
        let rhs_strides = row_major_strides(&rhs.shape)?;
        let mut output = Vec::with_capacity(output_len);
        let mut coordinates = vec![0usize; output_shape.len()];

        for linear in 0..output_len {
            linear_to_coordinates(linear, &output_shape, &mut coordinates)?;
            let lhs_offset =
                broadcast_offset(&coordinates, &output_shape, &self.shape, &lhs_strides);
            let rhs_offset =
                broadcast_offset(&coordinates, &output_shape, &rhs.shape, &rhs_strides);
            output.push(apply_binary(
                self.values[lhs_offset],
                rhs.values[rhs_offset],
                operation,
            ));
        }

        Self::new(output_shape, output)
    }

    pub fn reduce_sum(&self, axes: &[usize], keep_dimensions: bool) -> Result<Self, NumericError> {
        let mut reduced = vec![false; self.shape.len()];
        for &axis in axes {
            if axis >= self.shape.len() {
                return Err(NumericError::AxisOutOfBounds {
                    axis,
                    rank: self.shape.len(),
                });
            }
            if reduced[axis] {
                return Err(NumericError::DuplicateAxis { axis });
            }
            reduced[axis] = true;
        }

        let output_shape: Vec<u64> = self
            .shape
            .iter()
            .enumerate()
            .filter_map(|(axis, &dimension)| {
                if reduced[axis] {
                    keep_dimensions.then_some(1)
                } else {
                    Some(dimension)
                }
            })
            .collect();
        let output_len = element_count(&output_shape)?;
        let output_strides = row_major_strides(&output_shape)?;
        let mut output = vec![T::default(); output_len];
        let mut coordinates = vec![0usize; self.shape.len()];

        for (linear, value) in self.values.iter().copied().enumerate() {
            linear_to_coordinates(linear, &self.shape, &mut coordinates)?;
            let mut output_linear = 0usize;
            let mut output_axis = 0usize;
            for (axis, &coordinate) in coordinates.iter().enumerate() {
                if reduced[axis] {
                    if keep_dimensions {
                        output_axis += 1;
                    }
                } else {
                    output_linear = output_linear
                        .checked_add(
                            coordinate
                                .checked_mul(output_strides[output_axis])
                                .ok_or(NumericError::ShapeOverflow)?,
                        )
                        .ok_or(NumericError::ShapeOverflow)?;
                    output_axis += 1;
                }
            }
            output[output_linear] = output[output_linear] + value;
        }

        Self::new(output_shape, output)
    }

    pub fn matmul(&self, rhs: &Self) -> Result<Self, NumericError> {
        if self.shape.len() != 2 || rhs.shape.len() != 2 {
            return Err(NumericError::MatmulRequiresTwoDimensions);
        }
        let (rows, inner) = (self.shape[0], self.shape[1]);
        let (rhs_inner, columns) = (rhs.shape[0], rhs.shape[1]);
        if inner != rhs_inner {
            return Err(NumericError::MatmulInnerDimensionMismatch {
                lhs: inner,
                rhs: rhs_inner,
            });
        }

        let rows = usize::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?;
        let inner = usize::try_from(inner).map_err(|_| NumericError::ShapeOverflow)?;
        let columns = usize::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?;
        let output_len = rows
            .checked_mul(columns)
            .ok_or(NumericError::ShapeOverflow)?;
        let mut output = Vec::with_capacity(output_len);
        for row in 0..rows {
            for column in 0..columns {
                let mut value = T::default();
                for shared in 0..inner {
                    let lhs_offset = row
                        .checked_mul(inner)
                        .and_then(|base| base.checked_add(shared))
                        .ok_or(NumericError::ShapeOverflow)?;
                    let rhs_offset = shared
                        .checked_mul(columns)
                        .and_then(|base| base.checked_add(column))
                        .ok_or(NumericError::ShapeOverflow)?;
                    value = value + self.values[lhs_offset] * rhs.values[rhs_offset];
                }
                output.push(value);
            }
        }
        let output_shape = vec![
            u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
            u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
        ];
        Self::new(output_shape, output)
    }

    pub fn dot(&self, rhs: &Self) -> Result<T, NumericError> {
        if self.shape.len() != 1 || rhs.shape.len() != 1 {
            return Err(NumericError::DotRequiresOneDimension);
        }
        let (lhs_length, rhs_length) = (self.shape[0], rhs.shape[0]);
        if lhs_length != rhs_length {
            return Err(NumericError::DotLengthMismatch {
                lhs: lhs_length,
                rhs: rhs_length,
            });
        }

        let mut result = T::default();
        for (&lhs, &rhs) in self.values.iter().zip(rhs.values.iter()) {
            result = result + lhs * rhs;
        }
        Ok(result)
    }

    pub fn batched_matmul(&self, rhs: &Self) -> Result<Self, NumericError> {
        if self.shape.len() != 3 || rhs.shape.len() != 3 {
            return Err(NumericError::BatchedMatmulRequiresThreeDimensions);
        }
        let (lhs_batch, rows, inner) = (self.shape[0], self.shape[1], self.shape[2]);
        let (rhs_batch, rhs_inner, columns) = (rhs.shape[0], rhs.shape[1], rhs.shape[2]);
        if inner != rhs_inner {
            return Err(NumericError::BatchedMatmulInnerDimensionMismatch {
                lhs: inner,
                rhs: rhs_inner,
            });
        }
        let batch = if lhs_batch == rhs_batch {
            lhs_batch
        } else if lhs_batch == 1 {
            rhs_batch
        } else if rhs_batch == 1 {
            lhs_batch
        } else {
            return Err(NumericError::BatchedMatmulBatchMismatch {
                lhs: lhs_batch,
                rhs: rhs_batch,
            });
        };

        let batch = usize::try_from(batch).map_err(|_| NumericError::ShapeOverflow)?;
        let lhs_batch = usize::try_from(lhs_batch).map_err(|_| NumericError::ShapeOverflow)?;
        let rhs_batch = usize::try_from(rhs_batch).map_err(|_| NumericError::ShapeOverflow)?;
        let rows = usize::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?;
        let inner = usize::try_from(inner).map_err(|_| NumericError::ShapeOverflow)?;
        let columns = usize::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?;
        let lhs_batch_stride = rows.checked_mul(inner).ok_or(NumericError::ShapeOverflow)?;
        let rhs_batch_stride = inner
            .checked_mul(columns)
            .ok_or(NumericError::ShapeOverflow)?;
        let mut output = Vec::with_capacity(
            batch
                .checked_mul(rows)
                .and_then(|count| count.checked_mul(columns))
                .ok_or(NumericError::ShapeOverflow)?,
        );

        for batch_index in 0..batch {
            let lhs_index = if lhs_batch == 1 { 0 } else { batch_index };
            let rhs_index = if rhs_batch == 1 { 0 } else { batch_index };
            let lhs_base = lhs_index
                .checked_mul(lhs_batch_stride)
                .ok_or(NumericError::ShapeOverflow)?;
            let rhs_base = rhs_index
                .checked_mul(rhs_batch_stride)
                .ok_or(NumericError::ShapeOverflow)?;
            for row in 0..rows {
                for column in 0..columns {
                    let mut value = T::default();
                    for shared in 0..inner {
                        let lhs_offset = lhs_base
                            .checked_add(
                                row.checked_mul(inner)
                                    .and_then(|base| base.checked_add(shared))
                                    .ok_or(NumericError::ShapeOverflow)?,
                            )
                            .ok_or(NumericError::ShapeOverflow)?;
                        let rhs_offset = rhs_base
                            .checked_add(
                                shared
                                    .checked_mul(columns)
                                    .and_then(|base| base.checked_add(column))
                                    .ok_or(NumericError::ShapeOverflow)?,
                            )
                            .ok_or(NumericError::ShapeOverflow)?;
                        value = value + self.values[lhs_offset] * rhs.values[rhs_offset];
                    }
                    output.push(value);
                }
            }
        }

        Self::new(
            vec![
                u64::try_from(batch).map_err(|_| NumericError::ShapeOverflow)?,
                u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
                u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
            ],
            output,
        )
    }
}

impl DenseTensor<f64> {
    /// Factor a bounded matrix with a deterministic thin one-sided Jacobi SVD.
    /// The returned factors satisfy `A = U * diag(S) * Vᵀ`, with `k = min(m,n)`.
    pub fn svd(&self) -> Result<SvdFactorization, NumericError> {
        if self.shape.len() != 2 {
            return Err(NumericError::SvdRequiresTwoDimensions);
        }
        let rows = usize::try_from(self.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        let columns = usize::try_from(self.shape[1]).map_err(|_| NumericError::ShapeOverflow)?;
        if rows > MAX_REFERENCE_SVD_DIM || columns > MAX_REFERENCE_SVD_DIM {
            return Err(NumericError::SvdDimensionExceeded {
                rows,
                columns,
                max: MAX_REFERENCE_SVD_DIM,
            });
        }
        if self.values.iter().any(|value| !value.is_finite()) {
            return Err(NumericError::NonFiniteValue);
        }

        if rows >= columns {
            let (left, singular_values, right_transpose) =
                svd_tall_values(&self.values, rows, columns)?;
            Ok(SvdFactorization {
                left: Self::new(
                    vec![
                        u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
                        u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
                    ],
                    left,
                )?,
                singular_values: Self::new(
                    vec![u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?],
                    singular_values,
                )?,
                right_transpose: Self::new(
                    vec![
                        u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
                        u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
                    ],
                    right_transpose,
                )?,
            })
        } else {
            let transposed_len = rows
                .checked_mul(columns)
                .ok_or(NumericError::ShapeOverflow)?;
            let mut transposed = Vec::with_capacity(transposed_len);
            for column in 0..columns {
                for row in 0..rows {
                    transposed.push(self.values[row * columns + column]);
                }
            }
            let (transposed_left, singular_values, transposed_right_transpose) =
                svd_tall_values(&transposed, columns, rows)?;
            let rank = rows;
            let mut left = vec![0.0; rows.checked_mul(rank).ok_or(NumericError::ShapeOverflow)?];
            for row in 0..rows {
                for component in 0..rank {
                    left[row * rank + component] =
                        transposed_right_transpose[component * rows + row];
                }
            }
            let mut right_transpose = vec![
                0.0;
                rank.checked_mul(columns)
                    .ok_or(NumericError::ShapeOverflow)?
            ];
            for component in 0..rank {
                for column in 0..columns {
                    right_transpose[component * columns + column] =
                        transposed_left[column * rank + component];
                }
            }
            Ok(SvdFactorization {
                left: Self::new(
                    vec![
                        u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
                        u64::try_from(rank).map_err(|_| NumericError::ShapeOverflow)?,
                    ],
                    left,
                )?,
                singular_values: Self::new(
                    vec![u64::try_from(rank).map_err(|_| NumericError::ShapeOverflow)?],
                    singular_values,
                )?,
                right_transpose: Self::new(
                    vec![
                        u64::try_from(rank).map_err(|_| NumericError::ShapeOverflow)?,
                        u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
                    ],
                    right_transpose,
                )?,
            })
        }
    }

    /// Factor and require SVD reconstruction and orthogonality errors to fit
    /// within a finite, non-negative reference tolerance.
    pub fn svd_with_tolerance(&self, tolerance: f64) -> Result<SvdFactorization, NumericError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(NumericError::InvalidSvdTolerance);
        }
        let factor = self.svd()?;
        if factor.orthogonality_inf_norm()? > tolerance
            || factor.reconstruction_inf_norm(self)? > tolerance
        {
            return Err(NumericError::SvdErrorExceeded);
        }
        Ok(factor)
    }

    /// Factor a bounded tall-or-square matrix with deterministic Householder
    /// reflections. The returned thin factors satisfy `A = Q * R`.
    pub fn qr(&self) -> Result<QrFactorization, NumericError> {
        if self.shape.len() != 2 {
            return Err(NumericError::QrRequiresTwoDimensions);
        }
        if self.shape[0] < self.shape[1] {
            return Err(NumericError::QrRequiresAtLeastAsManyRows {
                rows: self.shape[0],
                columns: self.shape[1],
            });
        }

        let rows = usize::try_from(self.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        let columns = usize::try_from(self.shape[1]).map_err(|_| NumericError::ShapeOverflow)?;
        if rows > MAX_REFERENCE_QR_DIM || columns > MAX_REFERENCE_QR_DIM {
            return Err(NumericError::QrDimensionExceeded {
                rows,
                columns,
                max: MAX_REFERENCE_QR_DIM,
            });
        }
        if self.values.iter().any(|value| !value.is_finite()) {
            return Err(NumericError::NonFiniteValue);
        }

        let matrix_len = rows
            .checked_mul(columns)
            .ok_or(NumericError::ShapeOverflow)?;
        let q_len = rows.checked_mul(rows).ok_or(NumericError::ShapeOverflow)?;
        let mut transformed = self.values.clone();
        debug_assert_eq!(transformed.len(), matrix_len);
        let mut full_q = vec![0.0; q_len];
        for diagonal in 0..rows {
            full_q[diagonal * rows + diagonal] = 1.0;
        }

        for column in 0..columns {
            let mut norm_squared = 0.0;
            for row in column..rows {
                norm_squared +=
                    transformed[row * columns + column] * transformed[row * columns + column];
                if !norm_squared.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
            }
            let norm = norm_squared.sqrt();
            if norm == 0.0 || !norm.is_finite() {
                return Err(NumericError::SingularMatrix);
            }

            let first = transformed[column * columns + column];
            let alpha = if first >= 0.0 { -norm } else { norm };
            let mut reflector = vec![0.0; rows - column];
            reflector[0] = first - alpha;
            for (offset, component) in reflector.iter_mut().enumerate().skip(1) {
                *component = transformed[(column + offset) * columns + column];
            }

            let mut reflector_norm_squared = 0.0;
            for &component in &reflector {
                reflector_norm_squared += component * component;
                if !reflector_norm_squared.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
            }
            if reflector_norm_squared == 0.0 || !reflector_norm_squared.is_finite() {
                return Err(NumericError::SingularMatrix);
            }

            for target_column in column..columns {
                let mut dot = 0.0;
                for (offset, &component) in reflector.iter().enumerate() {
                    dot += component * transformed[(column + offset) * columns + target_column];
                    if !dot.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                }
                let factor = (2.0 * dot) / reflector_norm_squared;
                if !factor.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                for (offset, &component) in reflector.iter().enumerate() {
                    let index = (column + offset) * columns + target_column;
                    let updated = transformed[index] - factor * component;
                    if !updated.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                    transformed[index] = updated;
                }
            }
            for row in (column + 1)..rows {
                transformed[row * columns + column] = 0.0;
            }

            for row in 0..rows {
                let mut dot = 0.0;
                for (offset, &component) in reflector.iter().enumerate() {
                    dot += full_q[row * rows + column + offset] * component;
                    if !dot.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                }
                let factor = (2.0 * dot) / reflector_norm_squared;
                if !factor.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                for (offset, &component) in reflector.iter().enumerate() {
                    let index = row * rows + column + offset;
                    let updated = full_q[index] - factor * component;
                    if !updated.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                    full_q[index] = updated;
                }
            }
        }

        let mut orthogonal = Vec::with_capacity(matrix_len);
        for row in 0..rows {
            for column in 0..columns {
                orthogonal.push(full_q[row * rows + column]);
            }
        }
        let upper_len = columns
            .checked_mul(columns)
            .ok_or(NumericError::ShapeOverflow)?;
        let mut upper = Vec::with_capacity(upper_len);
        for row in 0..columns {
            for column in 0..columns {
                upper.push(transformed[row * columns + column]);
            }
        }

        Ok(QrFactorization {
            orthogonal: Self::new(
                vec![
                    u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
                    u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
                ],
                orthogonal,
            )?,
            upper: Self::new(
                vec![
                    u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
                    u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
                ],
                upper,
            )?,
        })
    }

    /// Factor and require both reconstruction and orthogonality errors to fit
    /// within a finite, non-negative reference tolerance.
    pub fn qr_with_tolerance(&self, tolerance: f64) -> Result<QrFactorization, NumericError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(NumericError::InvalidQrTolerance);
        }
        let factor = self.qr()?;
        if factor.orthogonality_inf_norm()? > tolerance
            || factor.reconstruction_inf_norm(self)? > tolerance
        {
            return Err(NumericError::QrErrorExceeded);
        }
        Ok(factor)
    }

    /// Factor a bounded square matrix with deterministic partial pivoting.
    ///
    /// The returned factors satisfy `P * A = L * U`, where each entry in the
    /// permutation records the original row now stored at that pivoted row.
    /// This is a small reference implementation rather than a production
    /// BLAS/LAPACK backend, so its dimension is explicitly capped.
    pub fn lu(&self) -> Result<LuFactorization, NumericError> {
        if self.shape.len() != 2 || self.shape[0] != self.shape[1] {
            return Err(NumericError::LuRequiresSquareMatrix);
        }

        let size = usize::try_from(self.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        if size > MAX_REFERENCE_LU_DIM {
            return Err(NumericError::LuDimensionExceeded {
                dimension: size,
                max: MAX_REFERENCE_LU_DIM,
            });
        }
        if self.values.iter().any(|value| !value.is_finite()) {
            return Err(NumericError::NonFiniteValue);
        }

        let matrix_len = size.checked_mul(size).ok_or(NumericError::ShapeOverflow)?;
        let mut combined = self.values.clone();
        debug_assert_eq!(combined.len(), matrix_len);
        let mut permutation: Vec<usize> = (0..size).collect();

        for column in 0..size {
            let mut pivot_row = column;
            for row in (column + 1)..size {
                if combined[row * size + column].abs() > combined[pivot_row * size + column].abs() {
                    pivot_row = row;
                }
            }

            let pivot = combined[pivot_row * size + column];
            if pivot == 0.0 || !pivot.is_finite() {
                return Err(NumericError::SingularMatrix);
            }
            if pivot_row != column {
                for index in 0..size {
                    combined.swap(column * size + index, pivot_row * size + index);
                }
                permutation.swap(column, pivot_row);
            }

            let pivot = combined[column * size + column];
            for row in (column + 1)..size {
                let factor = combined[row * size + column] / pivot;
                if !factor.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                combined[row * size + column] = factor;
                for index in (column + 1)..size {
                    let updated =
                        combined[row * size + index] - factor * combined[column * size + index];
                    if !updated.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                    combined[row * size + index] = updated;
                }
            }
        }

        let mut lower = vec![0.0; matrix_len];
        let mut upper = vec![0.0; matrix_len];
        for row in 0..size {
            for column in 0..size {
                let value = combined[row * size + column];
                if row > column {
                    lower[row * size + column] = value;
                } else {
                    upper[row * size + column] = value;
                }
            }
            lower[row * size + row] = 1.0;
        }

        Ok(LuFactorization {
            lower: Self::new(vec![self.shape[0], self.shape[1]], lower)?,
            upper: Self::new(vec![self.shape[0], self.shape[1]], upper)?,
            permutation,
        })
    }

    pub fn solve(&self, rhs: &Self) -> Result<Self, NumericError> {
        if self.shape.len() != 2 || self.shape[0] != self.shape[1] {
            return Err(NumericError::SolveRequiresSquareMatrix);
        }
        if rhs.shape.len() != 1 || rhs.shape[0] != self.shape[0] {
            return Err(NumericError::SolveDimensionMismatch {
                matrix: self.shape[0],
                rhs: rhs.shape.first().copied().unwrap_or(0),
            });
        }
        if self.values.iter().any(|value| !value.is_finite())
            || rhs.values.iter().any(|value| !value.is_finite())
        {
            return Err(NumericError::NonFiniteValue);
        }

        let size = usize::try_from(self.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        let mut matrix = self.values.clone();
        let mut solution = rhs.values.clone();

        for column in 0..size {
            let mut pivot_row = column;
            for row in (column + 1)..size {
                if matrix[row * size + column].abs() > matrix[pivot_row * size + column].abs() {
                    pivot_row = row;
                }
            }
            let pivot = matrix[pivot_row * size + column];
            if pivot == 0.0 || !pivot.is_finite() {
                return Err(NumericError::SingularMatrix);
            }
            if pivot_row != column {
                for index in 0..size {
                    matrix.swap(column * size + index, pivot_row * size + index);
                }
                solution.swap(column, pivot_row);
            }

            let pivot = matrix[column * size + column];
            for row in (column + 1)..size {
                let factor = matrix[row * size + column] / pivot;
                if !factor.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                matrix[row * size + column] = 0.0;
                for index in (column + 1)..size {
                    let updated =
                        matrix[row * size + index] - factor * matrix[column * size + index];
                    if !updated.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                    matrix[row * size + index] = updated;
                }
                let updated_rhs = solution[row] - factor * solution[column];
                if !updated_rhs.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                solution[row] = updated_rhs;
            }
        }

        for row in (0..size).rev() {
            let mut value = solution[row];
            for index in (row + 1)..size {
                value -= matrix[row * size + index] * solution[index];
            }
            let pivot = matrix[row * size + row];
            if pivot == 0.0 || !pivot.is_finite() {
                return Err(NumericError::SingularMatrix);
            }
            let solved = value / pivot;
            if !solved.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            solution[row] = solved;
        }

        Self::new(
            vec![u64::try_from(size).map_err(|_| NumericError::ShapeOverflow)?],
            solution,
        )
    }

    /// Compute the infinity norm of `self * solution - rhs` for a solved
    /// square system. The arithmetic is intentionally sequential so the
    /// reference result is replayable across supported backends.
    pub fn residual_inf_norm(&self, solution: &Self, rhs: &Self) -> Result<f64, NumericError> {
        if self.shape.len() != 2 || self.shape[0] != self.shape[1] {
            return Err(NumericError::SolveRequiresSquareMatrix);
        }
        if rhs.shape.len() != 1 || rhs.shape[0] != self.shape[0] {
            return Err(NumericError::SolveDimensionMismatch {
                matrix: self.shape[0],
                rhs: rhs.shape.first().copied().unwrap_or(0),
            });
        }
        if solution.shape.len() != 1 || solution.shape[0] != self.shape[0] {
            return Err(NumericError::SolveDimensionMismatch {
                matrix: self.shape[0],
                rhs: solution.shape.first().copied().unwrap_or(0),
            });
        }
        if self.values.iter().any(|value| !value.is_finite())
            || solution.values.iter().any(|value| !value.is_finite())
            || rhs.values.iter().any(|value| !value.is_finite())
        {
            return Err(NumericError::NonFiniteValue);
        }

        let size = usize::try_from(self.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        let mut maximum = 0.0_f64;
        for row in 0..size {
            let mut predicted = 0.0;
            for column in 0..size {
                predicted += self.values[row * size + column] * solution.values[column];
                if !predicted.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
            }
            let residual = (predicted - rhs.values[row]).abs();
            if !residual.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            maximum = maximum.max(residual);
        }
        Ok(maximum)
    }

    /// Solve a square system and reject the result unless its infinity-norm
    /// residual is within a finite, non-negative tolerance.
    pub fn solve_with_residual(&self, rhs: &Self, tolerance: f64) -> Result<Self, NumericError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(NumericError::InvalidResidualTolerance);
        }
        let solution = self.solve(rhs)?;
        if self.residual_inf_norm(&solution, rhs)? > tolerance {
            return Err(NumericError::ResidualExceeded);
        }
        Ok(solution)
    }

    /// Compute a bounded full-spectrum forward DFT for a real signal.
    /// The output uses the same unscaled convention as `Complex128Tensor::fft`.
    pub fn real_fft(&self) -> Result<Complex128Tensor, NumericError> {
        if self.shape.len() != 1 {
            return Err(NumericError::RealFftRequiresOneDimension);
        }
        let length = usize::try_from(self.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        if length > MAX_REFERENCE_FFT_LEN {
            return Err(NumericError::FftLengthExceeded {
                length,
                max: MAX_REFERENCE_FFT_LEN,
            });
        }
        if self.values.iter().any(|value| !value.is_finite()) {
            return Err(NumericError::NonFiniteValue);
        }
        if length == 0 {
            return Complex128Tensor::new(vec![0], Vec::new());
        }

        let length_as_f64 = length as f64;
        let mut output = Vec::with_capacity(length);
        for frequency in 0..length {
            let mut sum = Complex128::default();
            for sample in 0..length {
                let angle =
                    -2.0 * std::f64::consts::PI * frequency as f64 * sample as f64 / length_as_f64;
                let (sin, cos) = angle.sin_cos();
                sum.re += self.values[sample] * cos;
                sum.im += self.values[sample] * sin;
                if !sum.re.is_finite() || !sum.im.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
            }
            output.push(sum);
        }
        Complex128Tensor::new(
            vec![u64::try_from(length).map_err(|_| NumericError::ShapeOverflow)?],
            output,
        )
    }

    /// Alias for `real_fft` using the conventional rFFT spelling.
    pub fn rfft(&self) -> Result<Complex128Tensor, NumericError> {
        self.real_fft()
    }

    /// Invert a full conjugate-symmetric real-spectrum DFT with `1/n`
    /// normalization and reject spectra that cannot represent real samples.
    pub fn inverse_real_fft(spectrum: &Complex128Tensor) -> Result<Self, NumericError> {
        if spectrum.shape.len() != 1 {
            return Err(NumericError::RealFftRequiresOneDimension);
        }
        let length = usize::try_from(spectrum.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        if length > MAX_REFERENCE_FFT_LEN {
            return Err(NumericError::FftLengthExceeded {
                length,
                max: MAX_REFERENCE_FFT_LEN,
            });
        }
        if spectrum
            .values
            .iter()
            .any(|value| !value.re.is_finite() || !value.im.is_finite())
        {
            return Err(NumericError::NonFiniteValue);
        }
        if length == 0 {
            return Self::new(vec![0], Vec::new());
        }

        let symmetry_tolerance = 1e-10_f64;
        let validate_imaginary = |value: f64| {
            let scale = value.abs().max(1.0);
            value.abs() <= symmetry_tolerance * scale
        };
        if !validate_imaginary(spectrum.values[0].im) {
            return Err(NumericError::RealFftSpectrumNotConjugateSymmetric);
        }
        if length % 2 == 0 && !validate_imaginary(spectrum.values[length / 2].im) {
            return Err(NumericError::RealFftSpectrumNotConjugateSymmetric);
        }
        for frequency in 1..=((length - 1) / 2) {
            let mirror = length - frequency;
            let left = spectrum.values[frequency];
            let right = spectrum.values[mirror];
            let scale = left
                .re
                .abs()
                .max(right.re.abs())
                .max(left.im.abs())
                .max(right.im.abs())
                .max(1.0);
            if (left.re - right.re).abs() > symmetry_tolerance * scale
                || (left.im + right.im).abs() > symmetry_tolerance * scale
            {
                return Err(NumericError::RealFftSpectrumNotConjugateSymmetric);
            }
        }

        let length_as_f64 = length as f64;
        let mut output = Vec::with_capacity(length);
        for sample in 0..length {
            let mut real = 0.0;
            let mut imaginary = 0.0;
            for frequency in 0..length {
                let angle =
                    2.0 * std::f64::consts::PI * frequency as f64 * sample as f64 / length_as_f64;
                let (sin, cos) = angle.sin_cos();
                let value = spectrum.values[frequency];
                real += value.re * cos - value.im * sin;
                imaginary += value.re * sin + value.im * cos;
                if !real.is_finite() || !imaginary.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
            }
            real /= length_as_f64;
            imaginary /= length_as_f64;
            if !real.is_finite() || !imaginary.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            let scale = real.abs().max(1.0);
            if imaginary.abs() > symmetry_tolerance * scale {
                return Err(NumericError::RealFftSpectrumNotConjugateSymmetric);
            }
            output.push(real);
        }
        Self::new(
            vec![u64::try_from(length).map_err(|_| NumericError::ShapeOverflow)?],
            output,
        )
    }

    /// Alias for `inverse_real_fft` using the conventional rFFT spelling.
    pub fn irfft(spectrum: &Complex128Tensor) -> Result<Self, NumericError> {
        Self::inverse_real_fft(spectrum)
    }

    /// Compute the maximum absolute error after a real FFT/IFFT round trip.
    pub fn real_fft_round_trip_error_inf_norm(&self) -> Result<f64, NumericError> {
        let spectrum = self.real_fft()?;
        let round_trip = Self::inverse_real_fft(&spectrum)?;
        let mut maximum = 0.0_f64;
        for (actual, expected) in round_trip.values.iter().zip(self.values.iter()) {
            let error = (actual - expected).abs();
            if !error.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            maximum = maximum.max(error);
        }
        Ok(maximum)
    }

    /// Alias for `real_fft_round_trip_error_inf_norm` using rFFT spelling.
    pub fn rfft_round_trip_error_inf_norm(&self) -> Result<f64, NumericError> {
        self.real_fft_round_trip_error_inf_norm()
    }

    /// Require a real FFT/IFFT round trip to stay within a finite,
    /// non-negative component-wise error tolerance.
    pub fn real_fft_with_round_trip_tolerance(&self, tolerance: f64) -> Result<Self, NumericError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(NumericError::InvalidRealFftTolerance);
        }
        if self.real_fft_round_trip_error_inf_norm()? > tolerance {
            return Err(NumericError::RealFftErrorExceeded);
        }
        Ok(self.clone())
    }

    /// Alias for `real_fft_with_round_trip_tolerance` using rFFT spelling.
    pub fn rfft_with_round_trip_tolerance(&self, tolerance: f64) -> Result<Self, NumericError> {
        self.real_fft_with_round_trip_tolerance(tolerance)
    }
}

impl DenseTensor<Complex128> {
    pub fn fft(&self, inverse: bool) -> Result<Self, NumericError> {
        if self.shape.len() != 1 {
            return Err(NumericError::FftRequiresOneDimension);
        }
        let length = usize::try_from(self.shape[0]).map_err(|_| NumericError::ShapeOverflow)?;
        if length > MAX_REFERENCE_FFT_LEN {
            return Err(NumericError::FftLengthExceeded {
                length,
                max: MAX_REFERENCE_FFT_LEN,
            });
        }
        if self
            .values
            .iter()
            .any(|value| !value.re.is_finite() || !value.im.is_finite())
        {
            return Err(NumericError::NonFiniteValue);
        }
        if length == 0 {
            return Self::new(vec![0], Vec::new());
        }

        let sign = if inverse { 1.0 } else { -1.0 };
        let scale = if inverse { 1.0 / length as f64 } else { 1.0 };
        let length_as_f64 = length as f64;
        let mut output = Vec::with_capacity(length);
        for frequency in 0..length {
            let mut sum = Complex128::default();
            for sample in 0..length {
                let angle = sign * 2.0 * std::f64::consts::PI * frequency as f64 * sample as f64
                    / length_as_f64;
                let (sin, cos) = angle.sin_cos();
                sum = sum + self.values[sample] * Complex128::new(cos, sin);
            }
            let value = sum * Complex128::new(scale, 0.0);
            if !value.re.is_finite() || !value.im.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            output.push(value);
        }
        Self::new(
            vec![u64::try_from(length).map_err(|_| NumericError::ShapeOverflow)?],
            output,
        )
    }

    /// Compute the maximum component-wise error after a forward/inverse
    /// reference FFT round trip.
    pub fn fft_round_trip_error_inf_norm(&self) -> Result<f64, NumericError> {
        let spectrum = self.fft(false)?;
        let round_trip = spectrum.fft(true)?;
        let mut maximum = 0.0_f64;
        for (actual, expected) in round_trip.values.iter().zip(self.values.iter()) {
            let error = (actual.re - expected.re)
                .abs()
                .max((actual.im - expected.im).abs());
            if !error.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
            maximum = maximum.max(error);
        }
        Ok(maximum)
    }

    /// Require a forward/inverse reference FFT round trip to stay within a
    /// finite, non-negative component-wise error tolerance.
    pub fn fft_with_round_trip_tolerance(&self, tolerance: f64) -> Result<Self, NumericError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(NumericError::InvalidFftTolerance);
        }
        let error = self.fft_round_trip_error_inf_norm()?;
        if error > tolerance {
            return Err(NumericError::FftErrorExceeded);
        }
        Ok(self.clone())
    }
}

fn svd_tall_values(
    matrix: &[f64],
    rows: usize,
    columns: usize,
) -> Result<SvdTallFactors, NumericError> {
    let matrix_len = rows
        .checked_mul(columns)
        .ok_or(NumericError::ShapeOverflow)?;
    if matrix.len() != matrix_len || rows < columns {
        return Err(NumericError::SvdDimensionMismatch {
            expected_rows: u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
            expected_columns: u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
            actual: vec![
                u64::try_from(rows).map_err(|_| NumericError::ShapeOverflow)?,
                u64::try_from(columns).map_err(|_| NumericError::ShapeOverflow)?,
            ],
        });
    }
    if matrix.iter().any(|value| !value.is_finite()) {
        return Err(NumericError::NonFiniteValue);
    }

    let mut work = matrix.to_vec();
    let v_len = columns
        .checked_mul(columns)
        .ok_or(NumericError::ShapeOverflow)?;
    let mut right = vec![0.0; v_len];
    for diagonal in 0..columns {
        right[diagonal * columns + diagonal] = 1.0;
    }

    let mut converged = columns < 2;
    for _ in 0..128 {
        let mut rotated = false;
        for first in 0..columns {
            for second in (first + 1)..columns {
                let mut first_norm_squared = 0.0;
                let mut second_norm_squared = 0.0;
                let mut cross = 0.0;
                for row in 0..rows {
                    let first_value = work[row * columns + first];
                    let second_value = work[row * columns + second];
                    first_norm_squared += first_value * first_value;
                    second_norm_squared += second_value * second_value;
                    cross += first_value * second_value;
                    if !first_norm_squared.is_finite()
                        || !second_norm_squared.is_finite()
                        || !cross.is_finite()
                    {
                        return Err(NumericError::NonFiniteValue);
                    }
                }
                let scale = (first_norm_squared * second_norm_squared).sqrt();
                if !scale.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                if cross == 0.0 || cross.abs() <= f64::EPSILON * scale {
                    continue;
                }

                let tau = (second_norm_squared - first_norm_squared) / (2.0 * cross);
                if !tau.is_finite() {
                    return Err(NumericError::SvdNoConvergence);
                }
                let tau_norm = tau.hypot(1.0);
                let tangent = if tau >= 0.0 {
                    1.0 / (tau + tau_norm)
                } else {
                    -1.0 / (-tau + tau_norm)
                };
                let cosine = 1.0 / (1.0 + tangent * tangent).sqrt();
                let sine = tangent * cosine;
                if !cosine.is_finite() || !sine.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }

                for row in 0..rows {
                    let first_index = row * columns + first;
                    let second_index = row * columns + second;
                    let first_value = work[first_index];
                    let second_value = work[second_index];
                    let updated_first = cosine * first_value - sine * second_value;
                    let updated_second = sine * first_value + cosine * second_value;
                    if !updated_first.is_finite() || !updated_second.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                    work[first_index] = updated_first;
                    work[second_index] = updated_second;
                }
                for row in 0..columns {
                    let first_index = row * columns + first;
                    let second_index = row * columns + second;
                    let first_value = right[first_index];
                    let second_value = right[second_index];
                    let updated_first = cosine * first_value - sine * second_value;
                    let updated_second = sine * first_value + cosine * second_value;
                    if !updated_first.is_finite() || !updated_second.is_finite() {
                        return Err(NumericError::NonFiniteValue);
                    }
                    right[first_index] = updated_first;
                    right[second_index] = updated_second;
                }
                rotated = true;
            }
        }
        if !rotated {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(NumericError::SvdNoConvergence);
    }

    let mut singular_values = vec![0.0; columns];
    let mut largest = 0.0_f64;
    for column in 0..columns {
        let mut norm_squared = 0.0;
        for row in 0..rows {
            norm_squared += work[row * columns + column] * work[row * columns + column];
            if !norm_squared.is_finite() {
                return Err(NumericError::NonFiniteValue);
            }
        }
        let norm = norm_squared.sqrt();
        if !norm.is_finite() {
            return Err(NumericError::NonFiniteValue);
        }
        singular_values[column] = norm;
        largest = largest.max(norm);
    }

    let rank_floor = f64::EPSILON * (rows.max(columns) as f64) * largest.max(1.0) * 32.0;
    let mut order: Vec<usize> = (0..columns).collect();
    order.sort_by(|&left, &right| {
        singular_values[right]
            .partial_cmp(&singular_values[left])
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.cmp(&right))
    });

    let left_len = rows
        .checked_mul(columns)
        .ok_or(NumericError::ShapeOverflow)?;
    let mut left = vec![0.0; left_len];
    let mut right_transpose = vec![0.0; v_len];
    for (component, &source_column) in order.iter().enumerate() {
        let singular_value = singular_values[source_column];
        for column in 0..columns {
            right_transpose[component * columns + column] = right[column * columns + source_column];
        }

        if singular_value > rank_floor {
            for row in 0..rows {
                let value = work[row * columns + source_column] / singular_value;
                if !value.is_finite() {
                    return Err(NumericError::NonFiniteValue);
                }
                left[row * columns + component] = value;
            }
        } else {
            let mut completed = false;
            for basis in 0..rows {
                let mut candidate = vec![0.0; rows];
                candidate[basis] = 1.0;
                for prior in 0..component {
                    let mut projection = 0.0;
                    for row in 0..rows {
                        projection += left[row * columns + prior] * candidate[row];
                    }
                    for row in 0..rows {
                        candidate[row] -= projection * left[row * columns + prior];
                    }
                }
                let mut candidate_norm_squared = 0.0;
                for &value in &candidate {
                    candidate_norm_squared += value * value;
                }
                if candidate_norm_squared > rank_floor * rank_floor
                    && candidate_norm_squared.is_finite()
                {
                    let candidate_norm = candidate_norm_squared.sqrt();
                    for row in 0..rows {
                        left[row * columns + component] = candidate[row] / candidate_norm;
                    }
                    completed = true;
                    break;
                }
            }
            if !completed {
                return Err(NumericError::SvdNoConvergence);
            }
        }

        let mut sign = 1.0;
        for column in 0..columns {
            let value = right_transpose[component * columns + column];
            if value.abs() > rank_floor {
                if value < 0.0 {
                    sign = -1.0;
                }
                break;
            }
        }
        if sign < 0.0 {
            for row in 0..rows {
                left[row * columns + component] *= -1.0;
            }
            for column in 0..columns {
                right_transpose[component * columns + column] *= -1.0;
            }
        }
    }

    Ok((
        left,
        order
            .into_iter()
            .map(|index| singular_values[index])
            .collect(),
        right_transpose,
    ))
}

fn element_count(shape: &[u64]) -> Result<usize, NumericError> {
    let count = shape
        .iter()
        .try_fold(1u64, |count, &dimension| count.checked_mul(dimension))
        .ok_or(NumericError::ShapeOverflow)?;
    usize::try_from(count).map_err(|_| NumericError::ShapeOverflow)
}

fn row_major_strides(shape: &[u64]) -> Result<Vec<usize>, NumericError> {
    let mut strides = vec![0usize; shape.len()];
    let mut running = 1usize;
    for axis in (0..shape.len()).rev() {
        strides[axis] = running;
        let dimension = usize::try_from(shape[axis]).map_err(|_| NumericError::ShapeOverflow)?;
        running = running
            .checked_mul(dimension)
            .ok_or(NumericError::ShapeOverflow)?;
    }
    Ok(strides)
}

fn broadcast_shape(lhs: &[u64], rhs: &[u64]) -> Result<Vec<u64>, NumericError> {
    let rank = lhs.len().max(rhs.len());
    let lhs_offset = rank - lhs.len();
    let rhs_offset = rank - rhs.len();
    let mut shape = Vec::with_capacity(rank);
    for axis in 0..rank {
        let lhs_dimension = if axis < lhs_offset {
            1
        } else {
            lhs[axis - lhs_offset]
        };
        let rhs_dimension = if axis < rhs_offset {
            1
        } else {
            rhs[axis - rhs_offset]
        };
        let dimension = if lhs_dimension == rhs_dimension {
            lhs_dimension
        } else if lhs_dimension == 1 {
            rhs_dimension
        } else if rhs_dimension == 1 {
            lhs_dimension
        } else {
            return Err(NumericError::BroadcastIncompatible {
                axis,
                lhs: lhs_dimension,
                rhs: rhs_dimension,
            });
        };
        shape.push(dimension);
    }
    Ok(shape)
}

fn linear_to_coordinates(
    mut linear: usize,
    shape: &[u64],
    coordinates: &mut [usize],
) -> Result<(), NumericError> {
    for axis in (0..shape.len()).rev() {
        let dimension = usize::try_from(shape[axis]).map_err(|_| NumericError::ShapeOverflow)?;
        if dimension == 0 {
            return Err(NumericError::ShapeOverflow);
        }
        coordinates[axis] = linear % dimension;
        linear /= dimension;
    }
    Ok(())
}

fn broadcast_offset(
    output_coordinates: &[usize],
    output_shape: &[u64],
    input_shape: &[u64],
    input_strides: &[usize],
) -> usize {
    let rank_offset = output_shape.len() - input_shape.len();
    input_shape
        .iter()
        .enumerate()
        .map(|(axis, &dimension)| {
            let coordinate = if dimension == 1 {
                0
            } else {
                output_coordinates[rank_offset + axis]
            };
            coordinate * input_strides[axis]
        })
        .sum()
}

fn apply_binary<T>(lhs: T, rhs: T, operation: BinaryOp) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<Output = T> + Div<Output = T>,
{
    match operation {
        BinaryOp::Add => lhs + rhs,
        BinaryOp::Sub => lhs - rhs,
        BinaryOp::Mul => lhs * rhs,
        BinaryOp::Div => lhs / rhs,
    }
}

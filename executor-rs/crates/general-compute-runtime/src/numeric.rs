//! Deterministic CPU reference kernels for the first S2 numerical slice.
//!
//! The kernels operate on validated `f64` values in row-major dense tensors.
//! They intentionally stay independent of the binary tensor artifact layer so
//! callers can use them as a small reference implementation before wiring a
//! scientific backend image.

use std::fmt;

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
    ValueCountMismatch { expected: usize, actual: usize },
    BroadcastIncompatible { axis: usize, lhs: u64, rhs: u64 },
    AxisOutOfBounds { axis: usize, rank: usize },
    DuplicateAxis { axis: usize },
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
        }
    }
}

impl std::error::Error for NumericError {}

#[derive(Debug, Clone, PartialEq)]
pub struct F64Tensor {
    shape: Vec<u64>,
    values: Vec<f64>,
}

impl F64Tensor {
    pub fn new(shape: Vec<u64>, values: Vec<f64>) -> Result<Self, NumericError> {
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
    pub fn values(&self) -> &[f64] {
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
        let mut output = vec![0.0; output_len];
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
            output[output_linear] += value;
        }

        Self::new(output_shape, output)
    }
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

fn apply_binary(lhs: f64, rhs: f64, operation: BinaryOp) -> f64 {
    match operation {
        BinaryOp::Add => lhs + rhs,
        BinaryOp::Sub => lhs - rhs,
        BinaryOp::Mul => lhs * rhs,
        BinaryOp::Div => lhs / rhs,
    }
}

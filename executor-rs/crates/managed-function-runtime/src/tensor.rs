//! Dense tensors for CPU hcode.
//!
//! A tensor is an immutable, row-major array with a rank from 1 through
//! [`MAX_RANK`] and one dtype, `i64` or `f64`. The evaluator charges each
//! tensor operation before its work starts, so an unaffordable operation fails
//! without computing. Float elements follow the CPU hcode float rules: every
//! element stays finite, and -0.0 is folded to 0.0.
//!
//! Tensors have no index assignment and no broadcasting beyond a scalar
//! operand, so every value is fixed once it is built.

use crate::{
    BinaryOp, BoundedOutput, RuntimeError, Value, ValueMetrics, canonical_float_string,
    deterministic_math, finite_float, integer_arithmetic_overflow_error, truncate_to_i64,
    value_limit_error,
};

/// Largest supported rank. It bounds the canonical shape and the nesting depth.
pub(crate) const MAX_RANK: usize = 8;

/// Units per element for `relu`.
pub(crate) const RELU_UNITS_PER_ELEMENT: u64 = 1;
/// Units per element for `gelu`: one `tanh` plus the surrounding arithmetic.
pub(crate) const GELU_UNITS_PER_ELEMENT: u64 = 24;
/// Units per element for `softmax`: one `exp` plus the reduction arithmetic.
pub(crate) const SOFTMAX_UNITS_PER_ELEMENT: u64 = 20;

const CANONICAL_HEAD: &str = r#"{"data":["#;
const CANONICAL_DTYPE: &str = r#"],"dtype":""#;
const CANONICAL_SHAPE: &str = r#"","shape":["#;
const CANONICAL_TAIL: &str = "]}";

/// Element dtype of a tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DType {
    /// Signed 64-bit integers, with checked arithmetic.
    I64,
    /// Finite IEEE-754 binary64 values.
    F64,
}

impl DType {
    /// The name used in source code and canonical output.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::I64 => "i64",
            Self::F64 => "f64",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Elements {
    I64(Vec<i64>),
    F64(Vec<f64>),
}

/// A numeric scalar as stored in a tensor or passed as an operand.
#[derive(Debug, Clone, Copy)]
enum Leaf {
    Int(i64),
    Float(f64),
}

impl Leaf {
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Float(value) => value,
        }
    }
}

/// One operand of an elementwise operation: a tensor's storage or a scalar
/// that is applied to every element.
#[derive(Clone, Copy)]
enum Side<'a> {
    Elements(&'a Elements),
    Scalar(Leaf),
}

impl Side<'_> {
    fn is_float(&self) -> bool {
        matches!(
            self,
            Self::Elements(Elements::F64(_)) | Self::Scalar(Leaf::Float(_))
        )
    }

    fn leaf(&self, index: usize) -> Leaf {
        match self {
            Self::Elements(Elements::I64(values)) => Leaf::Int(values[index]),
            Self::Elements(Elements::F64(values)) => Leaf::Float(values[index]),
            Self::Scalar(leaf) => *leaf,
        }
    }

    fn float(&self, index: usize) -> f64 {
        self.leaf(index).as_f64()
    }

    fn int(&self, index: usize) -> i64 {
        match self.leaf(index) {
            Leaf::Int(value) => value,
            Leaf::Float(_) => unreachable!("integer kernels run only without float operands"),
        }
    }
}

/// An immutable dense tensor.
#[derive(Debug, Clone, PartialEq)]
pub struct Tensor {
    shape: Vec<usize>,
    elements: Elements,
}

impl Tensor {
    /// Dimensions, outermost first.
    #[must_use]
    pub fn shape(&self) -> &[usize] {
        &self.shape
    }

    /// The element dtype.
    #[must_use]
    pub fn dtype(&self) -> DType {
        match self.elements {
            Elements::I64(_) => DType::I64,
            Elements::F64(_) => DType::F64,
        }
    }

    /// Total number of elements.
    #[must_use]
    pub fn element_count(&self) -> usize {
        self.shape.iter().product()
    }

    fn new(shape: Vec<usize>, elements: Elements) -> Result<Self, RuntimeError> {
        let count = shape_count(&shape)?;
        let actual = match &elements {
            Elements::I64(values) => values.len(),
            Elements::F64(values) => values.len(),
        };
        if count != actual {
            return Err(RuntimeError::new(
                "type_error",
                "tensor elements do not match the shape",
            ));
        }
        Ok(Self { shape, elements })
    }

    fn element_value(&self, index: usize) -> Value {
        match &self.elements {
            Elements::I64(values) => Value::Int(values[index]),
            Elements::F64(values) => Value::Float(values[index]),
        }
    }

    fn side(&self) -> Side<'_> {
        Side::Elements(&self.elements)
    }
}

/// Number of elements for `shape`, checking the rank and the product.
pub(crate) fn shape_count(shape: &[usize]) -> Result<usize, RuntimeError> {
    if shape.is_empty() || shape.len() > MAX_RANK {
        return Err(RuntimeError::new(
            "type_error",
            "tensors have rank 1 through 8",
        ));
    }
    shape
        .iter()
        .try_fold(1_usize, |total, dim| total.checked_mul(*dim))
        .ok_or_else(value_limit_error)
}

/// The number of positions a walk over `shape` visits: the product of the
/// dimensions with each zero dimension counted as one. A zero-size tensor still
/// has its dimensions walked by nested conversion, transposition, and products,
/// so this count, not the element count, bounds that work.
pub(crate) fn walk_count(shape: &[usize]) -> Result<u64, RuntimeError> {
    shape
        .iter()
        .try_fold(1_u64, |total, dim| {
            let dim = u64::try_from((*dim).max(1)).ok()?;
            total.checked_mul(dim)
        })
        .ok_or_else(value_limit_error)
}

/// Positions visited by a tensor, as a logical `u64`, for charging and limit
/// checks. Every tensor is stored with `walk_count(shape) <= max_collection_items`.
pub(crate) fn position_units(tensor: &Tensor) -> u64 {
    walk_count(&tensor.shape).unwrap_or(u64::MAX)
}

/// Reads a shape literal: a list of non-negative ints.
pub(crate) fn shape_from_value(value: &Value) -> Result<Vec<usize>, RuntimeError> {
    let Value::List(items) = value else {
        return Err(RuntimeError::new(
            "type_error",
            "a shape must be a list of ints",
        ));
    };
    items
        .iter()
        .map(|item| match item {
            Value::Int(dim) => usize::try_from(*dim).map_err(|_| {
                RuntimeError::new("type_error", "tensor dimensions must not be negative")
            }),
            _ => Err(RuntimeError::new(
                "type_error",
                "a shape must be a list of ints",
            )),
        })
        .collect()
}

/// Reads a dtype literal: `"i64"` or `"f64"`.
pub(crate) fn dtype_from_value(value: &Value) -> Result<DType, RuntimeError> {
    match value {
        Value::String(name) if name == "i64" => Ok(DType::I64),
        Value::String(name) if name == "f64" => Ok(DType::F64),
        _ => Err(RuntimeError::new(
            "type_error",
            "dtype must be \"i64\" or \"f64\"",
        )),
    }
}

/// The shape of a tensor as a managed list of ints.
pub(crate) fn shape_value(tensor: &Tensor) -> Value {
    Value::List(
        tensor
            .shape
            .iter()
            .map(|dim| Value::Int(i64::try_from(*dim).unwrap_or(i64::MAX)))
            .collect(),
    )
}

/// Builds a tensor from a rectangular nested list of ints and floats. Any float
/// makes the result `f64`; otherwise it is `i64`.
///
/// The shape is inferred from the first elements, so it is checked against
/// `max_positions` before any storage is reserved. A ragged literal then fails
/// in `collect_leaves`, which only pushes leaves that the inferred shape allows.
pub(crate) fn from_nested(value: &Value, max_positions: u64) -> Result<Tensor, RuntimeError> {
    let shape = infer_shape(value)?;
    if walk_count(&shape)? > max_positions {
        return Err(value_limit_error());
    }
    let mut leaves = Vec::new();
    collect_leaves(value, &shape, &mut leaves)?;
    let elements = if leaves.iter().any(|leaf| matches!(leaf, Leaf::Float(_))) {
        Elements::F64(leaves.iter().map(|leaf| leaf.as_f64()).collect())
    } else {
        Elements::I64(
            leaves
                .iter()
                .map(|leaf| match leaf {
                    Leaf::Int(value) => *value,
                    Leaf::Float(_) => 0,
                })
                .collect(),
        )
    };
    Tensor::new(shape, elements)
}

fn infer_shape(value: &Value) -> Result<Vec<usize>, RuntimeError> {
    let mut shape = Vec::new();
    let mut current = value;
    while let Value::List(items) = current {
        if shape.len() == MAX_RANK {
            return Err(RuntimeError::new(
                "type_error",
                "tensors have rank 1 through 8",
            ));
        }
        shape.push(items.len());
        match items.first() {
            Some(first) => current = first,
            None => break,
        }
    }
    if shape.is_empty() {
        return Err(RuntimeError::new("type_error", "tensor expects a list"));
    }
    Ok(shape)
}

fn collect_leaves(
    value: &Value,
    shape: &[usize],
    leaves: &mut Vec<Leaf>,
) -> Result<(), RuntimeError> {
    let Some((dim, rest)) = shape.split_first() else {
        return match value {
            Value::Int(value) => {
                leaves.push(Leaf::Int(*value));
                Ok(())
            }
            Value::Float(value) => {
                leaves.push(Leaf::Float(*value));
                Ok(())
            }
            _ => Err(RuntimeError::new(
                "type_error",
                "tensor elements must be ints or floats",
            )),
        };
    };
    match value {
        Value::List(items) if items.len() == *dim => {
            for item in items {
                collect_leaves(item, rest, leaves)?;
            }
            Ok(())
        }
        _ => Err(RuntimeError::new(
            "type_error",
            "tensor literal must be rectangular",
        )),
    }
}

/// Converts a tensor back to nested lists. Shapes with a zero dimension lose
/// their extent in this form; use the canonical object to keep it.
pub(crate) fn to_nested(tensor: &Tensor) -> Value {
    let mut strides = vec![1_usize; tensor.shape.len()];
    for level in (0..tensor.shape.len().saturating_sub(1)).rev() {
        strides[level] = strides[level + 1] * tensor.shape[level + 1];
    }
    nested_value(tensor, 0, 0, &strides)
}

fn nested_value(tensor: &Tensor, level: usize, offset: usize, strides: &[usize]) -> Value {
    let dim = tensor.shape[level];
    if level + 1 == tensor.shape.len() {
        return Value::List((0..dim).map(|i| tensor.element_value(offset + i)).collect());
    }
    Value::List(
        (0..dim)
            .map(|i| nested_value(tensor, level + 1, offset + i * strides[level], strides))
            .collect(),
    )
}

/// `tensor[position]`: a scalar for rank 1, otherwise the sub-tensor at
/// `position`. `position` must already be in range.
pub(crate) fn index(tensor: &Tensor, position: usize) -> Value {
    if tensor.shape.len() == 1 {
        return tensor.element_value(position);
    }
    let inner_shape = tensor.shape[1..].to_vec();
    let inner_count: usize = inner_shape.iter().product();
    let start = position * inner_count;
    let elements = match &tensor.elements {
        Elements::I64(values) => Elements::I64(values[start..start + inner_count].to_vec()),
        Elements::F64(values) => Elements::F64(values[start..start + inner_count].to_vec()),
    };
    Value::Tensor(Tensor {
        shape: inner_shape,
        elements,
    })
}

/// Positions of the value `index` produces for `tensor`, for charging.
pub(crate) fn index_units(tensor: &Tensor) -> u64 {
    if tensor.shape.len() == 1 {
        1
    } else {
        walk_count(&tensor.shape[1..]).unwrap_or(u64::MAX)
    }
}

/// Elementwise `+ - * /` between two tensors of the same shape, or between a
/// tensor and a scalar. Mixed dtypes promote to `f64`.
pub(crate) fn binary(
    op: BinaryOp,
    left: &Value,
    right: &Value,
    checked: bool,
) -> Result<Value, RuntimeError> {
    check_arithmetic(op)?;
    let tensor = match (left, right) {
        (Value::Tensor(a), Value::Tensor(b)) => {
            if a.shape != b.shape {
                return Err(RuntimeError::new(
                    "type_error",
                    "elementwise operands must have the same shape",
                ));
            }
            elementwise(a.shape.clone(), a.side(), b.side(), op, checked)?
        }
        (Value::Tensor(a), scalar) => {
            elementwise(a.shape.clone(), a.side(), scalar_side(scalar)?, op, checked)?
        }
        (scalar, Value::Tensor(b)) => {
            elementwise(b.shape.clone(), scalar_side(scalar)?, b.side(), op, checked)?
        }
        _ => {
            return Err(RuntimeError::new(
                "type_error",
                "tensor arithmetic expects a tensor operand",
            ));
        }
    };
    Ok(Value::Tensor(tensor))
}

/// Positions charged for an elementwise operation on these operands.
pub(crate) fn elementwise_units(left: &Value, right: &Value) -> u64 {
    [left, right]
        .into_iter()
        .filter_map(|value| match value {
            Value::Tensor(tensor) => Some(position_units(tensor)),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

fn check_arithmetic(op: BinaryOp) -> Result<(), RuntimeError> {
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => Ok(()),
        _ => Err(RuntimeError::new(
            "type_error",
            "tensors support + - * / and == != only",
        )),
    }
}

fn scalar_side(value: &Value) -> Result<Side<'static>, RuntimeError> {
    match value {
        Value::Int(value) => Ok(Side::Scalar(Leaf::Int(*value))),
        Value::Float(value) => Ok(Side::Scalar(Leaf::Float(*value))),
        _ => Err(RuntimeError::new(
            "type_error",
            "tensor arithmetic expects numbers",
        )),
    }
}

fn elementwise(
    shape: Vec<usize>,
    left: Side<'_>,
    right: Side<'_>,
    op: BinaryOp,
    checked: bool,
) -> Result<Tensor, RuntimeError> {
    let count = shape_count(&shape)?;
    let elements = if left.is_float() || right.is_float() {
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            out.push(float_op(op, left.float(index), right.float(index))?);
        }
        Elements::F64(out)
    } else {
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            out.push(int_op(op, left.int(index), right.int(index), checked)?);
        }
        Elements::I64(out)
    };
    Tensor::new(shape, elements)
}

/// One checked integer operation. Division by zero fails before the operation.
fn int_op(op: BinaryOp, left: i64, right: i64, checked: bool) -> Result<i64, RuntimeError> {
    let result = match op {
        BinaryOp::Add => {
            if checked {
                left.checked_add(right)
            } else {
                Some(left.wrapping_add(right))
            }
        }
        BinaryOp::Sub => {
            if checked {
                left.checked_sub(right)
            } else {
                Some(left.wrapping_sub(right))
            }
        }
        BinaryOp::Mul => {
            if checked {
                left.checked_mul(right)
            } else {
                Some(left.wrapping_mul(right))
            }
        }
        BinaryOp::Div => {
            if right == 0 {
                return Err(RuntimeError::new("runtime_error", "division by zero"));
            }
            if checked {
                left.checked_div(right)
            } else {
                Some(left.wrapping_div(right))
            }
        }
        _ => {
            return Err(RuntimeError::new(
                "type_error",
                "unsupported tensor operator",
            ));
        }
    };
    result.ok_or_else(integer_arithmetic_overflow_error)
}

/// One float operation. The result must be finite; division by zero fails.
fn float_op(op: BinaryOp, left: f64, right: f64) -> Result<f64, RuntimeError> {
    let value = match op {
        BinaryOp::Add => left + right,
        BinaryOp::Sub => left - right,
        BinaryOp::Mul => left * right,
        BinaryOp::Div => {
            if right.abs().to_bits() == 0 {
                return Err(RuntimeError::new("runtime_error", "division by zero"));
            }
            left / right
        }
        _ => {
            return Err(RuntimeError::new(
                "type_error",
                "unsupported tensor operator",
            ));
        }
    };
    finite_float(value)
}

/// `sum(tensor)` as a scalar of the tensor's dtype.
pub(crate) fn sum(tensor: &Tensor, checked: bool) -> Result<Value, RuntimeError> {
    match &tensor.elements {
        Elements::I64(values) => {
            let mut total = 0_i64;
            for value in values {
                total = int_op(BinaryOp::Add, total, *value, checked)?;
            }
            Ok(Value::Int(total))
        }
        Elements::F64(values) => {
            let mut total = 0.0_f64;
            for value in values {
                total = float_op(BinaryOp::Add, total, *value)?;
            }
            Ok(Value::Float(total))
        }
    }
}

/// Length of the operands of `dot`, after checking they are equal vectors.
pub(crate) fn dot_length(left: &Tensor, right: &Tensor) -> Result<usize, RuntimeError> {
    match (left.shape.as_slice(), right.shape.as_slice()) {
        ([a], [b]) if a == b => Ok(*a),
        _ => Err(RuntimeError::new(
            "type_error",
            "dot expects two vectors of the same length",
        )),
    }
}

/// `dot(a, b)` for two equal-length vectors, computed as `sum(a * b)`.
pub(crate) fn dot(left: &Tensor, right: &Tensor, checked: bool) -> Result<Value, RuntimeError> {
    dot_length(left, right)?;
    let products = elementwise(
        left.shape.clone(),
        left.side(),
        right.side(),
        BinaryOp::Mul,
        checked,
    )?;
    sum(&products, checked)
}

/// Shape, dimensions, and metered cost of one matrix product.
pub(crate) struct MatmulPlan {
    shape: Vec<usize>,
    m: usize,
    k: usize,
    n: usize,
    units: u64,
}

impl MatmulPlan {
    /// Positions of the output, checked against the limit before it is computed.
    pub(crate) fn output_positions(&self) -> Result<u64, RuntimeError> {
        walk_count(&self.shape)
    }

    /// Charged units: one per output row, one per output position, and one per
    /// multiply-add. The loops visit exactly these positions.
    pub(crate) fn units(&self) -> u64 {
        self.units
    }
}

/// Plans `a @ b` for matrix-matrix, matrix-vector, or vector-matrix operands.
pub(crate) fn matmul_plan(left: &Tensor, right: &Tensor) -> Result<MatmulPlan, RuntimeError> {
    let (m, k, k_right, n, shape) = match (left.shape.as_slice(), right.shape.as_slice()) {
        ([m, k], [k_right, n]) => (*m, *k, *k_right, *n, vec![*m, *n]),
        ([m, k], [k_right]) => (*m, *k, *k_right, 1, vec![*m]),
        ([k], [k_right, n]) => (1, *k, *k_right, *n, vec![*n]),
        _ => {
            return Err(RuntimeError::new(
                "type_error",
                "matmul expects matrix-matrix, matrix-vector, or vector-matrix operands",
            ));
        }
    };
    if k != k_right {
        return Err(RuntimeError::new(
            "type_error",
            "matmul inner dimensions must match",
        ));
    }
    let units = (|| {
        m.checked_add(m.checked_mul(n)?)?
            .checked_add(m.checked_mul(k)?.checked_mul(n)?)
    })()
    .and_then(|total| u64::try_from(total).ok())
    .ok_or_else(value_limit_error)?;
    Ok(MatmulPlan {
        shape,
        m,
        k,
        n,
        units,
    })
}

/// Computes the product described by `plan`, accumulating in row order.
pub(crate) fn matmul(
    left: &Tensor,
    right: &Tensor,
    plan: &MatmulPlan,
    checked: bool,
) -> Result<Tensor, RuntimeError> {
    let (rows, inner, cols) = (plan.m, plan.k, plan.n);
    let (lhs, rhs) = (left.side(), right.side());
    let float_output = lhs.is_float() || rhs.is_float();
    let count = shape_count(&plan.shape)?;
    let elements = if float_output {
        let mut out = Vec::with_capacity(count);
        for row in 0..rows {
            for col in 0..cols {
                let mut total = 0.0_f64;
                for step in 0..inner {
                    let product = float_op(
                        BinaryOp::Mul,
                        lhs.float(row * inner + step),
                        rhs.float(step * cols + col),
                    )?;
                    total = float_op(BinaryOp::Add, total, product)?;
                }
                out.push(total);
            }
        }
        Elements::F64(out)
    } else {
        let mut out = Vec::with_capacity(count);
        for row in 0..rows {
            for col in 0..cols {
                let mut total = 0_i64;
                for step in 0..inner {
                    let product = int_op(
                        BinaryOp::Mul,
                        lhs.int(row * inner + step),
                        rhs.int(step * cols + col),
                        checked,
                    )?;
                    total = int_op(BinaryOp::Add, total, product, checked)?;
                }
                out.push(total);
            }
        }
        Elements::I64(out)
    };
    Tensor::new(plan.shape.clone(), elements)
}

/// `relu(tensor)`, keeping the dtype.
pub(crate) fn relu(tensor: &Tensor) -> Result<Tensor, RuntimeError> {
    let elements = match &tensor.elements {
        Elements::I64(values) => {
            Elements::I64(values.iter().map(|value| (*value).max(0)).collect())
        }
        // The comparison also folds -0.0 into the canonical zero.
        Elements::F64(values) => Elements::F64(
            values
                .iter()
                .map(|value| if *value > 0.0 { *value } else { 0.0 })
                .collect(),
        ),
    };
    Tensor::new(tensor.shape.clone(), elements)
}

const SQRT_2_OVER_PI: f64 = 0.797_884_560_802_865_4;
const GELU_CUBIC: f64 = 0.044_715;

/// `gelu(tensor)` with the tanh approximation, always returning `f64`.
pub(crate) fn gelu(tensor: &Tensor) -> Result<Tensor, RuntimeError> {
    let source = tensor.side();
    let count = tensor.element_count();
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let x = source.float(index);
        let square = float_op(BinaryOp::Mul, x, x)?;
        let cube = float_op(BinaryOp::Mul, square, x)?;
        let inner = float_op(
            BinaryOp::Mul,
            SQRT_2_OVER_PI,
            float_op(BinaryOp::Add, x, float_op(BinaryOp::Mul, GELU_CUBIC, cube)?)?,
        )?;
        let gate = float_op(BinaryOp::Add, 1.0, deterministic_math::tanh(inner))?;
        out.push(float_op(
            BinaryOp::Mul,
            float_op(BinaryOp::Mul, 0.5, x)?,
            gate,
        )?);
    }
    Tensor::new(tensor.shape.clone(), Elements::F64(out))
}

/// `softmax(tensor)` along the last axis, always returning `f64`. Each row is
/// shifted by its maximum before `exp`, so every exponent is at most zero.
pub(crate) fn softmax(tensor: &Tensor) -> Result<Tensor, RuntimeError> {
    let source = tensor.side();
    let count = tensor.element_count();
    let last = tensor.shape.last().copied().unwrap_or(0);
    let mut out = Vec::with_capacity(count);
    if last > 0 {
        for row_start in (0..count).step_by(last) {
            let row: Vec<f64> = (row_start..row_start + last)
                .map(|index| source.float(index))
                .collect();
            let max = row.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mut exps = Vec::with_capacity(last);
            for value in &row {
                let shifted = float_op(BinaryOp::Sub, *value, max)?;
                let exponential = deterministic_math::exp(shifted).ok_or_else(|| {
                    RuntimeError::new("runtime_error", "softmax exponent is out of range")
                })?;
                exps.push(finite_float(exponential)?);
            }
            let mut total = 0.0_f64;
            for exponential in &exps {
                total = float_op(BinaryOp::Add, total, *exponential)?;
            }
            for exponential in exps {
                out.push(float_op(BinaryOp::Div, exponential, total)?);
            }
        }
    }
    Tensor::new(tensor.shape.clone(), Elements::F64(out))
}

/// Swaps the axes of a rank-2 tensor.
pub(crate) fn transpose(tensor: &Tensor) -> Result<Tensor, RuntimeError> {
    let [rows, cols] = tensor.shape.as_slice() else {
        return Err(RuntimeError::new(
            "type_error",
            "transpose expects a rank-2 tensor",
        ));
    };
    let (rows, cols) = (*rows, *cols);
    let elements = match &tensor.elements {
        Elements::I64(values) => Elements::I64(transpose_values(values, rows, cols)),
        Elements::F64(values) => Elements::F64(transpose_values(values, rows, cols)),
    };
    Tensor::new(vec![cols, rows], elements)
}

fn transpose_values<T: Copy>(values: &[T], rows: usize, cols: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(values.len());
    for col in 0..cols {
        for row in 0..rows {
            out.push(values[row * cols + col]);
        }
    }
    out
}

/// Gives the same elements a new shape with the same count.
pub(crate) fn reshape(tensor: &Tensor, shape: Vec<usize>) -> Result<Tensor, RuntimeError> {
    if shape_count(&shape)? != tensor.element_count() {
        return Err(RuntimeError::new(
            "type_error",
            "reshape must keep the element count",
        ));
    }
    Tensor::new(shape, tensor.elements.clone())
}

/// Converts between dtypes. Float to int truncates toward zero and fails when
/// the result is outside the i64 range.
pub(crate) fn cast(tensor: &Tensor, dtype: DType) -> Result<Tensor, RuntimeError> {
    let elements = match (&tensor.elements, dtype) {
        (Elements::I64(values), DType::I64) => Elements::I64(values.clone()),
        (Elements::F64(values), DType::F64) => Elements::F64(values.clone()),
        (Elements::I64(values), DType::F64) => {
            Elements::F64(values.iter().map(|value| *value as f64).collect())
        }
        (Elements::F64(values), DType::I64) => Elements::I64(
            values
                .iter()
                .map(|value| {
                    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(value) {
                        Ok(truncate_to_i64(*value))
                    } else {
                        Err(RuntimeError::new(
                            "runtime_error",
                            "float is outside the int64 range",
                        ))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    };
    Tensor::new(tensor.shape.clone(), elements)
}

/// A zero-filled tensor of the given shape and dtype.
pub(crate) fn zeros(shape: Vec<usize>, dtype: DType) -> Result<Tensor, RuntimeError> {
    let count = shape_count(&shape)?;
    let elements = match dtype {
        DType::I64 => Elements::I64(vec![0; count]),
        DType::F64 => Elements::F64(vec![0.0; count]),
    };
    Tensor::new(shape, elements)
}

/// Metrics of the canonical object form, computed without rendering it.
pub(crate) fn metrics(tensor: &Tensor) -> Result<ValueMetrics, RuntimeError> {
    let count = tensor.element_count();
    let mut element_bytes = 0_u64;
    match &tensor.elements {
        Elements::I64(values) => {
            for value in values {
                element_bytes = element_bytes.saturating_add(decimal_length(&value.to_string()));
            }
        }
        Elements::F64(values) => {
            for value in values {
                element_bytes =
                    element_bytes.saturating_add(decimal_length(&canonical_float_string(*value)));
            }
        }
    }
    if count > 0 {
        element_bytes += u64::try_from(count - 1).unwrap_or(u64::MAX);
    }
    let mut shape_bytes = u64::try_from(tensor.shape.len() - 1).unwrap_or(u64::MAX);
    for dim in &tensor.shape {
        shape_bytes = shape_bytes.saturating_add(decimal_length(&dim.to_string()));
    }
    let fixed = [
        CANONICAL_HEAD,
        CANONICAL_DTYPE,
        tensor.dtype().name(),
        CANONICAL_SHAPE,
        CANONICAL_TAIL,
    ]
    .iter()
    .map(|piece| decimal_length(piece))
    .fold(0_u64, u64::saturating_add);
    let canonical_bytes = fixed
        .checked_add(element_bytes)
        .and_then(|total| total.checked_add(shape_bytes))
        .ok_or_else(value_limit_error)?;
    Ok(ValueMetrics {
        canonical_bytes,
        depth: 3,
        max_collection_items: u64::try_from(count.max(tensor.shape.len())).unwrap_or(u64::MAX),
    })
}

fn decimal_length(text: &str) -> u64 {
    u64::try_from(text.len()).unwrap_or(u64::MAX)
}

/// Writes the canonical object form: `{"data":[...],"dtype":"f64","shape":[...]}`.
pub(crate) fn write_canonical(
    tensor: &Tensor,
    output: &mut BoundedOutput,
) -> Result<(), RuntimeError> {
    output.push_str(CANONICAL_HEAD)?;
    match &tensor.elements {
        Elements::I64(values) => {
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push_str(",")?;
                }
                output.push_str(&value.to_string())?;
            }
        }
        Elements::F64(values) => {
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push_str(",")?;
                }
                if !value.is_finite() {
                    return Err(RuntimeError::new(
                        "type_error",
                        "non-finite floats cannot be rendered as JSON output",
                    ));
                }
                output.push_str(&canonical_float_string(*value))?;
            }
        }
    }
    output.push_str(CANONICAL_DTYPE)?;
    output.push_str(tensor.dtype().name())?;
    output.push_str(CANONICAL_SHAPE)?;
    for (index, dim) in tensor.shape.iter().enumerate() {
        if index > 0 {
            output.push_str(",")?;
        }
        output.push_str(&dim.to_string())?;
    }
    output.push_str(CANONICAL_TAIL)
}

/// The canonical object form as text, for `print`.
pub(crate) fn canonical_string(tensor: &Tensor) -> Result<String, RuntimeError> {
    let mut output = BoundedOutput::new(u64::MAX);
    write_canonical(tensor, &mut output)?;
    Ok(output.into_inner())
}

/// Largest absolute entry accepted by `freivalds_check`. With this bound every
/// intermediate sum of the verifier stays exact in `i128`, for any shape that
/// the element limit allows.
const FREIVALDS_ENTRY_LIMIT: i64 = i32::MAX as i64;
/// Largest number of random rounds, which bounds the work of one call.
const FREIVALDS_MAX_ROUNDS: i64 = 64;

/// Validated inputs and metered cost of one `freivalds_check` call.
pub(crate) struct FreivaldsPlan {
    rounds: u32,
    units: u64,
}

impl FreivaldsPlan {
    pub(crate) fn rounds(&self) -> u32 {
        self.rounds
    }

    /// Charged units: one per loop position in each round, which covers the
    /// random vector and the three matrix-vector products.
    pub(crate) fn units(&self) -> u64 {
        self.units
    }
}

/// Checks `lhs` (rows by inner), `rhs` (inner by cols), and `claimed` (rows by
/// cols), and plans the call.
pub(crate) fn freivalds_plan(
    lhs: &Tensor,
    rhs: &Tensor,
    claimed: &Tensor,
    rounds: i64,
) -> Result<FreivaldsPlan, RuntimeError> {
    let [rows, inner] = lhs.shape.as_slice() else {
        return Err(RuntimeError::new(
            "type_error",
            "freivalds_check expects a rank-2 matrix a",
        ));
    };
    let [rhs_inner, cols] = rhs.shape.as_slice() else {
        return Err(RuntimeError::new(
            "type_error",
            "freivalds_check expects a rank-2 matrix b",
        ));
    };
    if claimed.shape.as_slice() != [*rows, *cols] || inner != rhs_inner {
        return Err(RuntimeError::new(
            "type_error",
            "freivalds_check expects a of shape m by k, b of shape k by n, and c of shape m by n",
        ));
    }
    for tensor in [lhs, rhs, claimed] {
        integer_entries_within_limit(tensor)?;
    }
    if !(1..=FREIVALDS_MAX_ROUNDS).contains(&rounds) {
        return Err(RuntimeError::new(
            "runtime_error",
            "freivalds_check rounds must be between 1 and 64",
        ));
    }
    // Positions visited in one round: the `cols` bits of r; the `inner` outer
    // steps of b * r plus its `inner * cols` multiply-adds; and the `rows` outer
    // steps of a * (b * r) and c * r plus their `rows * inner` and `rows * cols`
    // multiply-adds. The loops in `freivalds_check` visit exactly these positions.
    let per_round = (|| {
        let draw = *cols;
        let product = inner.checked_mul(cols.checked_add(1)?)?;
        let check = rows.checked_mul(1_usize.checked_add(*inner)?.checked_add(*cols)?)?;
        draw.checked_add(product)?.checked_add(check)
    })()
    .ok_or_else(value_limit_error)?;
    let units = u64::try_from(per_round)
        .ok()
        .and_then(|per_round| per_round.checked_mul(u64::try_from(rounds).ok()?))
        .ok_or_else(value_limit_error)?;
    Ok(FreivaldsPlan {
        // The range check above keeps `rounds` in 1..=64.
        rounds: u32::try_from(rounds).unwrap_or(1),
        units,
    })
}

fn integer_entries_within_limit(tensor: &Tensor) -> Result<(), RuntimeError> {
    let Elements::I64(values) = &tensor.elements else {
        return Err(RuntimeError::new(
            "type_error",
            "freivalds_check requires i64 matrices",
        ));
    };
    if values
        .iter()
        .all(|value| value.unsigned_abs() <= FREIVALDS_ENTRY_LIMIT.unsigned_abs())
    {
        Ok(())
    } else {
        Err(RuntimeError::new(
            "type_error",
            "freivalds_check entries must lie within the i32 range",
        ))
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// Freivalds' test for `lhs * rhs == claimed`: each round draws a 0/1 vector
/// `r` and checks `lhs * (rhs * r) == claimed * r` exactly. A correct product
/// always passes. Calls validated by [`freivalds_plan`] cannot overflow `i128`.
pub(crate) fn freivalds_check(
    lhs: &Tensor,
    rhs: &Tensor,
    claimed: &Tensor,
    seed: i64,
    rounds: u32,
) -> Result<bool, RuntimeError> {
    let (rows, inner) = match lhs.shape.as_slice() {
        [rows, inner] => (*rows, *inner),
        _ => {
            return Err(RuntimeError::new(
                "type_error",
                "freivalds_check expects matrices",
            ));
        }
    };
    let cols = rhs.shape.get(1).copied().unwrap_or(0);
    let (Elements::I64(lhs_values), Elements::I64(rhs_values), Elements::I64(claimed_values)) =
        (&lhs.elements, &rhs.elements, &claimed.elements)
    else {
        return Err(RuntimeError::new(
            "type_error",
            "freivalds_check requires i64 matrices",
        ));
    };
    let mut state = seed.cast_unsigned();
    for _ in 0..rounds {
        let mut vector = vec![0_i128; cols];
        let mut word = 0_u64;
        let mut bits_left = 0_u32;
        for slot in &mut vector {
            if bits_left == 0 {
                word = splitmix64(&mut state);
                bits_left = 64;
            }
            *slot = i128::from(word & 1);
            word >>= 1;
            bits_left -= 1;
        }
        let rhs_product: Vec<i128> = (0..inner)
            .map(|index| {
                (0..cols)
                    .map(|col| i128::from(rhs_values[index * cols + col]) * vector[col])
                    .sum::<i128>()
            })
            .collect();
        for row in 0..rows {
            let left: i128 = (0..inner)
                .map(|index| i128::from(lhs_values[row * inner + index]) * rhs_product[index])
                .sum();
            let right: i128 = (0..cols)
                .map(|col| i128::from(claimed_values[row * cols + col]) * vector[col])
                .sum();
            if left != right {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{Tensor, canonical_string, from_nested, metrics, to_nested};
    use crate::{Value, render_output};

    fn tensor(source: &Value) -> Tensor {
        from_nested(source, u64::MAX).expect("a rectangular literal")
    }

    #[test]
    fn metrics_match_the_rendered_canonical_length() {
        let samples = [
            tensor(&Value::List(vec![
                Value::List(vec![Value::Int(1), Value::Int(-20)]),
                Value::List(vec![Value::Int(300), Value::Int(4)]),
            ])),
            tensor(&Value::List(vec![Value::Float(0.5), Value::Float(1e21)])),
            tensor(&Value::List(vec![])),
        ];
        for sample in &samples {
            let rendered = canonical_string(sample).unwrap();
            assert_eq!(
                metrics(sample).unwrap().canonical_bytes,
                u64::try_from(rendered.len()).unwrap(),
                "{rendered}"
            );
            assert_eq!(render_output(&Value::Tensor(sample.clone())), rendered);
        }
    }

    #[test]
    fn nested_conversion_round_trips_and_promotes_mixed_literals() {
        let source = Value::List(vec![
            Value::List(vec![Value::Int(1), Value::Float(2.5)]),
            Value::List(vec![Value::Int(3), Value::Int(4)]),
        ]);
        let converted = tensor(&source);
        assert_eq!(converted.shape(), [2, 2]);
        assert_eq!(to_nested(&converted), {
            Value::List(vec![
                Value::List(vec![Value::Float(1.0), Value::Float(2.5)]),
                Value::List(vec![Value::Float(3.0), Value::Float(4.0)]),
            ])
        });
    }

    #[test]
    fn ragged_and_non_numeric_literals_are_rejected() {
        let ragged = Value::List(vec![
            Value::List(vec![Value::Int(1)]),
            Value::List(vec![Value::Int(2), Value::Int(3)]),
        ]);
        assert!(from_nested(&ragged, u64::MAX).is_err());
        assert!(from_nested(&Value::List(vec![Value::Bool(true)]), u64::MAX).is_err());
        assert!(from_nested(&Value::Int(5), u64::MAX).is_err());
    }
}

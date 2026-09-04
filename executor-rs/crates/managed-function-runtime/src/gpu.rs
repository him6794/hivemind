//! Rust-owned GPU operations for the versioned managed-function GPU runtime.
//!
//! The DSL only sees ordinary managed values.  This module converts bounded
//! nested numeric lists into invocation-local tensors, dispatches a fixed
//! operation enum to an operator-selected backend, and converts the result
//! back to managed values.  Device handles and pointers never cross this API.

use super::{RuntimeError, Value};
use std::fmt::{Display, Formatter};

/// Versioned runtime identity for the GPU-capable interpreter path.
pub const GPU_RUNTIME_VERSION: &str = "managed-function-gpu-v1";
/// Versioned fixed operation registry identity.
pub const GPU_OPERATION_REGISTRY_VERSION: &str = "managed-function-gpu-ops-v1";
/// Versioned billing identity for GPU interpreter tasks.
pub const GPU_BILLING_VERSION: &str = "managed-function-gpu-billing-v1";
/// Versioned metering identity for GPU interpreter operations.
pub const GPU_COST_MODEL_VERSION: &str = "managed-function-gpu-metering-v1";

/// Fixed cost charged for one Rust GPU operation in addition to the normal
/// function-call charge.  The value is part of the GPU-v1 semantics contract.
pub const GPU_OPERATION_COST: u64 = 10;
/// Maximum host tensor payload declared by the GPU-v1 tensor ABI.
pub const GPU_MAX_TENSOR_BYTES: usize = 16 * 1024 * 1024;

/// The only operations exposed to GPU-v1 programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuOperation {
    AddF32,
    ScaleF32,
    MatmulF32,
}

impl GpuOperation {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AddF32 => "gpu_add_f32",
            Self::ScaleF32 => "gpu_scale_f32",
            Self::MatmulF32 => "gpu_matmul_f32",
        }
    }

    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            Self::AddF32 | Self::ScaleF32 | Self::MatmulF32 => 2,
        }
    }
}

/// Host-side tensor representation exchanged with a GPU backend.
///
/// `data` is contiguous row-major f32 data.  It is deliberately a value type:
/// no CUDA device allocation, pointer, stream, or context can escape the
/// backend implementation.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuTensor {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

impl GpuTensor {
    pub fn new(shape: Vec<usize>, data: Vec<f32>) -> Result<Self, GpuBackendError> {
        let tensor = Self { shape, data };
        tensor.validate()?;
        Ok(tensor)
    }

    pub fn validate(&self) -> Result<(), GpuBackendError> {
        if self.shape.len() > 8 {
            return Err(GpuBackendError::invalid(
                "tensor rank exceeds the GPU-v1 limit",
            ));
        }
        let expected = self
            .shape
            .iter()
            .try_fold(1usize, |count, dimension| {
                if *dimension == 0 {
                    return None;
                }
                count.checked_mul(*dimension)
            })
            .ok_or_else(|| GpuBackendError::invalid("tensor shape is empty or too large"))?;
        if expected != self.data.len() {
            return Err(GpuBackendError::invalid(
                "tensor shape does not match the f32 payload length",
            ));
        }
        let bytes = self
            .data
            .len()
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| GpuBackendError::invalid("tensor payload size overflow"))?;
        if bytes > GPU_MAX_TENSOR_BYTES {
            return Err(GpuBackendError::invalid(
                "tensor payload exceeds the GPU-v1 byte limit",
            ));
        }
        if self.data.iter().any(|value| !value.is_finite()) {
            return Err(GpuBackendError::invalid(
                "GPU tensors must contain finite f32 values",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        self.data.len().saturating_mul(std::mem::size_of::<f32>())
    }
}

/// Bounded error returned by an operator GPU implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuBackendError {
    code: &'static str,
    message: String,
}

impl GpuBackendError {
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "gpu_input_error",
            message: message.into(),
        }
    }

    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "gpu_backend_unavailable",
            message: message.into(),
        }
    }

    #[must_use]
    pub fn execution(message: impl Into<String>) -> Self {
        Self {
            code: "gpu_execution_error",
            message: message.into(),
        }
    }

    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for GpuBackendError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for GpuBackendError {}

mod sealed {
    use super::{GpuBackendError, GpuOperation, GpuTensor};

    pub trait GpuBackendImpl {
        fn execute_unchecked(
            &mut self,
            operation: GpuOperation,
            inputs: &[GpuTensor],
        ) -> Result<GpuTensor, GpuBackendError>;
    }
}

/// Rust implementation boundary for fixed GPU operations.
///
/// Implementations own all device resources.  The interpreter supplies only
/// validated host tensors and receives a validated host tensor in return.  The
/// trait is sealed so an implementation cannot expose a second unchecked
/// backend path outside this crate.
pub trait GpuBackend: sealed::GpuBackendImpl {
    fn execute(
        &mut self,
        operation: GpuOperation,
        inputs: &[GpuTensor],
    ) -> Result<GpuTensor, GpuBackendError> {
        for input in inputs {
            input.validate()?;
        }
        validate_operation_inputs(operation, inputs)?;
        validate_operation_output_size(operation, inputs)?;
        let output = sealed::GpuBackendImpl::execute_unchecked(self, operation, inputs)?;
        output.validate()?;
        validate_operation_output(operation, inputs, &output)?;
        Ok(output)
    }
}

/// Deterministic CPU reference implementation used by tests and explicit
/// non-production fallback registrations.
#[derive(Debug, Default, Clone, Copy)]
pub struct CpuGpuBackend;

impl sealed::GpuBackendImpl for CpuGpuBackend {
    fn execute_unchecked(
        &mut self,
        operation: GpuOperation,
        inputs: &[GpuTensor],
    ) -> Result<GpuTensor, GpuBackendError> {
        match operation {
            GpuOperation::AddF32 => cpu_add(inputs),
            GpuOperation::ScaleF32 => cpu_scale(inputs),
            GpuOperation::MatmulF32 => cpu_matmul(inputs),
        }
    }
}

impl GpuBackend for CpuGpuBackend {}

fn check_arity(operation: GpuOperation, inputs: &[GpuTensor]) -> Result<(), GpuBackendError> {
    if inputs.len() == operation.arity() {
        Ok(())
    } else {
        Err(GpuBackendError::invalid(format!(
            "{} expects {} tensors, got {}",
            operation.name(),
            operation.arity(),
            inputs.len()
        )))
    }
}

fn validate_operation_inputs(
    operation: GpuOperation,
    inputs: &[GpuTensor],
) -> Result<(), GpuBackendError> {
    check_arity(operation, inputs)?;
    match operation {
        GpuOperation::AddF32 => {
            if inputs[0].shape != inputs[1].shape {
                return Err(GpuBackendError::invalid(
                    "gpu_add_f32 expects equal tensor shapes",
                ));
            }
        }
        GpuOperation::ScaleF32 => {
            if !inputs[1].shape.is_empty() || inputs[1].data.len() != 1 {
                return Err(GpuBackendError::invalid(
                    "gpu_scale_f32 expects a scalar f32 multiplier",
                ));
            }
        }
        GpuOperation::MatmulF32 => {
            let [_, inner] = inputs[0].shape.as_slice() else {
                return Err(GpuBackendError::invalid(
                    "gpu_matmul_f32 expects rank-2 left input",
                ));
            };
            let [right_inner, _] = inputs[1].shape.as_slice() else {
                return Err(GpuBackendError::invalid(
                    "gpu_matmul_f32 expects rank-2 right input",
                ));
            };
            if inner != right_inner {
                return Err(GpuBackendError::invalid(
                    "gpu_matmul_f32 inner dimensions do not match",
                ));
            }
        }
    }
    Ok(())
}

fn validate_operation_output_size(
    operation: GpuOperation,
    inputs: &[GpuTensor],
) -> Result<(), GpuBackendError> {
    let output_elements = match operation {
        GpuOperation::AddF32 | GpuOperation::ScaleF32 => inputs[0].data.len(),
        GpuOperation::MatmulF32 => {
            let [rows, _] = inputs[0].shape.as_slice() else {
                unreachable!("operation inputs were validated above");
            };
            let [_, columns] = inputs[1].shape.as_slice() else {
                unreachable!("operation inputs were validated above");
            };
            rows.checked_mul(*columns)
                .ok_or_else(|| GpuBackendError::invalid("GPU operation output is too large"))?
        }
    };
    let output_bytes = output_elements
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| GpuBackendError::invalid("GPU operation output size overflow"))?;
    if output_bytes > GPU_MAX_TENSOR_BYTES {
        return Err(GpuBackendError::invalid(format!(
            "{} output exceeds the GPU-v1 byte limit",
            operation.name()
        )));
    }
    Ok(())
}

fn validate_operation_output(
    operation: GpuOperation,
    inputs: &[GpuTensor],
    output: &GpuTensor,
) -> Result<(), GpuBackendError> {
    let expected_shape = match operation {
        GpuOperation::AddF32 | GpuOperation::ScaleF32 => inputs[0].shape.clone(),
        GpuOperation::MatmulF32 => vec![inputs[0].shape[0], inputs[1].shape[1]],
    };
    if output.shape != expected_shape {
        return Err(GpuBackendError::execution(format!(
            "{} returned shape {:?}, expected {:?}",
            operation.name(),
            output.shape,
            expected_shape
        )));
    }
    Ok(())
}

fn cpu_add(inputs: &[GpuTensor]) -> Result<GpuTensor, GpuBackendError> {
    check_arity(GpuOperation::AddF32, inputs)?;
    if inputs[0].shape != inputs[1].shape {
        return Err(GpuBackendError::invalid(
            "gpu_add_f32 expects equal tensor shapes",
        ));
    }
    GpuTensor::new(
        inputs[0].shape.clone(),
        inputs[0]
            .data
            .iter()
            .zip(&inputs[1].data)
            .map(|(left, right)| left + right)
            .collect(),
    )
}

fn cpu_scale(inputs: &[GpuTensor]) -> Result<GpuTensor, GpuBackendError> {
    check_arity(GpuOperation::ScaleF32, inputs)?;
    if !inputs[1].shape.is_empty() || inputs[1].data.len() != 1 {
        return Err(GpuBackendError::invalid(
            "gpu_scale_f32 expects a scalar f32 multiplier",
        ));
    }
    let scalar = inputs[1].data[0];
    GpuTensor::new(
        inputs[0].shape.clone(),
        inputs[0].data.iter().map(|value| value * scalar).collect(),
    )
}

fn cpu_matmul(inputs: &[GpuTensor]) -> Result<GpuTensor, GpuBackendError> {
    check_arity(GpuOperation::MatmulF32, inputs)?;
    let [left, right] = inputs else {
        unreachable!("arity checked above");
    };
    let [rows, inner] = left.shape.as_slice() else {
        return Err(GpuBackendError::invalid(
            "gpu_matmul_f32 expects rank-2 left input",
        ));
    };
    let [right_inner, columns] = right.shape.as_slice() else {
        return Err(GpuBackendError::invalid(
            "gpu_matmul_f32 expects rank-2 right input",
        ));
    };
    if inner != right_inner {
        return Err(GpuBackendError::invalid(
            "gpu_matmul_f32 inner dimensions do not match",
        ));
    }
    let output_len = rows
        .checked_mul(*columns)
        .ok_or_else(|| GpuBackendError::invalid("gpu_matmul_f32 output is too large"))?;
    let output_bytes = output_len
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| GpuBackendError::invalid("gpu_matmul_f32 output size overflow"))?;
    if output_bytes > GPU_MAX_TENSOR_BYTES {
        return Err(GpuBackendError::invalid(
            "gpu_matmul_f32 output exceeds the GPU-v1 byte limit",
        ));
    }
    let mut output = vec![0.0f32; output_len];
    for row in 0..*rows {
        for column in 0..*columns {
            let mut value = 0.0f32;
            for offset in 0..*inner {
                value += left.data[row * *inner + offset] * right.data[offset * *columns + column];
            }
            output[row * *columns + column] = value;
        }
    }
    GpuTensor::new(vec![*rows, *columns], output)
}

/// Convert a managed scalar/list tree to a contiguous f32 tensor.
pub(crate) fn tensor_from_value(value: &Value) -> Result<GpuTensor, RuntimeError> {
    let mut shape = Vec::new();
    let mut data = Vec::new();
    flatten_value(value, &mut shape, &mut data, 0)?;
    GpuTensor::new(shape, data).map_err(|error| gpu_error_to_runtime(&error))
}

fn exact_f32_from_i64(value: i64) -> Option<f32> {
    // A binary32 value has 24 significant bits (including the hidden bit).
    // Check the integer bit pattern before converting: float-to-int casts
    // saturate at the i64 boundary, so a round-trip comparison would wrongly
    // accept i64::MAX after it rounds to 2^63.
    let magnitude = value.unsigned_abs();
    let significant_bits = 64 - magnitude.leading_zeros();
    let required_trailing_zeros = significant_bits.saturating_sub(24);
    if magnitude != 0 && magnitude.trailing_zeros() < required_trailing_zeros {
        return None;
    }
    let converted = value as f32;
    converted.is_finite().then_some(converted)
}

#[expect(clippy::cast_possible_truncation)]
fn finite_f32_from_f64(value: f64) -> Option<f32> {
    let converted = value as f32;
    converted.is_finite().then_some(converted)
}

fn flatten_value(
    value: &Value,
    shape: &mut Vec<usize>,
    data: &mut Vec<f32>,
    depth: usize,
) -> Result<(), RuntimeError> {
    match value {
        Value::Int(value) => {
            let Some(converted) = exact_f32_from_i64(*value) else {
                return Err(RuntimeError::new(
                    "gpu_input_error",
                    "integer tensor values must be exactly representable as f32",
                ));
            };
            push_tensor_value(data, converted)
        }
        Value::Float(value) => {
            let Some(converted) = finite_f32_from_f64(*value) else {
                return Err(RuntimeError::new(
                    "gpu_input_error",
                    "GPU tensor values must be finite f32-compatible numbers",
                ));
            };
            push_tensor_value(data, converted)
        }
        Value::List(values) => {
            if values.is_empty() {
                return Err(RuntimeError::new(
                    "gpu_input_error",
                    "GPU tensors cannot contain empty dimensions",
                ));
            }
            if depth >= 8 {
                return Err(RuntimeError::new(
                    "gpu_input_error",
                    "tensor rank exceeds the GPU-v1 limit",
                ));
            }
            let mut child_shape = None;
            for value in values {
                let mut nested_shape = Vec::new();
                flatten_value(value, &mut nested_shape, data, depth + 1)?;
                if let Some(expected_shape) = &child_shape {
                    if expected_shape != &nested_shape {
                        return Err(RuntimeError::new(
                            "gpu_input_error",
                            "GPU tensor dimensions must be rectangular",
                        ));
                    }
                } else {
                    child_shape = Some(nested_shape);
                }
            }
            shape.push(values.len());
            if let Some(child_shape) = child_shape {
                shape.extend(child_shape);
            }
            Ok(())
        }
        _ => Err(RuntimeError::new(
            "gpu_input_error",
            "GPU operations expect numbers or rectangular numeric lists",
        )),
    }
}

fn push_tensor_value(data: &mut Vec<f32>, value: f32) -> Result<(), RuntimeError> {
    let next_len = data
        .len()
        .checked_add(1)
        .ok_or_else(|| RuntimeError::new("gpu_input_error", "GPU tensor size overflow"))?;
    let next_bytes = next_len
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| RuntimeError::new("gpu_input_error", "GPU tensor size overflow"))?;
    if next_bytes > GPU_MAX_TENSOR_BYTES {
        return Err(RuntimeError::new(
            "gpu_input_error",
            "GPU tensor exceeds the GPU-v1 byte limit",
        ));
    }
    data.push(value);
    Ok(())
}

#[derive(Clone, Copy)]
struct TensorValueUpperBound {
    canonical_bytes: u64,
    depth: u64,
    max_collection_items: u64,
}

// A finite f32 converted to f64 always has a substantially shorter canonical
// representation than this bound.  Keeping the bound conservative lets the
// evaluator reject an oversized managed representation before allocating its
// nested Value tree without depending on the actual tensor contents.
const MAX_CANONICAL_FLOAT_BYTES: u64 = 64;

pub(crate) fn validate_tensor_value_limits(
    tensor: &GpuTensor,
    max_value_bytes: u64,
    max_collection_items: u64,
    max_value_depth: u64,
    max_value_materialization_bytes: u64,
    materialized_value_bytes: u64,
) -> Result<(), RuntimeError> {
    let bound = tensor_value_upper_bound(&tensor.shape)?;
    if bound.canonical_bytes > max_value_bytes
        || bound.depth > max_value_depth
        || bound.max_collection_items > max_collection_items
    {
        return Err(RuntimeError::new(
            "value_limit_exceeded",
            "GPU tensor output exceeds managed value limits",
        ));
    }
    if max_value_materialization_bytes != u64::MAX {
        let next = materialized_value_bytes
            .checked_add(bound.canonical_bytes)
            .ok_or_else(|| {
                RuntimeError::new(
                    "value_limit_exceeded",
                    "GPU tensor output exceeds the materialization limit",
                )
            })?;
        if next > max_value_materialization_bytes {
            return Err(RuntimeError::new(
                "value_limit_exceeded",
                "GPU tensor output exceeds the materialization limit",
            ));
        }
    }
    Ok(())
}

fn tensor_value_upper_bound(shape: &[usize]) -> Result<TensorValueUpperBound, RuntimeError> {
    if shape.is_empty() {
        return Ok(TensorValueUpperBound {
            canonical_bytes: MAX_CANONICAL_FLOAT_BYTES,
            depth: 1,
            max_collection_items: 0,
        });
    }
    let item_count = u64::try_from(shape[0]).map_err(|_| {
        RuntimeError::new(
            "value_limit_exceeded",
            "GPU tensor output dimensions are out of range",
        )
    })?;
    if item_count == 0 {
        return Err(RuntimeError::new(
            "value_limit_exceeded",
            "GPU tensor output contains an empty dimension",
        ));
    }
    let child = tensor_value_upper_bound(&shape[1..])?;
    let separators = item_count.checked_sub(1).ok_or_else(|| {
        RuntimeError::new("value_limit_exceeded", "GPU tensor output size overflow")
    })?;
    let canonical_bytes = 2u64
        .checked_add(separators)
        .and_then(|bytes| {
            item_count
                .checked_mul(child.canonical_bytes)
                .and_then(|children| bytes.checked_add(children))
        })
        .ok_or_else(|| {
            RuntimeError::new("value_limit_exceeded", "GPU tensor output is too large")
        })?;
    let depth = child.depth.checked_add(1).ok_or_else(|| {
        RuntimeError::new("value_limit_exceeded", "GPU tensor output is too deep")
    })?;
    Ok(TensorValueUpperBound {
        canonical_bytes,
        depth,
        max_collection_items: item_count.max(child.max_collection_items),
    })
}

/// Convert a contiguous tensor back into the ordinary managed value model.
pub(crate) fn value_from_tensor(tensor: &GpuTensor) -> Result<Value, RuntimeError> {
    tensor
        .validate()
        .map_err(|error| gpu_error_to_runtime(&error))?;
    let mut offset = 0usize;
    let value = value_from_tensor_at(&tensor.shape, &tensor.data, &mut offset)?;
    if offset != tensor.data.len() {
        return Err(RuntimeError::new(
            "gpu_execution_error",
            "GPU backend returned an invalid tensor payload",
        ));
    }
    Ok(value)
}

fn value_from_tensor_at(
    shape: &[usize],
    data: &[f32],
    offset: &mut usize,
) -> Result<Value, RuntimeError> {
    if shape.is_empty() {
        let value = *data.get(*offset).ok_or_else(|| {
            RuntimeError::new("gpu_execution_error", "GPU tensor payload is truncated")
        })?;
        *offset += 1;
        return Ok(Value::Float(f64::from(value)));
    }
    let mut values = Vec::with_capacity(shape[0]);
    for _ in 0..shape[0] {
        values.push(value_from_tensor_at(&shape[1..], data, offset)?);
    }
    Ok(Value::List(values))
}

fn gpu_error_to_runtime(error: &GpuBackendError) -> RuntimeError {
    RuntimeError::new(error.code(), error.message())
}

#[cfg(all(feature = "cuda", target_os = "linux"))]
mod cuda;

#[cfg(all(feature = "cuda", target_os = "linux"))]
pub use cuda::CudaGpuBackend;

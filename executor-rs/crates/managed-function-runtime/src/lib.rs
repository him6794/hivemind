//! Metered managed function runtime.

/// Canonical `managed-function-v0` semantics, metering, billing, and result
/// contract manifest.
///
/// The bytes are deliberately checked into the runtime crate and hash-pinned
/// by contract tests. Changing them is a runtime contract migration, not an
/// in-place behavior edit.
pub const V0_SEMANTICS_MANIFEST_JSON: &str = include_str!("../managed-function-v0-semantics.json");

/// SHA-256 of the canonical JSON bytes in [`V0_SEMANTICS_MANIFEST_JSON`],
/// excluding the file's trailing newline.
pub const V0_SEMANTICS_MANIFEST_SHA256: &str =
    "d61a8134f665100855402d7455cfcf3b3e701a79ad43e0039f4ad6c5f05bafef";

/// Canonical `hcode` semantics, metering, billing, and result
/// contract manifest. The v1 contract reuses the closed interpreter while
/// versioning its budget and settlement semantics independently from v0.
pub const V1_SEMANTICS_MANIFEST_JSON: &str = include_str!("../hcode-semantics.json");

/// SHA-256 of the canonical JSON bytes in [`V1_SEMANTICS_MANIFEST_JSON`],
/// excluding the file's trailing newline.
pub const V1_SEMANTICS_MANIFEST_SHA256: &str =
    "0c13a1da5fbbb94de2fbb5395ee807ab43e082b5c6d064deca7ece90700de822";

/// Versioned metering identity committed by the v1 semantics manifest.
pub const V1_COST_MODEL_ID: &str = "hcode-execution-cpt-v1";

/// Canonical GPU-v1 semantics and operation-registry manifest.
pub const GPU_SEMANTICS_MANIFEST_JSON: &str = include_str!("../hcode-gpu-semantics.json");
/// SHA-256 of the canonical GPU-v1 manifest excluding its trailing newline.
pub const GPU_SEMANTICS_MANIFEST_SHA256: &str =
    "4ad70f9be7ae18660566c727759a6dd780a8d70cecc6b2a075584c2c8567293f";

pub mod artifacts;
mod deterministic_math;
pub mod dry_run;
mod gpu;
mod tensor;

pub use artifacts::TensorArtifacts;
pub use tensor::{DType, Tensor};

#[cfg(all(feature = "cuda", target_os = "linux"))]
pub use gpu::CudaGpuBackend;
pub use gpu::{
    CpuGpuBackend, GPU_BILLING_VERSION, GPU_COST_MODEL_VERSION, GPU_MAX_TENSOR_BYTES,
    GPU_OPERATION_COST, GPU_OPERATION_REGISTRY_VERSION, GPU_RUNTIME_VERSION, GpuBackend,
    GpuBackendError, GpuOperation, GpuTensor,
};

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt::{Display, Formatter, Write},
    sync::atomic::{AtomicBool, Ordering},
};

/// Language surface selected for one execution.
///
/// The frozen v0 surface never gains syntax or builtins. CPU hcode and
/// hcode-gpu are separate surfaces, so a change to one cannot silently change
/// the other's consensus results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    /// The frozen `managed-function-v0` language, evaluated unchecked.
    ManagedV0,
    /// CPU `hcode`, evaluated with checked arithmetic.
    Hcode,
    /// `hcode-gpu`, reached only through [`ManagedGpuExecutor`].
    HcodeGpu,
}

impl Surface {
    /// Select the CPU surface from the execution profile. Only the budgeted
    /// hcode profile enables checked arithmetic, so the frozen v0 profile is
    /// identified by its unchecked limits.
    #[must_use]
    const fn for_cpu_limits(limits: &ExecutionLimits) -> Self {
        if limits.checked_integer_arithmetic {
            Self::Hcode
        } else {
            Self::ManagedV0
        }
    }

    /// Floats are available on both hcode surfaces. CPU hcode keeps every float
    /// finite and evaluates its transcendental builtins without the host libm.
    #[must_use]
    const fn floats_enabled(self) -> bool {
        matches!(self, Self::Hcode | Self::HcodeGpu)
    }

    /// CPU hcode extensions: integer remainder, bitwise and shift operators,
    /// and the builtins that exist only on the budgeted CPU surface.
    #[must_use]
    const fn hcode_extensions(self) -> bool {
        matches!(self, Self::Hcode)
    }

    /// `abs`, `min`, and `max` exist on both hcode surfaces. Their float
    /// behavior is still governed by [`Self::floats_enabled`].
    #[must_use]
    const fn numeric_builtins(self) -> bool {
        matches!(self, Self::Hcode | Self::HcodeGpu)
    }
}

/// Per-call charge for the CPU hcode transcendental builtins (`exp`, `ln`,
/// `sin`, `cos`, `tanh`, and `pow`), on top of the call overhead and arguments.
const TRANSCENDENTAL_UNITS: u64 = 16;

/// Applies the CPU hcode float rules to a numeric result. A non-finite float
/// fails the execution at the operation that produced it, and -0.0 folds into
/// 0.0 so every float has one canonical spelling. Other values and other
/// surfaces pass through unchanged.
fn finish_hcode_numeric(surface: Surface, value: Value) -> Result<Value, RuntimeError> {
    match value {
        Value::Float(value) if surface == Surface::Hcode => finite_float(value).map(Value::Float),
        other => Ok(other),
    }
}

fn is_zero_number(value: &Value) -> bool {
    match value {
        Value::Int(value) => *value == 0,
        Value::Float(value) => value.abs().to_bits() == 0,
        _ => false,
    }
}

/// Truncates a finite float that the caller has checked lies in the i64 range.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the caller checks the i64 range before truncating"
)]
fn truncate_to_i64(value: f64) -> i64 {
    value.trunc() as i64
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    List(Vec<Self>),
    Dict(BTreeMap<String, Self>),
    /// A dense tensor. Only CPU hcode can create one.
    Tensor(Tensor),
    Null,
}

/// The CPU hcode float rule for one value: a non-finite float is an error, and
/// -0.0 folds into 0.0.
fn finite_float(value: f64) -> Result<f64, RuntimeError> {
    if !value.is_finite() {
        return Err(RuntimeError::new(
            "runtime_error",
            "float result is not finite",
        ));
    }
    Ok(if value.abs().to_bits() == 0 {
        0.0
    } else {
        value
    })
}

/// Render a managed value using the canonical task-output representation.
///
/// Prefer [`render_output_bounded`] for untrusted task output. This legacy
/// convenience function keeps the historical unlimited behavior.
///
/// # Panics
///
/// Panics only if rendering would overflow `usize`; bounded callers should use
/// [`render_output_bounded`] and handle its structured error instead.
#[must_use]
pub fn render_output(value: &Value) -> String {
    render_output_bounded(value, u64::MAX)
        .expect("an unlimited canonical renderer can only fail on length overflow")
}

/// Render a managed value using the canonical task-output representation,
/// rejecting output that would exceed `max_bytes`.
///
/// Strings are emitted raw at the top level; all other values use the same
/// compact JSON representation as [`render_output`]. The renderer appends
/// incrementally and checks every append before allocation, so a rejected
/// value never first creates an unbounded serialized intermediate. `max_bytes`
/// is a fixed-width logical UTF-8 byte count, so the decision is identical on
/// every supported Worker platform.
pub fn render_output_bounded(value: &Value, max_bytes: u64) -> Result<String, RuntimeError> {
    let mut output = BoundedOutput::new(max_bytes);
    match value {
        Value::String(value) => output.push_str(value)?,
        Value::Int(value) => output.push_str(&value.to_string())?,
        Value::Float(value) => {
            // JSON has no NaN or Infinity; a non-finite result can never be a
            // portable canonical output, so it is rejected instead of rendered.
            if value.is_finite() {
                output.push_str(&canonical_float_string(*value))?;
            } else {
                return Err(RuntimeError::new(
                    "type_error",
                    "non-finite floats cannot be rendered as JSON output",
                ));
            }
        }
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" })?,
        Value::Null => output.push_str("null")?,
        Value::List(_) | Value::Dict(_) | Value::Tensor(_) => {
            write_json_value(value, &mut output)?;
        }
    }
    Ok(output.into_inner())
}

/// Canonical text for one finite f64 in task output and nested JSON.
///
/// `ryu`-free and stable across platforms: Rust's shortest-roundtrip `{}`
/// formatting for f64 is deterministic, and integral values gain a `.0`
/// suffix so floats stay distinguishable from ints after a round trip.
fn canonical_float_string(value: f64) -> String {
    let base = format!("{value}");
    if base.contains('.') || base.contains('e') || base.contains('E') {
        base
    } else {
        format!("{base}.0")
    }
}

struct BoundedOutput {
    value: String,
    rendered_bytes: u64,
    max_bytes: u64,
}

impl BoundedOutput {
    fn new(max_bytes: u64) -> Self {
        Self {
            value: String::new(),
            rendered_bytes: 0,
            max_bytes,
        }
    }

    fn into_inner(self) -> String {
        self.value
    }

    fn push_str(&mut self, text: &str) -> Result<(), RuntimeError> {
        let text_bytes = u64::try_from(text.len()).map_err(|_| output_limit_error())?;
        let next_len = self
            .rendered_bytes
            .checked_add(text_bytes)
            .ok_or_else(output_limit_error)?;
        if next_len > self.max_bytes {
            return Err(output_limit_error());
        }
        self.value.push_str(text);
        self.rendered_bytes = next_len;
        Ok(())
    }

    fn push_char(&mut self, character: char) -> Result<(), RuntimeError> {
        let mut encoded = [0; 4];
        self.push_str(character.encode_utf8(&mut encoded))
    }
}

fn output_limit_error() -> RuntimeError {
    RuntimeError::new("output_limit_exceeded", "output limit exceeded")
}

fn write_json_value(value: &Value, output: &mut BoundedOutput) -> Result<(), RuntimeError> {
    match value {
        Value::Int(value) => output.push_str(&value.to_string()),
        Value::Float(value) => {
            if value.is_finite() {
                output.push_str(&canonical_float_string(*value))
            } else {
                Err(RuntimeError::new(
                    "type_error",
                    "non-finite floats cannot be rendered as JSON output",
                ))
            }
        }
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::String(value) => write_json_string(value, output),
        Value::Null => output.push_str("null"),
        Value::List(values) => {
            output.push_char('[')?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push_char(',')?;
                }
                write_json_value(value, output)?;
            }
            output.push_char(']')
        }
        Value::Dict(values) => {
            output.push_char('{')?;
            for (index, (key, value)) in values.iter().enumerate() {
                if index > 0 {
                    output.push_char(',')?;
                }
                write_json_string(key, output)?;
                output.push_char(':')?;
                write_json_value(value, output)?;
            }
            output.push_char('}')
        }
        Value::Tensor(tensor) => tensor::write_canonical(tensor, output),
    }
}

fn write_json_string(value: &str, output: &mut BoundedOutput) -> Result<(), RuntimeError> {
    output.push_char('"')?;
    let mut segment_start = 0;
    for (index, byte) in value.bytes().enumerate() {
        let escaped = match byte {
            b'"' => Some(r#"\""#),
            b'\\' => Some(r"\\"),
            b'\x08' => Some(r"\b"),
            b'\x0c' => Some(r"\f"),
            b'\n' => Some(r"\n"),
            b'\r' => Some(r"\r"),
            b'\t' => Some(r"\t"),
            b'\x00'..=b'\x1f' => None,
            _ => continue,
        };
        output.push_str(&value[segment_start..index])?;
        if let Some(escaped) = escaped {
            output.push_str(escaped)?;
        } else {
            output.push_str(r"\u00")?;
            output.push_char(hex_digit(byte >> 4))?;
            output.push_char(hex_digit(byte & 0x0f))?;
        }
        segment_start = index + 1;
    }
    output.push_str(&value[segment_start..])?;
    output.push_char('"')
}

fn hex_digit(value: u8) -> char {
    char::from(if value < 10 {
        b'0' + value
    } else {
        b'a' + (value - 10)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionLimits {
    pub max_ops: u64,
    pub max_usage_units: Option<u64>,
    /// Use checked signed-i64 arithmetic for the versioned hcode
    /// profile without changing the frozen v0 arithmetic behavior.
    pub checked_integer_arithmetic: bool,
    pub max_call_depth: usize,
    pub max_output_bytes: u64,
    pub max_loop_iterations: u64,
    /// Maximum canonical-JSON byte size of one materialized managed value.
    ///
    /// This is deterministic logical byte accounting, not allocator capacity:
    /// string/key escaping and collection punctuation are included so native
    /// workers and consensus replicas make the same acceptance decision. The counter
    /// is fixed-width (`u64`), rather than pointer-sized.
    pub max_value_bytes: u64,
    /// Maximum number of direct elements in any materialized list or dict.
    pub max_collection_items: u64,
    /// Maximum nesting depth of any materialized managed value.
    pub max_value_depth: u64,
    /// Maximum cumulative deterministic logical bytes materialized by evaluator
    /// value copies and constructions. This is a safety limit only and never
    /// contributes to billed `usage_units`.
    pub max_value_materialization_bytes: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_ops: 1_000_000,
            max_usage_units: None,
            checked_integer_arithmetic: false,
            max_call_depth: 64,
            max_output_bytes: 1_048_576,
            max_loop_iterations: 100_000,
            max_value_bytes: 1_048_576,
            max_collection_items: 100_000,
            max_value_depth: 64,
            max_value_materialization_bytes: 16_777_216,
        }
    }
}

impl ExecutionLimits {
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            max_ops: u64::MAX,
            max_usage_units: None,
            checked_integer_arithmetic: false,
            max_call_depth: usize::MAX,
            max_output_bytes: u64::MAX,
            max_loop_iterations: u64::MAX,
            max_value_bytes: u64::MAX,
            max_collection_items: u64::MAX,
            max_value_depth: u64::MAX,
            max_value_materialization_bytes: u64::MAX,
        }
    }

    /// Build the production managed-function profile for one replica.
    ///
    /// The user's usage allowance is the only work-count limit. Operation and
    /// loop counters still use checked arithmetic, while output, value, and
    /// call-depth limits remain independent structural safety controls.
    #[must_use]
    pub fn for_managed_function_budget(max_usage_units: u64) -> Self {
        Self {
            max_ops: u64::MAX,
            max_usage_units: Some(max_usage_units),
            checked_integer_arithmetic: true,
            max_loop_iterations: u64::MAX,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionReceipt {
    pub executed_ops: u64,
    pub usage_units: u64,
    pub gpu_operations: u64,
    pub function_calls: u64,
    pub loop_iterations: u64,
    pub max_call_depth: usize,
    pub output_bytes: usize,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionResult {
    pub status: Status,
    pub value: Value,
    pub output: String,
    pub receipt: ExecutionReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    code: &'static str,
    message: String,
    line: Option<usize>,
    column: Option<usize>,
    partial_receipt: Option<Box<ExecutionReceipt>>,
}

impl RuntimeError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The human-readable reason, without the code prefix that `Display` adds.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn line(&self) -> Option<usize> {
        self.line
    }

    #[must_use]
    pub fn column(&self) -> Option<usize> {
        self.column
    }

    /// Return the metering state accumulated before this error.
    #[must_use]
    pub fn partial_receipt(&self) -> Option<&ExecutionReceipt> {
        self.partial_receipt.as_deref()
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            line: None,
            column: None,
            partial_receipt: None,
        }
    }

    fn with_partial_receipt(mut self, receipt: ExecutionReceipt) -> Self {
        self.partial_receipt = Some(Box::new(receipt));
        self
    }

    fn at(mut self, span: Span) -> Self {
        self.line = Some(span.line);
        self.column = Some(span.column);
        self
    }
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl Error for RuntimeError {}

#[derive(Debug, Default)]
pub struct ManagedExecutor;

/// GPU-enabled view of the same parser and evaluator.
///
/// This is a separate runtime entry point so the frozen `managed-function-v0`
/// methods never acquire GPU capabilities accidentally.
pub struct ManagedGpuExecutor<'a> {
    backend: &'a mut dyn GpuBackend,
}

impl<'a> ManagedGpuExecutor<'a> {
    #[must_use]
    pub fn new(backend: &'a mut dyn GpuBackend) -> Self {
        Self { backend }
    }

    pub fn execute(
        &mut self,
        source: &str,
        limits: ExecutionLimits,
    ) -> Result<ExecutionResult, RuntimeError> {
        let tokens = Lexer::with_surface(source, Surface::HcodeGpu).tokenize()?;
        let program = Parser::new(tokens, Surface::HcodeGpu).parse_program()?;
        Evaluator::with_gpu(limits, self.backend, None).eval_program(&program)
    }

    pub fn execute_json_input(
        &mut self,
        source: &str,
        limits: ExecutionLimits,
        input_json: &str,
    ) -> Result<ExecutionResult, RuntimeError> {
        self.execute_json_input_with_cancel(source, limits, input_json, None)
    }

    pub fn execute_json_input_with_cancel(
        &mut self,
        source: &str,
        limits: ExecutionLimits,
        input_json: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let input = Value::from_json_str_with_surface(input_json, Surface::HcodeGpu)?;
        let tokens = Lexer::with_surface(source, Surface::HcodeGpu).tokenize()?;
        let program = Parser::new(tokens, Surface::HcodeGpu).parse_program()?;
        let mut evaluator = Evaluator::with_gpu(limits, self.backend, cancelled);
        evaluator.validate_external_value(&input)?;
        evaluator.current_scope().insert("input".to_string(), input);
        evaluator.eval_program(&program)
    }
}

impl ManagedExecutor {
    #[must_use]
    pub fn with_gpu<'a>(&self, backend: &'a mut dyn GpuBackend) -> ManagedGpuExecutor<'a> {
        ManagedGpuExecutor::new(backend)
    }
    pub fn execute(
        &self,
        source: &str,
        limits: ExecutionLimits,
    ) -> Result<ExecutionResult, RuntimeError> {
        let surface = Surface::for_cpu_limits(&limits);
        let tokens = Lexer::with_surface(source, surface).tokenize()?;
        let program = Parser::new(tokens, surface).parse_program()?;
        Evaluator::new(limits, surface).eval_program(&program)
    }

    pub fn execute_json_input(
        &self,
        source: &str,
        limits: ExecutionLimits,
        input_json: &str,
    ) -> Result<ExecutionResult, RuntimeError> {
        let surface = Surface::for_cpu_limits(&limits);
        let input = Value::from_json_str_with_surface(input_json, surface)?;
        let tokens = Lexer::with_surface(source, surface).tokenize()?;
        let program = Parser::new(tokens, surface).parse_program()?;
        let mut evaluator = Evaluator::new(limits, surface);
        evaluator.validate_external_value(&input)?;
        evaluator.current_scope().insert("input".to_string(), input);
        evaluator.eval_program(&program)
    }

    pub fn execute_json_input_with_cancel(
        &self,
        source: &str,
        limits: ExecutionLimits,
        input_json: &str,
        cancelled: &AtomicBool,
    ) -> Result<ExecutionResult, RuntimeError> {
        let surface = Surface::for_cpu_limits(&limits);
        let input = Value::from_json_str_with_surface(input_json, surface)?;
        let tokens = Lexer::with_surface(source, surface).tokenize()?;
        let program = Parser::new(tokens, surface).parse_program()?;
        let mut evaluator = Evaluator::with_cancellation(limits, surface, cancelled);
        evaluator.validate_external_value(&input)?;
        evaluator.current_scope().insert("input".to_string(), input);
        evaluator.eval_program(&program)
    }
}

impl ManagedExecutor {
    /// Executes CPU hcode with execution-local, digest-verified tensor inputs.
    /// The language receives no path, network, or filesystem capability.
    pub fn execute_json_input_with_artifacts(
        &self,
        source: &str,
        limits: ExecutionLimits,
        input_json: &str,
        artifacts: &TensorArtifacts,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let surface = Surface::for_cpu_limits(&limits);
        if surface != Surface::Hcode {
            return Err(RuntimeError::new(
                "artifact_error",
                "tensor artifacts require CPU hcode",
            ));
        }
        let input = Value::from_json_str_with_surface(input_json, surface)?;
        let tokens = Lexer::with_surface(source, surface).tokenize()?;
        let program = Parser::new(tokens, surface).parse_program()?;
        let mut evaluator = Evaluator {
            artifacts: Some(artifacts),
            cancelled,
            ..Evaluator::new(limits, surface)
        };
        evaluator.validate_external_value(&input)?;
        evaluator.current_scope().insert("input".to_string(), input);
        evaluator.eval_program(&program)
    }
}

impl Value {
    pub fn from_json_str(input: &str) -> Result<Self, RuntimeError> {
        Self::from_json_str_with_surface(input, Surface::ManagedV0)
    }

    fn from_json_str_with_surface(input: &str, surface: Surface) -> Result<Self, RuntimeError> {
        let value = serde_json::from_str::<serde_json::Value>(input)
            .map_err(|e| RuntimeError::new("input_error", format!("invalid JSON input: {e}")))?;
        Self::from_json_value(&value, surface)
    }

    fn from_json_value(value: &serde_json::Value, surface: Surface) -> Result<Self, RuntimeError> {
        match value {
            serde_json::Value::Null => Ok(Self::Null),
            serde_json::Value::Bool(value) => Ok(Self::Bool(*value)),
            serde_json::Value::Number(value) => {
                if let Some(int_value) = value.as_i64() {
                    return Ok(Self::Int(int_value));
                }
                if surface.floats_enabled() {
                    // JSON floats arrive as f64. Non-finite values cannot exist
                    // in conforming JSON, but reject any non-finite conversion
                    // rather than allowing an unrenderable managed value.
                    return value
                        .as_f64()
                        .filter(|value| value.is_finite())
                        .map(Self::Float)
                        .and_then(|value| finish_hcode_numeric(surface, value).ok())
                        .ok_or_else(|| {
                            RuntimeError::new(
                                "input_error",
                                "JSON numbers must be i64 integers or finite f64 floats",
                            )
                        });
                }
                Err(RuntimeError::new(
                    "input_error",
                    "only signed 64-bit JSON integers are supported",
                ))
            }
            serde_json::Value::String(value) => Ok(Self::String(value.clone())),
            serde_json::Value::Array(values) => values
                .iter()
                .map(|value| Self::from_json_value(value, surface))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List),
            serde_json::Value::Object(values) => values
                .iter()
                .map(|(key, value)| Ok((key.clone(), Self::from_json_value(value, surface)?)))
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(Self::Dict),
        }
    }
}

#[derive(Clone, Copy)]
struct ValueMetrics {
    canonical_bytes: u64,
    depth: u64,
    max_collection_items: u64,
}

fn value_metrics(value: &Value) -> Result<ValueMetrics, RuntimeError> {
    match value {
        Value::Int(value) => Ok(ValueMetrics {
            canonical_bytes: logical_usize(value.to_string().len())?,
            depth: 1,
            max_collection_items: 0,
        }),
        Value::Float(value) => {
            // Non-finite floats are unrenderable, so their canonical size is
            // undefined; fail the same way the renderer would.
            if !value.is_finite() {
                return Err(RuntimeError::new(
                    "type_error",
                    "non-finite floats cannot be rendered as JSON output",
                ));
            }
            Ok(ValueMetrics {
                canonical_bytes: logical_usize(canonical_float_string(*value).len())?,
                depth: 1,
                max_collection_items: 0,
            })
        }
        Value::Bool(value) => Ok(ValueMetrics {
            canonical_bytes: if *value { 4 } else { 5 },
            depth: 1,
            max_collection_items: 0,
        }),
        Value::String(value) => Ok(ValueMetrics {
            canonical_bytes: json_string_len(value)?,
            depth: 1,
            max_collection_items: 0,
        }),
        Value::Null => Ok(ValueMetrics {
            canonical_bytes: 4,
            depth: 1,
            max_collection_items: 0,
        }),
        Value::List(values) => list_value_metrics(values),
        Value::Dict(values) => dict_value_metrics(values),
        Value::Tensor(tensor) => tensor::metrics(tensor),
    }
}

fn list_value_metrics(values: &[Value]) -> Result<ValueMetrics, RuntimeError> {
    let mut canonical_bytes = 2;
    let mut depth = 1;
    let mut max_collection_items = logical_usize(values.len())?;
    let mut first = true;
    for value in values {
        if first {
            first = false;
        } else {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        let metrics = value_metrics(value)?;
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    Ok(ValueMetrics {
        canonical_bytes,
        depth,
        max_collection_items,
    })
}

fn dict_value_metrics(values: &BTreeMap<String, Value>) -> Result<ValueMetrics, RuntimeError> {
    let mut canonical_bytes = 2;
    let mut depth = 1;
    let mut max_collection_items = logical_usize(values.len())?;
    let mut first = true;
    for (key, value) in values {
        if first {
            first = false;
        } else {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        canonical_bytes = checked_value_add(canonical_bytes, json_string_len(key)?)?;
        canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        let metrics = value_metrics(value)?;
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    Ok(ValueMetrics {
        canonical_bytes,
        depth,
        max_collection_items,
    })
}

fn list_metrics_after_assignment(
    values: &[Value],
    replacement_index: usize,
    replacement: &Value,
) -> Result<ValueMetrics, RuntimeError> {
    let mut canonical_bytes = 2;
    let mut depth = 1;
    let mut max_collection_items = logical_usize(values.len())?;
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        let metrics = if index == replacement_index {
            value_metrics(replacement)?
        } else {
            value_metrics(value)?
        };
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    Ok(ValueMetrics {
        canonical_bytes,
        depth,
        max_collection_items,
    })
}

fn dict_metrics_after_assignment(
    values: &BTreeMap<String, Value>,
    replacement_key: &str,
    replacement: &Value,
) -> Result<ValueMetrics, RuntimeError> {
    let contains_key = values.contains_key(replacement_key);
    let value_count = logical_usize(values.len())?;
    let item_count = if contains_key {
        value_count
    } else {
        checked_value_add(value_count, 1)?
    };
    let mut canonical_bytes = 2;
    let mut depth = 1;
    let mut max_collection_items = item_count;
    let mut first = true;
    for (key, value) in values {
        if first {
            first = false;
        } else {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        canonical_bytes = checked_value_add(canonical_bytes, json_string_len(key)?)?;
        canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        let metrics = if key == replacement_key {
            value_metrics(replacement)?
        } else {
            value_metrics(value)?
        };
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    if !contains_key {
        if !first {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        canonical_bytes = checked_value_add(canonical_bytes, json_string_len(replacement_key)?)?;
        canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        let metrics = value_metrics(replacement)?;
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    Ok(ValueMetrics {
        canonical_bytes,
        depth,
        max_collection_items,
    })
}

fn json_string_len(value: &str) -> Result<u64, RuntimeError> {
    let mut length = 2;
    for byte in value.bytes() {
        let encoded_len = match byte {
            b'"' | b'\\' | b'\x08' | b'\x0c' | b'\n' | b'\r' | b'\t' => 2,
            b'\x00'..=b'\x1f' => 6,
            _ => 1,
        };
        length = checked_value_add(length, encoded_len)?;
    }
    Ok(length)
}

fn logical_usize(value: usize) -> Result<u64, RuntimeError> {
    u64::try_from(value).map_err(|_| value_limit_error())
}

fn checked_value_add(left: u64, right: u64) -> Result<u64, RuntimeError> {
    left.checked_add(right).ok_or_else(value_limit_error)
}

fn checked_value_mul(left: u64, right: u64) -> Result<u64, RuntimeError> {
    left.checked_mul(right).ok_or_else(value_limit_error)
}

/// Sorts a list whose items are all ints, all floats, or all strings. Floats are finite, so total_cmp orders them
/// as numbers. Equal items are indistinguishable, so the result does not depend on the sorting algorithm.
fn sort_hcode_values(values: &mut Vec<Value>) -> Result<(), RuntimeError> {
    let mut ints = Vec::new();
    let mut floats = Vec::new();
    let mut strings = Vec::new();
    for value in values.drain(..) {
        match value {
            Value::Int(number) => ints.push(number),
            Value::Float(number) => floats.push(number),
            Value::String(text) => strings.push(text),
            _ => return Err(sort_type_error()),
        }
    }
    let kinds_present = usize::from(!ints.is_empty())
        + usize::from(!floats.is_empty())
        + usize::from(!strings.is_empty());
    if kinds_present > 1 {
        return Err(sort_type_error());
    }
    ints.sort_unstable();
    floats.sort_unstable_by(f64::total_cmp);
    strings.sort_unstable();
    values.extend(ints.into_iter().map(Value::Int));
    values.extend(floats.into_iter().map(Value::Float));
    values.extend(strings.into_iter().map(Value::String));
    Ok(())
}

fn sort_type_error() -> RuntimeError {
    RuntimeError::new(
        "type_error",
        "sort expects a list of ints, floats, or strings of one type",
    )
}

fn loop_control_outside_loop() -> RuntimeError {
    RuntimeError::new(
        "parse_error",
        "break and continue are only valid inside a loop",
    )
}

fn value_limit_error() -> RuntimeError {
    RuntimeError::new("value_limit_exceeded", "value limit exceeded")
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Null,
    And,
    Or,
    Not,
    Ident(String),
    Int(i64),
    Float(f64),
    String(String),
    True,
    False,
    Let,
    Fn,
    Return,
    For,
    In,
    If,
    Else,
    While,
    Break,
    Continue,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    Eq,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Semi,
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct SpannedToken {
    token: Token,
    span: Span,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    column: usize,
    surface: Surface,
}

impl<'a> Lexer<'a> {
    fn with_surface(source: &'a str, surface: Surface) -> Self {
        Self {
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
            surface,
        }
    }

    fn tokenize(mut self) -> Result<Vec<SpannedToken>, RuntimeError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let span = self.span();
            let Some(byte) = self.peek() else {
                tokens.push(SpannedToken {
                    token: Token::Eof,
                    span,
                });
                return Ok(tokens);
            };
            let token = match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.identifier(),
                b'0'..=b'9' => self.integer()?,
                b'"' => self.string()?,
                b'+' => {
                    self.bump();
                    Token::Plus
                }
                b'-' => {
                    self.bump();
                    Token::Minus
                }
                b'*' => {
                    self.bump();
                    Token::Star
                }
                b'/' => {
                    self.bump();
                    Token::Slash
                }
                b'%' if self.surface.hcode_extensions() => {
                    self.bump();
                    Token::Percent
                }
                b'&' if self.surface.hcode_extensions() => {
                    self.bump();
                    Token::Amp
                }
                b'|' if self.surface.hcode_extensions() => {
                    self.bump();
                    Token::Pipe
                }
                b'^' if self.surface.hcode_extensions() => {
                    self.bump();
                    Token::Caret
                }
                b'~' if self.surface.hcode_extensions() => {
                    self.bump();
                    Token::Tilde
                }
                b'=' => {
                    self.bump();
                    if self.consume_byte(b'=') {
                        Token::EqEq
                    } else {
                        Token::Eq
                    }
                }
                b'!' => {
                    self.bump();
                    if self.consume_byte(b'=') {
                        Token::BangEq
                    } else {
                        return Err(RuntimeError::new("parse_error", "expected != after !"));
                    }
                }
                b'<' => {
                    self.bump();
                    if self.consume_byte(b'=') {
                        Token::LtEq
                    } else if self.surface.hcode_extensions() && self.consume_byte(b'<') {
                        Token::Shl
                    } else {
                        Token::Lt
                    }
                }
                b'>' => {
                    self.bump();
                    if self.consume_byte(b'=') {
                        Token::GtEq
                    } else if self.surface.hcode_extensions() && self.consume_byte(b'>') {
                        Token::Shr
                    } else {
                        Token::Gt
                    }
                }
                b'(' => {
                    self.bump();
                    Token::LParen
                }
                b')' => {
                    self.bump();
                    Token::RParen
                }
                b'{' => {
                    self.bump();
                    Token::LBrace
                }
                b'}' => {
                    self.bump();
                    Token::RBrace
                }
                b'[' => {
                    self.bump();
                    Token::LBracket
                }
                b']' => {
                    self.bump();
                    Token::RBracket
                }
                b':' => {
                    self.bump();
                    Token::Colon
                }
                b',' => {
                    self.bump();
                    Token::Comma
                }
                b';' => {
                    self.bump();
                    Token::Semi
                }
                _ => {
                    return Err(RuntimeError::new(
                        "parse_error",
                        format!("unexpected character '{}'", char::from(byte)),
                    ));
                }
            };
            tokens.push(SpannedToken { token, span });
        }
    }

    fn skip_whitespace(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.bump(),
                // On the CPU hcode surface, `//` starts a comment that runs to the end of its line.
                Some(b'/')
                    if self.surface.hcode_extensions()
                        && self.byte_at(self.pos + 1) == Some(b'/') =>
                {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    fn span(&self) -> Span {
        Span {
            line: self.line,
            column: self.column,
        }
    }

    fn bump(&mut self) {
        let byte = self.bytes[self.pos];
        self.pos += 1;
        if byte == b'\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn byte_at(&self, position: usize) -> Option<u8> {
        self.bytes.get(position).copied()
    }

    fn consume_byte(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn identifier(&mut self) -> Token {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
            self.bump();
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.pos]);
        match text.as_ref() {
            "let" => Token::Let,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "for" => Token::For,
            "in" => Token::In,
            "if" => Token::If,
            "else" => Token::Else,
            "while" if self.surface.hcode_extensions() => Token::While,
            "break" if self.surface.hcode_extensions() => Token::Break,
            "continue" if self.surface.hcode_extensions() => Token::Continue,
            "true" => Token::True,
            "false" => Token::False,
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "null" => Token::Null,
            _ => Token::Ident(text.into_owned()),
        }
    }

    fn integer(&mut self) -> Result<Token, RuntimeError> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        // A digit run becomes a float literal when followed by a fractional
        // part (`1.5`) or an exponent (`1e3`, `1E-3`). `1.` alone is rejected:
        // JSON and most C-family grammars require a fraction digit.
        let is_float = self.surface.floats_enabled()
            && match self.peek() {
                Some(b'.') => matches!(self.byte_at(self.pos + 1), Some(b'0'..=b'9')),
                Some(b'e' | b'E') => true,
                _ => false,
            };
        if !is_float {
            let text = std::str::from_utf8(&self.bytes[start..self.pos])
                .map_err(|_| RuntimeError::new("parse_error", "invalid integer"))?;
            let value = text
                .parse::<i64>()
                .map_err(|_| RuntimeError::new("parse_error", "integer is out of range"))?;
            return Ok(Token::Int(value));
        }
        if matches!(self.peek(), Some(b'.')) {
            self.bump();
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(RuntimeError::new(
                    "parse_error",
                    "float exponent requires a digit",
                ));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| RuntimeError::new("parse_error", "invalid float literal"))?;
        let value = text
            .parse::<f64>()
            .map_err(|_| RuntimeError::new("parse_error", "float literal is out of range"))?;
        // Rust parses an overflowing literal such as `1e400` to infinity. CPU
        // hcode keeps every float finite, so the literal is rejected here.
        if self.surface.hcode_extensions() && !value.is_finite() {
            return Err(RuntimeError::new(
                "parse_error",
                "float literal is out of range",
            ));
        }
        Ok(Token::Float(value))
    }

    fn string(&mut self) -> Result<Token, RuntimeError> {
        self.bump();
        let mut value = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(RuntimeError::new("parse_error", "unterminated string"));
            };
            self.bump();
            match byte {
                b'"' => return Ok(Token::String(value)),
                b'\\' => {
                    let Some(escaped) = self.peek() else {
                        return Err(RuntimeError::new(
                            "parse_error",
                            "unterminated string escape",
                        ));
                    };
                    self.bump();
                    let ch = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        _ => {
                            return Err(RuntimeError::new(
                                "parse_error",
                                "unsupported string escape",
                            ));
                        }
                    };
                    value.push(ch);
                }
                _ => value.push(char::from(byte)),
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Program {
    statements: Vec<Stmt>,
}

#[derive(Debug, Clone)]
enum Stmt {
    Let(String, Expr),
    Fn(Function),
    Return(Expr),
    For {
        item: String,
        iterable: Expr,
        body: Vec<Self>,
    },
    Print(Expr),
    IndexAssign {
        target: Expr,
        index: Expr,
        value: Expr,
    },
    Expr(Expr),
    While {
        condition: Expr,
        body: Vec<Self>,
    },
    If {
        condition: Expr,
        then_body: Vec<Self>,
        else_body: Vec<Self>,
    },
    Break,
    Continue,
}

#[derive(Debug, Clone)]
struct Function {
    name: String,
    params: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
enum Expr {
    Value(Value),
    Variable(String),
    If {
        condition: Box<Self>,
        then_expr: Box<Self>,
        else_expr: Box<Self>,
    },
    Call {
        name: String,
        args: Vec<Self>,
    },
    List(Vec<Self>),
    Dict(Vec<(String, Self)>),
    Binary {
        left: Box<Self>,
        op: BinaryOp,
        right: Box<Self>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Self>,
    },
    Logical {
        left: Box<Self>,
        op: LogicalOp,
        right: Box<Self>,
    },
    Index {
        target: Box<Self>,
        index: Box<Self>,
    },
}

#[derive(Debug, Clone, Copy)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

#[derive(Debug, Clone, Copy)]
enum UnaryOp {
    Neg,
    Pos,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy)]
enum LogicalOp {
    And,
    Or,
}

struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    surface: Surface,
    loop_depth: usize,
}

impl Parser {
    fn new(tokens: Vec<SpannedToken>, surface: Surface) -> Self {
        Self {
            tokens,
            pos: 0,
            surface,
            loop_depth: 0,
        }
    }

    fn parse_program(mut self) -> Result<Program, RuntimeError> {
        let mut statements = Vec::new();
        while !matches!(self.peek(), Token::Eof) {
            statements.push(self.statement()?);
        }
        Ok(Program { statements })
    }

    fn statement(&mut self) -> Result<Stmt, RuntimeError> {
        match self.peek() {
            Token::Let => {
                self.advance();
                let name = self.ident()?;
                self.expect(&Token::Eq)?;
                let expr = self.expression()?;
                self.expect(&Token::Semi)?;
                Ok(Stmt::Let(name, expr))
            }
            Token::Fn => self.function().map(Stmt::Fn),
            Token::Return => {
                self.advance();
                let expr = self.expression()?;
                self.expect(&Token::Semi)?;
                Ok(Stmt::Return(expr))
            }
            Token::For => self.for_statement(),
            Token::While => self.while_statement(),
            Token::Break => self.loop_control(Stmt::Break),
            Token::Continue => self.loop_control(Stmt::Continue),
            Token::If if self.surface.hcode_extensions() => self.if_statement_or_expression(),
            Token::Ident(name) if name == "print" => {
                self.advance();
                self.expect(&Token::LParen)?;
                let expr = self.expression()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semi)?;
                Ok(Stmt::Print(expr))
            }
            _ => {
                let expr = self.expression()?;
                if matches!(self.peek(), Token::Eq) {
                    self.advance();
                    let value = self.expression()?;
                    self.expect(&Token::Semi)?;
                    let Expr::Index { target, index } = expr else {
                        return Err(RuntimeError::new(
                            "parse_error",
                            "assignment target must be an index expression",
                        ));
                    };
                    return Ok(Stmt::IndexAssign {
                        target: *target,
                        index: *index,
                        value,
                    });
                }
                self.expect(&Token::Semi)?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    /// `{ statements }`.
    fn statement_block(&mut self) -> Result<Vec<Stmt>, RuntimeError> {
        self.expect(&Token::LBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            body.push(self.statement()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(body)
    }

    /// The body of a `for` or `while` loop. break and continue inside it act on this loop.
    fn loop_body(&mut self) -> Result<Vec<Stmt>, RuntimeError> {
        self.loop_depth += 1;
        let body = self.statement_block();
        self.loop_depth -= 1;
        body
    }

    fn while_statement(&mut self) -> Result<Stmt, RuntimeError> {
        self.expect(&Token::While)?;
        let condition = self.expression()?;
        let body = self.loop_body()?;
        Ok(Stmt::While { condition, body })
    }

    fn loop_control(&mut self, statement: Stmt) -> Result<Stmt, RuntimeError> {
        let keyword = if matches!(self.peek(), Token::Break) {
            "break"
        } else {
            "continue"
        };
        self.advance();
        if self.loop_depth == 0 {
            return Err(RuntimeError::new(
                "parse_error",
                format!("{keyword} is only valid inside a loop"),
            ));
        }
        self.expect(&Token::Semi)?;
        Ok(statement)
    }

    /// An `if` that starts a statement is a statement when its branches are statements. Otherwise it is read
    /// as an expression statement, exactly as before, so existing programs keep their meaning.
    fn if_statement_or_expression(&mut self) -> Result<Stmt, RuntimeError> {
        let start = self.pos;
        let outer_loop_depth = self.loop_depth;
        let statement_error = match self.if_statement() {
            Ok(statement) => return Ok(statement),
            Err(error) => error,
        };
        self.pos = start;
        self.loop_depth = outer_loop_depth;
        let as_expression = self.expression().and_then(|expression| {
            self.expect(&Token::Semi)?;
            Ok(Stmt::Expr(expression))
        });
        if let Ok(statement) = as_expression {
            return Ok(statement);
        }
        // Neither reading parses. The statement error points at the broken branch.
        self.pos = start;
        self.loop_depth = outer_loop_depth;
        Err(statement_error)
    }

    fn if_statement(&mut self) -> Result<Stmt, RuntimeError> {
        self.expect(&Token::If)?;
        let condition = self.expression()?;
        let then_body = self.statement_block()?;
        let else_body = if matches!(self.peek(), Token::Else) {
            self.advance();
            if matches!(self.peek(), Token::If) {
                vec![self.if_statement()?]
            } else {
                self.statement_block()?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
        })
    }

    fn for_statement(&mut self) -> Result<Stmt, RuntimeError> {
        self.expect(&Token::For)?;
        let item = self.ident()?;
        self.expect(&Token::In)?;
        let iterable = self.expression()?;
        let body = self.loop_body()?;
        Ok(Stmt::For {
            item,
            iterable,
            body,
        })
    }

    fn function(&mut self) -> Result<Function, RuntimeError> {
        self.expect(&Token::Fn)?;
        let name = self.ident()?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                params.push(self.ident()?);
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RParen)?;
        // A function body is a new loop context, so break and continue cannot reach a loop around it.
        let outer_loop_depth = std::mem::take(&mut self.loop_depth);
        let body = self.statement_block();
        self.loop_depth = outer_loop_depth;
        Ok(Function {
            name,
            params,
            body: body?,
        })
    }

    fn expression(&mut self) -> Result<Expr, RuntimeError> {
        self.logic_or()
    }

    fn logic_or(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.logic_and()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let right = self.logic_and()?;
            expr = Expr::Logical {
                left: Box::new(expr),
                op: LogicalOp::Or,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn logic_and(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.logic_not()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let right = self.logic_not()?;
            expr = Expr::Logical {
                left: Box::new(expr),
                op: LogicalOp::And,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn logic_not(&mut self) -> Result<Expr, RuntimeError> {
        if matches!(self.peek(), Token::Not) {
            self.advance();
            let expr = self.logic_not()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        self.equality()
    }

    fn equality(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.comparison()?;
        loop {
            let op = match self.peek() {
                Token::EqEq => BinaryOp::Eq,
                Token::BangEq => BinaryOp::NotEq,
                _ => break,
            };
            self.advance();
            let right = self.comparison()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn comparison(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.bit_or()?;
        loop {
            let op = match self.peek() {
                Token::Lt => BinaryOp::Lt,
                Token::LtEq => BinaryOp::LtEq,
                Token::Gt => BinaryOp::Gt,
                Token::GtEq => BinaryOp::GtEq,
                _ => break,
            };
            self.advance();
            let right = self.bit_or()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    // The bitwise levels bind looser than additive arithmetic and tighter than
    // comparisons, as in Python. Their tokens exist only on the CPU hcode
    // surface, so the frozen v0 grammar never reaches them.
    fn bit_or(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.bit_xor()?;
        while matches!(self.peek(), Token::Pipe) {
            self.advance();
            let right = self.bit_xor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitOr,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn bit_xor(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.bit_and()?;
        while matches!(self.peek(), Token::Caret) {
            self.advance();
            let right = self.bit_and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitXor,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn bit_and(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.shift()?;
        while matches!(self.peek(), Token::Amp) {
            self.advance();
            let right = self.shift()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitAnd,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn shift(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.term()?;
        loop {
            let op = match self.peek() {
                Token::Shl => BinaryOp::Shl,
                Token::Shr => BinaryOp::Shr,
                _ => break,
            };
            self.advance();
            let right = self.term()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn term(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.factor()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.factor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn factor(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Rem,
                _ => break,
            };
            self.advance();
            let right = self.unary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr, RuntimeError> {
        let op = match self.peek() {
            Token::Minus => Some(UnaryOp::Neg),
            Token::Plus => Some(UnaryOp::Pos),
            Token::Tilde => Some(UnaryOp::BitNot),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let expr = self.unary()?;
            return Ok(Expr::Unary {
                op,
                expr: Box::new(expr),
            });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.primary()?;
        while matches!(self.peek(), Token::LBracket) {
            self.advance();
            let index = self.expression()?;
            self.expect(&Token::RBracket)?;
            expr = Expr::Index {
                target: Box::new(expr),
                index: Box::new(index),
            };
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, RuntimeError> {
        match self.peek().clone() {
            Token::Int(value) => {
                self.advance();
                Ok(Expr::Value(Value::Int(value)))
            }
            Token::Float(value) => {
                self.advance();
                Ok(Expr::Value(Value::Float(value)))
            }
            Token::String(value) => {
                self.advance();
                Ok(Expr::Value(Value::String(value)))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Value(Value::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Value(Value::Bool(false)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Value(Value::Null))
            }
            Token::If => self.if_expr(),
            Token::LBracket => self.list_expr(),
            Token::LBrace => self.dict_expr(),
            Token::Ident(name) => {
                self.advance();
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        loop {
                            args.push(self.expression()?);
                            if matches!(self.peek(), Token::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Variable(name))
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.expression()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            token => Err(RuntimeError::new(
                "parse_error",
                format!("expected expression, found {token:?}"),
            )
            .at(self.span())),
        }
    }

    fn list_expr(&mut self) -> Result<Expr, RuntimeError> {
        self.expect(&Token::LBracket)?;
        let mut values = Vec::new();
        if !matches!(self.peek(), Token::RBracket) {
            loop {
                values.push(self.expression()?);
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RBracket)?;
        Ok(Expr::List(values))
    }

    fn dict_expr(&mut self) -> Result<Expr, RuntimeError> {
        self.expect(&Token::LBrace)?;
        let mut values = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            loop {
                let key = match self.peek().clone() {
                    Token::String(key) => {
                        self.advance();
                        key
                    }
                    token => {
                        return Err(RuntimeError::new(
                            "parse_error",
                            format!("dict keys must be strings, found {token:?}"),
                        )
                        .at(self.span()));
                    }
                };
                self.expect(&Token::Colon)?;
                values.push((key, self.expression()?));
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Dict(values))
    }

    fn if_expr(&mut self) -> Result<Expr, RuntimeError> {
        self.expect(&Token::If)?;
        let condition = self.expression()?;
        self.expect(&Token::LBrace)?;
        let then_expr = self.expression()?;
        self.expect(&Token::RBrace)?;
        self.expect(&Token::Else)?;
        self.expect(&Token::LBrace)?;
        let else_expr = self.expression()?;
        self.expect(&Token::RBrace)?;
        Ok(Expr::If {
            condition: Box::new(condition),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        })
    }

    fn ident(&mut self) -> Result<String, RuntimeError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            token => Err(RuntimeError::new(
                "parse_error",
                format!("expected identifier, found {token:?}"),
            )
            .at(self.span())),
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<(), RuntimeError> {
        let actual = self.peek().clone();
        if std::mem::discriminant(&actual) == std::mem::discriminant(expected) {
            self.advance();
            Ok(())
        } else {
            Err(RuntimeError::new(
                "parse_error",
                format!("expected {expected:?}, found {actual:?}"),
            )
            .at(self.span()))
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn advance(&mut self) {
        self.pos += 1;
    }
}

/// Builtin names that resolve on the CPU hcode surface. A user function may
/// not reuse one, because builtins resolve before user functions.
const HCODE_BUILTIN_NAMES: &[&str] = &[
    "append",
    "join",
    "keys",
    "len",
    "get",
    "contains",
    "abs",
    "min",
    "max",
    "range",
    "wrapping_add",
    "wrapping_sub",
    "wrapping_mul",
    "sort",
    "split",
    "sqrt",
    "floor",
    "ceil",
    "round",
    "exp",
    "ln",
    "pow",
    "sin",
    "cos",
    "tanh",
    "float",
    "int",
    "tensor",
    "zeros",
    "shape",
    "reshape",
    "transpose",
    "tolist",
    "cast",
    "concat",
    "sum",
    "dot",
    "matmul",
    "relu",
    "gelu",
    "softmax",
    "freivalds_check",
    "load_tensor",
    "load_tensor_slice",
    "tensor_info",
];

struct Evaluator<'a> {
    limits: ExecutionLimits,
    receipt: ExecutionReceipt,
    output: String,
    functions: HashMap<String, Function>,
    scopes: Vec<HashMap<String, Value>>,
    call_depth: usize,
    materialized_value_bytes: u64,
    cancelled: Option<&'a AtomicBool>,
    gpu_backend: Option<&'a mut dyn GpuBackend>,
    artifacts: Option<&'a TensorArtifacts>,
    surface: Surface,
    checked_integer_arithmetic: bool,
}

impl<'a> Evaluator<'a> {
    fn new(limits: ExecutionLimits, surface: Surface) -> Self {
        let checked_integer_arithmetic = limits.checked_integer_arithmetic;
        Self {
            limits,
            receipt: ExecutionReceipt::default(),
            output: String::new(),
            functions: HashMap::new(),
            scopes: vec![HashMap::new()],
            call_depth: 0,
            materialized_value_bytes: 0,
            cancelled: None,
            gpu_backend: None,
            artifacts: None,
            surface,
            checked_integer_arithmetic,
        }
    }

    fn with_gpu(
        limits: ExecutionLimits,
        gpu_backend: &'a mut dyn GpuBackend,
        cancelled: Option<&'a AtomicBool>,
    ) -> Self {
        Self {
            gpu_backend: Some(gpu_backend),
            cancelled,
            ..Self::new(limits, Surface::HcodeGpu)
        }
    }

    fn with_cancellation(
        limits: ExecutionLimits,
        surface: Surface,
        cancelled: &'a AtomicBool,
    ) -> Self {
        Self {
            cancelled: Some(cancelled),
            ..Self::new(limits, surface)
        }
    }

    fn eval_program(mut self, program: &Program) -> Result<ExecutionResult, RuntimeError> {
        for statement in &program.statements {
            if let Stmt::Fn(function) = statement {
                // Builtins resolve before user functions, so a user function
                // with a builtin name would never run. Reject it up front.
                if self.surface.hcode_extensions()
                    && HCODE_BUILTIN_NAMES.contains(&function.name.as_str())
                {
                    return Err(RuntimeError::new(
                        "parse_error",
                        format!(
                            "function name '{}' is reserved for a builtin",
                            function.name
                        ),
                    ));
                }
                self.functions
                    .insert(function.name.clone(), function.clone());
            }
        }

        let evaluated: Result<Value, RuntimeError> = (|| {
            let mut last = Value::Null;
            for statement in &program.statements {
                if matches!(statement, Stmt::Fn(_)) {
                    continue;
                }
                match self.eval_stmt(statement)? {
                    Control::Continue(value) => last = value,
                    Control::Return(value) => {
                        last = value;
                        break;
                    }
                    Control::Break | Control::SkipIteration => {
                        return Err(loop_control_outside_loop());
                    }
                }
            }
            Ok(last)
        })();

        match evaluated {
            Ok(value) => Ok(ExecutionResult {
                status: Status::Completed,
                value,
                output: self.output,
                receipt: self.receipt,
            }),
            Err(error) => Err(error.with_partial_receipt(self.receipt)),
        }
    }

    fn eval_stmt(&mut self, statement: &Stmt) -> Result<Control, RuntimeError> {
        self.charge(1)?;
        match statement {
            Stmt::Let(name, expr) => {
                let value = self.eval_expr(expr)?;
                self.current_scope().insert(name.clone(), value);
                Ok(Control::Continue(Value::Null))
            }
            Stmt::Fn(_) => Ok(Control::Continue(Value::Null)),
            Stmt::Return(expr) => {
                let value = self.eval_expr(expr)?;
                Ok(Control::Return(value))
            }
            Stmt::For {
                item,
                iterable,
                body,
            } => {
                let iterable = self.eval_expr(iterable)?;
                let Value::List(values) = iterable else {
                    return Err(RuntimeError::new("type_error", "for expects a list"));
                };
                let mut last = Value::Null;
                for value in values {
                    self.count_loop_iteration()?;
                    self.current_scope().insert(item.clone(), value);
                    match self.eval_block(body)? {
                        Control::Continue(value) => last = value,
                        Control::SkipIteration => {}
                        Control::Break => break,
                        Control::Return(value) => return Ok(Control::Return(value)),
                    }
                }
                Ok(Control::Continue(last))
            }
            Stmt::While { condition, body } => {
                let mut last = Value::Null;
                loop {
                    let Value::Bool(go_on) = self.eval_expr(condition)? else {
                        return Err(RuntimeError::new(
                            "type_error",
                            "while condition must be bool",
                        ));
                    };
                    if !go_on {
                        break;
                    }
                    self.count_loop_iteration()?;
                    match self.eval_block(body)? {
                        Control::Continue(value) => last = value,
                        Control::SkipIteration => {}
                        Control::Break => break,
                        Control::Return(value) => return Ok(Control::Return(value)),
                    }
                }
                Ok(Control::Continue(last))
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let Value::Bool(take_then) = self.eval_expr(condition)? else {
                    return Err(RuntimeError::new("type_error", "if condition must be bool"));
                };
                self.eval_block(if take_then { then_body } else { else_body })
            }
            Stmt::Break => Ok(Control::Break),
            Stmt::Continue => Ok(Control::SkipIteration),
            Stmt::Print(expr) => {
                self.charge(5)?;
                let value = self.eval_expr(expr)?;
                append_debug_output(&mut self.output, self.limits.max_output_bytes, &value)?;
                self.receipt.output_bytes = self.output.len();
                Ok(Control::Continue(Value::Null))
            }
            Stmt::IndexAssign {
                target,
                index,
                value,
            } => {
                let value = self.eval_expr(value)?;
                let index = self.eval_expr(index)?;
                let Expr::Variable(name) = target else {
                    return Err(RuntimeError::new(
                        "type_error",
                        "index assignment target must be a variable",
                    ));
                };
                let mut current = self.lookup(name)?;
                self.assign_index(&mut current, index, value)?;
                self.assign(name, current)?;
                Ok(Control::Continue(Value::Null))
            }
            Stmt::Expr(expr) => self.eval_expr(expr).map(Control::Continue),
        }
    }

    /// Counts one entry into a `for` or `while` body against the loop limit.
    fn count_loop_iteration(&mut self) -> Result<(), RuntimeError> {
        let next_iterations = self.receipt.loop_iterations.checked_add(1).ok_or_else(|| {
            RuntimeError::new("loop_limit_exceeded", "loop iteration limit exceeded")
        })?;
        if next_iterations > self.limits.max_loop_iterations {
            return Err(RuntimeError::new(
                "loop_limit_exceeded",
                "loop iteration limit exceeded",
            ));
        }
        self.receipt.loop_iterations = next_iterations;
        Ok(())
    }

    /// Runs a statement list in the current scope. The first break, continue, or return ends the list.
    fn eval_block(&mut self, body: &[Stmt]) -> Result<Control, RuntimeError> {
        let mut last = Value::Null;
        for statement in body {
            match self.eval_stmt(statement)? {
                Control::Continue(value) => last = value,
                other => return Ok(other),
            }
        }
        Ok(Control::Continue(last))
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        self.charge(1)?;
        match expr {
            Expr::Value(value) => self.clone_value(value),
            Expr::Variable(name) => self.lookup(name),
            Expr::If {
                condition,
                then_expr,
                else_expr,
            } => {
                let condition = self.eval_expr(condition)?;
                match condition {
                    Value::Bool(true) => self.eval_expr(then_expr),
                    Value::Bool(false) => self.eval_expr(else_expr),
                    _ => Err(RuntimeError::new("type_error", "if condition must be bool")),
                }
            }
            Expr::Call { name, args } => self.call(name, args),
            Expr::List(values) => self.eval_list(values),
            Expr::Dict(values) => self.eval_dict(values),
            Expr::Binary { left, op, right } => {
                let left = self.eval_expr(left)?;
                let right = self.eval_expr(right)?;
                self.eval_binary(left, *op, right)
            }
            Expr::Unary { op, expr } => {
                let value = self.eval_expr(expr)?;
                if let (UnaryOp::Neg, Value::Tensor(_)) = (op, &value) {
                    // Negating a tensor is `0 - tensor`, charged like any
                    // elementwise operation.
                    let units = tensor::elementwise_units(&Value::Int(0), &value);
                    self.charge(units)?;
                    let negated = tensor::binary(
                        BinaryOp::Sub,
                        &Value::Int(0),
                        &value,
                        self.checked_integer_arithmetic,
                    )?;
                    return self.materialize_owned_value(negated);
                }
                eval_unary(*op, &value, self.surface, self.checked_integer_arithmetic)
            }
            Expr::Logical { left, op, right } => {
                let left = self.eval_expr(left)?;
                let Value::Bool(left) = left else {
                    return Err(RuntimeError::new(
                        "type_error",
                        "logical operators expect bool operands",
                    ));
                };
                match op {
                    LogicalOp::And => {
                        if !left {
                            return Ok(Value::Bool(false));
                        }
                    }
                    LogicalOp::Or => {
                        if left {
                            return Ok(Value::Bool(true));
                        }
                    }
                }
                let right = self.eval_expr(right)?;
                match right {
                    Value::Bool(right) => Ok(Value::Bool(right)),
                    _ => Err(RuntimeError::new(
                        "type_error",
                        "logical operators expect bool operands",
                    )),
                }
            }
            Expr::Index { target, index } => {
                let target = self.eval_expr(target)?;
                let index = self.eval_expr(index)?;
                self.eval_index(target, index)
            }
        }
    }

    fn validate_external_value(&self, value: &Value) -> Result<(), RuntimeError> {
        self.validate_value_metrics(value_metrics(value)?)
    }

    fn validate_value_metrics(&self, metrics: ValueMetrics) -> Result<(), RuntimeError> {
        if metrics.canonical_bytes > self.limits.max_value_bytes
            || metrics.depth > self.limits.max_value_depth
            || metrics.max_collection_items > self.limits.max_collection_items
        {
            return Err(value_limit_error());
        }
        Ok(())
    }

    fn clone_value(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let metrics = value_metrics(value)?;
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;
        Ok(value.clone())
    }

    fn materialize_owned_value(&mut self, value: Value) -> Result<Value, RuntimeError> {
        let metrics = value_metrics(&value)?;
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;
        Ok(value)
    }

    fn charge_value_materialization(&mut self, bytes: u64) -> Result<(), RuntimeError> {
        if self.limits.max_value_materialization_bytes == u64::MAX {
            return Ok(());
        }
        let next = self
            .materialized_value_bytes
            .checked_add(bytes)
            .ok_or_else(value_limit_error)?;
        if next > self.limits.max_value_materialization_bytes {
            return Err(value_limit_error());
        }
        self.materialized_value_bytes = next;
        Ok(())
    }

    fn eval_list(&mut self, expressions: &[Expr]) -> Result<Value, RuntimeError> {
        let mut metrics = ValueMetrics {
            canonical_bytes: 2,
            depth: 1,
            max_collection_items: logical_usize(expressions.len())?,
        };
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;

        let mut values = Vec::new();
        for (index, expression) in expressions.iter().enumerate() {
            let value = self.eval_expr(expression)?;
            let value_metrics = value_metrics(&value)?;
            let separator_bytes = u64::from(index > 0);
            let additional_bytes =
                checked_value_add(separator_bytes, value_metrics.canonical_bytes)?;
            metrics = ValueMetrics {
                canonical_bytes: checked_value_add(metrics.canonical_bytes, additional_bytes)?,
                depth: metrics.depth.max(
                    value_metrics
                        .depth
                        .checked_add(1)
                        .ok_or_else(value_limit_error)?,
                ),
                max_collection_items: metrics
                    .max_collection_items
                    .max(value_metrics.max_collection_items),
            };
            self.validate_value_metrics(metrics)?;
            self.charge_value_materialization(additional_bytes)?;
            values.push(value);
        }
        Ok(Value::List(values))
    }

    fn eval_dict(&mut self, expressions: &[(String, Expr)]) -> Result<Value, RuntimeError> {
        let mut metrics = ValueMetrics {
            canonical_bytes: 2,
            depth: 1,
            max_collection_items: logical_usize(expressions.len())?,
        };
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;

        let mut values = BTreeMap::new();
        for (index, (key, expression)) in expressions.iter().enumerate() {
            let key_bytes = json_string_len(key)?;
            let value = self.eval_expr(expression)?;
            let value_metrics = value_metrics(&value)?;
            let separator_bytes = u64::from(index > 0);
            let additional_bytes = checked_value_add(
                checked_value_add(separator_bytes, key_bytes)?,
                checked_value_add(1, value_metrics.canonical_bytes)?,
            )?;
            metrics = ValueMetrics {
                canonical_bytes: checked_value_add(metrics.canonical_bytes, additional_bytes)?,
                depth: metrics.depth.max(
                    value_metrics
                        .depth
                        .checked_add(1)
                        .ok_or_else(value_limit_error)?,
                ),
                max_collection_items: metrics
                    .max_collection_items
                    .max(value_metrics.max_collection_items),
            };
            self.validate_value_metrics(metrics)?;
            self.charge_value_materialization(additional_bytes)?;
            values.insert(key.clone(), value);
        }
        Ok(Value::Dict(values))
    }

    fn eval_binary(
        &mut self,
        left: Value,
        op: BinaryOp,
        right: Value,
    ) -> Result<Value, RuntimeError> {
        if matches!(left, Value::Tensor(_)) || matches!(right, Value::Tensor(_)) {
            return self.eval_tensor_binary(&left, op, &right);
        }
        if !self.surface.floats_enabled() {
            return match (op, left, right) {
                (BinaryOp::Add, Value::String(left), Value::String(right)) => {
                    self.concat_strings(&left, &right)
                }
                (op, left, right) => {
                    eval_binary_integer(left, op, right, self.checked_integer_arithmetic)
                }
            };
        }
        // CPU hcode rejects a zero divisor before dispatch, so int and float
        // division by zero fail the same way.
        if self.surface.hcode_extensions() && matches!(op, BinaryOp::Div) && is_zero_number(&right)
        {
            return Err(RuntimeError::new("runtime_error", "division by zero"));
        }
        // Mixed int/float arithmetic promotes to f64 with the same
        // IEEE-754 semantics on every GPU-v1 host.
        let mixed = matches!(
            (&left, &right),
            (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_))
        );
        let value = if mixed {
            let left_float = numeric_value(&left, "arithmetic")?;
            let right_float = numeric_value(&right, "arithmetic")?;
            eval_binary_float(
                Value::Float(left_float),
                op,
                Value::Float(right_float),
                self.checked_integer_arithmetic,
            )?
        } else {
            match (op, left, right) {
                (BinaryOp::Add, Value::String(left), Value::String(right)) => {
                    return self.concat_strings(&left, &right);
                }
                (op, left, right) => {
                    eval_binary_float(left, op, right, self.checked_integer_arithmetic)?
                }
            }
        };
        finish_hcode_numeric(self.surface, value)
    }

    fn concat_strings(&mut self, left: &str, right: &str) -> Result<Value, RuntimeError> {
        let raw_bytes = left
            .len()
            .checked_add(right.len())
            .ok_or_else(value_limit_error)?;
        let canonical_bytes = checked_value_add(json_string_len(left)?, json_string_len(right)?)?
            .checked_sub(2)
            .ok_or_else(value_limit_error)?;
        let metrics = ValueMetrics {
            canonical_bytes,
            depth: 1,
            max_collection_items: 0,
        };
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;

        let mut value = String::with_capacity(raw_bytes);
        value.push_str(left);
        value.push_str(right);
        Ok(Value::String(value))
    }

    fn eval_index(&mut self, target: Value, index: Value) -> Result<Value, RuntimeError> {
        match (target, index) {
            (Value::List(values), Value::Int(index)) => {
                let index = normalize_index(index, values.len())?;
                self.clone_value(&values[index])
            }
            (Value::Tensor(tensor), Value::Int(index)) => {
                let position = normalize_index(index, tensor.shape()[0])?;
                self.charge(tensor::index_units(&tensor))?;
                self.materialize_owned_value(tensor::index(&tensor, position))
            }
            (Value::String(value), Value::Int(index)) => {
                let index = normalize_index(index, value.chars().count())?;
                let character = value
                    .chars()
                    .nth(index)
                    .expect("a normalized string index must be present");
                self.materialize_owned_value(Value::String(character.to_string()))
            }
            (Value::Dict(values), Value::String(key)) => match values.get(&key) {
                Some(value) => self.clone_value(value),
                None => Err(RuntimeError::new(
                    "key_error",
                    format!("key '{key}' not found"),
                )),
            },
            _ => Err(RuntimeError::new(
                "type_error",
                "indexing expects list[int], string[int], or dict[string]",
            )),
        }
    }

    fn assign_index(
        &mut self,
        target: &mut Value,
        index: Value,
        value: Value,
    ) -> Result<(), RuntimeError> {
        match (target, index) {
            (Value::List(values), Value::Int(index)) => {
                let index = normalize_index(index, values.len())?;
                self.validate_value_metrics(list_metrics_after_assignment(values, index, &value)?)?;
                values[index] = value;
                Ok(())
            }
            (Value::Dict(values), Value::String(key)) => {
                let inserting = !values.contains_key(&key);
                self.validate_value_metrics(dict_metrics_after_assignment(values, &key, &value)?)?;
                if inserting {
                    let entry_bytes = checked_value_add(
                        checked_value_add(json_string_len(&key)?, 1)?,
                        value_metrics(&value)?.canonical_bytes,
                    )?;
                    self.charge_value_materialization(entry_bytes)?;
                }
                values.insert(key, value);
                Ok(())
            }
            _ => Err(RuntimeError::new(
                "type_error",
                "index assignment expects list[int] or dict[string]",
            )),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        self.charge(5)?;
        if let Some(value) = self.builtin_call(name, args)? {
            return Ok(value);
        }
        let function =
            self.functions.get(name).cloned().ok_or_else(|| {
                RuntimeError::new("name_error", format!("unknown function '{name}'"))
            })?;
        if function.params.len() != args.len() {
            return Err(RuntimeError::new(
                "arity_error",
                format!(
                    "function '{}' expected {} arguments, got {}",
                    name,
                    function.params.len(),
                    args.len()
                ),
            ));
        }
        let mut values = Vec::new();
        for arg in args {
            values.push(self.eval_expr(arg)?);
        }
        let next_call_depth = self
            .call_depth
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new("call_depth_exceeded", "call depth exceeded"))?;
        if next_call_depth > self.limits.max_call_depth {
            return Err(RuntimeError::new(
                "call_depth_exceeded",
                "call depth exceeded",
            ));
        }
        self.receipt.function_calls += 1;
        self.call_depth = next_call_depth;
        self.receipt.max_call_depth = self.receipt.max_call_depth.max(self.call_depth);
        let mut scope = HashMap::new();
        for (param, value) in function.params.iter().zip(values) {
            scope.insert(param.clone(), value);
        }
        self.scopes.push(scope);
        let outcome = self.eval_function_body(&function.body);
        self.scopes.pop();
        self.call_depth -= 1;
        outcome
    }

    fn eval_function_body(&mut self, body: &[Stmt]) -> Result<Value, RuntimeError> {
        // Preserve legacy op metering for the common single-`return` body: the
        // expression is evaluated directly, without an extra statement-dispatch
        // charge, so existing budget calibrations remain valid.
        if let [Stmt::Return(expr)] = body {
            return self.eval_expr(expr);
        }
        let mut result = Value::Null;
        for statement in body {
            match self.eval_stmt(statement)? {
                Control::Continue(value) => result = value,
                Control::Return(value) => return Ok(value),
                Control::Break | Control::SkipIteration => return Err(loop_control_outside_loop()),
            }
        }
        Ok(result)
    }

    fn builtin_call(&mut self, name: &str, args: &[Expr]) -> Result<Option<Value>, RuntimeError> {
        match name {
            "gpu_add_f32" if self.gpu_backend.is_some() => {
                self.eval_gpu_builtin(GpuOperation::AddF32, args).map(Some)
            }
            "gpu_scale_f32" if self.gpu_backend.is_some() => self
                .eval_gpu_builtin(GpuOperation::ScaleF32, args)
                .map(Some),
            "gpu_matmul_f32" if self.gpu_backend.is_some() => self
                .eval_gpu_builtin(GpuOperation::MatmulF32, args)
                .map(Some),
            // Keep the names unavailable to the default v0 evaluator.  This is
            // important for the frozen semantics contract.
            "gpu_add_f32" | "gpu_scale_f32" | "gpu_matmul_f32" => Ok(None),
            "len" => {
                let [arg] = args else {
                    return Err(RuntimeError::new("arity_error", "len expects 1 argument"));
                };
                let value = self.eval_expr(arg)?;
                let len = match value {
                    Value::String(value) => value.len(),
                    Value::List(value) => value.len(),
                    Value::Dict(value) => value.len(),
                    Value::Tensor(value) => value.shape()[0],
                    _ => {
                        // The frozen surfaces keep their original message.
                        let message = if self.surface.hcode_extensions() {
                            "len expects string, list, dict, or tensor"
                        } else {
                            "len expects string, list, or dict"
                        };
                        return Err(RuntimeError::new("type_error", message));
                    }
                };
                Ok(Some(Value::Int(i64::try_from(len).map_err(|_| {
                    RuntimeError::new("runtime_error", "length is out of range")
                })?)))
            }
            "get" => {
                let [target, key] = args else {
                    return Err(RuntimeError::new("arity_error", "get expects 2 arguments"));
                };
                let target = self.eval_expr(target)?;
                let key = self.eval_expr(key)?;
                let value = match (target, key) {
                    (Value::Dict(values), Value::String(key)) => match values.get(&key) {
                        Some(value) => self.clone_value(value)?,
                        None => Value::Null,
                    },
                    (Value::List(values), Value::Int(index)) if index >= 0 => {
                        match values.get(usize::try_from(index).map_err(|_| {
                            RuntimeError::new("runtime_error", "list index is out of range")
                        })?) {
                            Some(value) => self.clone_value(value)?,
                            None => Value::Null,
                        }
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "type_error",
                            "get expects dict/string key or list/int index",
                        ));
                    }
                };
                Ok(Some(value))
            }
            "contains" => {
                let [target, key] = args else {
                    return Err(RuntimeError::new(
                        "arity_error",
                        "contains expects 2 arguments",
                    ));
                };
                let target = self.eval_expr(target)?;
                let key = self.eval_expr(key)?;
                let value = match (target, key) {
                    (Value::Dict(values), Value::String(key)) => values.contains_key(&key),
                    (Value::String(value), Value::String(needle)) => value.contains(&needle),
                    (Value::List(values), needle) => values.iter().any(|value| value == &needle),
                    _ => {
                        return Err(RuntimeError::new(
                            "type_error",
                            "contains expects dict, string, or list",
                        ));
                    }
                };
                Ok(Some(Value::Bool(value)))
            }
            "append" if self.surface.hcode_extensions() => {
                let [list, item] = args else {
                    return Err(RuntimeError::new(
                        "arity_error",
                        "append expects 2 arguments",
                    ));
                };
                let Value::List(mut values) = self.eval_expr(list)? else {
                    return Err(RuntimeError::new("type_error", "append expects a list"));
                };
                let item = self.eval_expr(item)?;
                // The result is a new list, and its length is the cost of building it.
                let length = u64::try_from(values.len())
                    .ok()
                    .and_then(|length| length.checked_add(1))
                    .ok_or_else(|| RuntimeError::new("runtime_error", "length is out of range"))?;
                self.charge(length)?;
                values.push(item);
                self.materialize_owned_value(Value::List(values)).map(Some)
            }
            "sort" if self.surface.hcode_extensions() => {
                let [list] = args else {
                    return Err(RuntimeError::new("arity_error", "sort expects 1 argument"));
                };
                let Value::List(mut values) = self.eval_expr(list)? else {
                    return Err(RuntimeError::new("type_error", "sort expects a list"));
                };
                let count = u64::try_from(values.len())
                    .map_err(|_| RuntimeError::new("runtime_error", "length is out of range"))?;
                // Charged before any comparison: the count times the bit length of the count.
                let bits = u64::from(64 - count.leading_zeros());
                let cost = count.checked_mul(bits).ok_or_else(|| {
                    RuntimeError::new("budget_exhausted", "execution budget exhausted")
                })?;
                self.charge(cost)?;
                sort_hcode_values(&mut values)?;
                self.materialize_owned_value(Value::List(values)).map(Some)
            }
            "keys" if self.surface.hcode_extensions() => {
                let [dict] = args else {
                    return Err(RuntimeError::new("arity_error", "keys expects 1 argument"));
                };
                let Value::Dict(entries) = self.eval_expr(dict)? else {
                    return Err(RuntimeError::new("type_error", "keys expects a dict"));
                };
                // One unit per key, charged before the list is built.
                let count = u64::try_from(entries.len())
                    .map_err(|_| RuntimeError::new("runtime_error", "length is out of range"))?;
                self.charge(count)?;
                let keys = entries.into_keys().map(Value::String).collect();
                self.materialize_owned_value(Value::List(keys)).map(Some)
            }
            "split" if self.surface.hcode_extensions() => {
                let [text, separator] = args else {
                    return Err(RuntimeError::new(
                        "arity_error",
                        "split expects 2 arguments",
                    ));
                };
                let Value::String(text) = self.eval_expr(text)? else {
                    return Err(RuntimeError::new("type_error", "split expects a string"));
                };
                let Value::String(separator) = self.eval_expr(separator)? else {
                    return Err(RuntimeError::new(
                        "type_error",
                        "split expects a string separator",
                    ));
                };
                if separator.is_empty() {
                    return Err(RuntimeError::new(
                        "runtime_error",
                        "split separator must not be empty",
                    ));
                }
                // Preflight borrowed pieces: even empty pieces consume list
                // storage, so no result may be allocated before all bounds and
                // both usage charges have succeeded.
                self.charge(logical_usize(text.len())?)?;
                let mut metrics = ValueMetrics {
                    canonical_bytes: 2,
                    depth: 2, // A nonempty separator always produces at least one string.
                    max_collection_items: 0,
                };
                for piece in text.split(separator.as_str()) {
                    let comma = u64::from(metrics.max_collection_items > 0);
                    metrics.canonical_bytes = checked_value_add(
                        metrics.canonical_bytes,
                        checked_value_add(comma, json_string_len(piece)?)?,
                    )?;
                    metrics.max_collection_items =
                        checked_value_add(metrics.max_collection_items, 1)?;
                }
                self.charge(metrics.max_collection_items)?;
                self.validate_value_metrics(metrics)?;
                self.charge_value_materialization(metrics.canonical_bytes)?;
                let capacity = usize::try_from(metrics.max_collection_items)
                    .map_err(|_| value_limit_error())?;
                let mut pieces = Vec::with_capacity(capacity);
                pieces.extend(
                    text.split(separator.as_str())
                        .map(|piece| Value::String(piece.to_string())),
                );
                Ok(Some(Value::List(pieces)))
            }
            "join" if self.surface.hcode_extensions() => {
                let [list, separator] = args else {
                    return Err(RuntimeError::new("arity_error", "join expects 2 arguments"));
                };
                let Value::List(items) = self.eval_expr(list)? else {
                    return Err(RuntimeError::new(
                        "type_error",
                        "join expects a list of strings",
                    ));
                };
                let Value::String(separator) = self.eval_expr(separator)? else {
                    return Err(RuntimeError::new(
                        "type_error",
                        "join expects a string separator",
                    ));
                };
                let mut raw_bytes = 0;
                let mut canonical_bytes = 2;
                for item in &items {
                    let Value::String(text) = item else {
                        return Err(RuntimeError::new(
                            "type_error",
                            "join expects a list of strings",
                        ));
                    };
                    raw_bytes = checked_value_add(raw_bytes, logical_usize(text.len())?)?;
                    canonical_bytes =
                        checked_value_add(canonical_bytes, json_string_len(text)? - 2)?;
                }
                let separators = logical_usize(items.len().saturating_sub(1))?;
                raw_bytes = checked_value_add(
                    raw_bytes,
                    checked_value_mul(logical_usize(separator.len())?, separators)?,
                )?;
                canonical_bytes = checked_value_add(
                    canonical_bytes,
                    checked_value_mul(json_string_len(&separator)? - 2, separators)?,
                )?;
                // One unit per raw UTF-8 byte; safety limits use escaped JSON
                // bytes instead. Check both before allocating the result.
                self.charge(raw_bytes)?;
                self.validate_value_metrics(ValueMetrics {
                    canonical_bytes,
                    depth: 1,
                    max_collection_items: 0,
                })?;
                self.charge_value_materialization(canonical_bytes)?;
                let capacity = usize::try_from(raw_bytes).map_err(|_| value_limit_error())?;
                let mut joined = String::with_capacity(capacity);
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        joined.push_str(&separator);
                    }
                    let Value::String(text) = item else {
                        unreachable!("join preflight validated every item")
                    };
                    joined.push_str(text);
                }
                Ok(Some(Value::String(joined)))
            }
            "abs" if self.surface.numeric_builtins() => {
                let [arg] = args else {
                    return Err(RuntimeError::new("arity_error", "abs expects 1 argument"));
                };
                match self.eval_expr(arg)? {
                    // CPU hcode fails on i64::MIN like every other checked
                    // integer operation; the hcode-gpu surface is unchanged.
                    Value::Int(value) if self.surface.hcode_extensions() => value
                        .checked_abs()
                        .map(Value::Int)
                        .ok_or_else(integer_arithmetic_overflow_error)
                        .map(Some),
                    Value::Int(value) => Ok(Some(Value::Int(value.wrapping_abs()))),
                    Value::Float(value) => Ok(Some(Value::Float(value.abs()))),
                    _ => Err(RuntimeError::new("type_error", "abs expects a number")),
                }
            }
            "min" | "max" if self.surface.numeric_builtins() => {
                let [left_arg, right_arg] = args else {
                    return Err(RuntimeError::new(
                        "arity_error",
                        format!("{name} expects 2 arguments"),
                    ));
                };
                let left = self.eval_expr(left_arg)?;
                let right = self.eval_expr(right_arg)?;
                let pick_min = name == "min";
                // Ints stay ints when both sides are ints; any float promotes
                // the result to float so no precision is silently dropped.
                let result: Value = if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
                    let take_left = if pick_min {
                        left <= right
                    } else {
                        left >= right
                    };
                    if take_left {
                        Value::Int(*left)
                    } else {
                        Value::Int(*right)
                    }
                } else {
                    let left_float = numeric_value(&left, name)?;
                    let right_float = numeric_value(&right, name)?;
                    let take_left = if pick_min {
                        left_float <= right_float
                    } else {
                        left_float >= right_float
                    };
                    if !left_float.is_finite() || !right_float.is_finite() {
                        return Err(RuntimeError::new(
                            "type_error",
                            format!("{name} received a non-finite float"),
                        ));
                    }
                    // A mixed numeric comparison is promoted even when the
                    // selected operand was originally an integer.
                    Value::Float(if take_left { left_float } else { right_float })
                };
                finish_hcode_numeric(self.surface, result).map(Some)
            }
            "exp" | "ln" | "pow" | "sin" | "cos" | "tanh" if self.surface.hcode_extensions() => {
                self.eval_cpu_math(name, args).map(Some)
            }
            "float" | "int" if self.surface.hcode_extensions() => {
                self.eval_numeric_conversion(name, args).map(Some)
            }
            "load_tensor" | "load_tensor_slice" | "tensor_info"
                if self.surface.hcode_extensions() =>
            {
                self.eval_artifact_builtin(name, args).map(Some)
            }
            "tensor" | "zeros" | "shape" | "reshape" | "transpose" | "tolist" | "cast"
            | "concat" | "sum" | "dot" | "matmul" | "relu" | "gelu" | "softmax"
            | "freivalds_check"
                if self.surface.hcode_extensions() =>
            {
                self.eval_tensor_builtin(name, args).map(Some)
            }
            "sqrt" | "floor" | "ceil" | "round" | "exp" | "ln" if self.surface.floats_enabled() => {
                let [arg] = args else {
                    return Err(RuntimeError::new(
                        "arity_error",
                        format!("{name} expects 1 argument"),
                    ));
                };
                let value = numeric_value(&self.eval_expr(arg)?, name)?;
                let result = match name {
                    "sqrt" => {
                        if value < 0.0 {
                            return Err(RuntimeError::new(
                                "runtime_error",
                                "sqrt of a negative number",
                            ));
                        }
                        value.sqrt()
                    }
                    "floor" => value.floor(),
                    "ceil" => value.ceil(),
                    "round" => value.round(),
                    "exp" => value.exp(),
                    "ln" => {
                        if value <= 0.0 {
                            return Err(RuntimeError::new(
                                "runtime_error",
                                "ln requires a positive number",
                            ));
                        }
                        value.ln()
                    }
                    _ => unreachable!("matched above"),
                };
                finish_hcode_numeric(self.surface, Value::Float(result)).map(Some)
            }
            "pow" if self.surface.floats_enabled() => {
                let [base_arg, exponent_arg] = args else {
                    return Err(RuntimeError::new("arity_error", "pow expects 2 arguments"));
                };
                let base = numeric_value(&self.eval_expr(base_arg)?, "pow")?;
                let exponent = numeric_value(&self.eval_expr(exponent_arg)?, "pow")?;
                Ok(Some(Value::Float(base.powf(exponent))))
            }
            "range" if self.surface.hcode_extensions() => self.eval_range(args).map(Some),
            "wrapping_add" | "wrapping_sub" | "wrapping_mul" if self.surface.hcode_extensions() => {
                self.eval_wrapping(name, args).map(Some)
            }
            "abs" | "min" | "max" | "sqrt" | "floor" | "ceil" | "round" | "exp" | "ln" | "pow" => {
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// `range(stop)`, `range(start, stop)`, and `range(start, stop, step)`
    /// build an int list. The element count is charged before the list is
    /// built, so an unaffordable range fails without allocating it.
    fn eval_range(&mut self, args: &[Expr]) -> Result<Value, RuntimeError> {
        let (start, stop, step) = match args {
            [stop] => (0, self.eval_int_arg(stop, "range")?, 1),
            [start, stop] => (
                self.eval_int_arg(start, "range")?,
                self.eval_int_arg(stop, "range")?,
                1,
            ),
            [start, stop, step] => (
                self.eval_int_arg(start, "range")?,
                self.eval_int_arg(stop, "range")?,
                self.eval_int_arg(step, "range")?,
            ),
            _ => {
                return Err(RuntimeError::new(
                    "arity_error",
                    "range expects 1 to 3 arguments",
                ));
            }
        };
        if step == 0 {
            return Err(RuntimeError::new(
                "runtime_error",
                "range step must not be zero",
            ));
        }
        let count = range_len(start, stop, step);
        if count > self.limits.max_collection_items {
            return Err(value_limit_error());
        }
        self.charge(count)?;
        let values = (0..count)
            .map(|index| {
                // Every index is below `count`, so the element lies between
                // `start` and `stop`; the conversion cannot fail in practice.
                let element = i128::from(start) + i128::from(step) * i128::from(index);
                i64::try_from(element)
                    .map(Value::Int)
                    .map_err(|_| integer_arithmetic_overflow_error())
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.materialize_owned_value(Value::List(values))
    }

    /// `wrapping_add`, `wrapping_sub`, and `wrapping_mul` keep the low 64 bits
    /// of the result instead of failing, for hashes and pseudo-random streams.
    fn eval_wrapping(&mut self, name: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        let [left, right] = args else {
            return Err(RuntimeError::new(
                "arity_error",
                format!("{name} expects 2 arguments"),
            ));
        };
        let left = self.eval_int_arg(left, name)?;
        let right = self.eval_int_arg(right, name)?;
        Ok(Value::Int(match name {
            "wrapping_add" => left.wrapping_add(right),
            "wrapping_sub" => left.wrapping_sub(right),
            _ => left.wrapping_mul(right),
        }))
    }

    /// CPU hcode `exp`, `ln`, `pow`, `sin`, `cos`, and `tanh`. They run the
    /// pinned software series in `deterministic_math`, never the host libm, and
    /// each call is charged `TRANSCENDENTAL_UNITS` before it is evaluated.
    fn eval_cpu_math(&mut self, name: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        let arity = if name == "pow" { 2 } else { 1 };
        if args.len() != arity {
            return Err(RuntimeError::new(
                "arity_error",
                format!("{name} expects {arity} argument(s)"),
            ));
        }
        let mut values = Vec::with_capacity(arity);
        for arg in args {
            values.push(numeric_value(&self.eval_expr(arg)?, name)?);
        }
        self.charge(TRANSCENDENTAL_UNITS)?;
        let result = match (name, values.as_slice()) {
            ("exp", [x]) => deterministic_math::exp(*x)
                .ok_or_else(|| RuntimeError::new("runtime_error", "exp overflows"))?,
            ("ln", [x]) => deterministic_math::ln(*x).ok_or_else(|| {
                RuntimeError::new("runtime_error", "ln requires a positive number")
            })?,
            ("sin", [x]) => deterministic_math::sin(*x)
                .ok_or_else(|| RuntimeError::new("runtime_error", "sin requires |x| <= 1000000"))?,
            ("cos", [x]) => deterministic_math::cos(*x)
                .ok_or_else(|| RuntimeError::new("runtime_error", "cos requires |x| <= 1000000"))?,
            ("tanh", [x]) => deterministic_math::tanh(*x),
            ("pow", [base, exponent]) => {
                deterministic_math::pow(*base, *exponent).ok_or_else(|| {
                    RuntimeError::new(
                        "runtime_error",
                        "pow is undefined or overflows for these arguments",
                    )
                })?
            }
            _ => unreachable!("the arity was checked above"),
        };
        finish_hcode_numeric(self.surface, Value::Float(result))
    }

    /// CPU hcode `float(x)` and `int(x)`. `int` truncates toward zero and fails
    /// when the truncated value is outside the i64 range.
    fn eval_numeric_conversion(
        &mut self,
        name: &str,
        args: &[Expr],
    ) -> Result<Value, RuntimeError> {
        let [arg] = args else {
            return Err(RuntimeError::new(
                "arity_error",
                format!("{name} expects 1 argument"),
            ));
        };
        match (name, self.eval_expr(arg)?) {
            ("float", Value::Int(value)) => {
                finish_hcode_numeric(self.surface, Value::Float(value as f64))
            }
            ("float", value @ Value::Float(_)) => Ok(value),
            ("int", Value::Int(value)) => Ok(Value::Int(value)),
            ("int", Value::Float(value)) => {
                // 2^63 is exactly representable, so this range is exact.
                if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) {
                    return Err(RuntimeError::new(
                        "runtime_error",
                        "int argument is outside the int64 range",
                    ));
                }
                Ok(Value::Int(truncate_to_i64(value)))
            }
            _ => Err(RuntimeError::new(
                "type_error",
                format!("{name} expects a number"),
            )),
        }
    }

    /// Elementwise tensor operators. `==` and `!=` compare structurally, and the
    /// arithmetic operators follow the tensor rules. Every other operator is a
    /// type error for tensors.
    fn eval_tensor_binary(
        &mut self,
        left: &Value,
        op: BinaryOp,
        right: &Value,
    ) -> Result<Value, RuntimeError> {
        match op {
            BinaryOp::Eq | BinaryOp::NotEq => {
                self.charge(tensor::elementwise_units(left, right))?;
                let equal = left == right;
                Ok(Value::Bool(equal == matches!(op, BinaryOp::Eq)))
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                self.charge(tensor::elementwise_units(left, right))?;
                let value = tensor::binary(op, left, right, self.checked_integer_arithmetic)?;
                self.materialize_owned_value(value)
            }
            _ => Err(RuntimeError::new(
                "type_error",
                "tensors support + - * / and == != only",
            )),
        }
    }

    /// Charges `per_position` units for each position a tensor operation visits.
    fn charge_positions(&mut self, tensor: &Tensor, per_position: u64) -> Result<(), RuntimeError> {
        let units = tensor::position_units(tensor)
            .checked_mul(per_position)
            .ok_or_else(value_limit_error)?;
        self.charge(units)
    }

    /// Fails before any allocation or charge when a tensor shape would walk more
    /// positions than the element limit allows.
    fn check_element_limit(&self, positions: u64) -> Result<(), RuntimeError> {
        let within = positions <= self.limits.max_collection_items;
        if within {
            Ok(())
        } else {
            Err(value_limit_error())
        }
    }

    /// CPU hcode tensor builtins. Each one checks its size, charges its work,
    /// and only then computes the result.
    fn eval_tensor_builtin(&mut self, name: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        let arity = match name {
            "freivalds_check" => 5,
            "zeros" | "reshape" | "cast" | "dot" | "matmul" => 2,
            _ => 1,
        };
        if args.len() != arity {
            return Err(RuntimeError::new(
                "arity_error",
                format!("{name} expects {arity} argument(s)"),
            ));
        }
        let mut values = Vec::with_capacity(arity);
        for arg in args {
            values.push(self.eval_expr(arg)?);
        }
        let checked = self.checked_integer_arithmetic;
        let result = match (name, values.as_slice()) {
            ("tensor", [nested]) => {
                let shape = tensor::infer_shape(nested)?;
                let positions = tensor::walk_count(&shape)?;
                self.check_element_limit(positions)?;
                self.charge(positions)?;
                Value::Tensor(tensor::from_nested(
                    nested,
                    self.limits.max_collection_items,
                )?)
            }
            ("zeros", [shape, dtype]) => {
                let shape = tensor::shape_from_value(shape)?;
                let dtype = tensor::dtype_from_value(dtype)?;
                let positions = tensor::walk_count(&shape)?;
                self.check_element_limit(positions)?;
                self.charge(positions)?;
                Value::Tensor(tensor::zeros(shape, dtype)?)
            }
            ("shape", [Value::Tensor(tensor)]) => {
                self.charge(u64::try_from(tensor.shape().len()).unwrap_or(u64::MAX))?;
                tensor::shape_value(tensor)
            }
            ("reshape", [Value::Tensor(tensor), shape]) => {
                let shape = tensor::shape_from_value(shape)?;
                // The new shape can walk more positions than the old one when it
                // moves zero dimensions, so it is limited on its own.
                self.check_element_limit(tensor::walk_count(&shape)?)?;
                self.charge(tensor::position_units(tensor))?;
                Value::Tensor(tensor::reshape(tensor, shape)?)
            }
            ("transpose", [Value::Tensor(tensor)]) => {
                self.charge(tensor::position_units(tensor))?;
                Value::Tensor(tensor::transpose(tensor)?)
            }
            ("tolist", [Value::Tensor(tensor)]) => {
                self.charge(tensor::position_units(tensor))?;
                tensor::to_nested(tensor)
            }
            ("cast", [Value::Tensor(tensor), dtype]) => {
                let dtype = tensor::dtype_from_value(dtype)?;
                self.charge(tensor::position_units(tensor))?;
                Value::Tensor(tensor::cast(tensor, dtype)?)
            }
            ("concat", [Value::List(items)]) => {
                let tensors = items
                    .iter()
                    .map(|item| match item {
                        Value::Tensor(tensor) => Ok(tensor),
                        _ => Err(RuntimeError::new(
                            "type_error",
                            "concat expects a list of tensors",
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                // The result is sized and charged before any element is copied.
                let shape = tensor::concat_shape(&tensors)?;
                let positions = tensor::walk_count(&shape)?;
                self.check_element_limit(positions)?;
                self.charge(positions)?;
                Value::Tensor(tensor::concat(&tensors, shape)?)
            }
            ("concat", _) => {
                return Err(RuntimeError::new(
                    "type_error",
                    "concat expects a list of tensors",
                ));
            }
            ("sum", [Value::Tensor(tensor)]) => {
                self.charge(tensor::position_units(tensor))?;
                tensor::sum(tensor, checked)?
            }
            ("dot", [Value::Tensor(left), Value::Tensor(right)]) => {
                let length = tensor::dot_length(left, right)?;
                self.charge(u64::try_from(length).unwrap_or(u64::MAX))?;
                tensor::dot(left, right, checked)?
            }
            ("matmul", [Value::Tensor(left), Value::Tensor(right)]) => {
                let plan = tensor::matmul_plan(left, right)?;
                self.check_element_limit(plan.output_positions()?)?;
                self.charge(plan.units())?;
                Value::Tensor(tensor::matmul(left, right, &plan, checked)?)
            }
            ("relu", [Value::Tensor(tensor)]) => {
                self.charge_positions(tensor, tensor::RELU_UNITS_PER_ELEMENT)?;
                Value::Tensor(tensor::relu(tensor)?)
            }
            ("gelu", [Value::Tensor(tensor)]) => {
                self.charge_positions(tensor, tensor::GELU_UNITS_PER_ELEMENT)?;
                Value::Tensor(tensor::gelu(tensor)?)
            }
            (
                "freivalds_check",
                [
                    Value::Tensor(a),
                    Value::Tensor(b),
                    Value::Tensor(c),
                    Value::Int(seed),
                    Value::Int(rounds),
                ],
            ) => {
                let plan = tensor::freivalds_plan(a, b, c, *rounds)?;
                self.charge(plan.units())?;
                Value::Bool(tensor::freivalds_check(a, b, c, *seed, plan.rounds())?)
            }
            ("softmax", [Value::Tensor(tensor)]) => {
                self.charge_positions(tensor, tensor::SOFTMAX_UNITS_PER_ELEMENT)?;
                Value::Tensor(tensor::softmax(tensor)?)
            }
            _ => {
                return Err(RuntimeError::new(
                    "type_error",
                    format!("{name} expects tensor arguments"),
                ));
            }
        };
        self.materialize_owned_value(result)
    }

    fn eval_artifact_builtin(&mut self, name: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        let arity = if name == "load_tensor_slice" { 4 } else { 2 };
        if args.len() != arity {
            return Err(RuntimeError::new(
                "arity_error",
                format!("{name} expects {arity} arguments"),
            ));
        }
        let values = args
            .iter()
            .map(|arg| self.eval_expr(arg))
            .collect::<Result<Vec<_>, _>>()?;
        let (Value::String(artifact), Value::String(key)) = (&values[0], &values[1]) else {
            return Err(RuntimeError::new(
                "type_error",
                "artifact and tensor names must be strings",
            ));
        };
        let artifacts = self
            .artifacts
            .ok_or_else(|| RuntimeError::new("artifact_error", "no tensor artifacts supplied"))?;
        let input = artifacts.get(artifact, key)?;
        let result = if name == "tensor_info" {
            self.charge(u64::try_from(input.shape().len()).unwrap_or(u64::MAX))?;
            input.info()
        } else {
            let (offset, shape) = if name == "load_tensor_slice" {
                let Value::Int(offset) = values[2] else {
                    return Err(RuntimeError::new(
                        "type_error",
                        "tensor slice offset must be a non-negative integer",
                    ));
                };
                let offset = usize::try_from(offset).map_err(|_| {
                    RuntimeError::new("type_error", "tensor slice offset must be non-negative")
                })?;
                (offset, tensor::shape_from_value(&values[3])?)
            } else {
                (0, input.shape().to_vec())
            };
            tensor::shape_count(&shape)?;
            let positions = tensor::walk_count(&shape)?;
            self.check_element_limit(positions)?;
            self.charge(positions)?;
            Value::Tensor(input.decode(offset, shape)?)
        };
        self.materialize_owned_value(result)
    }

    fn eval_int_arg(&mut self, expr: &Expr, name: &str) -> Result<i64, RuntimeError> {
        match self.eval_expr(expr)? {
            Value::Int(value) => Ok(value),
            _ => Err(RuntimeError::new(
                "type_error",
                format!("{name} expects int arguments"),
            )),
        }
    }

    fn eval_gpu_builtin(
        &mut self,
        operation: GpuOperation,
        args: &[Expr],
    ) -> Result<Value, RuntimeError> {
        if args.len() != operation.arity() {
            return Err(RuntimeError::new(
                "arity_error",
                format!(
                    "{} expects {} arguments, got {}",
                    operation.name(),
                    operation.arity(),
                    args.len()
                ),
            ));
        }
        let values = args
            .iter()
            .map(|arg| self.eval_expr(arg))
            .collect::<Result<Vec<_>, _>>()?;
        let tensors = values
            .iter()
            .map(crate::gpu::tensor_from_value)
            .collect::<Result<Vec<_>, _>>()?;
        let input_bytes = tensors.iter().try_fold(0u64, |total, tensor| {
            total
                .checked_add(u64::try_from(tensor.bytes()).map_err(|_| {
                    RuntimeError::new("gpu_input_error", "GPU tensor size is out of range")
                })?)
                .ok_or_else(|| RuntimeError::new("gpu_input_error", "GPU tensor size overflow"))
        })?;
        if input_bytes > self.limits.max_value_materialization_bytes {
            return Err(RuntimeError::new(
                "value_limit_exceeded",
                "GPU tensor inputs exceed the materialization limit",
            ));
        }
        self.charge(GPU_OPERATION_COST)?;
        self.receipt.gpu_operations =
            self.receipt.gpu_operations.checked_add(1).ok_or_else(|| {
                RuntimeError::new("op_limit_exceeded", "GPU operation count overflow")
            })?;
        let output = {
            let backend = self.gpu_backend.as_deref_mut().ok_or_else(|| {
                RuntimeError::new(
                    "gpu_backend_unavailable",
                    "GPU operations require an enabled GPU execution context",
                )
            })?;
            backend
                .execute(operation, &tensors)
                .map_err(|error| RuntimeError::new(error.code(), error.message()))?
        };
        let output_bytes = u64::try_from(output.bytes()).map_err(|_| {
            RuntimeError::new(
                "gpu_execution_error",
                "GPU tensor output size is out of range",
            )
        })?;
        if output_bytes > self.limits.max_value_materialization_bytes {
            return Err(RuntimeError::new(
                "value_limit_exceeded",
                "GPU tensor output exceeds the materialization limit",
            ));
        }
        crate::gpu::validate_tensor_value_limits(
            &output,
            self.limits.max_value_bytes,
            self.limits.max_collection_items,
            self.limits.max_value_depth,
            self.limits.max_value_materialization_bytes,
            self.materialized_value_bytes,
        )?;
        let value = crate::gpu::value_from_tensor(&output)?;
        self.materialize_owned_value(value)
    }

    fn lookup(&mut self, name: &str) -> Result<Value, RuntimeError> {
        let metrics = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).map(value_metrics))
            .transpose()?
            .ok_or_else(|| RuntimeError::new("name_error", format!("unknown variable '{name}'")))?;
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        unreachable!("a value found while measuring must still be present while cloning")
    }

    fn assign(&mut self, name: &str, value: Value) -> Result<(), RuntimeError> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(name) {
                *slot = value;
                return Ok(());
            }
        }
        Err(RuntimeError::new(
            "name_error",
            format!("unknown variable '{name}'"),
        ))
    }

    fn current_scope(&mut self) -> &mut HashMap<String, Value> {
        self.scopes
            .last_mut()
            .expect("evaluator always has a current scope")
    }

    fn charge(&mut self, cost: u64) -> Result<(), RuntimeError> {
        if self
            .cancelled
            .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
        {
            return Err(RuntimeError::new("cancelled", "task execution stopped"));
        }
        let next = self.receipt.executed_ops.checked_add(cost).ok_or_else(|| {
            RuntimeError::new("op_limit_exceeded", "operation counter overflowed")
        })?;
        if next > self.limits.max_ops {
            return Err(RuntimeError::new(
                "op_limit_exceeded",
                "operation limit exceeded",
            ));
        }
        let next_usage =
            self.receipt.usage_units.checked_add(cost).ok_or_else(|| {
                RuntimeError::new("budget_exhausted", "execution budget exhausted")
            })?;
        if self
            .limits
            .max_usage_units
            .is_some_and(|limit| next_usage > limit)
        {
            return Err(RuntimeError::new(
                "budget_exhausted",
                "execution budget exhausted",
            ));
        }
        self.receipt.executed_ops = next;
        self.receipt.usage_units = next_usage;
        Ok(())
    }
}

enum Control {
    Continue(Value),
    Return(Value),
    /// `break`: ends the innermost loop.
    Break,
    /// `continue`: ends this pass through the innermost loop body.
    SkipIteration,
}

fn eval_binary_integer(
    left: Value,
    op: BinaryOp,
    right: Value,
    checked_integer_arithmetic: bool,
) -> Result<Value, RuntimeError> {
    match op {
        BinaryOp::Add => match (left, right) {
            (Value::Int(left), Value::Int(right)) => {
                apply_integer_binary(left, "+", right, checked_integer_arithmetic)
            }
            _ => Err(RuntimeError::new(
                "type_error",
                "+ expects matching ints or strings",
            )),
        },
        BinaryOp::Sub => int_binary_integer(left, right, "-", checked_integer_arithmetic),
        BinaryOp::Mul => int_binary_integer(left, right, "*", checked_integer_arithmetic),
        BinaryOp::Div => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(RuntimeError::new("runtime_error", "division by zero"))
            }
            (Value::Int(left), Value::Int(right)) => {
                apply_integer_binary(left, "/", right, checked_integer_arithmetic)
            }
            _ => Err(RuntimeError::new("type_error", "/ expects ints")),
        },
        BinaryOp::Rem => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(RuntimeError::new("runtime_error", "division by zero"))
            }
            (Value::Int(left), Value::Int(right)) => {
                apply_integer_rem(left, right, checked_integer_arithmetic)
            }
            _ => Err(RuntimeError::new("type_error", "% expects ints")),
        },
        BinaryOp::BitAnd => int_bitwise(left, right, "&", |left, right| left & right),
        BinaryOp::BitOr => int_bitwise(left, right, "|", |left, right| left | right),
        BinaryOp::BitXor => int_bitwise(left, right, "^", |left, right| left ^ right),
        BinaryOp::Shl => int_shift(left, right, "<<", true),
        BinaryOp::Shr => int_shift(left, right, ">>", false),
        BinaryOp::Eq => Ok(Value::Bool(left == right)),
        BinaryOp::NotEq => Ok(Value::Bool(left != right)),
        BinaryOp::Lt => int_compare_integer(left, right, "<", |left, right| left < right),
        BinaryOp::LtEq => int_compare_integer(left, right, "<=", |left, right| left <= right),
        BinaryOp::Gt => int_compare_integer(left, right, ">", |left, right| left > right),
        BinaryOp::GtEq => int_compare_integer(left, right, ">=", |left, right| left >= right),
    }
}

/// Remainder takes the sign of the dividend, matching the truncating `/`.
/// `i64::MIN % -1` overflows in checked mode, like `i64::MIN / -1`.
fn apply_integer_rem(
    left: i64,
    right: i64,
    checked_integer_arithmetic: bool,
) -> Result<Value, RuntimeError> {
    let result = if checked_integer_arithmetic {
        left.checked_rem(right)
    } else {
        Some(left.wrapping_rem(right))
    };
    result
        .map(Value::Int)
        .ok_or_else(integer_arithmetic_overflow_error)
}

fn int_bitwise(
    left: Value,
    right: Value,
    op: &'static str,
    apply: impl FnOnce(i64, i64) -> i64,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Int(apply(left, right))),
        _ => Err(RuntimeError::new(
            "type_error",
            format!("{op} expects ints"),
        )),
    }
}

/// Shift counts must be in `0..=63`. `<<` discards bits shifted out of the
/// 64-bit word, and `>>` is arithmetic, so the sign is kept.
fn int_shift(
    left: Value,
    right: Value,
    op: &'static str,
    shift_left: bool,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(value), Value::Int(amount)) => {
            let amount = u32::try_from(amount)
                .ok()
                .filter(|amount| *amount < i64::BITS)
                .ok_or_else(|| {
                    RuntimeError::new(
                        "runtime_error",
                        format!("{op} count must be between 0 and 63"),
                    )
                })?;
            Ok(Value::Int(if shift_left {
                value.wrapping_shl(amount)
            } else {
                value >> amount
            }))
        }
        _ => Err(RuntimeError::new(
            "type_error",
            format!("{op} expects ints"),
        )),
    }
}

/// Number of elements `range(start, stop, step)` produces for a non-zero
/// step. The arithmetic is done in i128, so no endpoints can overflow.
fn range_len(start: i64, stop: i64, step: i64) -> u64 {
    let (start, stop, step) = (i128::from(start), i128::from(stop), i128::from(step));
    let span = if step > 0 { stop - start } else { start - stop };
    if span <= 0 {
        return 0;
    }
    let stride = step.abs();
    u64::try_from((span + stride - 1) / stride).unwrap_or(u64::MAX)
}

fn integer_arithmetic_overflow_error() -> RuntimeError {
    RuntimeError::new(
        "integer_arithmetic_overflow",
        "signed i64 arithmetic overflow",
    )
}

fn apply_integer_binary(
    left: i64,
    op: &'static str,
    right: i64,
    checked_integer_arithmetic: bool,
) -> Result<Value, RuntimeError> {
    let result = match (op, checked_integer_arithmetic) {
        ("+", true) => left.checked_add(right),
        ("+", false) => Some(left + right),
        ("-", true) => left.checked_sub(right),
        ("-", false) => Some(left - right),
        ("*", true) => left.checked_mul(right),
        ("*", false) => Some(left * right),
        ("/", true) => left.checked_div(right),
        ("/", false) => Some(left / right),
        _ => {
            return Err(RuntimeError::new(
                "type_error",
                format!("{op} expects ints"),
            ));
        }
    };
    result
        .map(Value::Int)
        .ok_or_else(integer_arithmetic_overflow_error)
}

fn apply_integer_negation(
    value: i64,
    checked_integer_arithmetic: bool,
) -> Result<Value, RuntimeError> {
    let result = if checked_integer_arithmetic {
        value.checked_neg()
    } else {
        Some(-value)
    };
    result
        .map(Value::Int)
        .ok_or_else(integer_arithmetic_overflow_error)
}

fn int_binary_integer(
    left: Value,
    right: Value,
    op: &'static str,
    checked_integer_arithmetic: bool,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => {
            apply_integer_binary(left, op, right, checked_integer_arithmetic)
        }
        _ => Err(RuntimeError::new(
            "type_error",
            format!("{op} expects ints"),
        )),
    }
}

fn int_compare_integer(
    left: Value,
    right: Value,
    op: &'static str,
    apply: impl FnOnce(i64, i64) -> bool,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Bool(apply(left, right))),
        _ => Err(RuntimeError::new(
            "type_error",
            format!("{op} expects ints"),
        )),
    }
}

fn eval_binary_float(
    left: Value,
    op: BinaryOp,
    right: Value,
    checked_integer_arithmetic: bool,
) -> Result<Value, RuntimeError> {
    match op {
        BinaryOp::Add => match (left, right) {
            (Value::Int(left), Value::Int(right)) => {
                apply_integer_binary(left, "+", right, checked_integer_arithmetic)
            }
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + right)),
            _ => Err(RuntimeError::new(
                "type_error",
                "+ expects matching ints, floats, or strings",
            )),
        },
        BinaryOp::Sub => match (left, right) {
            (Value::Int(left), Value::Int(right)) => {
                apply_integer_binary(left, "-", right, checked_integer_arithmetic)
            }
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left - right)),
            (left, right) => int_binary(&left, &right, "-", checked_integer_arithmetic),
        },
        BinaryOp::Mul => match (left, right) {
            (Value::Int(left), Value::Int(right)) => {
                apply_integer_binary(left, "*", right, checked_integer_arithmetic)
            }
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left * right)),
            (left, right) => int_binary(&left, &right, "*", checked_integer_arithmetic),
        },
        BinaryOp::Div => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(RuntimeError::new("runtime_error", "division by zero"))
            }
            (Value::Int(left), Value::Int(right)) => {
                apply_integer_binary(left, "/", right, checked_integer_arithmetic)
            }
            // Float division never traps: IEEE-754 yields inf/NaN, which the
            // canonical renderer rejects at output time.
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left / right)),
            _ => Err(RuntimeError::new("type_error", "/ expects ints or floats")),
        },
        BinaryOp::Rem
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => eval_binary_integer(left, op, right, checked_integer_arithmetic),
        BinaryOp::Eq => Ok(Value::Bool(numeric_or_value_equal(&left, &right))),
        BinaryOp::NotEq => Ok(Value::Bool(!numeric_or_value_equal(&left, &right))),
        BinaryOp::Lt => numeric_compare(
            left,
            right,
            |left, right| left < right,
            |left, right| left < right,
        ),
        BinaryOp::LtEq => numeric_compare(
            left,
            right,
            |left, right| left <= right,
            |left, right| left <= right,
        ),
        BinaryOp::Gt => numeric_compare(
            left,
            right,
            |left, right| left > right,
            |left, right| left > right,
        ),
        BinaryOp::GtEq => numeric_compare(
            left,
            right,
            |left, right| left >= right,
            |left, right| left >= right,
        ),
    }
}

#[expect(clippy::float_cmp)]
fn numeric_or_value_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Int(left), Value::Float(right)) => (*left as f64) == *right,
        (Value::Float(left), Value::Int(right)) => *left == (*right as f64),
        _ => left == right,
    }
}

/// Ordered comparison across the numeric tower. Integer-only comparisons stay
/// in `i64` so enabling floats cannot change the ordering of large integers.
/// Mixed comparisons promote the integer operand to `f64`. NaN compares false
/// against everything (matching IEEE-754 and Rust's `PartialOrd` for `f64`).
fn numeric_compare(
    left: Value,
    right: Value,
    apply_int: impl FnOnce(i64, i64) -> bool,
    apply_float: impl FnOnce(f64, f64) -> bool,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Bool(apply_int(left, right))),
        (Value::Float(left), Value::Float(right)) => {
            if left.is_nan() || right.is_nan() {
                return Ok(Value::Bool(false));
            }
            Ok(Value::Bool(apply_float(left, right)))
        }
        (Value::Int(left), Value::Float(right)) => {
            if right.is_nan() {
                return Ok(Value::Bool(false));
            }
            Ok(Value::Bool(apply_float(left as f64, right)))
        }
        (Value::Float(left), Value::Int(right)) => {
            if left.is_nan() {
                return Ok(Value::Bool(false));
            }
            Ok(Value::Bool(apply_float(left, right as f64)))
        }
        _ => Err(RuntimeError::new(
            "type_error",
            "ordered comparison expects numbers",
        )),
    }
}

fn eval_unary(
    op: UnaryOp,
    value: &Value,
    surface: Surface,
    checked_integer_arithmetic: bool,
) -> Result<Value, RuntimeError> {
    let floats = surface.floats_enabled();
    match (op, value) {
        (UnaryOp::Neg, Value::Int(value)) => {
            apply_integer_negation(*value, checked_integer_arithmetic)
        }
        (UnaryOp::Neg, Value::Float(value)) if floats => {
            finish_hcode_numeric(surface, Value::Float(-value))
        }
        (UnaryOp::Pos, Value::Int(value)) => Ok(Value::Int(*value)),
        (UnaryOp::Pos, Value::Float(value)) if floats => Ok(Value::Float(*value)),
        (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
        (UnaryOp::BitNot, Value::Int(value)) => Ok(Value::Int(!value)),
        (UnaryOp::BitNot, _) => Err(RuntimeError::new("type_error", "~ expects an int")),
        (UnaryOp::Neg, _) => Err(RuntimeError::new(
            "type_error",
            if floats {
                "unary - expects a number"
            } else {
                "unary - expects an int"
            },
        )),
        (UnaryOp::Pos, _) => Err(RuntimeError::new(
            "type_error",
            if floats {
                "unary + expects a number"
            } else {
                "unary + expects an int"
            },
        )),
        (UnaryOp::Not, _) => Err(RuntimeError::new("type_error", "not expects a bool")),
    }
}

/// Coerce a managed value to f64 for the math builtins. Ints promote exactly
/// (i64 → f64 is lossless for |value| < 2^53, and Rust's cast rounds toward
/// zero beyond that — the same behavior on every target, including the guest).
fn numeric_value(value: &Value, function: &str) -> Result<f64, RuntimeError> {
    match value {
        Value::Int(value) => Ok(*value as f64),
        Value::Float(value) => Ok(*value),
        _ => Err(RuntimeError::new(
            "type_error",
            format!("{function} expects a number"),
        )),
    }
}

fn normalize_index(index: i64, len: usize) -> Result<usize, RuntimeError> {
    let len_i64 = i64::try_from(len)
        .map_err(|_| RuntimeError::new("runtime_error", "collection is too large to index"))?;
    let resolved = if index < 0 { index + len_i64 } else { index };
    if resolved < 0 || resolved >= len_i64 {
        return Err(RuntimeError::new("index_error", "index out of range"));
    }
    usize::try_from(resolved)
        .map_err(|_| RuntimeError::new("runtime_error", "index is out of range"))
}

fn int_binary(
    left: &Value,
    right: &Value,
    op: &'static str,
    checked_integer_arithmetic: bool,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => {
            apply_integer_binary(*left, op, *right, checked_integer_arithmetic)
        }
        _ => Err(RuntimeError::new(
            "type_error",
            format!("{op} expects ints"),
        )),
    }
}

fn append_debug_output(
    output: &mut String,
    max_bytes: u64,
    value: &Value,
) -> Result<(), RuntimeError> {
    let rendered_bytes = logical_usize(output.len())?;
    let mut writer = BoundedFmtWriter {
        output,
        rendered_bytes,
        max_bytes,
    };
    match value {
        Value::Int(value) => write!(&mut writer, "{value}"),
        Value::Float(value) => write!(&mut writer, "{}", canonical_float_string(*value)),
        Value::Bool(value) => write!(&mut writer, "{value}"),
        Value::String(value) => writer.write_str(value),
        Value::List(values) => write!(&mut writer, "{values:?}"),
        Value::Dict(values) => write!(&mut writer, "{values:?}"),
        Value::Tensor(tensor) => match tensor::canonical_string(tensor) {
            Ok(text) => writer.write_str(&text),
            Err(_) => Err(std::fmt::Error),
        },
        Value::Null => writer.write_str("null"),
    }
    .map_err(|_| output_limit_error())?;
    writer.write_char('\n').map_err(|_| output_limit_error())
}

struct BoundedFmtWriter<'a> {
    output: &'a mut String,
    rendered_bytes: u64,
    max_bytes: u64,
}

impl Write for BoundedFmtWriter<'_> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let value_bytes = u64::try_from(value.len()).map_err(|_| std::fmt::Error)?;
        let next_len = self
            .rendered_bytes
            .checked_add(value_bytes)
            .ok_or(std::fmt::Error)?;
        if next_len > self.max_bytes {
            return Err(std::fmt::Error);
        }
        self.output.push_str(value);
        self.rendered_bytes = next_len;
        Ok(())
    }

    fn write_char(&mut self, character: char) -> std::fmt::Result {
        let mut encoded = [0; 4];
        self.write_str(character.encode_utf8(&mut encoded))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CpuGpuBackend, ExecutionLimits, ManagedExecutor, ManagedGpuExecutor, Value,
        render_output_bounded,
    };

    #[test]
    fn string_preflight_arithmetic_rejects_overflow_without_allocating() {
        assert_eq!(super::checked_value_add(u64::MAX - 1, 1).unwrap(), u64::MAX);
        assert_eq!(super::checked_value_mul(u64::MAX, 0).unwrap(), 0);
        assert_eq!(super::checked_value_mul(u64::MAX, 1).unwrap(), u64::MAX);
        assert_eq!(
            super::checked_value_add(u64::MAX, 1).unwrap_err().code(),
            "value_limit_exceeded"
        );
        assert_eq!(
            super::checked_value_mul(u64::MAX, 2).unwrap_err().code(),
            "value_limit_exceeded"
        );
        assert_eq!(
            super::checked_value_mul(1 << 32, 1 << 32)
                .unwrap_err()
                .code(),
            "value_limit_exceeded"
        );
    }

    #[test]
    fn bounded_canonical_renderer_matches_serde_json_escaping() {
        for value in [
            "",
            "plain text",
            "quote: \"; slash: \\; controls: \u{0008}\u{000c}\n\r\t\u{0000}\u{001f}",
            "snowman: ☃; line separator: \u{2028}",
        ] {
            let managed = Value::List(vec![Value::String(value.to_string())]);
            let expected =
                serde_json::to_string(&serde_json::Value::Array(vec![serde_json::Value::String(
                    value.to_string(),
                )]))
                .unwrap();
            assert_eq!(render_output_bounded(&managed, u64::MAX).unwrap(), expected);
        }
    }

    fn eval_gpu(source: &str, input: &str) -> Result<String, String> {
        let max_output_bytes = ExecutionLimits::default().max_output_bytes;
        let mut backend = CpuGpuBackend;
        let execution = ManagedGpuExecutor::new(&mut backend)
            .execute_json_input(source, ExecutionLimits::default(), input)
            .map_err(|error| error.to_string())?;
        render_output_bounded(&execution.value, max_output_bytes).map_err(|error| error.to_string())
    }

    fn eval_v0(source: &str, input: &str) -> Result<String, String> {
        let max_output_bytes = ExecutionLimits::default().max_output_bytes;
        let execution = ManagedExecutor
            .execute_json_input(source, ExecutionLimits::default(), input)
            .map_err(|error| error.to_string())?;
        render_output_bounded(&execution.value, max_output_bytes).map_err(|error| error.to_string())
    }

    #[test]
    fn float_literals_and_arithmetic_round_trip() {
        assert_eq!(eval_gpu("return 1.5;", "null").unwrap(), "1.5");
        assert_eq!(eval_gpu("return 1.5 + 2.25;", "null").unwrap(), "3.75");
        // Exponent literals are floats, so integral results keep a .0 suffix.
        assert_eq!(eval_gpu("return 1e3;", "null").unwrap(), "1000.0");
        // Mixed int/float promotes to float; the result stays distinguishable
        // from an int by its .0 suffix.
        assert_eq!(eval_gpu("return 2 * 0.5;", "null").unwrap(), "1.0");
        assert_eq!(eval_gpu("return 10 / 4.0;", "null").unwrap(), "2.5");
        assert_eq!(eval_gpu("return 2.0 + 3.0;", "null").unwrap(), "5.0");
    }

    #[test]
    fn json_float_input_is_accepted() {
        assert_eq!(
            eval_gpu("return get(input, \"x\") * 2.0;", r#"{"x": 1.25}"#).unwrap(),
            "2.5"
        );
        // An integer JSON number promotes through arithmetic with a float.
        assert_eq!(
            eval_gpu("return get(input, \"n\") + 0.0;", r#"{"n": 7}"#).unwrap(),
            "7.0"
        );
    }

    #[test]
    fn float_comparisons_work_across_the_numeric_tower() {
        assert_eq!(eval_gpu("return 0.1 < 1;", "null").unwrap(), "true");
        assert_eq!(eval_gpu("return 2 >= 1.5;", "null").unwrap(), "true");
        assert_eq!(eval_gpu("return 1.5 == 1.5;", "null").unwrap(), "true");
        assert_eq!(eval_gpu("return 1.5 != 1.5;", "null").unwrap(), "false");
        assert_eq!(eval_gpu("return 1 == 1.0;", "null").unwrap(), "true");
        assert_eq!(eval_gpu("return 1.0 == 1;", "null").unwrap(), "true");
        assert_eq!(eval_gpu("return 1 != 1.0;", "null").unwrap(), "false");
    }

    #[test]
    fn float_mode_preserves_exact_large_integer_ordering() {
        assert_eq!(
            eval_gpu("return 9007199254740992 < 9007199254740993;", "null").unwrap(),
            "true"
        );
        assert_eq!(
            eval_gpu("return 9007199254740993 > 9007199254740992;", "null").unwrap(),
            "true"
        );
        assert_eq!(
            eval_gpu("return 9007199254740992 == 9007199254740993;", "null").unwrap(),
            "false"
        );
    }

    #[test]
    fn math_builtins_compute_deterministic_results() {
        // Float-valued builtins keep the .0 suffix on integral results.
        assert_eq!(eval_gpu("return sqrt(16.0);", "null").unwrap(), "4.0");
        assert_eq!(eval_gpu("return floor(2.7);", "null").unwrap(), "2.0");
        assert_eq!(eval_gpu("return ceil(2.1);", "null").unwrap(), "3.0");
        assert_eq!(eval_gpu("return round(2.5);", "null").unwrap(), "3.0");
        assert_eq!(eval_gpu("return abs(-3);", "null").unwrap(), "3");
        assert_eq!(eval_gpu("return abs(-2.5);", "null").unwrap(), "2.5");
        assert_eq!(eval_gpu("return min(1.5, 2);", "null").unwrap(), "1.5");
        assert_eq!(eval_gpu("return min(1, 1.5);", "null").unwrap(), "1.0");
        assert_eq!(eval_gpu("return max(1, 2.5);", "null").unwrap(), "2.5");
        assert_eq!(eval_gpu("return max(2, 1.5);", "null").unwrap(), "2.0");
        // min/max of two ints stay ints.
        assert_eq!(eval_gpu("return min(3, 7);", "null").unwrap(), "3");
        assert_eq!(eval_gpu("return pow(2, 10);", "null").unwrap(), "1024.0");
        assert_eq!(eval_gpu("return exp(0) + ln(1);", "null").unwrap(), "1.0");
    }

    #[test]
    fn non_finite_results_are_rejected_at_output() {
        // Division by zero on floats follows IEEE-754 (inf), and the renderer
        // must reject it rather than emit invalid JSON.
        assert!(eval_gpu("return 1.0 / 0.0;", "null").is_err());
        assert!(eval_gpu("return sqrt(-1.0);", "null").is_err());
        assert!(eval_gpu("return ln(0);", "null").is_err());
    }

    #[test]
    fn frozen_v0_rejects_gpu_float_surface() {
        let error = ManagedExecutor
            .execute("return 1.5;", ExecutionLimits::default())
            .unwrap_err();
        assert_eq!(error.code(), "parse_error");

        let error = ManagedExecutor
            .execute_json_input("return input;", ExecutionLimits::default(), "1.5")
            .unwrap_err();
        assert_eq!(error.code(), "input_error");

        let error = ManagedExecutor
            .execute("return sqrt(4);", ExecutionLimits::default())
            .unwrap_err();
        assert_eq!(error.code(), "name_error");
    }

    #[test]
    fn integer_semantics_are_unchanged() {
        // Integer division still truncates; ints never silently become floats.
        assert_eq!(eval_v0("return 10 / 4;", "null").unwrap(), "2");
        assert_eq!(eval_v0("return 1 + 1;", "null").unwrap(), "2");
        assert_eq!(eval_v0("return -5 / 2;", "null").unwrap(), "-2");
    }

    fn eval_hcode(source: &str) -> Result<String, String> {
        let max_output_bytes = ExecutionLimits::default().max_output_bytes;
        let execution = ManagedExecutor
            .execute_json_input(
                source,
                ExecutionLimits::for_managed_function_budget(u64::MAX),
                "null",
            )
            .map_err(|error| error.to_string())?;
        render_output_bounded(&execution.value, max_output_bytes).map_err(|error| error.to_string())
    }

    fn hcode_error_code(source: &str) -> &'static str {
        ManagedExecutor
            .execute_json_input(
                source,
                ExecutionLimits::for_managed_function_budget(u64::MAX),
                "null",
            )
            .expect_err("the source must fail on the hcode surface")
            .code()
    }

    #[test]
    fn hcode_remainder_truncates_toward_zero() {
        assert_eq!(eval_hcode("return 7 % 3;").unwrap(), "1");
        assert_eq!(eval_hcode("return -7 % 3;").unwrap(), "-1");
        assert_eq!(eval_hcode("return 7 % -3;").unwrap(), "1");
        // The quotient and remainder agree with truncating division.
        assert_eq!(eval_hcode("return -7 / 2 * 2 + -7 % 2;").unwrap(), "-7");
        assert_eq!(hcode_error_code("return 1 % 0;"), "runtime_error");
        assert_eq!(
            hcode_error_code("return (-9223372036854775807 - 1) % -1;"),
            "integer_arithmetic_overflow"
        );
    }

    #[test]
    fn hcode_bitwise_and_shift_operators_follow_python_precedence() {
        assert_eq!(eval_hcode("return (12 & 10) | (1 ^ 3);").unwrap(), "10");
        // Shifts bind looser than + and -, so this is (1 + 2) << 1.
        assert_eq!(eval_hcode("return 1 + 2 << 1;").unwrap(), "6");
        // Bitwise operators bind tighter than ==, so this is (6 & 3) == 2.
        assert_eq!(eval_hcode("return 6 & 3 == 2;").unwrap(), "true");
        assert_eq!(
            eval_hcode("return 1 << 62;").unwrap(),
            "4611686018427387904"
        );
        // >> is arithmetic, so the sign survives.
        assert_eq!(eval_hcode("return -16 >> 2;").unwrap(), "-4");
        assert_eq!(eval_hcode("return ~0;").unwrap(), "-1");
        assert_eq!(hcode_error_code("return 1 << 64;"), "runtime_error");
        assert_eq!(hcode_error_code("return 1 >> -1;"), "runtime_error");
    }

    #[test]
    fn range_builds_bounded_int_lists() {
        assert_eq!(eval_hcode("return range(4);").unwrap(), "[0,1,2,3]");
        assert_eq!(eval_hcode("return range(2, 5);").unwrap(), "[2,3,4]");
        assert_eq!(eval_hcode("return range(5, 0, -2);").unwrap(), "[5,3,1]");
        assert_eq!(eval_hcode("return range(0, 10, 3);").unwrap(), "[0,3,6,9]");
        assert_eq!(eval_hcode("return range(3, 3);").unwrap(), "[]");
        assert_eq!(eval_hcode("return len(range(100000));").unwrap(), "100000");
        assert_eq!(
            eval_hcode("let total = 0; for i in range(5) { let total = total + i; } total;")
                .unwrap(),
            "10"
        );
        assert_eq!(hcode_error_code("return range(1, 2, 0);"), "runtime_error");
        assert_eq!(
            hcode_error_code("return range(100001);"),
            "value_limit_exceeded"
        );
    }

    #[test]
    fn wrapping_builtins_keep_the_low_64_bits() {
        assert_eq!(
            eval_hcode("return wrapping_add(9223372036854775807, 1);").unwrap(),
            "-9223372036854775808"
        );
        assert_eq!(
            eval_hcode("return wrapping_mul(3037000500, 3037000500);").unwrap(),
            "-9223372036709301616"
        );
        assert_eq!(
            eval_hcode("return wrapping_sub(-9223372036854775807, 2);").unwrap(),
            "9223372036854775807"
        );
        // Plain operators still fail on overflow.
        assert_eq!(
            hcode_error_code("return 9223372036854775807 + 1;"),
            "integer_arithmetic_overflow"
        );
    }

    #[test]
    fn abs_min_and_max_work_on_hcode_integers() {
        assert_eq!(eval_hcode("return abs(-5);").unwrap(), "5");
        assert_eq!(eval_hcode("return min(3, 7);").unwrap(), "3");
        assert_eq!(eval_hcode("return max(2, 9);").unwrap(), "9");
        assert_eq!(
            hcode_error_code("return abs(-9223372036854775807 - 1);"),
            "integer_arithmetic_overflow"
        );
    }

    #[test]
    fn frozen_v0_and_hcode_gpu_reject_the_cpu_extensions() {
        for source in [
            "return 7 % 3;",
            "return 1 << 2;",
            "return ~1;",
            "return 6 & 3;",
        ] {
            let error = ManagedExecutor
                .execute(source, ExecutionLimits::default())
                .unwrap_err();
            assert_eq!(error.code(), "parse_error", "{source}");
        }
        for source in ["return range(3);", "return wrapping_add(1, 2);"] {
            let error = ManagedExecutor
                .execute(source, ExecutionLimits::default())
                .unwrap_err();
            assert_eq!(error.code(), "name_error", "{source}");
        }
        // v0 never reserved builtin names, so its programs may define them.
        assert_eq!(
            eval_v0("fn range(n) { return n; } return range(2);", "null").unwrap(),
            "2"
        );
        assert!(
            eval_gpu("return 7 % 3;", "null")
                .unwrap_err()
                .starts_with("parse_error")
        );
        assert!(
            eval_gpu("return range(3);", "null")
                .unwrap_err()
                .starts_with("name_error")
        );
    }

    #[test]
    fn hcode_rejects_user_functions_named_like_builtins() {
        assert_eq!(
            hcode_error_code("fn range(n) { return n; } return range(2);"),
            "parse_error"
        );
        assert_eq!(
            hcode_error_code("fn min(a, b) { return a; } return min(1, 2);"),
            "parse_error"
        );
        // Names that are not builtins on hcode stay available to programs.
        assert_eq!(
            eval_hcode("fn twice(n) { return n * 2; } return twice(4);").unwrap(),
            "8"
        );
    }

    #[test]
    fn hcode_line_comments_run_to_the_end_of_their_line() {
        assert_eq!(
            eval_hcode("// leading note\nreturn 1 + 1; // trailing note").unwrap(),
            "2"
        );
        assert_eq!(
            eval_hcode("let x = 4; // not a parameter\nreturn x * 2;").unwrap(),
            "8"
        );
        assert_eq!(
            eval_hcode("return 9; // no newline at the end").unwrap(),
            "9"
        );
        // Inside a string literal, // is text, and a single slash is still division.
        assert_eq!(eval_hcode(r#"return "a // b";"#).unwrap(), "a // b");
        assert_eq!(eval_hcode("return 8 / 2;").unwrap(), "4");
        // The frozen v0 surface has no comments.
        assert!(eval_v0("return 1; // not a comment", "null").is_err());
    }

    #[test]
    fn the_manifest_lists_exactly_the_builtin_names_the_evaluator_reserves() {
        let manifest: serde_json::Value =
            serde_json::from_str(crate::V1_SEMANTICS_MANIFEST_JSON).unwrap();
        let mut listed: Vec<&str> = manifest["semantics"]["reserved_builtin_names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        listed.sort_unstable();
        let mut reserved = crate::HCODE_BUILTIN_NAMES.to_vec();
        reserved.sort_unstable();
        assert_eq!(listed, reserved);
    }

    #[test]
    fn hcode_while_runs_while_its_condition_holds() {
        assert_eq!(
            eval_hcode(
                "let i = 0; let total = 0; while i < 5 { let total = total + i; let i = i + 1; } return total;"
            )
            .unwrap(),
            "10"
        );
        // A false condition on entry means the body never runs.
        assert_eq!(
            eval_hcode("let n = 7; while n < 0 { let n = 0; } return n;").unwrap(),
            "7"
        );
    }

    #[test]
    fn hcode_break_and_continue_act_on_the_innermost_loop() {
        // break stops at the first multiple of four.
        assert_eq!(
            eval_hcode(
                "let i = 0; while true { let i = i + 1; if i % 4 == 0 { break; } } return i;"
            )
            .unwrap(),
            "4"
        );
        // continue skips the rest of the body, so only the odd numbers are counted.
        assert_eq!(
            eval_hcode(
                "let i = 0; let odd = 0; while i < 6 { let i = i + 1; if i % 2 == 0 { continue; } let odd = odd + 1; } return odd;"
            )
            .unwrap(),
            "3"
        );
        // A break in the inner loop leaves only the inner loop.
        assert_eq!(
            eval_hcode(
                "let i = 0; let total = 0; while i < 3 { let i = i + 1; let j = 0; while true { let j = j + 1; if j == 2 { break; } } let total = total + j; } return total;"
            )
            .unwrap(),
            "6"
        );
        // break and continue work in for loops too.
        assert_eq!(
            eval_hcode(
                "let seen = 0; for x in [1, 2, 3, 4] { if x == 3 { break; } let seen = seen + x; } return seen;"
            )
            .unwrap(),
            "3"
        );
        assert_eq!(
            eval_hcode(
                "let sum = 0; for x in [1, 2, 3] { if x == 2 { continue; } let sum = sum + x; } return sum;"
            )
            .unwrap(),
            "4"
        );
    }

    #[test]
    fn hcode_while_needs_a_bool_condition_and_shares_the_loop_limit() {
        assert_eq!(hcode_error_code("while 1 { }"), "type_error");
        let limited = ExecutionLimits {
            max_loop_iterations: 3,
            ..ExecutionLimits::for_managed_function_budget(u64::MAX)
        };
        let error = ManagedExecutor
            .execute_json_input("let i = 0; while true { let i = i + 1; }", limited, "null")
            .expect_err("the fourth entry into the body must hit the loop limit");
        assert_eq!(error.code(), "loop_limit_exceeded");
    }

    #[test]
    fn hcode_break_and_continue_outside_a_loop_are_parse_errors() {
        assert_eq!(hcode_error_code("break;"), "parse_error");
        assert_eq!(hcode_error_code("continue;"), "parse_error");
        // A function body is a new loop context, so it cannot break out of a loop around its definition.
        assert_eq!(
            hcode_error_code("while true { fn f() { break; } }"),
            "parse_error"
        );
    }

    #[test]
    fn hcode_if_statements_and_if_expressions_both_keep_their_meaning() {
        assert_eq!(
            eval_hcode("let x = 0; if x == 0 { let x = 1; } else { let x = 2; } return x;")
                .unwrap(),
            "1"
        );
        assert_eq!(
            eval_hcode("let x = 5; if x == 0 { let x = 1; } return x;").unwrap(),
            "5"
        );
        assert_eq!(
            eval_hcode(
                "let x = 2; if x == 1 { let y = 10; } else if x == 2 { let y = 20; } else { let y = 30; } return y;"
            )
            .unwrap(),
            "20"
        );
        // An if at the start of an expression statement is still an expression.
        assert_eq!(eval_hcode("if false { 1 } else { 2 };").unwrap(), "2");
        assert_eq!(
            eval_hcode("return if 1 < 2 { \"yes\" } else { \"no\" };").unwrap(),
            "yes"
        );
        assert_eq!(
            eval_hcode("let v = if false { 1 } else { 2 } + 40; return v;").unwrap(),
            "42"
        );
        assert_eq!(hcode_error_code("if 1 { let x = 1; }"), "type_error");
    }

    #[test]
    fn the_frozen_v0_surface_has_no_control_flow_keywords() {
        // On v0, while, break, and continue are ordinary names.
        assert_eq!(
            eval_v0(
                "let while = 1; let break = 2; let continue = 3; return while + break + continue;",
                "null"
            )
            .unwrap(),
            "6"
        );
        assert!(
            eval_v0(
                "let i = 0; while i < 1 { let i = i + 1; } return i;",
                "null"
            )
            .is_err()
        );
        assert!(eval_v0("if true { 1; } else { 2; } return 3;", "null").is_err());
    }

    #[test]
    fn hcode_append_returns_a_new_list_and_leaves_its_argument_alone() {
        assert_eq!(
            eval_hcode("let a = [1]; let b = append(a, 2); return [len(a), len(b), get(b, 1)];")
                .unwrap(),
            "[1,2,2]"
        );
        assert_eq!(eval_hcode("return append([], \"x\");").unwrap(), "[\"x\"]");
        assert_eq!(hcode_error_code("return append(1, 2);"), "type_error");
        assert_eq!(hcode_error_code("return append([1]);"), "arity_error");
    }

    #[test]
    fn hcode_sort_orders_one_item_type_and_rejects_mixed_types() {
        assert_eq!(
            eval_hcode("return sort([3, -1, 2, 2]);").unwrap(),
            "[-1,2,2,3]"
        );
        assert_eq!(
            eval_hcode("return sort([2.5, -1.0, 0.5]);").unwrap(),
            "[-1.0,0.5,2.5]"
        );
        // Strings sort by their UTF-8 bytes, so capitals come first.
        assert_eq!(
            eval_hcode("return sort([\"b\", \"B\", \"a\"]);").unwrap(),
            "[\"B\",\"a\",\"b\"]"
        );
        assert_eq!(eval_hcode("return sort([]);").unwrap(), "[]");
        assert_eq!(hcode_error_code("return sort([1, \"a\"]);"), "type_error");
        assert_eq!(hcode_error_code("return sort(\"abc\");"), "type_error");
    }

    #[test]
    fn hcode_sort_and_append_charge_their_documented_costs() {
        let usage = |source: &str| {
            ManagedExecutor
                .execute_json_input(
                    source,
                    ExecutionLimits::for_managed_function_budget(u64::MAX),
                    "null",
                )
                .expect("the source must succeed")
                .receipt
                .usage_units
        };
        // The call and the list literal appear on both sides of each difference, so they cancel. Subtracting the
        // empty list leaves only the charge that depends on the list length.
        let sort_delta = |items: &str| {
            usage(&format!("return sort({items});")) - usage(&format!("return {items};"))
        };
        assert_eq!(sort_delta("[3, 1, 2]") - sort_delta("[]"), 3 * 2);
        assert_eq!(sort_delta("[4, 3, 2, 1]") - sort_delta("[]"), 4 * 3);
        let append_delta = |items: &str| {
            usage(&format!("return append({items}, 0);")) - usage(&format!("return {items};"))
        };
        assert_eq!(append_delta("[1, 2]") - append_delta("[]"), 2);
    }

    fn hcode_usage(source: &str) -> u64 {
        ManagedExecutor
            .execute_json_input(
                source,
                ExecutionLimits::for_managed_function_budget(u64::MAX),
                "null",
            )
            .expect("the source must succeed")
            .receipt
            .usage_units
    }

    #[test]
    fn hcode_keys_lists_a_dicts_keys_in_sorted_order() {
        assert_eq!(
            eval_hcode("return keys({\"b\": 1, \"a\": 2, \"B\": 3});").unwrap(),
            "[\"B\",\"a\",\"b\"]"
        );
        assert_eq!(eval_hcode("return keys({});").unwrap(), "[]");
        assert_eq!(hcode_error_code("return keys([1]);"), "type_error");
        assert_eq!(hcode_error_code("return keys({}, 1);"), "arity_error");
        // One unit per key. The call and the dict literal cancel out, so the empty dict is the baseline.
        let keys_delta = |entries: &str| {
            hcode_usage(&format!("return keys({entries});"))
                - hcode_usage(&format!("return {entries};"))
        };
        assert_eq!(keys_delta("{\"a\": 1, \"b\": 2}") - keys_delta("{}"), 2);
    }

    #[test]
    fn hcode_split_and_join_follow_str_split_for_a_nonempty_separator() {
        assert_eq!(
            eval_hcode("return split(\"a,b,,c\", \",\");").unwrap(),
            "[\"a\",\"b\",\"\",\"c\"]"
        );
        assert_eq!(eval_hcode("return split(\"\", \",\");").unwrap(), "[\"\"]");
        assert_eq!(
            eval_hcode("return join([\"a\", \"b\"], \", \");").unwrap(),
            "a, b"
        );
        assert_eq!(eval_hcode("return join([], \"-\");").unwrap(), "");
        assert_eq!(
            eval_hcode("return join([\"x\", \"y\"], \"\");").unwrap(),
            "xy"
        );
        assert_eq!(
            eval_hcode("return join(split(\"p-q-r\", \"-\"), \"+\");").unwrap(),
            "p+q+r"
        );
        assert_eq!(
            hcode_error_code("return split(\"a\", \"\");"),
            "runtime_error"
        );
        assert_eq!(hcode_error_code("return split(1, \",\");"), "type_error");
        assert_eq!(hcode_error_code("return join([1], \",\");"), "type_error");
        assert_eq!(
            hcode_error_code("return join(\"ab\", \",\");"),
            "type_error"
        );
    }

    #[test]
    fn hcode_split_and_join_charge_bytes_and_pieces() {
        // split costs one unit per byte of text and one per piece. The call and the arguments cancel out.
        let split_delta = |text: &str| {
            hcode_usage(&format!("return split(\"{text}\", \",\");"))
                - hcode_usage(&format!("return \"{text}\";"))
        };
        // "a,b,c" has 5 bytes and 3 pieces. "" has 0 bytes and 1 piece.
        assert_eq!(split_delta("a,b,c") - split_delta(""), 5 + 3 - 1);
        // join costs one unit per byte of the joined text.
        let join_delta = |items: &str| {
            hcode_usage(&format!("return join({items}, \"-\");"))
                - hcode_usage(&format!("return {items};"))
        };
        // ["ab", "c"] joins to "ab-c", which is 4 bytes. [] joins to "", which is 0.
        assert_eq!(join_delta("[\"ab\", \"c\"]") - join_delta("[]"), 4);
    }

    fn eval_hcode_json(source: &str, input: &str) -> Result<String, String> {
        let max_output_bytes = ExecutionLimits::default().max_output_bytes;
        let execution = ManagedExecutor
            .execute_json_input(
                source,
                ExecutionLimits::for_managed_function_budget(u64::MAX),
                input,
            )
            .map_err(|error| error.to_string())?;
        render_output_bounded(&execution.value, max_output_bytes).map_err(|error| error.to_string())
    }

    fn parse_output(text: &str) -> f64 {
        text.parse::<f64>().expect("a canonical float output")
    }

    #[test]
    fn hcode_float_arithmetic_and_literals_round_trip() {
        assert_eq!(eval_hcode("return 1.5;").unwrap(), "1.5");
        assert_eq!(eval_hcode("return 1.5 + 2.25;").unwrap(), "3.75");
        assert_eq!(eval_hcode("return 1e3;").unwrap(), "1000.0");
        assert_eq!(eval_hcode("return 2 * 0.5;").unwrap(), "1.0");
        assert_eq!(eval_hcode("return 10 / 4.0;").unwrap(), "2.5");
        assert_eq!(eval_hcode("return 1 == 1.0;").unwrap(), "true");
        assert_eq!(eval_hcode("return 0.1 < 1;").unwrap(), "true");
        // Negative zero folds into the one canonical zero.
        assert_eq!(eval_hcode("return 0.0 * -1.0;").unwrap(), "0.0");
        assert_eq!(eval_hcode("return -0.0;").unwrap(), "0.0");
    }

    #[test]
    fn hcode_json_floats_are_accepted_and_canonicalized() {
        assert_eq!(
            eval_hcode_json("return get(input, \"x\") * 2.0;", r#"{"x": 1.25}"#).unwrap(),
            "2.5"
        );
        assert_eq!(
            eval_hcode_json("return get(input, \"z\");", r#"{"z": -0.0}"#).unwrap(),
            "0.0"
        );
    }

    #[test]
    fn hcode_non_finite_floats_fail_at_the_operation_that_makes_them() {
        assert_eq!(hcode_error_code("return 1.0 / 0.0;"), "runtime_error");
        assert_eq!(hcode_error_code("return 0.0 / 0.0;"), "runtime_error");
        assert_eq!(hcode_error_code("return 1e308 * 10.0;"), "runtime_error");
        assert_eq!(hcode_error_code("return sqrt(-1.0);"), "runtime_error");
        assert_eq!(hcode_error_code("return ln(0.0);"), "runtime_error");
        assert_eq!(hcode_error_code("return exp(1000.0);"), "runtime_error");
        assert_eq!(hcode_error_code("return pow(-2.0, 0.5);"), "runtime_error");
        assert_eq!(hcode_error_code("return 1e400;"), "parse_error");
        assert_eq!(hcode_error_code("return int(1e19);"), "runtime_error");
    }

    #[test]
    fn hcode_transcendental_builtins_match_the_host_within_documented_error() {
        for x in [0.5_f64, 1.0, 2.5, -3.25, 10.0, -20.0] {
            let got = parse_output(&eval_hcode(&format!("return exp({x});")).unwrap());
            assert!((got - x.exp()).abs() <= 1e-14 * x.exp(), "exp({x})");
        }
        for x in [0.5_f64, 2.0, 10.0, 0.001, 1.0e6] {
            let got = parse_output(&eval_hcode(&format!("return ln({x});")).unwrap());
            assert!((got - x.ln()).abs() <= 1e-14 * x.ln().abs(), "ln({x})");
        }
        for x in [0.5_f64, 1.0, 2.5, -3.25, 10.0, 1234.5] {
            let sin = parse_output(&eval_hcode(&format!("return sin({x});")).unwrap());
            let cos = parse_output(&eval_hcode(&format!("return cos({x});")).unwrap());
            assert!((sin - x.sin()).abs() <= 1e-15, "sin({x})");
            assert!((cos - x.cos()).abs() <= 1e-15, "cos({x})");
        }
        for x in [0.5_f64, -1.5, 3.0, 25.0] {
            let got = parse_output(&eval_hcode(&format!("return tanh({x});")).unwrap());
            assert!((got - x.tanh()).abs() <= 1e-15, "tanh({x})");
        }
        for (base, exponent) in [(2.0_f64, 0.5), (3.7, 2.3), (10.0, -2.0), (0.5, 7.25)] {
            let got =
                parse_output(&eval_hcode(&format!("return pow({base}, {exponent});")).unwrap());
            assert!(
                (got - base.powf(exponent)).abs() <= 1e-13 * base.powf(exponent),
                "pow({base}, {exponent})"
            );
        }
    }

    #[test]
    fn hcode_int_and_float_conversions() {
        assert_eq!(eval_hcode("return int(2.9);").unwrap(), "2");
        assert_eq!(eval_hcode("return int(-2.9);").unwrap(), "-2");
        assert_eq!(eval_hcode("return int(-0.5);").unwrap(), "0");
        assert_eq!(eval_hcode("return float(3);").unwrap(), "3.0");
        assert_eq!(eval_hcode("return float(7) / 2;").unwrap(), "3.5");
    }

    #[test]
    fn hcode_transcendental_builtins_are_charged_and_arity_checked() {
        // Return statement 1, call node 1, call overhead 5, argument 1, and
        // the transcendental charge 16.
        let execution = ManagedExecutor
            .execute_json_input(
                "return exp(0.0);",
                ExecutionLimits::for_managed_function_budget(1000),
                "null",
            )
            .unwrap();
        assert_eq!(execution.receipt.usage_units, 24);
        assert_eq!(hcode_error_code("return exp(1.0, 2.0);"), "arity_error");
        assert_eq!(hcode_error_code("return pow(2.0);"), "arity_error");
        assert_eq!(
            hcode_error_code("fn sin(x) { return x; } return sin(1.0);"),
            "parse_error"
        );
    }

    fn eval_hcode_with_budget(source: &str, budget: u64) -> Result<String, String> {
        let max_output_bytes = ExecutionLimits::default().max_output_bytes;
        let execution = ManagedExecutor
            .execute_json_input(
                source,
                ExecutionLimits::for_managed_function_budget(budget),
                "null",
            )
            .map_err(|error| error.to_string())?;
        render_output_bounded(&execution.value, max_output_bytes).map_err(|error| error.to_string())
    }

    #[test]
    fn tensors_render_in_canonical_form_and_convert_to_lists() {
        assert_eq!(
            eval_hcode("return tensor([[1, 2], [3, 4]]);").unwrap(),
            r#"{"data":[1,2,3,4],"dtype":"i64","shape":[2,2]}"#
        );
        assert_eq!(
            eval_hcode("return tensor([1, 2.5]);").unwrap(),
            r#"{"data":[1.0,2.5],"dtype":"f64","shape":[2]}"#
        );
        assert_eq!(
            eval_hcode("return zeros([2, 0], \"f64\");").unwrap(),
            r#"{"data":[],"dtype":"f64","shape":[2,0]}"#
        );
        assert_eq!(
            eval_hcode("return shape(zeros([2, 3], \"i64\"));").unwrap(),
            "[2,3]"
        );
        assert_eq!(
            eval_hcode("return tensor([[1, 2], [3, 4]])[1][0];").unwrap(),
            "3"
        );
        assert_eq!(eval_hcode("return len(tensor([5, 6, 7]));").unwrap(), "3");
        assert_eq!(
            eval_hcode("return tolist(reshape(tensor([1, 2, 3, 4]), [2, 2]));").unwrap(),
            "[[1,2],[3,4]]"
        );
        assert_eq!(
            eval_hcode("return tolist(transpose(tensor([[1, 2, 3], [4, 5, 6]])));").unwrap(),
            "[[1,4],[2,5],[3,6]]"
        );
        assert_eq!(
            eval_hcode("return tolist(cast(tensor([1.9, -2.9]), \"i64\"));").unwrap(),
            "[1,-2]"
        );
        assert_eq!(
            eval_hcode("return tolist(cast(tensor([1, 2]), \"f64\"));").unwrap(),
            "[1.0,2.0]"
        );
        assert_eq!(eval_hcode("return sum(tensor([1, 2, 3]));").unwrap(), "6");
        assert_eq!(
            eval_hcode("return sum(tensor([0.5, 0.25]));").unwrap(),
            "0.75"
        );
    }

    #[test]
    fn tensor_arithmetic_is_elementwise_with_scalar_broadcast() {
        assert_eq!(
            eval_hcode("return tolist(tensor([1, 2]) + tensor([10, 20]));").unwrap(),
            "[11,22]"
        );
        assert_eq!(
            eval_hcode("return tolist(tensor([1, 2]) * 3);").unwrap(),
            "[3,6]"
        );
        assert_eq!(
            eval_hcode("return tolist(10 - tensor([1, 2]));").unwrap(),
            "[9,8]"
        );
        assert_eq!(
            eval_hcode("return tolist(tensor([1, 2]) / 2.0);").unwrap(),
            "[0.5,1.0]"
        );
        assert_eq!(
            eval_hcode("return tolist(-tensor([1, -2]));").unwrap(),
            "[-1,2]"
        );
        assert_eq!(
            eval_hcode("return tensor([1, 2]) == tensor([1, 2]);").unwrap(),
            "true"
        );
        assert_eq!(
            eval_hcode("return tensor([1, 2]) != tensor([1, 3]);").unwrap(),
            "true"
        );
        assert_eq!(eval_hcode("return tensor([1, 2]) == 1;").unwrap(), "false");
        assert_eq!(
            hcode_error_code("return tensor([1, 2]) + tensor([1, 2, 3]);"),
            "type_error"
        );
        assert_eq!(
            hcode_error_code("return tensor([9223372036854775807]) + 1;"),
            "integer_arithmetic_overflow"
        );
        assert_eq!(
            hcode_error_code("return tensor([1e308]) * 10.0;"),
            "runtime_error"
        );
        assert_eq!(
            hcode_error_code("return tensor([1, 2]) / 0;"),
            "runtime_error"
        );
        assert_eq!(
            hcode_error_code("return tensor([1]) < tensor([2]);"),
            "type_error"
        );
    }

    #[test]
    fn matmul_and_dot_follow_row_major_order() {
        assert_eq!(
            eval_hcode(
                "return tolist(matmul(tensor([[1, 2], [3, 4]]), tensor([[5, 6], [7, 8]])));"
            )
            .unwrap(),
            "[[19,22],[43,50]]"
        );
        assert_eq!(
            eval_hcode("return tolist(matmul(tensor([[1, 2], [3, 4]]), tensor([1, 1])));").unwrap(),
            "[3,7]"
        );
        assert_eq!(
            eval_hcode("return tolist(matmul(tensor([1, 1]), tensor([[1, 2], [3, 4]])));").unwrap(),
            "[4,6]"
        );
        assert_eq!(
            eval_hcode("return tolist(matmul(tensor([[0.5]]), tensor([[4]])));").unwrap(),
            "[[2.0]]"
        );
        assert_eq!(
            eval_hcode("return dot(tensor([1, 2, 3]), tensor([4, 5, 6]));").unwrap(),
            "32"
        );
        assert_eq!(
            hcode_error_code("return matmul(tensor([[1, 2]]), tensor([[1, 2]]));"),
            "type_error"
        );
        assert_eq!(
            hcode_error_code("return dot(tensor([1, 2]), tensor([1]));"),
            "type_error"
        );
        assert_eq!(
            hcode_error_code("return matmul(zeros([400, 1], \"i64\"), zeros([1, 400], \"i64\"));"),
            "value_limit_exceeded"
        );
    }

    #[test]
    fn concat_joins_tensors_along_the_first_axis() {
        assert_eq!(
            eval_hcode("return tolist(concat([tensor([1, 2]), tensor([3, 4])]));").unwrap(),
            "[1,2,3,4]"
        );
        // A rank-2 join adds the rows of the second tensor below the first.
        assert_eq!(
            eval_hcode(
                "return tolist(concat([tensor([[1, 2, 3], [4, 5, 6]]), tensor([[7, 8, 9]])]));"
            )
            .unwrap(),
            "[[1,2,3],[4,5,6],[7,8,9]]"
        );
        // A single tensor is joined with itself and is unchanged.
        assert_eq!(
            eval_hcode("return tolist(concat([tensor([1, 2, 3])]));").unwrap(),
            "[1,2,3]"
        );
        // Built-ins and literals of the same dtype mix freely.
        assert_eq!(
            eval_hcode("return tolist(concat([tensor([[1, 2, 3]]), zeros([1, 3], \"i64\")]));")
                .unwrap(),
            "[[1,2,3],[0,0,0]]"
        );
        // Concatenation is a value: the list argument keeps its length.
        assert_eq!(
            eval_hcode(
                "let parts = [tensor([1]), tensor([2])]; let joined = concat(parts); [len(parts), tolist(joined)];"
            )
            .unwrap(),
            "[2,[1,2]]"
        );
    }

    #[test]
    fn concat_checks_dtype_rank_and_trailing_dimensions() {
        assert_eq!(hcode_error_code("return concat([]);"), "type_error");
        assert_eq!(hcode_error_code("return concat([1, 2]);"), "type_error");
        assert_eq!(hcode_error_code("return concat(1);"), "type_error");
        assert_eq!(
            hcode_error_code("return concat([tensor([1]), tensor([1.5])]);"),
            "type_error"
        );
        assert_eq!(
            hcode_error_code("return concat([tensor([1, 2]), tensor([[3, 4]])]);"),
            "type_error"
        );
        assert_eq!(
            hcode_error_code("return concat([tensor([[1, 2]]), tensor([[3, 4, 5]])]);"),
            "type_error"
        );
        assert_eq!(hcode_error_code("return concat([1]);"), "type_error");
    }

    #[test]
    fn concat_checks_the_result_against_the_element_limit() {
        // Each operand fits; their join does not, so the result shape is the
        // thing that fails, before any element is copied.
        assert_eq!(
            hcode_error_code("return concat([zeros([100000], \"i64\"), zeros([1], \"i64\")]);"),
            "value_limit_exceeded"
        );
    }

    #[test]
    fn concat_charges_one_unit_per_result_position() {
        // Growing the second operand by four positions grows the result by four
        // as well. Subtracting the plain list return leaves exactly those four
        // result positions, so the marginal charge is one per position. The
        // call, return, list, and operand costs cancel between the two.
        let concat_grow =
            hcode_usage("return concat([zeros([4], \"i64\"), zeros([12], \"i64\")]);")
                - hcode_usage("return concat([zeros([4], \"i64\"), zeros([8], \"i64\")]);");
        let list_grow = hcode_usage("return [zeros([4], \"i64\"), zeros([12], \"i64\")];")
            - hcode_usage("return [zeros([4], \"i64\"), zeros([8], \"i64\")];");
        assert_eq!(concat_grow - list_grow, 4);
    }

    #[test]
    fn activations_and_softmax_are_deterministic_and_normalized() {
        assert_eq!(
            eval_hcode("return tolist(relu(tensor([-1, 2])));").unwrap(),
            "[0,2]"
        );
        assert_eq!(
            eval_hcode("return tolist(relu(tensor([-0.5, 0.5])));").unwrap(),
            "[0.0,0.5]"
        );
        assert_eq!(
            eval_hcode("return tolist(gelu(tensor([0.0])));").unwrap(),
            "[0.0]"
        );
        let gelu_one = parse_output(&eval_hcode("return tolist(gelu(tensor([1.0])))[0];").unwrap());
        let x = 1.0_f64;
        let inner = 0.797_884_560_802_865_4_f64 * (x + 0.044_715 * x * x * x);
        let reference = 0.5 * x * (1.0 + inner.tanh());
        assert!((gelu_one - reference).abs() < 1e-14, "gelu(1.0)");
        assert_eq!(
            eval_hcode("return tolist(softmax(tensor([0.0, 0.0])));").unwrap(),
            "[0.5,0.5]"
        );
        let probabilities =
            eval_hcode("return tolist(softmax(tensor([[1.0, 2.0, 3.0], [0.0, 0.0, 0.0]])));")
                .unwrap();
        assert!(probabilities.starts_with("[["), "{probabilities}");
        let total = eval_hcode("return sum(softmax(tensor([1.0, 2.0, 3.0])));").unwrap();
        assert!(
            (parse_output(&total) - 1.0).abs() < 1e-15,
            "softmax total {total}"
        );
    }

    #[test]
    fn tensor_builtins_check_size_rank_dtype_and_budget() {
        assert_eq!(
            hcode_error_code("return zeros([400, 400], \"i64\");"),
            "value_limit_exceeded"
        );
        assert_eq!(hcode_error_code("return zeros([], \"i64\");"), "type_error");
        assert_eq!(
            hcode_error_code("return zeros([1, 1, 1, 1, 1, 1, 1, 1, 1], \"i64\");"),
            "type_error"
        );
        assert_eq!(hcode_error_code("return zeros([2], \"u8\");"), "type_error");
        assert_eq!(
            hcode_error_code("return reshape(tensor([1, 2, 3]), [2, 2]);"),
            "type_error"
        );
        assert_eq!(
            hcode_error_code("return transpose(tensor([1, 2]));"),
            "type_error"
        );
        assert_eq!(
            hcode_error_code("for x in tensor([1]) { return x; }"),
            "type_error"
        );
        // Ten elements cost ten units, so a budget of five stops before the work.
        assert_eq!(
            eval_hcode_with_budget("return sum(zeros([10], \"i64\"));", 5)
                .unwrap_err()
                .split(':')
                .next(),
            Some("budget_exhausted")
        );
        // The tensor surface is closed to v0 and to hcode-gpu.
        let error = ManagedExecutor
            .execute("return tensor([1]);", ExecutionLimits::default())
            .unwrap_err();
        assert_eq!(error.code(), "name_error");
        assert!(
            eval_gpu("return tensor([1]);", "null")
                .unwrap_err()
                .starts_with("name_error")
        );
    }

    #[test]
    fn tensor_construction_charges_before_walking_the_leaves() {
        // Eleven units evaluate the call and its arguments. The two-position
        // construction charge must fail before detecting the ragged second row.
        let error = eval_hcode_with_budget("return tensor([[1], 2]);", 11).unwrap_err();
        assert!(error.starts_with("budget_exhausted"), "{error}");
    }

    #[test]
    fn freivalds_accepts_the_full_signed_i32_range() {
        assert_eq!(eval_hcode("return freivalds_check(tensor([[-2147483648]]), tensor([[0]]), tensor([[0]]), 1, 4);").unwrap(), "true");
    }

    #[test]
    fn print_writes_the_canonical_tensor_form() {
        let execution = ManagedExecutor
            .execute_json_input(
                "print(tensor([1, 2])); return 0;",
                ExecutionLimits::for_managed_function_budget(1000),
                "null",
            )
            .unwrap();
        assert_eq!(
            execution.output,
            "{\"data\":[1,2],\"dtype\":\"i64\",\"shape\":[2]}\n"
        );
    }

    #[test]
    fn freivalds_check_accepts_the_product_and_rejects_a_wrong_claim() {
        let product = "tensor([[1, 2], [3, 4]]), tensor([[5, 6], [7, 8]])";
        for seed in [1, 7, -3, 1_000_003] {
            assert_eq!(
                eval_hcode(&format!(
                    "return freivalds_check({product}, tensor([[19, 22], [43, 50]]), {seed}, 20);"
                ))
                .unwrap(),
                "true",
                "seed {seed}"
            );
            assert_eq!(
                eval_hcode(&format!(
                    "return freivalds_check({product}, tensor([[19, 22], [43, 51]]), {seed}, 20);"
                ))
                .unwrap(),
                "false",
                "seed {seed}"
            );
        }
        // Rectangular shapes: a is 2 by 3, b is 3 by 1, and c is 2 by 1.
        assert_eq!(
            eval_hcode(
                "return freivalds_check(tensor([[1, 2, 3], [4, 5, 6]]), tensor([[1], [0], [2]]), tensor([[7], [16]]), 5, 8);"
            )
            .unwrap(),
            "true"
        );
    }

    #[test]
    fn freivalds_check_rejects_unsupported_inputs() {
        let operands =
            "tensor([[1, 2], [3, 4]]), tensor([[5, 6], [7, 8]]), tensor([[19, 22], [43, 50]])";
        assert_eq!(
            hcode_error_code(&format!("return freivalds_check({operands}, 1);")),
            "arity_error"
        );
        assert_eq!(
            hcode_error_code(&format!("return freivalds_check({operands}, 1, 0);")),
            "runtime_error"
        );
        assert_eq!(
            hcode_error_code(&format!("return freivalds_check({operands}, 1, 65);")),
            "runtime_error"
        );
        assert_eq!(
            hcode_error_code(
                "return freivalds_check(tensor([[1.0, 2.0], [3.0, 4.0]]), tensor([[5, 6], [7, 8]]), tensor([[19, 22], [43, 50]]), 1, 4);"
            ),
            "type_error"
        );
        assert_eq!(
            hcode_error_code(
                "return freivalds_check(tensor([[1, 2], [3, 4]]), tensor([[5, 6], [7, 8]]), tensor([[19, 22]]), 1, 4);"
            ),
            "type_error"
        );
        assert_eq!(
            hcode_error_code(
                "return freivalds_check(tensor([[3000000000, 2], [3, 4]]), tensor([[5, 6], [7, 8]]), tensor([[19, 22], [43, 50]]), 1, 4);"
            ),
            "type_error"
        );
        // The three literals and two integer arguments cost 60 units before the
        // check. Each round visits cols + inner * (cols + 1) + rows * (1 + inner +
        // cols) = 2 + 6 + 10 = 18 positions, so four rounds cost 72 more. A budget
        // of 131 stops inside the check and 132 completes it.
        let source = format!("return freivalds_check({operands}, 1, 4);");
        assert_eq!(
            eval_hcode_with_budget(&source, 131)
                .unwrap_err()
                .split(':')
                .next(),
            Some("budget_exhausted")
        );
        assert_eq!(eval_hcode_with_budget(&source, 132).unwrap(), "true");
    }

    #[test]
    fn tensor_literal_with_a_ragged_first_element_chain_fails_before_allocation() {
        // Every level's first element is the next level, so the inferred shape is
        // 40 by 40 by ... (eight levels, about 6.5e12 elements) while the literal
        // has only a few hundred elements. The element limit must reject it before
        // any storage is reserved.
        let mut nested = format!("[{}]", vec!["1"; 40].join(","));
        for _ in 0..7 {
            nested = format!("[{nested},{}]", vec!["[]"; 39].join(","));
        }
        assert_eq!(
            hcode_error_code(&format!("return tensor({nested});")),
            "value_limit_exceeded"
        );
    }

    #[test]
    fn zero_size_tensors_cannot_hide_large_dimensions() {
        // A zero-size tensor costs nothing, so a large dimension next to a zero
        // would let tolist, transpose, matmul, or freivalds_check walk billions
        // of empty positions for no charge. The shape check must refuse it.
        assert_eq!(
            hcode_error_code("return tolist(zeros([1099511627776, 0], \"i64\"));"),
            "value_limit_exceeded"
        );
        assert_eq!(
            hcode_error_code("return transpose(zeros([0, 1099511627776], \"i64\"));"),
            "value_limit_exceeded"
        );
        assert_eq!(
            hcode_error_code(
                "return matmul(zeros([2, 0], \"i64\"), zeros([0, 1099511627776], \"i64\"));"
            ),
            "value_limit_exceeded"
        );
        assert_eq!(
            hcode_error_code(
                "return freivalds_check(zeros([0, 0], \"i64\"), zeros([0, 1099511627776], \"i64\"), zeros([0, 1099511627776], \"i64\"), 1, 4);"
            ),
            "value_limit_exceeded"
        );
    }

    #[test]
    fn hcode_gpu_keeps_its_libm_math_and_lacks_the_cpu_only_builtins() {
        assert_eq!(eval_gpu("return exp(0) + ln(1);", "null").unwrap(), "1.0");
        assert!(
            eval_gpu("return sin(1.0);", "null")
                .unwrap_err()
                .starts_with("name_error")
        );
        assert!(
            eval_gpu("return float(1);", "null")
                .unwrap_err()
                .starts_with("name_error")
        );
    }
}

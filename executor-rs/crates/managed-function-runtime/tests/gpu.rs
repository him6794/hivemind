use std::sync::atomic::AtomicBool;

use managed_function_runtime::{
    CpuGpuBackend, ExecutionLimits, GPU_BILLING_VERSION, GPU_COST_MODEL_VERSION,
    GPU_OPERATION_REGISTRY_VERSION, GPU_RUNTIME_VERSION, GPU_SEMANTICS_MANIFEST_JSON,
    GPU_SEMANTICS_MANIFEST_SHA256, GpuBackend, GpuOperation, GpuTensor, ManagedExecutor,
    ManagedGpuExecutor, Value,
};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const EXPECTED_GPU_MANIFEST_SHA256: &str =
    "4b5230145a43f05df6e8e09a4fa682e3babcfe43aa980883f72dd95d74d8cb13";

#[test]
fn gpu_manifest_and_runtime_identifiers_are_pinned() {
    assert_eq!(GPU_RUNTIME_VERSION, "managed-function-gpu-v1");
    assert_eq!(
        GPU_OPERATION_REGISTRY_VERSION,
        "managed-function-gpu-ops-v1"
    );
    assert_eq!(GPU_BILLING_VERSION, "managed-function-gpu-billing-v1");
    assert_eq!(GPU_COST_MODEL_VERSION, "managed-function-gpu-metering-v1");

    let manifest: JsonValue =
        serde_json::from_str(GPU_SEMANTICS_MANIFEST_JSON).expect("GPU manifest must be valid JSON");
    let canonical = serde_json::to_string(&manifest).expect("GPU manifest must serialize");
    let digest = format!("{:x}", Sha256::digest(canonical.as_bytes()));

    assert_eq!(GPU_SEMANTICS_MANIFEST_JSON.trim_end(), canonical);
    assert_eq!(GPU_SEMANTICS_MANIFEST_SHA256, EXPECTED_GPU_MANIFEST_SHA256);
    assert_eq!(digest, EXPECTED_GPU_MANIFEST_SHA256);
    assert_eq!(manifest["runtime"], GPU_RUNTIME_VERSION);
    assert_eq!(manifest["interpreter"], "managed-function-runtime-gpu-v1");
    assert_eq!(
        manifest["operation_registry"],
        GPU_OPERATION_REGISTRY_VERSION
    );
    assert_eq!(manifest["billing_version"], GPU_BILLING_VERSION);
    assert_eq!(manifest["cost_model_version"], GPU_COST_MODEL_VERSION);
    assert_eq!(manifest["tensor_abi"], "managed-function-gpu-tensor-v1");
    assert_eq!(manifest["supported_dtype"], "float32");
    assert_eq!(manifest["max_rank"], 8);
    assert_eq!(manifest["max_tensor_bytes"], 16_777_216);
    assert_eq!(manifest["cpu_fallback"], false);
    assert_eq!(manifest["determinism"], "fixed-f32-operation-order");
    assert_eq!(manifest["proof"], "none");
    assert_eq!(
        manifest["numeric_builtins"],
        serde_json::json!([
            {"arity": 1, "name": "abs", "result": "numeric"},
            {"arity": 2, "name": "min", "result": "numeric"},
            {"arity": 2, "name": "max", "result": "numeric"},
            {"arity": 1, "name": "sqrt", "result": "float64"},
            {"arity": 1, "name": "floor", "result": "float64"},
            {"arity": 1, "name": "ceil", "result": "float64"},
            {"arity": 1, "name": "round", "result": "float64"},
            {"arity": 1, "name": "exp", "result": "float64"},
            {"arity": 1, "name": "ln", "result": "float64"},
            {"arity": 2, "name": "pow", "result": "float64"}
        ])
    );
    assert_eq!(
        manifest["operations"],
        serde_json::json!([
            {"arity": 2, "cost_units": 10, "dtype": "float32", "name": "gpu_add_f32", "shape": "equal"},
            {"arity": 2, "cost_units": 10, "dtype": "float32", "name": "gpu_scale_f32", "shape": "tensor_and_scalar"},
            {"arity": 2, "cost_units": 10, "dtype": "float32", "name": "gpu_matmul_f32", "shape": "rank2_inner_match"}
        ])
    );
}

#[test]
fn gpu_builtins_are_unavailable_to_the_frozen_v0_executor() {
    let error = ManagedExecutor
        .execute("return gpu_add_f32([1], [2]);", ExecutionLimits::default())
        .unwrap_err();

    assert_eq!(error.code(), "name_error");
}

#[test]
fn gpu_matmul_uses_the_same_language_and_rust_reference_backend() {
    let mut backend = CpuGpuBackend;
    let mut executor = ManagedGpuExecutor::new(&mut backend);
    let result = executor
        .execute(
            "return gpu_matmul_f32([[1.0, 2.0], [3.0, 4.0]], [[5.0, 6.0], [7.0, 8.0]]);",
            ExecutionLimits::default(),
        )
        .unwrap();

    assert_eq!(
        result.value,
        Value::List(vec![
            Value::List(vec![Value::Float(19.0), Value::Float(22.0)]),
            Value::List(vec![Value::Float(43.0), Value::Float(50.0)]),
        ])
    );
    assert_eq!(result.receipt.executed_ops, 31);
}

#[test]
fn gpu_operations_can_be_composed_without_exposing_device_handles() {
    let mut backend = CpuGpuBackend;
    let mut executor = ManagedExecutor.with_gpu(&mut backend);
    let result = executor
        .execute(
            "return gpu_scale_f32(gpu_add_f32([1.0, 2.0], [3.0, 4.0]), 2.0);",
            ExecutionLimits::default(),
        )
        .unwrap();

    assert_eq!(
        result.value,
        Value::List(vec![Value::Float(8.0), Value::Float(12.0)])
    );
}

#[test]
fn gpu_tensor_inputs_must_be_rectangular_and_f32_compatible() {
    let mut backend = CpuGpuBackend;
    let mut executor = ManagedGpuExecutor::new(&mut backend);

    let error = executor
        .execute(
            "return gpu_add_f32([[1.0], [2.0, 3.0]], [[1.0], [2.0, 3.0]]);",
            ExecutionLimits::default(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "gpu_input_error");

    let error = executor
        .execute(
            "return gpu_scale_f32([16777217], 1.0);",
            ExecutionLimits::default(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "gpu_input_error");
}

#[test]
fn gpu_tensor_rejects_integer_values_that_round_at_i64_boundaries() {
    let mut backend = CpuGpuBackend;
    let mut executor = ManagedGpuExecutor::new(&mut backend);
    let error = executor
        .execute(
            "return gpu_scale_f32([9223372036854775807], 1.0);",
            ExecutionLimits::default(),
        )
        .unwrap_err();

    assert_eq!(error.code(), "gpu_input_error");
}

#[test]
fn gpu_backend_rejects_oversized_matmul_before_output_allocation() {
    let left = GpuTensor::new(vec![100_000, 1], vec![1.0; 100_000]).unwrap();
    let right = GpuTensor::new(vec![1, 100_000], vec![1.0; 100_000]).unwrap();
    let mut backend = CpuGpuBackend;

    let error = backend
        .execute(GpuOperation::MatmulF32, &[left, right])
        .unwrap_err();

    assert_eq!(error.code(), "gpu_input_error");
    assert!(error.message().contains("output exceeds"));
}

#[test]
fn gpu_operations_honor_cancellation_and_tensor_validation() {
    let cancelled = AtomicBool::new(true);
    let mut backend = CpuGpuBackend;
    let mut executor = ManagedGpuExecutor::new(&mut backend);
    let error = executor
        .execute_json_input_with_cancel(
            "return gpu_add_f32([1.0], [2.0]);",
            ExecutionLimits::default(),
            "null",
            Some(&cancelled),
        )
        .unwrap_err();
    assert_eq!(error.code(), "cancelled");

    let error = GpuTensor::new(vec![2, 2], vec![1.0, 2.0]).unwrap_err();
    assert_eq!(error.code(), "gpu_input_error");
}

#[test]
fn gpu_output_is_rendered_as_bounded_managed_values() {
    let mut backend = CpuGpuBackend;
    let mut executor = ManagedGpuExecutor::new(&mut backend);
    let result = executor
        .execute(
            "return gpu_add_f32([1.0, 2.0], [3.0, 4.0]);",
            ExecutionLimits {
                max_value_materialization_bytes: 8,
                ..ExecutionLimits::default()
            },
        )
        .unwrap_err();

    assert_eq!(result.code(), "value_limit_exceeded");
}

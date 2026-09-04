#![cfg(all(feature = "cuda", target_os = "linux"))]

use managed_function_runtime::{
    CudaGpuBackend, ExecutionLimits, GpuBackend, GpuOperation, GpuTensor, ManagedGpuExecutor, Value,
};

fn cuda_test_backend() -> (CudaGpuBackend, String) {
    let ordinal = std::env::var("HIVEMIND_CUDA_TEST_DEVICE_ORDINAL")
        .unwrap_or_else(|_| "0".to_string())
        .parse::<i32>()
        .expect("HIVEMIND_CUDA_TEST_DEVICE_ORDINAL must be a non-negative integer");
    let device_id = std::env::var("HIVEMIND_CUDA_TEST_DEVICE_ID")
        .unwrap_or_else(|_| format!("gpu-test-device-{ordinal}"));
    let cuda_uuid = std::env::var("HIVEMIND_CUDA_TEST_UUID")
        .expect("HIVEMIND_CUDA_TEST_UUID must contain the selected GPU UUID");
    let backend = CudaGpuBackend::new(device_id.clone(), ordinal, cuda_uuid)
        .expect("configured CUDA device and UUID must initialize");
    (backend, device_id)
}

#[test]
fn cuda_backend_executes_fixed_operations_on_the_selected_device() {
    if std::env::var_os("HIVEMIND_RUN_CUDA_TESTS").is_none() {
        eprintln!("skipping CUDA hardware test; set HIVEMIND_RUN_CUDA_TESTS=1 to enable");
        return;
    }

    let (mut backend, expected_device_id) = cuda_test_backend();
    assert_eq!(backend.device_id(), expected_device_id);

    let add = backend
        .execute(
            GpuOperation::AddF32,
            &[
                GpuTensor::new(vec![2], vec![1.0, 2.0]).unwrap(),
                GpuTensor::new(vec![2], vec![3.0, 4.0]).unwrap(),
            ],
        )
        .expect("cuBLAS add must execute");
    assert_eq!(add.shape, vec![2]);
    assert_eq!(add.data, vec![4.0, 6.0]);

    let scale = backend
        .execute(
            GpuOperation::ScaleF32,
            &[
                GpuTensor::new(vec![2], vec![4.0, 6.0]).unwrap(),
                GpuTensor::new(Vec::new(), vec![0.5]).unwrap(),
            ],
        )
        .expect("cuBLAS scale must execute");
    assert_eq!(scale.data, vec![2.0, 3.0]);

    let matmul = backend
        .execute(
            GpuOperation::MatmulF32,
            &[
                GpuTensor::new(vec![2, 2], vec![1.0, 2.0, 3.0, 4.0]).unwrap(),
                GpuTensor::new(vec![2, 2], vec![5.0, 6.0, 7.0, 8.0]).unwrap(),
            ],
        )
        .expect("cuBLAS matmul must execute");
    assert_eq!(matmul.shape, vec![2, 2]);
    assert_eq!(matmul.data, vec![19.0, 22.0, 43.0, 50.0]);
}

#[test]
fn cuda_backend_runs_the_managed_gpu_interpreter_without_exposing_device_state() {
    if std::env::var_os("HIVEMIND_RUN_CUDA_TESTS").is_none() {
        eprintln!("skipping CUDA interpreter test; set HIVEMIND_RUN_CUDA_TESTS=1 to enable");
        return;
    }

    let (mut backend, _) = cuda_test_backend();
    let mut executor = ManagedGpuExecutor::new(&mut backend);
    let result = executor
        .execute(
            "return gpu_scale_f32(gpu_add_f32([1.0, 2.0], [3.0, 4.0]), 2.0);",
            ExecutionLimits::default(),
        )
        .expect("managed GPU interpreter must execute through CUDA backend");

    assert_eq!(
        result.value,
        Value::List(vec![Value::Float(8.0), Value::Float(12.0)])
    );
    assert_eq!(result.receipt.executed_ops, 40);
}

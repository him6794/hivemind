use general_compute_runtime::artifact::ArtifactMaterializer;
use general_compute_runtime::cp_python::{PythonBackendRegistration, PythonBackendRegistry};
use general_compute_runtime::execution::ReferenceBackendExecutor;
use general_compute_runtime::gpu::{GpuCapability, GpuRuntime, GpuSelection, GpuVendor};
use general_compute_runtime::sandbox::BackendExecutionMode;
use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, BackendRegistration, DeterminismPolicy, ExecutionPolicy,
    GENERAL_COMPUTE_RUNTIME_VERSION, GeneralComputeRequest, ResultStatus,
    TrustedWorkerCapabilityRegistration, WorkerCapabilities,
};

const IMAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn reference_executor_writes_operator_selected_gpu_identity_into_result() {
    let mut request = request();
    let selected = gpu("gpu-trusted");
    request.execution_policy.gpu_required = true;
    request.execution_policy.gpu_requirement = Some(
        general_compute_runtime::gpu::GpuRequirement::new(
            GpuVendor::Nvidia,
            "sm_80",
            GpuRuntime::Cuda,
            "550.54",
            8 * 1024 * 1024 * 1024,
            4,
            IMAGE,
            false,
        )
        .unwrap(),
    );
    request.request_digest = request.canonical_request_digest();
    let capabilities = capabilities(&request);
    let registration = TrustedWorkerCapabilityRegistration {
        worker: worker(),
        gpu_capabilities: vec![selected.clone()],
        managed_gpu_backends: vec![],
        backends: capabilities.backends.clone(),
    };
    let executor = ReferenceBackendExecutor::new_with_trusted_registration(
        capabilities.clone(),
        worker(),
        python_registry(),
        registration,
    );
    let root = std::env::temp_dir().join(format!("hivemind-gpu-execution-{}", std::process::id()));
    let materializer = ArtifactMaterializer::new(&root).unwrap();

    let result = executor
        .execute(&request, &materializer)
        .expect("trusted GPU selection should be emitted with the result");

    assert_eq!(result.status, ResultStatus::Completed);
    assert_eq!(result.gpu_selection, Some(GpuSelection::Gpu(selected)));
    result.validate_against(&request, &capabilities).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

fn request() -> GeneralComputeRequest {
    let mut request = GeneralComputeRequest {
        execution_id: "gpu-execution".into(),
        attempt_id: "gpu-attempt".into(),
        idempotency_key: "gpu-idempotency".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: IMAGE.into(),
        backend_id: "python-cpython-312".into(),
        entrypoint: "main".into(),
        source_artifact: ArtifactManifest::inline_json(
            "source",
            ArtifactRole::Source,
            b"result = 7",
        ),
        input_artifacts: vec![],
        execution_policy: ExecutionPolicy::default(),
        determinism: DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn gpu(device_id: &str) -> GpuCapability {
    GpuCapability::new(
        GpuVendor::Nvidia,
        device_id,
        "sm_80",
        GpuRuntime::Cuda,
        "12.4",
        "550.54",
        16 * 1024 * 1024 * 1024,
        8,
        IMAGE,
    )
    .unwrap()
}

fn capabilities(request: &GeneralComputeRequest) -> general_compute_runtime::CapabilityMatrix {
    general_compute_runtime::CapabilityMatrix::new(vec![BackendRegistration {
        backend_id: request.backend_id.clone(),
        execution_mode: BackendExecutionMode::ReferenceDirect,
        guest_image_digest: IMAGE.into(),
        capabilities: vec!["cpu".into()],
        max_threads: 2,
        network_allowed: false,
        filesystem_read_only: true,
        gpu_allowed: true,
    }])
}

fn worker() -> WorkerCapabilities {
    WorkerCapabilities {
        guest_image_digests: vec![IMAGE.into()],
        capabilities: vec!["cpu".into()],
        max_threads: 2,
        gpu_available: true,
    }
}

fn python_registry() -> PythonBackendRegistry {
    PythonBackendRegistry::new(vec![PythonBackendRegistration {
        backend_id: "python-cpython-312".into(),
        executable: "python".into(),
        runtime_version: "CPython 3.12.9".into(),
        guest_image_digest: IMAGE.into(),
        protocol_version: "general-compute-wire-v1".into(),
        max_output_bytes: 1024,
        execution_mode: BackendExecutionMode::ReferenceDirect,
    }])
    .unwrap()
}

use general_compute_runtime::gpu::{GpuCapability, GpuRuntime, GpuSelection, GpuVendor};
use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, BackendRegistration, DeterminismPolicy, ExecutionPolicy,
    GeneralComputeRequest, TrustedWorkerCapabilityRegistration, WorkerCapabilities,
    GENERAL_COMPUTE_RUNTIME_VERSION,
};
use hivemind_worker_executor::runtime_admission::WorkerRuntimeAdmission;

const IMAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn worker_admission_uses_operator_registration_for_deterministic_gpu_selection() {
    let request = gpu_request();
    let first = gpu("gpu-a");
    let second = gpu("gpu-b");
    let registration = trusted_registration(&request, vec![second, first.clone()]);
    let admission = WorkerRuntimeAdmission::new_with_trusted_registration(registration);

    admission
        .admit(
            GENERAL_COMPUTE_RUNTIME_VERSION,
            &serde_json::to_vec(&request).unwrap(),
        )
        .expect("operator-approved registration should admit the request");

    assert_eq!(
        admission.select_gpu_for_request(&request).unwrap(),
        Some(GpuSelection::Gpu(first))
    );
}

#[test]
fn worker_admission_rejects_gpu_request_without_a_typed_operator_identity() {
    let request = gpu_request();
    let registration = trusted_registration(&request, Vec::new());
    let admission = WorkerRuntimeAdmission::new_with_trusted_registration(registration);

    let error = admission
        .admit(
            GENERAL_COMPUTE_RUNTIME_VERSION,
            &serde_json::to_vec(&request).unwrap(),
        )
        .expect_err("a typed GPU request must not use a boolean-only registration");

    assert!(error.to_string().contains("GPU"));
}

fn gpu_request() -> GeneralComputeRequest {
    let requirement = general_compute_runtime::gpu::GpuRequirement::new(
        GpuVendor::Nvidia,
        "sm_80",
        GpuRuntime::Cuda,
        "550.54",
        8 * 1024 * 1024 * 1024,
        4,
        IMAGE,
        false,
    )
    .unwrap();
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
        execution_policy: ExecutionPolicy {
            gpu_required: true,
            gpu_requirement: Some(requirement),
            ..ExecutionPolicy::default()
        },
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

fn trusted_registration(
    request: &GeneralComputeRequest,
    gpu_capabilities: Vec<GpuCapability>,
) -> TrustedWorkerCapabilityRegistration {
    TrustedWorkerCapabilityRegistration {
        worker: WorkerCapabilities {
            guest_image_digests: vec![IMAGE.into()],
            capabilities: vec!["cpu".into()],
            max_threads: 2,
            gpu_available: true,
        },
        gpu_capabilities,
        managed_gpu_backends: vec![],
        backends: vec![BackendRegistration {
            backend_id: request.backend_id.clone(),
            execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
            guest_image_digest: IMAGE.into(),
            capabilities: vec!["cpu".into()],
            max_threads: 2,
            network_allowed: false,
            filesystem_read_only: true,
            gpu_allowed: true,
        }],
    }
}

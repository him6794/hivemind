use general_compute_runtime::gpu::{GpuCapability, GpuRuntime, GpuSelection, GpuVendor};
use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, ExecutionPolicy, GeneralComputeRequest,
    TrustedWorkerCapabilityRegistration, WorkerCapabilities, GENERAL_COMPUTE_RUNTIME_VERSION,
};

const IMAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn request_with_gpu_requirement() -> GeneralComputeRequest {
    let mut execution_policy = ExecutionPolicy::default();
    execution_policy.gpu_required = true;
    execution_policy.gpu_requirement = Some(
        general_compute_runtime::gpu::GpuRequirement::new(
            GpuVendor::Nvidia,
            "sm_80",
            GpuRuntime::Cuda,
            "550.54",
            16 * 1024 * 1024 * 1024,
            8,
            IMAGE,
            false,
        )
        .expect("valid GPU requirement"),
    );
    let mut request = GeneralComputeRequest {
        execution_id: "execution-gpu".into(),
        attempt_id: "attempt-gpu".into(),
        idempotency_key: "idempotency-gpu".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: IMAGE.into(),
        backend_id: "cuda-reference".into(),
        entrypoint: "main".into(),
        source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, b"{}"),
        input_artifacts: vec![],
        execution_policy,
        determinism: Default::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn capability(device_id: &str) -> GpuCapability {
    GpuCapability::new(
        GpuVendor::Nvidia,
        device_id,
        "sm_80",
        GpuRuntime::Cuda,
        "12.4",
        "550.54",
        24 * 1024 * 1024 * 1024,
        16,
        IMAGE,
    )
    .expect("valid GPU capability")
}

fn registration(gpu_capabilities: Vec<GpuCapability>) -> TrustedWorkerCapabilityRegistration {
    TrustedWorkerCapabilityRegistration {
        worker: WorkerCapabilities {
            guest_image_digests: vec![IMAGE.into()],
            capabilities: vec!["cuda".into()],
            max_threads: 4,
            gpu_available: true,
        },
        gpu_capabilities,
        backends: vec![],
    }
}

#[test]
fn trusted_registration_round_trips_and_selects_a_stable_gpu_identity() {
    let request = request_with_gpu_requirement();
    let registration = registration(vec![capability("gpu-b"), capability("gpu-a")]);
    let encoded = serde_json::to_vec(&registration).expect("registration JSON should encode");
    let decoded: TrustedWorkerCapabilityRegistration =
        serde_json::from_slice(&encoded).expect("registration JSON should decode");

    let selection = decoded
        .select_gpu_for_request(&request)
        .expect("trusted registration should validate")
        .expect("GPU request should produce a selection");
    match selection {
        GpuSelection::Gpu(capability) => assert_eq!(capability.device_id, "gpu-a"),
        GpuSelection::CpuFallback { .. } => panic!("compatible registered GPU must be selected"),
    }
}

#[test]
fn legacy_trusted_registration_json_defaults_to_no_typed_gpu_capabilities() {
    let legacy = format!(
        r#"{{"worker":{{"guest_image_digests":["{IMAGE}"],"capabilities":["cpu"],"max_threads":1,"gpu_available":false}},"backends":[]}}"#
    );
    let registration: TrustedWorkerCapabilityRegistration =
        serde_json::from_str(&legacy).expect("legacy registration must remain readable");
    assert!(registration.gpu_capabilities.is_empty());
}

#[test]
fn trusted_registration_rejects_a_malformed_gpu_capability_before_selection() {
    let request = request_with_gpu_requirement();
    let mut registration = registration(vec![capability("gpu-a")]);
    registration.gpu_capabilities[0].driver_abi.clear();

    let error = registration
        .select_gpu_for_request(&request)
        .expect_err("malformed trusted GPU state must fail closed");
    assert_eq!(error.code, general_compute_runtime::ValidationErrorCode::GpuUnavailable);
}

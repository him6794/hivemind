use general_compute_runtime::gpu::{GpuRequirement, GpuRuntime, GpuVendor};
use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, DeterminismPolicy, ExecutionPolicy, GeneralComputeRequest,
    ValidationErrorCode,
};

fn image(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn request() -> GeneralComputeRequest {
    let source = ArtifactManifest::inline_json("source", ArtifactRole::Source, b"return 1;");
    let mut request = GeneralComputeRequest {
        execution_id: "execution-gpu-request".into(),
        attempt_id: "attempt-gpu-request".into(),
        idempotency_key: "idempotency-gpu-request".into(),
        request_digest: image('0'),
        runtime_version: general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: image('a'),
        backend_id: "gpu-backend".into(),
        entrypoint: "main".into(),
        source_artifact: source,
        input_artifacts: Vec::new(),
        execution_policy: ExecutionPolicy::default(),
        determinism: DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn requirement() -> GpuRequirement {
    GpuRequirement::new(
        GpuVendor::Nvidia,
        "sm_80",
        GpuRuntime::Cuda,
        "550.54",
        16 * 1024 * 1024 * 1024,
        8,
        image('b'),
        false,
    )
    .expect("valid GPU requirement")
}

#[test]
fn request_round_trip_preserves_typed_gpu_requirement() {
    let cpu_policy = serde_json::to_value(ExecutionPolicy::default()).expect("policy serializes");
    assert!(cpu_policy.get("gpu_requirement").is_none());

    let mut request = request();
    request.execution_policy.gpu_required = true;
    let expected = requirement();
    request.execution_policy.gpu_requirement = Some(expected.clone());
    request.request_digest = request.canonical_request_digest();

    request
        .validate()
        .expect("typed GPU request should validate");
    let encoded = serde_json::to_vec(&request).expect("request should serialize");
    let decoded: GeneralComputeRequest =
        serde_json::from_slice(&encoded).expect("request should deserialize");
    assert_eq!(decoded.execution_policy.gpu_requirement, Some(expected));
    assert_eq!(decoded.request_digest, decoded.canonical_request_digest());
}

#[test]
fn request_rejects_missing_or_unrequested_typed_gpu_requirement() {
    let mut missing = request();
    missing.execution_policy.gpu_required = true;
    missing.request_digest = missing.canonical_request_digest();
    assert_eq!(
        missing
            .validate()
            .expect_err("GPU flag requires a typed requirement")
            .code,
        ValidationErrorCode::PolicyInvalid
    );

    let mut unrequested = request();
    unrequested.execution_policy.gpu_requirement = Some(requirement());
    unrequested.request_digest = unrequested.canonical_request_digest();
    assert_eq!(
        unrequested
            .validate()
            .expect_err("typed GPU requirement must set gpu_required")
            .code,
        ValidationErrorCode::PolicyInvalid
    );
}

use general_compute_runtime::gpu::{
    GpuCapability, GpuFallbackReason, GpuRequirement, GpuRuntime, GpuSelection, GpuVendor,
};
use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, EvidenceEnvelope, ExecutionPolicy,
    GENERAL_COMPUTE_RUNTIME_VERSION, GeneralComputeRequest, GeneralComputeResult, ResultStatus,
    UsageClaim, ValidationErrorCode,
};

const IMAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn gpu_request(allow_cpu_fallback: bool) -> GeneralComputeRequest {
    let mut policy = ExecutionPolicy::default();
    policy.gpu_required = true;
    policy.gpu_requirement = Some(
        GpuRequirement::new(
            GpuVendor::Nvidia,
            "sm_80",
            GpuRuntime::Cuda,
            "550.54",
            16 * 1024 * 1024 * 1024,
            8,
            IMAGE,
            allow_cpu_fallback,
        )
        .expect("valid GPU requirement"),
    );
    let mut request = GeneralComputeRequest {
        execution_id: "execution-result-gpu".into(),
        attempt_id: "attempt-result-gpu".into(),
        idempotency_key: "idempotency-result-gpu".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: IMAGE.into(),
        backend_id: "cuda-reference".into(),
        entrypoint: "main".into(),
        source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, b"{}"),
        input_artifacts: vec![],
        execution_policy: policy,
        determinism: Default::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn gpu_capability() -> GpuCapability {
    GpuCapability::new(
        GpuVendor::Nvidia,
        "gpu-a",
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

fn result(request: &GeneralComputeRequest) -> GeneralComputeResult {
    let output = ArtifactManifest::inline_json("stdout", ArtifactRole::Output, b"ok");
    GeneralComputeResult {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        status: ResultStatus::Completed,
        exit_code: Some(0),
        error_code: None,
        stdout: "ok".into(),
        stderr: String::new(),
        output_artifacts: vec![output.clone()],
        usage: UsageClaim {
            output_bytes: 2,
            ..UsageClaim::default()
        },
        runtime_version: request.runtime_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        input_sha256: general_compute_runtime::sha256_digest(b"{}"),
        determinism: request.determinism.clone(),
        capability_summary: vec![],
        output_manifest_root: general_compute_runtime::canonical_artifact_root(&[output]),
        evidence: EvidenceEnvelope::default(),
        gpu_selection: None,
    }
}

#[test]
fn result_round_trip_preserves_the_trusted_gpu_device_identity() {
    let request = gpu_request(false);
    let mut result = result(&request);
    result.gpu_selection = Some(GpuSelection::Gpu(gpu_capability()));

    result
        .validate_gpu_selection(&request)
        .expect("matching selected device must validate");
    let encoded = serde_json::to_vec(&result).expect("result should encode");
    let decoded: GeneralComputeResult = serde_json::from_slice(&encoded).expect("result decodes");
    match decoded.gpu_selection.expect("selection is bound") {
        GpuSelection::Gpu(capability) => assert_eq!(capability.device_id, "gpu-a"),
        GpuSelection::CpuFallback { .. } => panic!("GPU selection must retain device identity"),
    }
}

#[test]
fn result_rejects_a_missing_gpu_selection_for_a_required_gpu_request() {
    let request = gpu_request(false);
    let result = result(&request);

    let error = result
        .validate_gpu_selection(&request)
        .expect_err("GPU request without selected identity must fail closed");
    assert_eq!(error.code, ValidationErrorCode::GpuUnavailable);
}

#[test]
fn result_rejects_cpu_fallback_when_the_request_does_not_allow_it() {
    let request = gpu_request(false);
    let mut result = result(&request);
    result.gpu_selection = Some(GpuSelection::CpuFallback {
        reason: GpuFallbackReason::NoCompatibleDevice,
    });

    let error = result
        .validate_gpu_selection(&request)
        .expect_err("CPU fallback must be explicit in the request");
    assert_eq!(error.code, ValidationErrorCode::GpuUnavailable);
}

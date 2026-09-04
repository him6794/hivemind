use general_compute_runtime::managed_gpu::{
    MANAGED_GPU_BILLING_VERSION, MANAGED_GPU_COST_MODEL_VERSION,
    MANAGED_GPU_OPERATION_REGISTRY_VERSION, MANAGED_GPU_REQUEST_PROTOCOL_VERSION,
    MANAGED_GPU_RESULT_PROTOCOL_VERSION, MANAGED_GPU_RUNTIME_VERSION,
    MANAGED_GPU_SEMANTICS_MANIFEST_SHA256, MANAGED_GPU_SETTLEMENT_BASIS,
    ManagedGpuBackendRegistration, ManagedGpuCapability, ManagedGpuEvidence,
    ManagedGpuEvidenceLevel, ManagedGpuLimits, ManagedGpuRequest, ManagedGpuRequirement,
    ManagedGpuResult, ManagedGpuStatus, ManagedGpuUsage,
};
use general_compute_runtime::{
    TrustedWorkerCapabilityRegistration, WorkerCapabilities, sha256_digest,
};

const IMAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn request() -> ManagedGpuRequest {
    let requirement =
        ManagedGpuRequirement::new("sm_89", "12.6", "550.54", 16 * 1024 * 1024 * 1024, 8, IMAGE)
            .expect("valid managed GPU requirement");
    let mut request = ManagedGpuRequest {
        protocol_version: MANAGED_GPU_REQUEST_PROTOCOL_VERSION.into(),
        execution_id: "execution-gpu-v1".into(),
        attempt_id: "attempt-gpu-v1".into(),
        idempotency_key: "idempotency-gpu-v1".into(),
        request_digest: String::new(),
        runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
        semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
        operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
        backend_id: "managed-cuda-ada".into(),
        guest_image_digest: IMAGE.into(),
        source: "let result = gpu_scale_f32([1.0], 2.0);".into(),
        input_json: "{}".into(),
        gpu_requirement: requirement,
        limits: ManagedGpuLimits::default(),
        reservation_cpt: 777,
        billing_version: MANAGED_GPU_BILLING_VERSION.into(),
        cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
        settlement_basis: MANAGED_GPU_SETTLEMENT_BASIS.into(),
        proof_policy: general_compute_runtime::managed_gpu::ManagedGpuProofPolicy::None,
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn capability(device_id: &str, ordinal: i32, uuid: &str) -> ManagedGpuCapability {
    ManagedGpuCapability::new(
        device_id,
        "sm_89",
        "12.6",
        "550.54",
        24 * 1024 * 1024 * 1024,
        16,
        IMAGE,
        ordinal,
        uuid,
    )
    .expect("valid managed GPU capability")
}

fn registration(capabilities: Vec<ManagedGpuCapability>) -> TrustedWorkerCapabilityRegistration {
    TrustedWorkerCapabilityRegistration {
        worker: WorkerCapabilities {
            guest_image_digests: vec![IMAGE.into()],
            capabilities: vec!["managed-function-gpu-v1".into()],
            max_threads: 4,
            gpu_available: true,
        },
        gpu_capabilities: vec![],
        managed_gpu_backends: vec![ManagedGpuBackendRegistration {
            backend_id: "managed-cuda-ada".into(),
            runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
            semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
            operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
            guest_image_digest: IMAGE.into(),
            billing_version: MANAGED_GPU_BILLING_VERSION.into(),
            cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
            reservation_cpt: 777,
            max_source_bytes: 256 * 1024,
            max_input_bytes: 16 * 1024 * 1024,
            max_output_bytes: 16 * 1024 * 1024,
            max_operations: 1_000_000,
            max_gpu_time_ms: 120_000,
            capabilities,
        }],
        backends: vec![],
    }
}

fn completed_result(
    request: &ManagedGpuRequest,
    registration: &TrustedWorkerCapabilityRegistration,
) -> ManagedGpuResult {
    let selected_gpu = registration
        .select_managed_gpu_for_request(request)
        .expect("trusted GPU should be selected");
    let output = "42";
    ManagedGpuResult {
        protocol_version: MANAGED_GPU_RESULT_PROTOCOL_VERSION.into(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        runtime_version: request.runtime_version.clone(),
        semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
        operation_registry_version: request.operation_registry_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        source_sha256: request.source_sha256(),
        input_sha256: request.input_sha256(),
        reservation_cpt: request.reservation_cpt,
        status: ManagedGpuStatus::Completed,
        exit_code: Some(0),
        error_code: None,
        output: output.into(),
        output_sha256: sha256_digest(output.as_bytes()),
        selected_gpu,
        usage: ManagedGpuUsage {
            source_bytes: request.source.len() as u64,
            input_bytes: request.input_json.len() as u64,
            output_bytes: output.len() as u64,
            executed_operations: 2,
            operation_cost_units: 20,
            wall_time_ms: 4,
            gpu_time_ms: 2,
            gpu_memory_bytes: 1024,
        },
        evidence: ManagedGpuEvidence::default(),
    }
}

#[test]
fn request_digest_binds_the_complete_independent_gpu_envelope() {
    let mut request = request();
    request.validate().expect("valid request should validate");

    request.source.push('x');
    let error = request
        .validate()
        .expect_err("changing source without a new digest must fail");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::RequestDigestMismatch
    );
}

#[test]
fn request_rejects_generic_semantics_and_cpu_fallback() {
    let mut digest_request = request();
    digest_request.semantics_manifest_sha256 = format!("sha256:{}", "b".repeat(64));
    digest_request.request_digest = digest_request.canonical_request_digest();
    let error = digest_request
        .validate()
        .expect_err("generic digest must be rejected");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::RequestBindingMismatch
    );

    let mut fallback = request();
    fallback.gpu_requirement.allow_cpu_fallback = true;
    fallback.request_digest = fallback.canonical_request_digest();
    let error = fallback
        .validate()
        .expect_err("GPU-v1 must reject CPU fallback");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::GpuUnavailable
    );
}

#[test]
fn request_deserialization_rejects_unknown_fields() {
    let request = request();
    let mut json = serde_json::to_value(request).expect("request should serialize");
    json.as_object_mut()
        .expect("request should be an object")
        .insert("proof".into(), serde_json::json!({"receipt": "forbidden"}));
    assert!(serde_json::from_value::<ManagedGpuRequest>(json).is_err());
}

#[test]
fn trusted_selection_is_independent_and_deterministic() {
    let request = request();
    let registration = registration(vec![
        capability("gpu-b", 1, "GPU-bbbbbbbb"),
        capability("gpu-a", 0, "GPU-aaaaaaaa"),
    ]);
    let selected = registration
        .select_managed_gpu_for_request(&request)
        .expect("trusted managed GPU should be selected");
    assert_eq!(selected.device_id, "gpu-a");
    assert_eq!(selected.cuda_device_ordinal, 0);
    assert_eq!(selected.cuda_uuid, "GPU-aaaaaaaa");
}

#[test]
fn trusted_selection_rejects_duplicate_device_identity() {
    let request = request();
    let registration = registration(vec![
        capability("gpu-a", 0, "GPU-aaaaaaaa"),
        capability("gpu-a", 1, "GPU-bbbbbbbb"),
    ]);
    let error = registration
        .select_managed_gpu_for_request(&request)
        .expect_err("duplicate operator device IDs must fail closed");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::BackendUnavailable
    );
}

#[test]
fn trusted_selection_rejects_duplicate_cuda_ordinals() {
    let request = request();
    let registration = registration(vec![
        capability("gpu-a", 0, "GPU-aaaaaaaa"),
        capability("gpu-b", 0, "GPU-bbbbbbbb"),
    ]);
    let error = registration
        .select_managed_gpu_for_request(&request)
        .expect_err("duplicate CUDA ordinals must fail closed");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::BackendUnavailable
    );
}

#[test]
fn result_requires_exact_trusted_identity_and_usage_accounting() {
    let request = request();
    let registration = registration(vec![capability("gpu-a", 0, "GPU-aaaaaaaa")]);
    let result = completed_result(&request, &registration);
    result
        .validate_against(&request, &registration)
        .expect("valid typed result should validate");

    let mut forged = result;
    forged.usage.operation_cost_units = 0;
    let error = forged
        .validate_against(&request, &registration)
        .expect_err("forged operation accounting must fail");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::UsageExceedsPolicy
    );
}

#[test]
fn result_rejects_output_source_and_proof_drift() {
    let request = request();
    let registration = registration(vec![capability("gpu-a", 0, "GPU-aaaaaaaa")]);
    let result = completed_result(&request, &registration);

    let mut output_drift = result.clone();
    output_drift.output.push('x');
    let error = output_drift
        .validate_against(&request, &registration)
        .expect_err("output digest drift must fail");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::ResultBindingMismatch
    );

    let mut source_drift = result.clone();
    source_drift.source_sha256 = sha256_digest(b"different source");
    let error = source_drift
        .validate_against(&request, &registration)
        .expect_err("source digest drift must fail");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::ResultBindingMismatch
    );

    let mut json = serde_json::to_value(result).expect("result should serialize");
    json.as_object_mut()
        .expect("result should be an object")
        .insert("proof".into(), serde_json::json!({"seal": "forbidden"}));
    assert!(serde_json::from_value::<ManagedGpuResult>(json).is_err());
}

#[test]
fn result_evidence_and_status_are_fail_closed() {
    let request = request();
    let registration = registration(vec![capability("gpu-a", 0, "GPU-aaaaaaaa")]);
    let mut result = completed_result(&request, &registration);
    result.evidence = ManagedGpuEvidence {
        level: ManagedGpuEvidenceLevel::Unverified,
        payload_sha256: Some("not-a-digest".into()),
    };
    let error = result
        .validate_against(&request, &registration)
        .expect_err("invalid evidence digest must fail");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::EvidenceInvalid
    );

    let mut result = completed_result(&request, &registration);
    result.status = ManagedGpuStatus::Completed;
    result.exit_code = None;
    let error = result
        .validate_against(&request, &registration)
        .expect_err("completed result without zero exit must fail");
    assert_eq!(
        error.code,
        general_compute_runtime::ValidationErrorCode::ResultStatusInvalid
    );
}

#[test]
fn capability_rejects_partial_or_invalid_cuda_binding() {
    let mut capability = capability("gpu-a", 0, "GPU-aaaaaaaa");
    capability.cuda_uuid = "GPU-".into();
    assert!(capability.validate().is_err());

    capability.cuda_uuid = "GPU-aaaaaaaa".into();
    capability.cuda_device_ordinal = -1;
    assert!(capability.validate().is_err());
}

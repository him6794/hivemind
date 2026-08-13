use general_compute_runtime::{
    ArtifactChunk, ArtifactManifest, ArtifactRole, BackendRegistration, CapabilityMatrix, ExecutionPolicy,
    GeneralComputeRequest, GeneralComputeResult, ResultStatus, ValidationErrorCode, WorkerCapabilities,
    GENERAL_COMPUTE_RUNTIME_VERSION,
};

fn valid_request() -> GeneralComputeRequest {
    GeneralComputeRequest {
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        backend_id: "python-numpy-scipy".into(),
        entrypoint: "main:run".into(),
        source_artifact: ArtifactManifest::inline_json("input-source", ArtifactRole::Source, br#"{}"#),
        input_artifacts: vec![ArtifactManifest::inline_json(
            "input-data",
            ArtifactRole::Input,
            br#"{"x":1}"#,
        )],
        execution_policy: ExecutionPolicy::default(),
        determinism: Default::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-model-v1".into(),
    }
}

#[test]
fn alpha_runtime_id_is_the_only_pre_release_contract() {
    assert_eq!(GENERAL_COMPUTE_RUNTIME_VERSION, "general-compute-v1alpha1");

    let mut request = valid_request();
    request.runtime_version = GENERAL_COMPUTE_RUNTIME_VERSION.into();
    request.validate().expect("alpha runtime id should be accepted");

    request.runtime_version = "general-compute-v1".into();
    let error = request
        .validate()
        .expect_err("stable runtime id must remain gated before release promotion");
    assert_eq!(error.code, ValidationErrorCode::RuntimeVersionMismatch);
}

#[test]
fn request_round_trip_preserves_versioned_execution_contract() {
    let request = GeneralComputeRequest {
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        backend_id: "python-numpy-scipy".into(),
        entrypoint: "main:run".into(),
        source_artifact: ArtifactManifest::inline_json("input-source", ArtifactRole::Source, br#"{}"#),
        input_artifacts: vec![ArtifactManifest::inline_json(
            "input-data",
            ArtifactRole::Input,
            br#"{"x":1}"#,
        )],
        execution_policy: ExecutionPolicy::default(),
        determinism: Default::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-model-v1".into(),
    };

    let encoded = serde_json::to_vec(&request).expect("request serializes");
    let decoded: GeneralComputeRequest = serde_json::from_slice(&encoded).expect("request decodes");

    assert_eq!(decoded, request);
    assert_eq!(decoded.runtime_version, GENERAL_COMPUTE_RUNTIME_VERSION);
    assert_eq!(decoded.source_artifact.sha256, request.source_artifact.sha256);
}

#[test]
fn result_round_trip_keeps_claimed_usage_and_output_manifest() {
    let result = GeneralComputeResult {
        status: ResultStatus::Completed,
        exit_code: Some(0),
        error_code: None,
        stdout: "ok".into(),
        stderr: String::new(),
        output_artifacts: vec![ArtifactManifest::inline_json(
            "output-data",
            ArtifactRole::Output,
            br#"{"answer":42}"#,
        )],
        usage: Default::default(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        backend_id: "python-numpy-scipy".into(),
        guest_image_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        input_sha256: "sha256:input".into(),
        determinism: Default::default(),
        capability_summary: vec!["cpu".into()],
    };

    let encoded = serde_json::to_vec(&result).expect("result serializes");
    let decoded: GeneralComputeResult = serde_json::from_slice(&encoded).expect("result decodes");

    assert_eq!(decoded, result);
    assert_eq!(decoded.status, ResultStatus::Completed);
    assert_eq!(decoded.output_artifacts[0].size_bytes, 13);
}

#[test]
fn artifact_manifest_rejects_tampered_inline_bytes() {
    let mut artifact = ArtifactManifest::inline_json("input", ArtifactRole::Input, br#"{"x":1}"#);
    artifact.inline_bytes = Some(br#"{"x":2}"#.to_vec());

    let error = artifact.validate().expect_err("tampered bytes must fail validation");
    assert_eq!(error, "artifact checksum does not match bytes");
}

#[test]
fn request_validation_rejects_unbounded_or_writable_policy() {
    let mut request = valid_request();
    request.execution_policy.cpu_millis = u64::MAX;

    let error = request.validate().expect_err("unbounded quota must fail closed");
    assert_eq!(error.code, ValidationErrorCode::PolicyInvalid);

    request.execution_policy = ExecutionPolicy::default();
    request.execution_policy.filesystem_read_only = false;
    let error = request
        .validate()
        .expect_err("writable host filesystem must fail closed");
    assert_eq!(error.code, ValidationErrorCode::FilesystemPolicyViolation);
}

#[test]
fn capability_matrix_rejects_unregistered_image_and_missing_worker_capability() {
    let request = valid_request();
    let matrix = CapabilityMatrix::new(vec![BackendRegistration {
        backend_id: "python-numpy-scipy".into(),
        guest_image_digest: "sha256:registered".into(),
        capabilities: vec!["cpu".into(), "numpy".into()],
        max_threads: 4,
        network_allowed: false,
        filesystem_read_only: true,
        gpu_allowed: false,
    }]);
    let worker = WorkerCapabilities {
        guest_image_digests: vec!["sha256:guest".into()],
        capabilities: vec!["cpu".into()],
        max_threads: 4,
        gpu_available: false,
    };

    let error = matrix
        .validate_request(&request, &worker)
        .expect_err("request image must be registered");
    assert_eq!(error.code, ValidationErrorCode::GuestImageMismatch);
}

#[test]
fn capability_matrix_rejects_network_and_gpu_requirements_without_registration() {
    let mut request = valid_request();
    request.execution_policy.network_allowed = true;
    request.execution_policy.gpu_required = true;
    let matrix = CapabilityMatrix::new(vec![BackendRegistration {
        backend_id: "python-numpy-scipy".into(),
        guest_image_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        capabilities: vec!["cpu".into()],
        max_threads: 4,
        network_allowed: false,
        filesystem_read_only: true,
        gpu_allowed: false,
    }]);
    let worker = WorkerCapabilities {
        guest_image_digests: vec!["sha256:0000000000000000000000000000000000000000000000000000000000000000".into()],
        capabilities: vec!["cpu".into()],
        max_threads: 4,
        gpu_available: false,
    };

    let error = matrix
        .validate_request(&request, &worker)
        .expect_err("network and gpu requirements must not bypass registration");
    assert_eq!(error.code, ValidationErrorCode::NetworkDenied);
}

#[test]
fn artifact_manifest_rejects_chunk_gaps() {
    let mut artifact = ArtifactManifest::inline_json("input", ArtifactRole::Input, b"12345678");
    artifact.inline_bytes = None;
    artifact.chunks = vec![
        ArtifactChunk {
            offset: 0,
            size_bytes: 3,
            sha256: "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        },
        ArtifactChunk {
            offset: 5,
            size_bytes: 3,
            sha256: "sha256:1111111111111111111111111111111111111111111111111111111111111111".into(),
        },
    ];

    let error = artifact.validate().expect_err("chunk gaps must fail closed");
    assert_eq!(error, "artifact chunks do not cover artifact bytes");
}

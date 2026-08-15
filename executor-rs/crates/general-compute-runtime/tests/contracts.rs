use general_compute_runtime::{
    canonical_artifact_root, canonical_input_digest, sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRange,
    ArtifactRole, BackendRegistration, CapabilityMatrix, EvidenceEnvelope, ExecutionPolicy,
    GeneralComputeRequest, GeneralComputeResult, ResultStatus, ValidationErrorCode,
    ProductionResultEnvelope, UsageClaim, WorkerCapabilities, GENERAL_COMPUTE_RUNTIME_VERSION,
    PRODUCTION_RESULT_PROTOCOL_VERSION,
};
use general_compute_runtime::gpu::{GpuRequirement, GpuRuntime, GpuVendor};
use general_compute_runtime::sandbox::BackendExecutionMode;

#[test]
fn production_windows_execution_mode_round_trips_as_a_distinct_contract() {
    let encoded = serde_json::to_string(&BackendExecutionMode::ProductionSandboxedWindows)
        .expect("execution mode should serialize");
    assert_eq!(encoded, "\"production_sandboxed_windows\"");
    let decoded: BackendExecutionMode = serde_json::from_str(&encoded)
        .expect("execution mode should deserialize");
    assert_eq!(decoded, BackendExecutionMode::ProductionSandboxedWindows);
}

fn valid_request() -> GeneralComputeRequest {
    let mut request = GeneralComputeRequest {
        execution_id: "execution-1".into(),
        attempt_id: "attempt-1".into(),
        idempotency_key: "idempotency-1".into(),
        request_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .into(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest:
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        backend_id: "python-numpy-scipy".into(),
        entrypoint: "main:run".into(),
        source_artifact: ArtifactManifest::inline_json(
            "input-source",
            ArtifactRole::Source,
            br#"{}"#,
        ),
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
    request.request_digest = request.canonical_request_digest();
    request
}

#[test]
fn production_result_envelope_requires_version_and_verified_output_root() {
    let request = valid_request();
    let output = ArtifactManifest::inline_json("stdout", ArtifactRole::Output, b"ok");
    let valid = ProductionResultEnvelope {
        protocol_version: PRODUCTION_RESULT_PROTOCOL_VERSION.into(),
        status: ResultStatus::Completed,
        exit_code: Some(0),
        error_code: None,
        stdout: "ok".into(),
        stderr: String::new(),
        output_manifest_root: canonical_artifact_root(std::slice::from_ref(&output)),
        output_artifacts: vec![output],
        usage: UsageClaim {
            input_bytes: request.input_artifacts.iter().map(|artifact| artifact.size_bytes).sum(),
            output_bytes: 2,
            ..UsageClaim::default()
        },
        input_sha256: canonical_input_digest(br#"{}"#, &[br#"{"x":1}"#]),
    };
    assert!(valid.validate_for(&request).is_ok());

    let mut wrong_protocol = valid;
    wrong_protocol.protocol_version = "general-compute-result-v0".into();
    assert_eq!(
        wrong_protocol.validate_for(&request).unwrap_err().code,
        ValidationErrorCode::ResultBindingMismatch
    );
}

#[test]
fn production_result_envelope_binds_materialized_source_and_input_bytes() {
    let request = valid_request();
    let output = ArtifactManifest::inline_json("stdout", ArtifactRole::Output, b"ok");
    let mut envelope = ProductionResultEnvelope {
        protocol_version: PRODUCTION_RESULT_PROTOCOL_VERSION.into(),
        status: ResultStatus::Completed,
        exit_code: Some(0),
        error_code: None,
        stdout: "ok".into(),
        stderr: String::new(),
        output_artifacts: vec![output.clone()],
        usage: UsageClaim {
            input_bytes: request.input_artifacts[0].size_bytes,
            output_bytes: 2,
            ..UsageClaim::default()
        },
        input_sha256: canonical_input_digest(br#"{}"#, &[br#"{"x":1}"#]),
        output_manifest_root: canonical_artifact_root(std::slice::from_ref(&output)),
    };

    assert!(envelope.validate_for(&request).is_ok());

    envelope.input_sha256 = sha256_digest(br#"{"x":1}"#);
    assert_eq!(
        envelope.validate_for(&request).unwrap_err().code,
        ValidationErrorCode::ResultBindingMismatch
    );
}

#[test]
fn alpha_runtime_id_is_the_only_pre_release_contract() {
    assert_eq!(GENERAL_COMPUTE_RUNTIME_VERSION, "general-compute-v1alpha1");

    let mut request = valid_request();
    request.runtime_version = GENERAL_COMPUTE_RUNTIME_VERSION.into();
    request
        .validate()
        .expect("alpha runtime id should be accepted");

    request.runtime_version = "general-compute-v1".into();
    request.request_digest = request.canonical_request_digest();
    let error = request
        .validate()
        .expect_err("stable runtime id must remain gated before release promotion");
    assert_eq!(error.code, ValidationErrorCode::RuntimeVersionMismatch);
}

#[test]
fn request_contract_carries_retry_identity_and_digest() {
    let encoded = serde_json::to_value(valid_request()).expect("request serializes");
    for field in [
        "execution_id",
        "attempt_id",
        "idempotency_key",
        "request_digest",
    ] {
        assert!(
            encoded
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_some(),
            "request must carry non-null {field}"
        );
    }
}

#[test]
fn request_round_trip_preserves_versioned_execution_contract() {
    let request = valid_request();

    let encoded = serde_json::to_vec(&request).expect("request serializes");
    let decoded: GeneralComputeRequest = serde_json::from_slice(&encoded).expect("request decodes");

    assert_eq!(decoded, request);
    assert_eq!(decoded.runtime_version, GENERAL_COMPUTE_RUNTIME_VERSION);
    assert_eq!(
        decoded.source_artifact.sha256,
        request.source_artifact.sha256
    );
}

#[test]
fn result_round_trip_keeps_claimed_usage_and_output_manifest() {
    let request = valid_request();
    let result = GeneralComputeResult {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
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
        runtime_version: request.runtime_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        input_sha256: "sha256:input".into(),
        determinism: request.determinism.clone(),
        capability_summary: vec!["cpu".into()],
        gpu_selection: None,
        output_manifest_root: canonical_artifact_root(&[ArtifactManifest::inline_json(
            "output-data",
            ArtifactRole::Output,
            br#"{"answer":42}"#,
        )]),
        evidence: EvidenceEnvelope::default(),
    };

    let encoded = serde_json::to_vec(&result).expect("result serializes");
    let decoded: GeneralComputeResult = serde_json::from_slice(&encoded).expect("result decodes");

    assert_eq!(decoded, result);
    assert_eq!(decoded.status, ResultStatus::Completed);
    assert_eq!(decoded.output_artifacts[0].size_bytes, 13);

    let encoded = serde_json::to_value(&result).expect("result serializes");
    for field in [
        "execution_id",
        "attempt_id",
        "idempotency_key",
        "request_digest",
    ] {
        assert!(
            encoded
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_some(),
            "result must carry non-null {field}"
        );
    }
}

#[test]
fn result_validation_rejects_retry_identity_mismatch() {
    let request = valid_request();
    let mut result = GeneralComputeResult {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        status: ResultStatus::Completed,
        exit_code: Some(0),
        error_code: None,
        stdout: String::new(),
        stderr: String::new(),
        output_artifacts: Vec::new(),
        usage: Default::default(),
        runtime_version: request.runtime_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        input_sha256: "sha256:input".into(),
        determinism: request.determinism.clone(),
        capability_summary: vec!["cpu".into()],
        gpu_selection: None,
        output_manifest_root: canonical_artifact_root(&[]),
        evidence: EvidenceEnvelope::default(),
    };
    let registry = CapabilityMatrix::new(vec![BackendRegistration {
        backend_id: request.backend_id.clone(),
        execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
        guest_image_digest: request.guest_image_digest.clone(),
        capabilities: vec!["cpu".into()],
        max_threads: 1,
        network_allowed: false,
        filesystem_read_only: true,
        gpu_allowed: false,
    }]);

    result.attempt_id = "attempt-2".into();
    let error = result
        .validate_against(&request, &registry)
        .expect_err("a result from another retry attempt must fail closed");
    assert_eq!(error.code, ValidationErrorCode::ResultBindingMismatch);
}

fn valid_registry(request: &GeneralComputeRequest) -> CapabilityMatrix {
    CapabilityMatrix::new(vec![BackendRegistration {
        backend_id: request.backend_id.clone(),
        execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
        guest_image_digest: request.guest_image_digest.clone(),
        capabilities: vec!["cpu".into()],
        max_threads: 1,
        network_allowed: false,
        filesystem_read_only: true,
        gpu_allowed: false,
    }])
}

fn valid_result(request: &GeneralComputeRequest) -> GeneralComputeResult {
    let mut result = GeneralComputeResult {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        status: ResultStatus::Completed,
        exit_code: Some(0),
        error_code: None,
        stdout: String::new(),
        stderr: String::new(),
        output_artifacts: Vec::new(),
        usage: Default::default(),
        runtime_version: request.runtime_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        input_sha256: "sha256:input".into(),
        determinism: request.determinism.clone(),
        capability_summary: vec!["cpu".into()],
        gpu_selection: None,
        output_manifest_root: String::new(),
        evidence: EvidenceEnvelope::default(),
    };
    result.output_manifest_root = canonical_artifact_root(&result.output_artifacts);
    result
}

#[test]
fn result_contract_carries_unverified_evidence_and_output_manifest_root() {
    let result = valid_result(&valid_request());
    let encoded = serde_json::to_value(result).expect("result serializes");
    assert!(
        encoded.get("evidence").is_some(),
        "result must carry an evidence envelope"
    );
    assert!(
        encoded
            .get("output_manifest_root")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "result must bind output artifacts to a manifest root"
    );
}

#[test]
fn result_validation_rejects_completed_with_nonzero_exit_code() {
    let request = valid_request();
    let mut result = valid_result(&request);
    result.exit_code = Some(7);

    let error = result
        .validate_against(&request, &valid_registry(&request))
        .expect_err("completed result with nonzero exit code must fail closed");
    assert_eq!(error.code, ValidationErrorCode::ResultStatusInvalid);
}

#[test]
fn result_validation_rejects_usage_claim_above_execution_policy() {
    let request = valid_request();
    let mut result = valid_result(&request);
    result.usage.cpu_time_ms = request.execution_policy.cpu_millis + 1;

    let error = result
        .validate_against(&request, &valid_registry(&request))
        .expect_err("usage claims above policy must fail closed");
    assert_eq!(error.code, ValidationErrorCode::UsageExceedsPolicy);
}

#[test]
fn result_validation_rejects_non_output_artifact_role() {
    let request = valid_request();
    let mut result = valid_result(&request);
    result.output_artifacts = vec![ArtifactManifest::inline_json(
        "wrong-role",
        ArtifactRole::Input,
        br#"{}"#,
    )];

    let error = result
        .validate_against(&request, &valid_registry(&request))
        .expect_err("output manifest with an input role must fail closed");
    assert_eq!(error.code, ValidationErrorCode::ArtifactInvalid);
}

fn chunked_artifact() -> ArtifactManifest {
    let bytes = b"abcdefgh";
    let mut artifact = ArtifactManifest::inline_json("chunked", ArtifactRole::Input, bytes);
    artifact.chunks = vec![
        ArtifactChunk {
            offset: 0,
            size_bytes: 4,
            sha256: sha256_digest(&bytes[..4]),
        },
        ArtifactChunk {
            offset: 4,
            size_bytes: 4,
            sha256: sha256_digest(&bytes[4..]),
        },
    ];
    artifact
}

#[test]
fn chunked_artifact_root_is_stable_across_inline_and_cas_forms() {
    let inline = chunked_artifact();
    inline.validate().expect("chunked inline artifact is valid");

    let mut cas = inline.clone();
    cas.inline_bytes = None;
    cas.validate().expect("CAS artifact is valid");

    assert_eq!(
        canonical_artifact_root(&[inline]),
        canonical_artifact_root(&[cas])
    );
}

#[test]
fn artifact_range_must_be_nonempty_and_chunk_aligned() {
    let artifact = chunked_artifact();
    let aligned = artifact
        .validate_range(ArtifactRange {
            offset: 4,
            size_bytes: 4,
        })
        .expect("aligned range should be resumable");
    assert_eq!(aligned.len(), 1);
    assert_eq!(aligned[0].offset, 4);

    let error = artifact
        .validate_range(ArtifactRange {
            offset: 2,
            size_bytes: 4,
        })
        .expect_err("partial chunk range must fail closed");
    assert!(error.contains("chunk-aligned"));

    let error = artifact
        .validate_range(ArtifactRange {
            offset: 0,
            size_bytes: 0,
        })
        .expect_err("empty range must fail closed");
    assert!(error.contains("non-empty"));
}

#[test]
fn artifact_resume_rejects_unknown_completed_chunk_and_returns_missing_chunks() {
    let artifact = chunked_artifact();
    let missing = artifact
        .missing_chunks(&[sha256_digest(b"abcd")])
        .expect("known completed chunk should be accepted");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].offset, 4);

    let error = artifact
        .missing_chunks(&[sha256_digest(b"unknown")])
        .expect_err("unknown completed chunk must fail closed");
    assert!(error.contains("not present"));
}

#[test]
fn inline_chunk_checksum_mismatch_is_rejected() {
    let mut artifact = chunked_artifact();
    artifact.chunks[0].sha256 = sha256_digest(b"wxyz");
    let error = artifact
        .validate()
        .expect_err("inline bytes must agree with chunk checksums");
    assert!(error.contains("chunk checksum does not match inline bytes"));
}

#[test]
fn artifact_manifest_rejects_tampered_inline_bytes() {
    let mut artifact = ArtifactManifest::inline_json("input", ArtifactRole::Input, br#"{"x":1}"#);
    artifact.inline_bytes = Some(br#"{"x":2}"#.to_vec());

    let error = artifact
        .validate()
        .expect_err("tampered bytes must fail validation");
    assert_eq!(error, "artifact checksum does not match bytes");
}

#[test]
fn request_validation_rejects_unbounded_or_writable_policy() {
    let mut request = valid_request();
    request.execution_policy.cpu_millis = u64::MAX;
    request.request_digest = request.canonical_request_digest();

    let error = request
        .validate()
        .expect_err("unbounded quota must fail closed");
    assert_eq!(error.code, ValidationErrorCode::PolicyInvalid);

    request.execution_policy = ExecutionPolicy::default();
    request.execution_policy.filesystem_read_only = false;
    request.request_digest = request.canonical_request_digest();
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
        execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
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
    request.execution_policy.gpu_requirement = Some(
        GpuRequirement::new(
            GpuVendor::Nvidia,
            "sm_80",
            GpuRuntime::Cuda,
            "550.54",
            16 * 1024 * 1024 * 1024,
            8,
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            false,
        )
        .expect("valid GPU requirement"),
    );
    request.request_digest = request.canonical_request_digest();
    let matrix = CapabilityMatrix::new(vec![BackendRegistration {
        backend_id: "python-numpy-scipy".into(),
        execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
        guest_image_digest:
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        capabilities: vec!["cpu".into()],
        max_threads: 4,
        network_allowed: false,
        filesystem_read_only: true,
        gpu_allowed: false,
    }]);
    let worker = WorkerCapabilities {
        guest_image_digests: vec![
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        ],
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
            sha256: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .into(),
        },
        ArtifactChunk {
            offset: 5,
            size_bytes: 3,
            sha256: "sha256:1111111111111111111111111111111111111111111111111111111111111111"
                .into(),
        },
    ];

    let error = artifact
        .validate()
        .expect_err("chunk gaps must fail closed");
    assert_eq!(error, "artifact chunks do not cover artifact bytes");
}

#[test]
fn artifact_manifest_rejects_oversized_metadata_before_cas_allocation() {
    let mut artifact = ArtifactManifest::inline_json("oversized", ArtifactRole::Input, b"x");
    artifact.inline_bytes = None;
    artifact.size_bytes = 1024 * 1024 * 1024 + 1;
    artifact.sha256 = sha256_digest(b"x");

    let error = artifact
        .validate()
        .expect_err("oversized artifact metadata must fail before materialization");
    assert_eq!(error, "artifact size exceeds the runtime limit");
}

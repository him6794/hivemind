use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, ExecutionPolicy, GeneralComputeRequest, GeneralComputeResult, ResultStatus,
};

#[test]
fn request_round_trip_preserves_versioned_execution_contract() {
    let request = GeneralComputeRequest {
        runtime_version: "general-compute-v1".into(),
        guest_image_digest: "sha256:guest".into(),
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
    assert_eq!(decoded.runtime_version, "general-compute-v1");
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
        runtime_version: "general-compute-v1".into(),
        backend_id: "python-numpy-scipy".into(),
        guest_image_digest: "sha256:guest".into(),
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

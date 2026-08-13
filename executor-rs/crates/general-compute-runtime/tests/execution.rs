use general_compute_runtime::artifact::{ArtifactMaterializer, CasChunkStore};
use general_compute_runtime::cp_python::{PythonBackendRegistration, PythonBackendRegistry};
use general_compute_runtime::execution::ReferenceBackendExecutor;
use general_compute_runtime::sandbox::BackendExecutionMode;
use general_compute_runtime::{
    sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRole, BackendRegistration,
    CapabilityMatrix, DeterminismPolicy, ExecutionPolicy, GeneralComputeRequest, ResultStatus,
    WorkerCapabilities, GENERAL_COMPUTE_RUNTIME_VERSION,
};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn image() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn source() -> ArtifactManifest {
    ArtifactManifest::inline_json(
        "source",
        ArtifactRole::Source,
        b"result = input['value'] + 1",
    )
}

fn input() -> ArtifactManifest {
    ArtifactManifest::inline_json("input", ArtifactRole::Input, br#"{"value":4}"#)
}

fn request() -> GeneralComputeRequest {
    let mut request = GeneralComputeRequest {
        execution_id: "execution-1".into(),
        attempt_id: "attempt-1".into(),
        idempotency_key: "idempotency-1".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: image(),
        backend_id: "python-cpython-312".into(),
        entrypoint: "main".into(),
        source_artifact: source(),
        input_artifacts: vec![input()],
        execution_policy: ExecutionPolicy::default(),
        determinism: DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn capability_matrix(request: &GeneralComputeRequest) -> CapabilityMatrix {
    CapabilityMatrix::new(vec![BackendRegistration {
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        capabilities: vec!["cpu".into()],
        max_threads: 2,
        network_allowed: false,
        filesystem_read_only: true,
        gpu_allowed: false,
    }])
}

fn worker() -> WorkerCapabilities {
    WorkerCapabilities {
        guest_image_digests: vec![image()],
        capabilities: vec!["cpu".into()],
        max_threads: 2,
        gpu_available: false,
    }
}

fn python_registry() -> PythonBackendRegistry {
    PythonBackendRegistry::new(vec![PythonBackendRegistration {
        backend_id: "python-cpython-312".into(),
        executable: "python".into(),
        runtime_version: "CPython 3.12.9".into(),
        guest_image_digest: image(),
        protocol_version: "general-compute-wire-v1".into(),
        max_output_bytes: 1024,
        execution_mode: BackendExecutionMode::ReferenceDirect,
    }])
    .expect("reference registry should be valid")
}

fn temp_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "hivemind-execution-{}-{suffix}",
        std::process::id()
    ))
}

fn cas_artifact(
    artifact_id: &str,
    role: ArtifactRole,
    bytes: &[u8],
) -> (ArtifactManifest, Vec<Vec<u8>>) {
    let split = bytes.len() / 2;
    let parts = vec![bytes[..split].to_vec(), bytes[split..].to_vec()];
    let artifact = ArtifactManifest {
        artifact_id: artifact_id.into(),
        role,
        size_bytes: bytes.len() as u64,
        mime_type: "text/plain".into(),
        sha256: sha256_digest(bytes),
        chunks: parts
            .iter()
            .enumerate()
            .map(|(index, chunk)| ArtifactChunk {
                offset: if index == 0 { 0 } else { split as u64 },
                size_bytes: chunk.len() as u64,
                sha256: sha256_digest(chunk),
            })
            .collect(),
        inline_bytes: None,
    };
    (artifact, parts)
}

#[test]
fn reference_backend_materializes_verified_artifacts_and_emits_a_validated_result() {
    let request = request();
    request.validate().expect("fixture request should be valid");
    capability_matrix(&request)
        .validate_request(&request, &worker())
        .expect("fixture capabilities should admit request");

    let root = temp_root();
    let materializer =
        ArtifactMaterializer::new(&root).expect("materialization root should be valid");
    let source = materializer
        .materialize(&request.source_artifact)
        .expect("source should materialize");
    let input = materializer
        .materialize(&request.input_artifacts[0])
        .expect("input should materialize");
    assert_eq!(
        std::fs::read(&source.path).unwrap(),
        request.source_artifact.inline_bytes.clone().unwrap()
    );
    assert_eq!(
        std::fs::read(&input.path).unwrap(),
        request.input_artifacts[0].inline_bytes.clone().unwrap()
    );

    let result =
        ReferenceBackendExecutor::new(capability_matrix(&request), worker(), python_registry())
            .execute(&request, &materializer)
            .expect("reference backend should execute the materialized bytes");
    assert_eq!(result.status, ResultStatus::Completed);
    assert_eq!(result.stdout, "5");
    assert_eq!(
        result.usage.input_bytes,
        request.input_artifacts[0].size_bytes
    );
    result
        .validate_against(&request, &capability_matrix(&request))
        .expect("reference result must satisfy the typed result contract");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reference_backend_rejects_non_main_entrypoints_instead_of_ignoring_them() {
    let mut request = request();
    request.entrypoint = "other:run".into();
    request.request_digest = request.canonical_request_digest();
    let root = temp_root();
    let materializer =
        ArtifactMaterializer::new(&root).expect("materialization root should be valid");
    let error =
        ReferenceBackendExecutor::new(capability_matrix(&request), worker(), python_registry())
            .execute(&request, &materializer)
            .expect_err("unsupported entrypoint must fail closed");
    assert!(matches!(
        error,
        general_compute_runtime::execution::ExecutionError::UnsupportedEntrypoint
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reference_backend_maps_source_exceptions_to_a_failed_typed_result() {
    let mut request = request();
    request.source_artifact = ArtifactManifest::inline_json(
        "source",
        ArtifactRole::Source,
        b"raise ValueError('bad input')",
    );
    request.request_digest = request.canonical_request_digest();
    let root = temp_root();
    let materializer =
        ArtifactMaterializer::new(&root).expect("materialization root should be valid");
    let result =
        ReferenceBackendExecutor::new(capability_matrix(&request), worker(), python_registry())
            .execute(&request, &materializer)
            .expect("source exceptions should become typed backend results");

    assert_eq!(result.status, ResultStatus::Failed);
    assert_eq!(result.exit_code, Some(1));
    assert_eq!(result.error_code.as_deref(), Some("backend_exception"));
    assert!(result.stdout.contains("ValueError"));
    result
        .validate_against(&request, &capability_matrix(&request))
        .expect("failed result must still satisfy the result contract");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reference_backend_executes_verified_cas_only_artifacts() {
    let source_bytes = b"result = input['value'] + 1";
    let input_bytes = br#"{"value":4}"#;
    let (source, source_chunks) = cas_artifact("source-cas", ArtifactRole::Source, source_bytes);
    let (input, input_chunks) = cas_artifact("input-cas", ArtifactRole::Input, input_bytes);
    let mut request = request();
    request.source_artifact = source;
    request.input_artifacts = vec![input];
    request.request_digest = request.canonical_request_digest();

    let root = temp_root();
    let cas_root = temp_root();
    let materializer =
        ArtifactMaterializer::new(&root).expect("materialization root should be valid");
    let store = CasChunkStore::new(&cas_root).expect("CAS root should be valid");
    for (artifact, chunks) in [
        (&request.source_artifact, source_chunks),
        (&request.input_artifacts[0], input_chunks),
    ] {
        for (manifest_chunk, bytes) in artifact.chunks.iter().zip(chunks) {
            store
                .put_chunk(&manifest_chunk.sha256, &bytes)
                .expect("verified CAS chunk should be accepted");
        }
    }

    let result =
        ReferenceBackendExecutor::new(capability_matrix(&request), worker(), python_registry())
            .execute_with_cas(&request, &materializer, &store)
            .expect("reference backend should execute verified CAS artifacts");
    assert_eq!(result.status, ResultStatus::Completed);
    assert_eq!(result.stdout, "5");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(cas_root);
}

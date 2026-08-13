use general_compute_runtime::artifact::CasChunkStore;
use general_compute_runtime::transport::{
    ChunkResumeEnvelope, ChunkTransportError, ChunkUploadEnvelope, MAX_CHUNK_UPLOAD_BYTES,
    ingest_chunk,
};
use general_compute_runtime::{
    ArtifactChunk, ArtifactManifest, ArtifactRole, ExecutionPolicy,
    GENERAL_COMPUTE_RUNTIME_VERSION, GeneralComputeRequest, sha256_digest,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_root(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "hivemind-transport-{label}-{}-{suffix}",
        std::process::id()
    ))
}

fn remove_root(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn chunked_source() -> (ArtifactManifest, &'static [u8]) {
    let bytes = b"print(42)";
    (
        ArtifactManifest {
            artifact_id: "source".into(),
            role: ArtifactRole::Source,
            size_bytes: bytes.len() as u64,
            mime_type: "text/plain".into(),
            sha256: sha256_digest(bytes),
            chunks: vec![ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(bytes),
            }],
            inline_bytes: None,
        },
        bytes,
    )
}

fn valid_request() -> (GeneralComputeRequest, &'static [u8]) {
    let (source_artifact, source_bytes) = chunked_source();
    let mut request = GeneralComputeRequest {
        execution_id: "execution-1".into(),
        attempt_id: "attempt-1".into(),
        idempotency_key: "idempotency-1".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest:
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        backend_id: "python-reference".into(),
        entrypoint: "main".into(),
        source_artifact,
        input_artifacts: vec![ArtifactManifest::inline_json(
            "input",
            ArtifactRole::Input,
            br#"{"x":1}"#,
        )],
        execution_policy: ExecutionPolicy::default(),
        determinism: Default::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-model-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    (request, source_bytes)
}

fn upload(request: &GeneralComputeRequest, bytes: &[u8]) -> ChunkUploadEnvelope {
    ChunkUploadEnvelope {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        artifact_id: "source".into(),
        offset: 0,
        size_bytes: bytes.len() as u64,
        sha256: sha256_digest(bytes),
        bytes: bytes.to_vec(),
    }
}

#[test]
fn chunk_upload_envelope_round_trips_all_binding_identity_fields() {
    let (request, bytes) = valid_request();
    let envelope = upload(&request, bytes);

    let encoded = serde_json::to_vec(&envelope).expect("chunk envelope serializes");
    let decoded: ChunkUploadEnvelope =
        serde_json::from_slice(&encoded).expect("chunk envelope decodes");
    assert_eq!(decoded, envelope);
    assert_eq!(decoded.execution_id, request.execution_id);
    assert_eq!(decoded.attempt_id, request.attempt_id);
    assert_eq!(decoded.idempotency_key, request.idempotency_key);
    assert_eq!(decoded.request_digest, request.request_digest);
}

#[test]
fn chunk_upload_accepts_verified_manifest_chunk_and_identical_retry() {
    let (request, bytes) = valid_request();
    let root = temporary_root("idempotent");
    let store = CasChunkStore::new(&root).expect("absolute CAS root is valid");
    let envelope = upload(&request, bytes);

    ingest_chunk(&store, &request, &envelope).expect("first chunk upload should succeed");
    ingest_chunk(&store, &request, &envelope)
        .expect("identical duplicate chunk upload should be idempotent");
    assert_eq!(
        fs::read(store.chunk_path(&envelope.sha256).unwrap()).unwrap(),
        bytes
    );

    remove_root(&root);
}

#[test]
fn chunk_upload_rejects_stale_attempt_and_wrong_request_digest() {
    let (request, bytes) = valid_request();
    let root = temporary_root("identity");
    let store = CasChunkStore::new(&root).expect("absolute CAS root is valid");

    let mut stale_attempt = upload(&request, bytes);
    stale_attempt.attempt_id = "attempt-previous".into();
    assert!(matches!(
        ingest_chunk(&store, &request, &stale_attempt),
        Err(ChunkTransportError::IdentityMismatch)
    ));

    let mut wrong_digest = upload(&request, bytes);
    wrong_digest.request_digest =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111".into();
    assert!(matches!(
        ingest_chunk(&store, &request, &wrong_digest),
        Err(ChunkTransportError::IdentityMismatch)
    ));

    remove_root(&root);
}

#[test]
fn chunk_upload_requires_exact_manifest_offset_size_and_digest() {
    let (request, bytes) = valid_request();
    let root = temporary_root("manifest-binding");
    let store = CasChunkStore::new(&root).expect("absolute CAS root is valid");

    let mut wrong_offset = upload(&request, bytes);
    wrong_offset.offset = 1;
    assert!(matches!(
        ingest_chunk(&store, &request, &wrong_offset),
        Err(ChunkTransportError::ManifestChunkMismatch)
    ));

    let mut wrong_size = upload(&request, bytes);
    wrong_size.size_bytes -= 1;
    assert!(matches!(
        ingest_chunk(&store, &request, &wrong_size),
        Err(ChunkTransportError::ManifestChunkMismatch)
    ));

    let mut wrong_digest = upload(&request, bytes);
    wrong_digest.sha256 = sha256_digest(b"different");
    assert!(matches!(
        ingest_chunk(&store, &request, &wrong_digest),
        Err(ChunkTransportError::ManifestChunkMismatch)
    ));

    remove_root(&root);
}

#[test]
fn chunk_upload_rejects_payload_size_and_digest_mismatch() {
    let (request, bytes) = valid_request();
    let root = temporary_root("payload");
    let store = CasChunkStore::new(&root).expect("absolute CAS root is valid");

    let mut wrong_bytes = upload(&request, bytes);
    wrong_bytes.bytes = b"tampered".to_vec();
    assert!(matches!(
        ingest_chunk(&store, &request, &wrong_bytes),
        Err(ChunkTransportError::ChunkSizeMismatch) | Err(ChunkTransportError::ChunkDigestMismatch)
    ));

    let mut oversized = upload(&request, bytes);
    oversized.size_bytes = (MAX_CHUNK_UPLOAD_BYTES as u64) + 1;
    assert!(matches!(
        ingest_chunk(&store, &request, &oversized),
        Err(ChunkTransportError::ChunkTooLarge)
    ));

    remove_root(&root);
}

#[test]
fn chunk_upload_rejects_a_tampered_existing_digest_object() {
    let (request, bytes) = valid_request();
    let root = temporary_root("conflict");
    let store = CasChunkStore::new(&root).expect("absolute CAS root is valid");
    let envelope = upload(&request, bytes);

    ingest_chunk(&store, &request, &envelope).expect("first chunk upload should succeed");
    fs::write(store.chunk_path(&envelope.sha256).unwrap(), b"tampered").unwrap();
    assert!(matches!(
        ingest_chunk(&store, &request, &envelope),
        Err(ChunkTransportError::ConflictingChunk)
    ));

    remove_root(&root);
}

#[test]
fn chunk_resume_envelope_is_identity_bound_and_returns_only_missing_manifest_chunks() {
    let (request, bytes) = valid_request();
    let resume = ChunkResumeEnvelope {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        artifact_id: "source".into(),
        completed_sha256: vec![],
    };

    let missing = resume
        .missing_chunks(&request)
        .expect("current attempt may resume source chunks");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].sha256, sha256_digest(bytes));

    let mut stale = resume.clone();
    stale.attempt_id = "attempt-previous".into();
    assert!(matches!(
        stale.missing_chunks(&request),
        Err(ChunkTransportError::IdentityMismatch)
    ));
}

use general_compute_runtime::artifact::{
    ArtifactMaterializationError, ArtifactMaterializer, CasChunkStore,
};
use general_compute_runtime::{
    ArtifactChunk, ArtifactManifest, ArtifactRole, DeterminismPolicy, ExecutionPolicy,
    sha256_digest,
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
        "hivemind-artifact-{label}-{}-{suffix}",
        std::process::id()
    ))
}

fn remove_root(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

#[test]
fn inline_artifact_materializer_writes_and_reuses_verified_bytes() {
    let root = temporary_root("idempotent");
    let artifact = ArtifactManifest::inline_json("source", ArtifactRole::Source, b"print(42)");
    let materializer = ArtifactMaterializer::new(&root).expect("absolute artifact root is valid");

    let first = materializer
        .materialize(&artifact)
        .expect("inline artifact should materialize");
    assert_eq!(first.path, materializer.root().join("source"));
    assert_eq!(fs::read(&first.path).unwrap(), b"print(42)");

    let second = materializer
        .materialize(&artifact)
        .expect("replaying the same artifact should be idempotent");
    assert_eq!(second.path, first.path);
    assert_eq!(fs::read(&second.path).unwrap(), b"print(42)");
    remove_root(&root);
}

#[test]
fn inline_artifact_materializer_rejects_path_traversal_and_symlink_targets() {
    let root = temporary_root("unsafe");
    let materializer = ArtifactMaterializer::new(&root).expect("absolute artifact root is valid");

    let traversal = ArtifactManifest::inline_json("../escape", ArtifactRole::Input, b"nope");
    assert!(matches!(
        materializer.materialize(&traversal),
        Err(ArtifactMaterializationError::UnsafeArtifactId)
    ));

    let outside = temporary_root("outside");
    fs::create_dir_all(&outside).unwrap();
    let target = outside.join("target");
    fs::write(&target, b"outside").unwrap();
    let linked = root.join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&target, &linked) {
        if error.raw_os_error() == Some(1314) {
            remove_root(&root);
            remove_root(&outside);
            return;
        }
        panic!("file symlink should be created: {error}");
    }

    let artifact = ArtifactManifest::inline_json("linked", ArtifactRole::Input, b"inside");
    assert!(matches!(
        materializer.materialize(&artifact),
        Err(ArtifactMaterializationError::SymlinkTarget)
    ));
    assert_eq!(fs::read(&target).unwrap(), b"outside");

    remove_root(&root);
    remove_root(&outside);
}

#[test]
fn artifact_materializer_fails_closed_when_content_is_not_local_inline_bytes() {
    let root = temporary_root("cas");
    let materializer = ArtifactMaterializer::new(&root).expect("absolute artifact root is valid");
    let artifact = ArtifactManifest {
        artifact_id: "cas-source".into(),
        role: ArtifactRole::Source,
        size_bytes: 4,
        mime_type: "text/plain".into(),
        sha256: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        chunks: vec![],
        inline_bytes: None,
    };

    assert!(matches!(
        materializer.materialize(&artifact),
        Err(ArtifactMaterializationError::ContentUnavailable)
    ));
    remove_root(&root);
}

fn chunked_artifact() -> (ArtifactManifest, Vec<&'static [u8]>) {
    let chunks = [b"abcd" as &[u8], b"efgh" as &[u8]];
    let bytes = chunks.concat();
    let artifact = ArtifactManifest {
        artifact_id: "chunked-source".into(),
        role: ArtifactRole::Source,
        size_bytes: bytes.len() as u64,
        mime_type: "text/plain".into(),
        sha256: sha256_digest(&bytes),
        chunks: chunks
            .iter()
            .enumerate()
            .map(|(index, chunk)| ArtifactChunk {
                offset: (index * 4) as u64,
                size_bytes: chunk.len() as u64,
                sha256: sha256_digest(chunk),
            })
            .collect(),
        inline_bytes: None,
    };
    (artifact, chunks.to_vec())
}

#[test]
fn cas_chunk_store_supports_verified_resume_and_materialization() {
    let root = temporary_root("cas-resume-artifact");
    let cas_root = temporary_root("cas-resume-store");
    let materializer = ArtifactMaterializer::new(&root).expect("absolute artifact root is valid");
    let store = CasChunkStore::new(&cas_root).expect("absolute CAS root is valid");
    let (artifact, chunks) = chunked_artifact();

    assert_eq!(store.missing_chunks(&artifact).unwrap().len(), 2);
    store
        .put_chunk(&artifact.chunks[0].sha256, chunks[0])
        .expect("first verified chunk should be stored");
    assert_eq!(store.missing_chunks(&artifact).unwrap().len(), 1);
    store
        .put_chunk(&artifact.chunks[1].sha256, chunks[1])
        .expect("second verified chunk should be stored");

    let materialized = materializer
        .materialize_with_cas(&artifact, &store)
        .expect("complete CAS artifact should materialize");
    assert_eq!(fs::read(&materialized.path).unwrap(), b"abcdefgh");
    assert_eq!(materialized.sha256, artifact.sha256);

    remove_root(&root);
    remove_root(&cas_root);
}

#[test]
fn cas_chunk_store_rejects_bad_or_tampered_chunks() {
    let cas_root = temporary_root("cas-invalid");
    let store = CasChunkStore::new(&cas_root).expect("absolute CAS root is valid");
    let digest = sha256_digest(b"expected");

    assert!(matches!(
        store.put_chunk(&digest, b"tampered"),
        Err(ArtifactMaterializationError::ChunkChecksumMismatch)
    ));

    store
        .put_chunk(&digest, b"expected")
        .expect("verified chunk should be stored");
    let (artifact, _) = chunked_artifact();
    let path = store
        .chunk_path(&artifact.chunks[0].sha256)
        .expect("digest path is safe");
    store
        .put_chunk(&artifact.chunks[0].sha256, b"abcd")
        .expect("artifact chunk should be stored");
    fs::write(path, b"tampered-on-disk").unwrap();
    assert!(matches!(
        store.missing_chunks(&artifact),
        Err(ArtifactMaterializationError::ChunkChecksumMismatch)
    ));

    remove_root(&cas_root);
}

#[test]
fn cas_transfer_state_survives_store_recreation_and_attempt_rotation() {
    let cas_root = temporary_root("cas-transfer-state");
    let (artifact, chunks) = chunked_artifact();
    let execution_id = "execution-stable";

    let store = CasChunkStore::new(&cas_root).expect("absolute CAS root is valid");
    store
        .prepare_transfer(execution_id, &artifact)
        .expect("transfer manifest should be persisted");
    assert_eq!(
        store
            .missing_transfer_chunks(execution_id, &artifact)
            .expect("fresh transfer should be resumable")
            .len(),
        2
    );
    store
        .put_transfer_chunk(execution_id, &artifact, &artifact.chunks[0], chunks[0])
        .expect("verified transfer chunk should be recorded");

    drop(store);
    let recreated = CasChunkStore::new(&cas_root).expect("CAS state should be reopenable");
    // A retry rotates attempt_id but keeps execution_id and the immutable
    // artifact identity, so the durable state must remain available.
    recreated
        .prepare_transfer(execution_id, &artifact)
        .expect("same artifact may be prepared by a later attempt");
    let missing = recreated
        .missing_transfer_chunks(execution_id, &artifact)
        .expect("recreated store should resume from durable state");
    assert_eq!(missing, vec![artifact.chunks[1].clone()]);

    recreated
        .put_transfer_chunk(execution_id, &artifact, &artifact.chunks[1], chunks[1])
        .expect("second verified transfer chunk should be recorded");
    assert!(
        recreated
            .missing_transfer_chunks(execution_id, &artifact)
            .expect("complete transfer should remain readable")
            .is_empty()
    );

    remove_root(&cas_root);
}

#[test]
fn cas_transfer_state_rejects_manifest_redefinition_for_stable_execution() {
    let cas_root = temporary_root("cas-transfer-conflict");
    let (artifact, _) = chunked_artifact();
    let store = CasChunkStore::new(&cas_root).expect("absolute CAS root is valid");
    store
        .prepare_transfer("execution-stable", &artifact)
        .expect("initial transfer manifest should be persisted");

    let mut changed = artifact;
    changed.sha256 = sha256_digest(b"different-artifact");
    assert!(matches!(
        store.prepare_transfer("execution-stable", &changed),
        Err(ArtifactMaterializationError::TransferStateMismatch)
    ));

    remove_root(&cas_root);
}

#[test]
fn cas_transfer_state_reconciles_a_store_recreated_after_adapter_upload() {
    let cas_root = temporary_root("cas-transfer-adapter-restart");
    let (artifact, chunks) = chunked_artifact();
    let request = {
        let mut request = general_compute_runtime::GeneralComputeRequest {
            execution_id: "execution-adapter".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-adapter".into(),
            request_digest: String::new(),
            runtime_version: general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest: format!("sha256:{}", "a".repeat(64)),
            backend_id: "python-reference".into(),
            entrypoint: "main".into(),
            source_artifact: artifact.clone(),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        request
    };
    let envelope = general_compute_runtime::transport::ChunkUploadEnvelope {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        artifact_id: artifact.artifact_id.clone(),
        offset: artifact.chunks[0].offset,
        size_bytes: artifact.chunks[0].size_bytes,
        sha256: artifact.chunks[0].sha256.clone(),
        bytes: chunks[0].to_vec(),
    };

    let first = CasChunkStore::new(&cas_root).expect("absolute CAS root is valid");
    general_compute_runtime::transport::ingest_chunk(&first, &request, &envelope)
        .expect("authenticated adapter upload should persist the first chunk");
    drop(first);

    let recreated = CasChunkStore::new(&cas_root).expect("CAS state should be reopenable");
    let missing = recreated
        .missing_transfer_chunks(&request.execution_id, &artifact)
        .expect("adapter upload should leave durable resume state");
    assert_eq!(missing, vec![artifact.chunks[1].clone()]);

    remove_root(&cas_root);
}

#[test]
fn cas_transfer_state_fails_closed_when_a_completion_marker_is_corrupt() {
    let cas_root = temporary_root("cas-transfer-corrupt-marker");
    let (artifact, chunks) = chunked_artifact();
    let store = CasChunkStore::new(&cas_root).expect("absolute CAS root is valid");
    store
        .put_transfer_chunk(
            "execution-corrupt",
            &artifact,
            &artifact.chunks[0],
            chunks[0],
        )
        .expect("first transfer chunk should be persisted");

    let marker = fs::read_dir(cas_root.join(".transfers"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("complete"))
        .expect("completion marker should exist");
    fs::write(&marker, b"sha256:corrupt").unwrap();

    // Remove the CAS object as well: a corrupt marker must not be silently
    // ignored just because the corresponding chunk is already missing.
    fs::remove_file(store.chunk_path(&artifact.chunks[0].sha256).unwrap()).unwrap();
    assert!(matches!(
        store.missing_transfer_chunks("execution-corrupt", &artifact),
        Err(ArtifactMaterializationError::TransferStateCorrupt)
    ));

    remove_root(&cas_root);
}

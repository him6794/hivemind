use general_compute_runtime::artifact::{
    ArtifactMaterializationError, ArtifactMaterializer, CasChunkStore,
};
use general_compute_runtime::{sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRole};
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
    std::os::windows::fs::symlink_file(&target, &linked).unwrap();

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

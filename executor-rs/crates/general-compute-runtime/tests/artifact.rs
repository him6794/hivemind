use general_compute_runtime::artifact::{ArtifactMaterializationError, ArtifactMaterializer};
use general_compute_runtime::{ArtifactManifest, ArtifactRole};
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

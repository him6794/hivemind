//! Local materialization for already-verified inline runtime artifacts.
//!
//! This module deliberately does not fetch arbitrary URLs, interpret CAS
//! paths, or invoke a host command.  Network/CAS transfer belongs to a later
//! control-plane checkpoint; the first execution path only accepts bytes that
//! arrived inside a validated [`ArtifactManifest`].

use crate::{ArtifactManifest, sha256_digest};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactMaterializationError {
    RootMustBeAbsolute,
    RootSymlink,
    UnsafeArtifactId,
    ManifestInvalid(String),
    ContentUnavailable,
    SymlinkTarget,
    ExistingContentMismatch,
    Io(String),
}

impl std::fmt::Display for ArtifactMaterializationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "artifact materialization failed: {self:?}")
    }
}

impl std::error::Error for ArtifactMaterializationError {}

/// The path and immutable content identity of a materialized artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedArtifact {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Materializes only inline bytes below one operator-selected directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMaterializer {
    root: PathBuf,
}

impl ArtifactMaterializer {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ArtifactMaterializationError> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(ArtifactMaterializationError::RootMustBeAbsolute);
        }
        if let Ok(metadata) = fs::symlink_metadata(root) {
            if metadata.file_type().is_symlink() {
                return Err(ArtifactMaterializationError::RootSymlink);
            }
            if !metadata.is_dir() {
                return Err(ArtifactMaterializationError::Io(
                    "artifact root is not a directory".into(),
                ));
            }
        } else {
            fs::create_dir_all(root).map_err(io_error)?;
        }
        let canonical_root = fs::canonicalize(root).map_err(io_error)?;
        if fs::symlink_metadata(&canonical_root)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(ArtifactMaterializationError::RootSymlink);
        }
        Ok(Self {
            root: canonical_root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn materialize(
        &self,
        artifact: &ArtifactManifest,
    ) -> Result<MaterializedArtifact, ArtifactMaterializationError> {
        artifact
            .validate()
            .map_err(ArtifactMaterializationError::ManifestInvalid)?;
        let bytes = artifact
            .inline_bytes
            .as_deref()
            .ok_or(ArtifactMaterializationError::ContentUnavailable)?;
        let file_name = safe_artifact_id(&artifact.artifact_id)?;
        let path = self.root.join(file_name);

        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() {
                return Err(ArtifactMaterializationError::SymlinkTarget);
            }
            if !metadata.is_file() {
                return Err(ArtifactMaterializationError::ExistingContentMismatch);
            }
            let existing = fs::read(&path).map_err(io_error)?;
            if existing == bytes {
                return Ok(materialized(path, bytes));
            }
            return Err(ArtifactMaterializationError::ExistingContentMismatch);
        }

        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return self.materialize(artifact);
            }
            Err(error) => return Err(io_error(error)),
        };
        if let Err(error) = write_and_sync(&mut file, bytes) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(io_error(error));
        }
        Ok(materialized(path, bytes))
    }
}

fn safe_artifact_id(value: &str) -> Result<&str, ArtifactMaterializationError> {
    if value.trim().is_empty()
        || value == "."
        || value == ".."
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_' | '.')
        })
        || Path::new(value)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ArtifactMaterializationError::UnsafeArtifactId);
    }
    Ok(value)
}

fn materialized(path: PathBuf, bytes: &[u8]) -> MaterializedArtifact {
    MaterializedArtifact {
        path,
        size_bytes: bytes.len() as u64,
        sha256: sha256_digest(bytes),
    }
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

fn io_error(error: std::io::Error) -> ArtifactMaterializationError {
    ArtifactMaterializationError::Io(error.to_string())
}

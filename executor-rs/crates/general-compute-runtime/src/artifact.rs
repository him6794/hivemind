//! Local materialization for already-verified inline runtime artifacts.
//!
//! This module deliberately does not fetch arbitrary URLs, invoke a host
//! command, or decide how a network transfer is authenticated. Network/CAS
//! transfer belongs to the control-plane boundary; this module only accepts
//! validated inline bytes or chunks explicitly submitted to an operator-rooted
//! local [`CasChunkStore`].

use crate::{sha256_digest, ArtifactManifest};
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
    ChunkChecksumMismatch,
    ChunkMissing,
    InvalidChunkDigest,
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

/// A local, operator-rooted content-addressed chunk store.
///
/// The store is intentionally transport-agnostic: callers must obtain chunks
/// through an authenticated transfer protocol and then submit the bytes here.
/// Every write and read is checked against the manifest's SHA-256 digest before
/// a chunk can be used for materialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasChunkStore {
    root: PathBuf,
}

impl CasChunkStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ArtifactMaterializationError> {
        Ok(Self {
            root: canonical_root(root.as_ref())?,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn chunk_path(&self, digest: &str) -> Result<PathBuf, ArtifactMaterializationError> {
        let hex = digest
            .strip_prefix("sha256:")
            .filter(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or(ArtifactMaterializationError::InvalidChunkDigest)?;
        Ok(self.root.join(hex))
    }

    /// Store one verified chunk, preserving an identical existing object.
    pub fn put_chunk(
        &self,
        digest: &str,
        bytes: &[u8],
    ) -> Result<(), ArtifactMaterializationError> {
        let path = self.chunk_path(digest)?;
        if sha256_digest(bytes) != digest {
            return Err(ArtifactMaterializationError::ChunkChecksumMismatch);
        }

        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() {
                return Err(ArtifactMaterializationError::SymlinkTarget);
            }
            if !metadata.is_file() {
                return Err(ArtifactMaterializationError::ChunkChecksumMismatch);
            }
            let existing = fs::read(&path).map_err(io_error)?;
            return if existing == bytes {
                Ok(())
            } else {
                Err(ArtifactMaterializationError::ChunkChecksumMismatch)
            };
        }

        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return self.put_chunk(digest, bytes);
            }
            Err(error) => return Err(io_error(error)),
        };
        if let Err(error) = write_and_sync(&mut file, bytes) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(io_error(error));
        }
        Ok(())
    }

    /// Return chunks absent from this store, rejecting any corrupted object.
    pub fn missing_chunks(
        &self,
        artifact: &ArtifactManifest,
    ) -> Result<Vec<crate::ArtifactChunk>, ArtifactMaterializationError> {
        artifact
            .validate()
            .map_err(ArtifactMaterializationError::ManifestInvalid)?;
        if artifact.chunks.is_empty() {
            return Err(ArtifactMaterializationError::ContentUnavailable);
        }

        let mut missing = Vec::new();
        for chunk in &artifact.chunks {
            let path = self.chunk_path(&chunk.sha256)?;
            match self.read_verified(&path, &chunk.sha256)? {
                Some(_) => {}
                None => missing.push(chunk.clone()),
            }
        }
        Ok(missing)
    }

    fn read_artifact(
        &self,
        artifact: &ArtifactManifest,
    ) -> Result<Vec<u8>, ArtifactMaterializationError> {
        artifact
            .validate()
            .map_err(ArtifactMaterializationError::ManifestInvalid)?;
        if artifact.chunks.is_empty() {
            return Err(ArtifactMaterializationError::ContentUnavailable);
        }

        let mut bytes = Vec::with_capacity(artifact.size_bytes as usize);
        for chunk in &artifact.chunks {
            let path = self.chunk_path(&chunk.sha256)?;
            let Some(chunk_bytes) = self.read_verified(&path, &chunk.sha256)? else {
                return Err(ArtifactMaterializationError::ChunkMissing);
            };
            if chunk_bytes.len() as u64 != chunk.size_bytes {
                return Err(ArtifactMaterializationError::ChunkChecksumMismatch);
            }
            bytes.extend_from_slice(&chunk_bytes);
        }
        if bytes.len() as u64 != artifact.size_bytes || sha256_digest(&bytes) != artifact.sha256 {
            return Err(ArtifactMaterializationError::ChunkChecksumMismatch);
        }
        Ok(bytes)
    }

    fn read_verified(
        &self,
        path: &Path,
        digest: &str,
    ) -> Result<Option<Vec<u8>>, ArtifactMaterializationError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        if metadata.file_type().is_symlink() {
            return Err(ArtifactMaterializationError::SymlinkTarget);
        }
        if !metadata.is_file() {
            return Err(ArtifactMaterializationError::ChunkChecksumMismatch);
        }
        let bytes = fs::read(path).map_err(io_error)?;
        if sha256_digest(&bytes) != digest {
            return Err(ArtifactMaterializationError::ChunkChecksumMismatch);
        }
        Ok(Some(bytes))
    }
}

impl ArtifactMaterializer {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ArtifactMaterializationError> {
        Ok(Self {
            root: canonical_root(root.as_ref())?,
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

    /// Materialize a complete artifact from verified local CAS chunks.
    pub fn materialize_with_cas(
        &self,
        artifact: &ArtifactManifest,
        store: &CasChunkStore,
    ) -> Result<MaterializedArtifact, ArtifactMaterializationError> {
        artifact
            .validate()
            .map_err(ArtifactMaterializationError::ManifestInvalid)?;
        if let Some(bytes) = artifact.inline_bytes.as_deref() {
            return self.materialize_bytes(artifact, bytes);
        }
        let bytes = store.read_artifact(artifact)?;
        self.materialize_bytes(artifact, &bytes)
    }

    fn materialize_bytes(
        &self,
        artifact: &ArtifactManifest,
        bytes: &[u8],
    ) -> Result<MaterializedArtifact, ArtifactMaterializationError> {
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
                return self.materialize_bytes(artifact, bytes);
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

fn canonical_root(root: &Path) -> Result<PathBuf, ArtifactMaterializationError> {
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
    Ok(canonical_root)
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

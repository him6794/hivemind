//! Local materialization for already-verified inline runtime artifacts.
//!
//! This module deliberately does not fetch arbitrary URLs, invoke a host
//! command, or decide how a network transfer is authenticated. Network/CAS
//! transfer belongs to the control-plane boundary; this module only accepts
//! validated inline bytes or chunks explicitly submitted to an operator-rooted
//! local [`CasChunkStore`].

use crate::{ArtifactManifest, sha256_digest};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

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
    TransferIdentityInvalid,
    TransferStateMismatch,
    TransferStateCorrupt,
    TransferChunkMismatch,
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

/// The immutable part of a transfer is persisted below the operator CAS
/// root.  Attempt ids are deliberately absent: retries of one execution must
/// resume the same artifact, while a changed manifest cannot redefine it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransferManifest {
    execution_id: String,
    artifact_id: String,
    size_bytes: u64,
    sha256: String,
    chunks: Vec<crate::ArtifactChunk>,
}

impl CasChunkStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ArtifactMaterializationError> {
        let root = canonical_root(root.as_ref())?;
        let transfer_root = root.join(".transfers");
        if let Ok(metadata) = fs::symlink_metadata(&transfer_root) {
            if metadata.file_type().is_symlink() {
                return Err(ArtifactMaterializationError::SymlinkTarget);
            }
            if !metadata.is_dir() {
                return Err(ArtifactMaterializationError::Io(
                    "CAS transfer state root is not a directory".into(),
                ));
            }
        } else {
            fs::create_dir_all(&transfer_root).map_err(|error| io_error(&error))?;
        }
        Ok(Self { root })
    }

    #[must_use]
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
            let existing = fs::read(&path).map_err(|error| io_error(&error))?;
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
            Err(error) => return Err(io_error(&error)),
        };
        if let Err(error) = write_and_sync(&mut file, bytes) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(io_error(&error));
        }
        Ok(())
    }

    /// Persist the immutable identity for one execution/artifact transfer.
    /// This is safe to call again after a Worker restart or retry attempt.
    pub fn prepare_transfer(
        &self,
        execution_id: &str,
        artifact: &ArtifactManifest,
    ) -> Result<(), ArtifactMaterializationError> {
        validate_transfer_execution_id(execution_id)?;
        artifact
            .validate()
            .map_err(ArtifactMaterializationError::ManifestInvalid)?;
        if artifact.chunks.is_empty() {
            return Err(ArtifactMaterializationError::ContentUnavailable);
        }
        let expected = TransferManifest {
            execution_id: execution_id.to_owned(),
            artifact_id: artifact.artifact_id.clone(),
            size_bytes: artifact.size_bytes,
            sha256: artifact.sha256.clone(),
            chunks: artifact.chunks.clone(),
        };
        let path = self.transfer_manifest_path(execution_id, artifact)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let existing: TransferManifest = serde_json::from_slice(&bytes)
                    .map_err(|_| ArtifactMaterializationError::TransferStateCorrupt)?;
                if existing == expected {
                    Ok(())
                } else {
                    Err(ArtifactMaterializationError::TransferStateMismatch)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let bytes = serde_json::to_vec(&expected)
                    .map_err(|error| ArtifactMaterializationError::Io(error.to_string()))?;
                let file = OpenOptions::new().write(true).create_new(true).open(&path);
                match file {
                    Ok(mut file) => {
                        write_and_sync(&mut file, &bytes).map_err(|error| io_error(&error))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let existing = fs::read(&path).map_err(|error| io_error(&error))?;
                        let existing: TransferManifest = serde_json::from_slice(&existing)
                            .map_err(|_| ArtifactMaterializationError::TransferStateCorrupt)?;
                        if existing == expected {
                            Ok(())
                        } else {
                            Err(ArtifactMaterializationError::TransferStateMismatch)
                        }
                    }
                    Err(error) => Err(io_error(&error)),
                }
            }
            Err(error) => Err(io_error(&error)),
        }
    }

    /// Store a verified chunk and durably record its availability for the
    /// execution/artifact transfer. The marker is create-new and therefore
    /// idempotent; a conflicting marker fails closed.
    pub fn put_transfer_chunk(
        &self,
        execution_id: &str,
        artifact: &ArtifactManifest,
        chunk: &crate::ArtifactChunk,
        bytes: &[u8],
    ) -> Result<(), ArtifactMaterializationError> {
        self.prepare_transfer(execution_id, artifact)?;
        let manifest_chunk = artifact
            .chunks
            .iter()
            .find(|candidate| *candidate == chunk)
            .ok_or(ArtifactMaterializationError::TransferChunkMismatch)?;
        if bytes.len() as u64 != manifest_chunk.size_bytes
            || sha256_digest(bytes) != manifest_chunk.sha256
        {
            return Err(ArtifactMaterializationError::ChunkChecksumMismatch);
        }
        self.put_chunk(&manifest_chunk.sha256, bytes)?;

        let marker = self.transfer_marker_path(execution_id, artifact, manifest_chunk)?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
        {
            Ok(mut file) => write_and_sync(&mut file, manifest_chunk.sha256.as_bytes())
                .map_err(|error| io_error(&error)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read(&marker).map_err(|error| io_error(&error))?;
                if existing == manifest_chunk.sha256.as_bytes() {
                    Ok(())
                } else {
                    Err(ArtifactMaterializationError::TransferStateCorrupt)
                }
            }
            Err(error) => Err(io_error(&error)),
        }
    }

    /// Return missing chunks after reopening durable transfer state. CAS bytes
    /// remain the source of truth; markers are reconciled from verified CAS
    /// objects so a crash between the object write and marker write is safe.
    pub fn missing_transfer_chunks(
        &self,
        execution_id: &str,
        artifact: &ArtifactManifest,
    ) -> Result<Vec<crate::ArtifactChunk>, ArtifactMaterializationError> {
        self.prepare_transfer(execution_id, artifact)?;
        let missing = self.missing_chunks(artifact)?;
        let missing_digests: std::collections::HashSet<_> =
            missing.iter().map(|chunk| chunk.sha256.as_str()).collect();
        for chunk in &artifact.chunks {
            let marker = self.transfer_marker_path(execution_id, artifact, chunk)?;
            match fs::read(&marker) {
                Ok(existing) if existing == chunk.sha256.as_bytes() => {}
                Ok(_) => return Err(ArtifactMaterializationError::TransferStateCorrupt),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if missing_digests.contains(chunk.sha256.as_str()) {
                        continue;
                    }
                    let file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&marker);
                    match file {
                        Ok(mut file) => {
                            write_and_sync(&mut file, chunk.sha256.as_bytes())
                                .map_err(|error| io_error(&error))?;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            let existing = fs::read(&marker).map_err(|error| io_error(&error))?;
                            if existing != chunk.sha256.as_bytes() {
                                return Err(ArtifactMaterializationError::TransferStateCorrupt);
                            }
                        }
                        Err(error) => return Err(io_error(&error)),
                    }
                }
                Err(error) => return Err(io_error(&error)),
            }
        }
        Ok(missing)
    }

    fn transfer_manifest_path(
        &self,
        execution_id: &str,
        artifact: &ArtifactManifest,
    ) -> Result<PathBuf, ArtifactMaterializationError> {
        let key = transfer_key(execution_id, &artifact.artifact_id);
        Ok(self.transfer_root()?.join(format!("{key}.manifest.json")))
    }

    fn transfer_marker_path(
        &self,
        execution_id: &str,
        artifact: &ArtifactManifest,
        chunk: &crate::ArtifactChunk,
    ) -> Result<PathBuf, ArtifactMaterializationError> {
        let key = transfer_key(execution_id, &artifact.artifact_id);
        let digest = chunk
            .sha256
            .strip_prefix("sha256:")
            .ok_or(ArtifactMaterializationError::InvalidChunkDigest)?;
        Ok(self
            .transfer_root()?
            .join(format!("{key}.{digest}.complete")))
    }

    fn transfer_root(&self) -> Result<PathBuf, ArtifactMaterializationError> {
        let path = self.root.join(".transfers");
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&error))?;
        if metadata.file_type().is_symlink() {
            return Err(ArtifactMaterializationError::SymlinkTarget);
        }
        if !metadata.is_dir() {
            return Err(ArtifactMaterializationError::Io(
                "CAS transfer state root is not a directory".into(),
            ));
        }
        Ok(path)
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
            match Self::read_verified(&path, &chunk.sha256)? {
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

        let capacity = usize::try_from(artifact.size_bytes).map_err(|_| {
            ArtifactMaterializationError::ManifestInvalid(
                "artifact size does not fit in the host address space".into(),
            )
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        for chunk in &artifact.chunks {
            let path = self.chunk_path(&chunk.sha256)?;
            let Some(chunk_bytes) = Self::read_verified(&path, &chunk.sha256)? else {
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
        path: &Path,
        digest: &str,
    ) -> Result<Option<Vec<u8>>, ArtifactMaterializationError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(&error)),
        };
        if metadata.file_type().is_symlink() {
            return Err(ArtifactMaterializationError::SymlinkTarget);
        }
        if !metadata.is_file() {
            return Err(ArtifactMaterializationError::ChunkChecksumMismatch);
        }
        let bytes = fs::read(path).map_err(|error| io_error(&error))?;
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

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn materialize(
        &self,
        artifact: &ArtifactManifest,
    ) -> Result<MaterializedArtifact, ArtifactMaterializationError> {
        // Preserve the materializer's path-specific error even though the
        // manifest validator also rejects unsafe IDs.
        let file_name = safe_artifact_id(&artifact.artifact_id)?;
        artifact
            .validate()
            .map_err(ArtifactMaterializationError::ManifestInvalid)?;
        let bytes = artifact
            .inline_bytes
            .as_deref()
            .ok_or(ArtifactMaterializationError::ContentUnavailable)?;
        let path = self.root.join(file_name);

        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() {
                return Err(ArtifactMaterializationError::SymlinkTarget);
            }
            if !metadata.is_file() {
                return Err(ArtifactMaterializationError::ExistingContentMismatch);
            }
            let existing = fs::read(&path).map_err(|error| io_error(&error))?;
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
            Err(error) => return Err(io_error(&error)),
        };
        if let Err(error) = write_and_sync(&mut file, bytes) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(io_error(&error));
        }
        Ok(materialized(path, bytes))
    }

    /// Materialize a complete artifact from verified local CAS chunks.
    pub fn materialize_with_cas(
        &self,
        artifact: &ArtifactManifest,
        store: &CasChunkStore,
    ) -> Result<MaterializedArtifact, ArtifactMaterializationError> {
        // Keep path validation ahead of manifest validation so callers receive
        // the materializer-specific unsafe-ID error consistently.
        safe_artifact_id(&artifact.artifact_id)?;
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
            let existing = fs::read(&path).map_err(|error| io_error(&error))?;
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
            Err(error) => return Err(io_error(&error)),
        };
        if let Err(error) = write_and_sync(&mut file, bytes) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(io_error(&error));
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
        fs::create_dir_all(root).map_err(|error| io_error(&error))?;
    }
    let canonical_root = fs::canonicalize(root).map_err(|error| io_error(&error))?;
    if fs::symlink_metadata(&canonical_root)
        .map_err(|error| io_error(&error))?
        .file_type()
        .is_symlink()
    {
        return Err(ArtifactMaterializationError::RootSymlink);
    }
    Ok(canonical_root)
}

fn safe_artifact_id(value: &str) -> Result<&str, ArtifactMaterializationError> {
    crate::validate_artifact_id(value)
        .map_err(|_| ArtifactMaterializationError::UnsafeArtifactId)?;
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

fn io_error(error: &std::io::Error) -> ArtifactMaterializationError {
    ArtifactMaterializationError::Io(error.to_string())
}

fn validate_transfer_execution_id(value: &str) -> Result<(), ArtifactMaterializationError> {
    if value.trim().is_empty()
        || value.len() > 256
        || value.chars().any(|character| character == '\0')
    {
        return Err(ArtifactMaterializationError::TransferIdentityInvalid);
    }
    Ok(())
}

fn transfer_key(execution_id: &str, artifact_id: &str) -> String {
    sha256_digest(format!("general-compute-transfer-v1\0{execution_id}\0{artifact_id}").as_bytes())
        .strip_prefix("sha256:")
        .unwrap_or_default()
        .to_owned()
}

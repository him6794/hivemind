//! Identity-bound chunk upload and resume contracts.
//!
//! This module stops at the local CAS boundary. It validates the control-plane
//! request identity and the exact manifest chunk before passing bytes to
//! [`crate::artifact::CasChunkStore`]. A network protocol may wrap these types,
//! but it must not replace them with unbound raw-byte uploads.

use crate::artifact::{ArtifactMaterializationError, CasChunkStore};
use crate::{ArtifactChunk, ArtifactManifest, GeneralComputeRequest, sha256_digest};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Maximum raw payload accepted in one chunk upload.
pub const MAX_CHUNK_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

/// A chunk upload bound to one execution attempt and one manifest chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkUploadEnvelope {
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub artifact_id: String,
    pub offset: u64,
    pub size_bytes: u64,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// A request to resume one artifact's missing manifest chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkResumeEnvelope {
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub artifact_id: String,
    pub completed_sha256: Vec<String>,
}

impl ChunkResumeEnvelope {
    /// Return manifest chunks not listed as completed by the current attempt.
    pub fn missing_chunks(
        &self,
        request: &GeneralComputeRequest,
    ) -> Result<Vec<ArtifactChunk>, ChunkTransportError> {
        request
            .validate()
            .map_err(|error| ChunkTransportError::RequestInvalid(error.message))?;
        validate_request_identity(request, &self.identity())?;
        let artifact = find_artifact(request, &self.artifact_id)?;
        artifact
            .missing_chunks(&self.completed_sha256)
            .map_err(ChunkTransportError::ManifestInvalid)
    }

    fn identity(&self) -> ChunkIdentity<'_> {
        ChunkIdentity {
            execution_id: &self.execution_id,
            attempt_id: &self.attempt_id,
            idempotency_key: &self.idempotency_key,
            request_digest: &self.request_digest,
        }
    }
}

/// Errors returned before an untrusted chunk can enter the local CAS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkTransportError {
    RequestInvalid(String),
    IdentityMismatch,
    ArtifactNotFound,
    ManifestInvalid(String),
    ManifestChunkMismatch,
    ChunkTooLarge,
    ChunkSizeMismatch,
    ChunkDigestMismatch,
    ConflictingChunk,
    Storage(ArtifactMaterializationError),
}

impl fmt::Display for ChunkTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestInvalid(message) => {
                write!(formatter, "chunk request is invalid: {message}")
            }
            Self::IdentityMismatch => {
                formatter.write_str("chunk identity does not match the current request")
            }
            Self::ArtifactNotFound => formatter.write_str("chunk artifact is not in the request"),
            Self::ManifestInvalid(message) => {
                write!(formatter, "chunk manifest is invalid: {message}")
            }
            Self::ManifestChunkMismatch => {
                formatter.write_str("chunk does not match a manifest chunk")
            }
            Self::ChunkTooLarge => formatter.write_str("chunk exceeds the upload limit"),
            Self::ChunkSizeMismatch => formatter.write_str("chunk size does not match its bytes"),
            Self::ChunkDigestMismatch => {
                formatter.write_str("chunk digest does not match its bytes")
            }
            Self::ConflictingChunk => {
                formatter.write_str("an existing CAS chunk has conflicting bytes")
            }
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ChunkTransportError {}

/// Validate and ingest one chunk into the operator-owned local CAS.
pub fn ingest_chunk(
    store: &CasChunkStore,
    request: &GeneralComputeRequest,
    envelope: &ChunkUploadEnvelope,
) -> Result<(), ChunkTransportError> {
    request
        .validate()
        .map_err(|error| ChunkTransportError::RequestInvalid(error.message))?;
    validate_request_identity(request, &envelope.identity())?;

    if envelope.size_bytes > MAX_CHUNK_UPLOAD_BYTES as u64
        || envelope.bytes.len() > MAX_CHUNK_UPLOAD_BYTES
    {
        return Err(ChunkTransportError::ChunkTooLarge);
    }
    let artifact = find_artifact(request, &envelope.artifact_id)?;
    let manifest_chunk = artifact
        .chunks
        .iter()
        .find(|chunk| {
            chunk.offset == envelope.offset
                && chunk.size_bytes == envelope.size_bytes
                && chunk.sha256 == envelope.sha256
        })
        .ok_or(ChunkTransportError::ManifestChunkMismatch)?;
    if envelope.size_bytes != envelope.bytes.len() as u64 {
        return Err(ChunkTransportError::ChunkSizeMismatch);
    }
    if usize::try_from(manifest_chunk.size_bytes).ok() != Some(envelope.bytes.len())
        || sha256_digest(&envelope.bytes) != manifest_chunk.sha256
    {
        return Err(ChunkTransportError::ChunkDigestMismatch);
    }

    store
        .put_transfer_chunk(
            &envelope.execution_id,
            artifact,
            manifest_chunk,
            &envelope.bytes,
        )
        .map_err(|error| match error {
            ArtifactMaterializationError::ChunkChecksumMismatch => {
                ChunkTransportError::ConflictingChunk
            }
            other => ChunkTransportError::Storage(other),
        })
}

#[derive(Debug, Clone, Copy)]
struct ChunkIdentity<'a> {
    execution_id: &'a str,
    attempt_id: &'a str,
    idempotency_key: &'a str,
    request_digest: &'a str,
}

impl ChunkUploadEnvelope {
    fn identity(&self) -> ChunkIdentity<'_> {
        ChunkIdentity {
            execution_id: &self.execution_id,
            attempt_id: &self.attempt_id,
            idempotency_key: &self.idempotency_key,
            request_digest: &self.request_digest,
        }
    }
}

fn validate_request_identity(
    request: &GeneralComputeRequest,
    identity: &ChunkIdentity<'_>,
) -> Result<(), ChunkTransportError> {
    if identity.execution_id != request.execution_id
        || identity.attempt_id != request.attempt_id
        || identity.idempotency_key != request.idempotency_key
        || identity.request_digest != request.request_digest
    {
        return Err(ChunkTransportError::IdentityMismatch);
    }
    Ok(())
}

fn find_artifact<'a>(
    request: &'a GeneralComputeRequest,
    artifact_id: &str,
) -> Result<&'a ArtifactManifest, ChunkTransportError> {
    std::iter::once(&request.source_artifact)
        .chain(request.input_artifacts.iter())
        .find(|artifact| artifact.artifact_id == artifact_id)
        .ok_or(ChunkTransportError::ArtifactNotFound)
}

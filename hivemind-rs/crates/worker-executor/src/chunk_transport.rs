//! Worker boundary adapter for authenticated general-compute CAS chunks.
//!
//! The gRPC envelope is untrusted input. This module only accepts it after
//! the caller has verified a Nodepool-issued worker-execution JWT and supplied
//! the claims checked against the assigned task and worker. The adapter then
//! binds the protobuf fields to the runtime transport envelope; the runtime
//! performs the request, manifest, and bytes checks before CAS ingest.

use general_compute_runtime::artifact::CasChunkStore;
use general_compute_runtime::transport::{
    ingest_chunk, ChunkResumeEnvelope, ChunkTransportError, ChunkUploadEnvelope,
};
use general_compute_runtime::GeneralComputeRequest;
use hivemind_auth::worker_execution::WorkerExecutionVerifier;
use hivemind_proto::{
    validate_general_compute_chunk_resume_request, validate_general_compute_chunk_upload,
    GeneralComputeChunkResumeRequest, GeneralComputeChunkUpload,
};

/// A JWT verified with the configured Nodepool public key and bound to the
/// task and worker that accepted the execution request.
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedWorkerExecution {
    token: String,
}

impl std::fmt::Debug for VerifiedWorkerExecution {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedWorkerExecution")
            .field("token", &"<redacted>")
            .finish()
    }
}

impl VerifiedWorkerExecution {
    /// Verify a Nodepool execution token and bind it to the assigned task and
    /// worker.
    pub fn from_token(
        verifier: &WorkerExecutionVerifier,
        token: &str,
        expected_task_id: &str,
        expected_worker_id: &str,
    ) -> Result<Self, WorkerChunkIngestError> {
        if token.trim().is_empty()
            || expected_task_id.trim().is_empty()
            || expected_worker_id.trim().is_empty()
        {
            return Err(WorkerChunkIngestError::AuthorizationInvalid);
        }
        let claims = verifier
            .decode(token)
            .map_err(|_| WorkerChunkIngestError::AuthorizationInvalid)?;
        if claims.role.as_deref() != Some("worker-execution")
            || claims.task_id.as_deref() != Some(expected_task_id)
            || claims.worker_id.as_deref() != Some(expected_worker_id)
        {
            return Err(WorkerChunkIngestError::AuthorizationMismatch);
        }
        Ok(Self {
            token: token.to_owned(),
        })
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }
}

/// Errors raised at the Worker adapter or local CAS boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerChunkIngestError {
    AuthorizationInvalid,
    AuthorizationMismatch,
    TokenMismatch,
    WireInvalid(&'static str),
    Transport(ChunkTransportError),
}

impl std::fmt::Display for WorkerChunkIngestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthorizationInvalid => {
                formatter.write_str("verified worker execution authorization is invalid")
            }
            Self::AuthorizationMismatch => {
                formatter.write_str("worker execution claims are not bound to this assignment")
            }
            Self::TokenMismatch => {
                formatter.write_str("chunk token does not match the verified execution token")
            }
            Self::WireInvalid(message) => {
                write!(formatter, "chunk wire envelope is invalid: {message}")
            }
            Self::Transport(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for WorkerChunkIngestError {}

/// Adapt one untrusted protobuf upload into the identity-bound runtime CAS
/// transport. The raw token is accepted only when it is byte equal to the
/// token represented by `verified_execution`.
pub fn ingest_general_compute_chunk(
    store: &CasChunkStore,
    request: &GeneralComputeRequest,
    upload: &GeneralComputeChunkUpload,
    verified_execution: &VerifiedWorkerExecution,
) -> Result<(), WorkerChunkIngestError> {
    if upload.token != verified_execution.token() {
        return Err(WorkerChunkIngestError::TokenMismatch);
    }
    validate_general_compute_chunk_upload(upload).map_err(WorkerChunkIngestError::WireInvalid)?;

    let size_bytes = u64::try_from(upload.size_bytes)
        .map_err(|_| WorkerChunkIngestError::WireInvalid("chunk size must be positive"))?;
    let offset = u64::try_from(upload.offset)
        .map_err(|_| WorkerChunkIngestError::WireInvalid("chunk offset must not be negative"))?;
    let envelope = ChunkUploadEnvelope {
        execution_id: upload.execution_id.clone(),
        attempt_id: upload.attempt_id.clone(),
        idempotency_key: upload.idempotency_key.clone(),
        request_digest: upload.request_digest.clone(),
        artifact_id: upload.artifact_id.clone(),
        offset,
        size_bytes,
        sha256: upload.sha256.clone(),
        bytes: upload.bytes.clone(),
    };
    ingest_chunk(store, request, &envelope).map_err(WorkerChunkIngestError::Transport)
}

/// Return the manifest chunks not listed as complete by an authenticated
/// attempt. The local CAS is checked so a claimed completed digest is only
/// treated as complete when its object is actually present and verified.
pub fn resume_general_compute_chunks(
    store: &CasChunkStore,
    request: &GeneralComputeRequest,
    resume: &GeneralComputeChunkResumeRequest,
    verified_execution: &VerifiedWorkerExecution,
) -> Result<Vec<general_compute_runtime::ArtifactChunk>, WorkerChunkIngestError> {
    if resume.token != verified_execution.token() {
        return Err(WorkerChunkIngestError::TokenMismatch);
    }
    validate_general_compute_chunk_resume_request(resume)
        .map_err(WorkerChunkIngestError::WireInvalid)?;
    let envelope = ChunkResumeEnvelope {
        execution_id: resume.execution_id.clone(),
        attempt_id: resume.attempt_id.clone(),
        idempotency_key: resume.idempotency_key.clone(),
        request_digest: resume.request_digest.clone(),
        artifact_id: resume.artifact_id.clone(),
        completed_sha256: resume.completed_sha256.clone(),
    };
    let _ = envelope
        .missing_chunks(request)
        .map_err(WorkerChunkIngestError::Transport)?;
    let artifact = std::iter::once(&request.source_artifact)
        .chain(request.input_artifacts.iter())
        .find(|artifact| artifact.artifact_id == resume.artifact_id)
        .ok_or(WorkerChunkIngestError::Transport(
            ChunkTransportError::ArtifactNotFound,
        ))?;
    // The caller's completed list is only an admission hint. Recompute the
    // actual missing set from the operator-owned CAS so a false claim cannot
    // suppress a required transfer.
    let actual_missing = store
        .missing_chunks(artifact)
        .map_err(|error| WorkerChunkIngestError::Transport(ChunkTransportError::Storage(error)))?;
    Ok(actual_missing)
}

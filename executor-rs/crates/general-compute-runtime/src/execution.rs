//! Typed execution for the reference-only, registry-approved backend.
//!
//! This adapter is intentionally narrower than the public runtime contract:
//! it reads materialized inline artifacts and invokes only a
//! [`PinnedPythonAdapter`] registered as `ReferenceDirect`. Production
//! backends must use the validated OCI launcher in [`crate::sandbox`] and are
//! not silently downgraded to this path.

use crate::artifact::{ArtifactMaterializationError, ArtifactMaterializer, CasChunkStore};
use crate::cp_python::{PinnedPythonAdapter, PythonAdapterError, PythonBackendRegistry};
use crate::sandbox::BackendExecutionMode;
use crate::supervisor::Cancellation;
use crate::{
    canonical_artifact_root, sha256_digest, ArtifactManifest, ArtifactRole, CapabilityMatrix,
    GeneralComputeRequest, GeneralComputeResult, ResultStatus, UsageClaim, WorkerCapabilities,
};
use std::fmt;
use std::fs;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    Request(String),
    Capability(String),
    Artifact(ArtifactMaterializationError),
    BackendUnavailable(String),
    UnsupportedExecutionMode,
    UnsupportedEntrypoint,
    SourceNotUtf8,
    InputNotUtf8,
    MultipleInputArtifacts,
    Backend(PythonAdapterError),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(message) => {
                write!(formatter, "invalid general-compute request: {message}")
            }
            Self::Capability(message) => {
                write!(formatter, "general-compute capability rejected: {message}")
            }
            Self::Artifact(error) => error.fmt(formatter),
            Self::BackendUnavailable(backend) => {
                write!(formatter, "general-compute backend unavailable: {backend}")
            }
            Self::UnsupportedExecutionMode => {
                formatter.write_str("reference executor cannot run a production backend")
            }
            Self::UnsupportedEntrypoint => {
                formatter.write_str("reference python backend requires the `main` entrypoint")
            }
            Self::SourceNotUtf8 => formatter.write_str("source artifact is not valid UTF-8"),
            Self::InputNotUtf8 => formatter.write_str("input artifact is not valid UTF-8"),
            Self::MultipleInputArtifacts => {
                formatter.write_str("reference backend accepts at most one input artifact")
            }
            Self::Backend(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<ArtifactMaterializationError> for ExecutionError {
    fn from(error: ArtifactMaterializationError) -> Self {
        Self::Artifact(error)
    }
}

impl From<PythonAdapterError> for ExecutionError {
    fn from(error: PythonAdapterError) -> Self {
        Self::Backend(error)
    }
}

/// Executes one admitted request through the direct reference backend only.
#[derive(Debug, Clone)]
pub struct ReferenceBackendExecutor {
    capabilities: CapabilityMatrix,
    worker: WorkerCapabilities,
    python_registry: PythonBackendRegistry,
}

impl ReferenceBackendExecutor {
    #[must_use]
    pub fn new(
        capabilities: CapabilityMatrix,
        worker: WorkerCapabilities,
        python_registry: PythonBackendRegistry,
    ) -> Self {
        Self {
            capabilities,
            worker,
            python_registry,
        }
    }

    pub fn execute(
        &self,
        request: &GeneralComputeRequest,
        materializer: &ArtifactMaterializer,
    ) -> Result<GeneralComputeResult, ExecutionError> {
        self.execute_with_materializer(request, materializer, None, &Cancellation::new())
    }

    pub fn execute_with_cancellation(
        &self,
        request: &GeneralComputeRequest,
        materializer: &ArtifactMaterializer,
        cancellation: &Cancellation,
    ) -> Result<GeneralComputeResult, ExecutionError> {
        self.execute_with_materializer(request, materializer, None, cancellation)
    }

    /// Execute using complete, locally verified CAS chunks.
    ///
    /// The store is supplied by an operator-owned transport boundary. This
    /// method performs no network access and does not interpret remote paths.
    pub fn execute_with_cas(
        &self,
        request: &GeneralComputeRequest,
        materializer: &ArtifactMaterializer,
        store: &CasChunkStore,
    ) -> Result<GeneralComputeResult, ExecutionError> {
        self.execute_with_materializer(request, materializer, Some(store), &Cancellation::new())
    }

    fn execute_with_materializer(
        &self,
        request: &GeneralComputeRequest,
        materializer: &ArtifactMaterializer,
        cas_store: Option<&CasChunkStore>,
        cancellation: &Cancellation,
    ) -> Result<GeneralComputeResult, ExecutionError> {
        request
            .validate()
            .map_err(|error| ExecutionError::Request(error.message))?;
        self.capabilities
            .validate_request(request, &self.worker)
            .map_err(|error| ExecutionError::Capability(error.message))?;
        if request.input_artifacts.len() > 1 {
            return Err(ExecutionError::MultipleInputArtifacts);
        }
        if request.entrypoint != "main" {
            return Err(ExecutionError::UnsupportedEntrypoint);
        }

        let source_path =
            materialize_artifact(materializer, cas_store, &request.source_artifact)?.path;
        let source_bytes = fs::read(source_path).map_err(|error| {
            ExecutionError::Artifact(ArtifactMaterializationError::Io(error.to_string()))
        })?;
        let source =
            std::str::from_utf8(&source_bytes).map_err(|_| ExecutionError::SourceNotUtf8)?;

        let (input_bytes, input_json) =
            if let Some(input_artifact) = request.input_artifacts.first() {
                let path = materialize_artifact(materializer, cas_store, input_artifact)?.path;
                let bytes = fs::read(path).map_err(|error| {
                    ExecutionError::Artifact(ArtifactMaterializationError::Io(error.to_string()))
                })?;
                let input_json = {
                    let input =
                        std::str::from_utf8(&bytes).map_err(|_| ExecutionError::InputNotUtf8)?;
                    input.to_owned()
                };
                (bytes, input_json)
            } else {
                (Vec::new(), "null".to_owned())
            };

        let backend = self
            .capabilities
            .backends
            .iter()
            .find(|backend| backend.backend_id == request.backend_id)
            .ok_or_else(|| ExecutionError::BackendUnavailable(request.backend_id.clone()))?;
        let adapter =
            PinnedPythonAdapter::from_registry(&self.python_registry, &request.backend_id)
                .map_err(|error| ExecutionError::BackendUnavailable(error.to_string()))?;
        if adapter.registration().execution_mode != BackendExecutionMode::ReferenceDirect {
            return Err(ExecutionError::UnsupportedExecutionMode);
        }
        if adapter.registration().guest_image_digest != request.guest_image_digest
            || backend.guest_image_digest != request.guest_image_digest
        {
            return Err(ExecutionError::BackendUnavailable(
                "backend image digest does not match the admitted request".into(),
            ));
        }

        let started = Instant::now();
        let observation = adapter.execute_with_timeout(
            source,
            &input_json,
            request.determinism.seed.unwrap_or_default(),
            Duration::from_millis(request.execution_policy.wall_time_ms),
            cancellation,
        )?;
        let wall_time_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let status = match observation.status.as_str() {
            "halted" => ResultStatus::Completed,
            "exception" => ResultStatus::Failed,
            "cancelled" => ResultStatus::Cancelled,
            "resource_exhausted" => ResultStatus::ResourceExhausted,
            "exited" => ResultStatus::Failed,
            _ => {
                return Err(ExecutionError::BackendUnavailable(
                    "unknown backend status".into(),
                ));
            }
        };
        let stdout = observation.output;
        let output_len = stdout.len();
        let output_artifacts = if stdout.is_empty() {
            Vec::new()
        } else {
            vec![text_output_artifact(stdout.as_bytes())]
        };
        let result = GeneralComputeResult {
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            status,
            exit_code: match status {
                ResultStatus::Completed => Some(0),
                ResultStatus::Failed => Some(1),
                _ => None,
            },
            error_code: match status {
                ResultStatus::Completed => None,
                ResultStatus::Failed => Some("backend_exception".into()),
                ResultStatus::Cancelled => Some("cancelled".into()),
                ResultStatus::ResourceExhausted => Some("resource_exhausted".into()),
                ResultStatus::TimedOut => Some("wall_time_exceeded".into()),
                ResultStatus::BackendUnavailable => Some("backend_unavailable".into()),
            },
            stdout,
            stderr: String::new(),
            output_manifest_root: canonical_artifact_root(&output_artifacts),
            output_artifacts,
            usage: UsageClaim {
                wall_time_ms,
                input_bytes: input_bytes.len() as u64,
                output_bytes: output_len as u64,
                ..UsageClaim::default()
            },
            runtime_version: request.runtime_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            input_sha256: if input_bytes.is_empty() {
                sha256_digest(&[])
            } else {
                sha256_digest(&input_bytes)
            },
            determinism: request.determinism.clone(),
            capability_summary: backend.capabilities.clone(),
            evidence: Default::default(),
        };
        result
            .validate_against(request, &self.capabilities)
            .map_err(|error| ExecutionError::Request(error.message))?;
        Ok(result)
    }
}

fn materialize_artifact(
    materializer: &ArtifactMaterializer,
    cas_store: Option<&CasChunkStore>,
    artifact: &ArtifactManifest,
) -> Result<crate::artifact::MaterializedArtifact, ArtifactMaterializationError> {
    match cas_store {
        Some(store) => materializer.materialize_with_cas(artifact, store),
        None => materializer.materialize(artifact),
    }
}

fn text_output_artifact(bytes: &[u8]) -> ArtifactManifest {
    ArtifactManifest {
        artifact_id: "stdout".into(),
        role: ArtifactRole::Output,
        size_bytes: bytes.len() as u64,
        mime_type: "text/plain".into(),
        sha256: sha256_digest(bytes),
        chunks: Vec::new(),
        inline_bytes: Some(bytes.to_vec()),
    }
}

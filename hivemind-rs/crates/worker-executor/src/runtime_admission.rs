//! Worker-side runtime admission and routing.
//!
//! The Worker is an untrusted executor.  It may decide whether it can run a
//! request, but it must use the versioned runtime contract and an explicit
//! capability registry rather than inferring a backend from user-provided
//! strings.

use general_compute_runtime::{
    BackendRegistration, CapabilityMatrix, GeneralComputeRequest, ValidationError,
    ValidationErrorCode, WorkerCapabilities, GENERAL_COMPUTE_RUNTIME_VERSION,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRoute {
    Legacy,
    ManagedFunctionV0,
    GeneralComputeV1Alpha1(GeneralComputeRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeAdmissionError {
    ManifestRequired,
    ManifestRuntimeMismatch,
    ManifestMalformed(String),
    ManifestRejected {
        code: ValidationErrorCode,
        message: String,
    },
    UnsupportedRuntime(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeAdmissionConfigError {
    InvalidBackends(String),
    InvalidWorkerCapabilities(String),
}

impl fmt::Display for RuntimeAdmissionConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBackends(message) => {
                write!(formatter, "general-compute backend registry is invalid: {message}")
            }
            Self::InvalidWorkerCapabilities(message) => write!(
                formatter,
                "general-compute worker capabilities are invalid: {message}"
            ),
        }
    }
}

impl std::error::Error for RuntimeAdmissionConfigError {}

impl std::fmt::Display for RuntimeAdmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManifestRequired => {
                formatter.write_str("general-compute-v1alpha1 request manifest is required")
            }
            Self::ManifestRuntimeMismatch => formatter.write_str(
                "general-compute request manifest requires runtime general-compute-v1alpha1",
            ),
            Self::ManifestMalformed(message) => {
                write!(formatter, "general-compute-v1alpha1 request manifest is malformed: {message}")
            }
            Self::ManifestRejected { code, message } => {
                write!(formatter, "general-compute-v1alpha1 request was rejected ({code:?}): {message}")
            }
            Self::UnsupportedRuntime(runtime) => {
                let _ = runtime;
                formatter.write_str("unsupported task runtime")
            }
        }
    }
}

impl std::error::Error for RuntimeAdmissionError {}

#[derive(Debug, Clone)]
pub struct WorkerRuntimeAdmission {
    registry: CapabilityMatrix,
    worker: WorkerCapabilities,
}

impl Default for WorkerRuntimeAdmission {
    fn default() -> Self {
        Self {
            registry: CapabilityMatrix::default(),
            worker: WorkerCapabilities {
                guest_image_digests: Vec::new(),
                capabilities: Vec::new(),
                max_threads: 0,
                gpu_available: false,
            },
        }
    }
}

impl WorkerRuntimeAdmission {
    #[must_use]
    pub fn new(registry: CapabilityMatrix, worker: WorkerCapabilities) -> Self {
        Self { registry, worker }
    }

    #[must_use]
    pub fn capability_matrix(&self) -> CapabilityMatrix {
        self.registry.clone()
    }

    #[must_use]
    pub fn worker_capabilities(&self) -> WorkerCapabilities {
        self.worker.clone()
    }

    /// Load an operator-owned capability registry.  An absent registry keeps
    /// the alpha runtime disabled; malformed configuration fails closed at
    /// worker startup instead of silently widening admission.
    pub fn from_environment() -> Result<Self, RuntimeAdmissionConfigError> {
        let backends = std::env::var("HIVEMIND_GENERAL_COMPUTE_BACKENDS").unwrap_or_default();
        let worker = std::env::var("HIVEMIND_GENERAL_COMPUTE_WORKER_CAPABILITIES")
            .unwrap_or_default();
        if backends.trim().is_empty() && worker.trim().is_empty() {
            return Ok(Self::default());
        }
        if backends.trim().is_empty() || worker.trim().is_empty() {
            return Err(RuntimeAdmissionConfigError::InvalidBackends(
                "backend registry and worker capabilities must be configured together".into(),
            ));
        }
        let backends = serde_json::from_str::<Vec<BackendRegistration>>(&backends)
            .map_err(|error| RuntimeAdmissionConfigError::InvalidBackends(error.to_string()))?;
        let worker = serde_json::from_str::<WorkerCapabilities>(&worker).map_err(|error| {
            RuntimeAdmissionConfigError::InvalidWorkerCapabilities(error.to_string())
        })?;
        Ok(Self::new(CapabilityMatrix::new(backends), worker))
    }

    pub fn admit(
        &self,
        runtime: &str,
        manifest_json: &[u8],
    ) -> Result<RuntimeRoute, RuntimeAdmissionError> {
        if !manifest_json.is_empty() && runtime.trim() != GENERAL_COMPUTE_RUNTIME_VERSION {
            return Err(RuntimeAdmissionError::ManifestRuntimeMismatch);
        }
        match runtime.trim() {
            "" => Ok(RuntimeRoute::Legacy),
            "managed-function-v0" => Ok(RuntimeRoute::ManagedFunctionV0),
            GENERAL_COMPUTE_RUNTIME_VERSION => {
                if manifest_json.is_empty() {
                    return Err(RuntimeAdmissionError::ManifestRequired);
                }
                let request = serde_json::from_slice::<GeneralComputeRequest>(manifest_json)
                    .map_err(|error| RuntimeAdmissionError::ManifestMalformed(error.to_string()))?;
                self.registry
                    .validate_request(&request, &self.worker)
                    .map_err(rejected)
                    .map(|()| RuntimeRoute::GeneralComputeV1Alpha1(request))
            }
            other => Err(RuntimeAdmissionError::UnsupportedRuntime(other.into())),
        }
    }
}

fn rejected(error: ValidationError) -> RuntimeAdmissionError {
    RuntimeAdmissionError::ManifestRejected {
        code: error.code,
        message: error.message,
    }
}

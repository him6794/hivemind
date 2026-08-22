//! Worker-side runtime admission and routing.
//!
//! The Worker is an untrusted executor.  It may decide whether it can run a
//! request, but it must use the versioned runtime contract and an explicit
//! capability registry rather than inferring a backend from user-provided
//! strings.

use general_compute_runtime::{
    BackendRegistration, CapabilityMatrix, GeneralComputeRequest,
    TrustedWorkerCapabilityRegistration, ValidationError, ValidationErrorCode, WorkerCapabilities,
    GENERAL_COMPUTE_RUNTIME_VERSION,
};
use hivemind_models::WorkerCapabilityReport;
use std::fmt;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRoute {
    Legacy,
    ManagedFunctionV0,
    ProductionSandboxedDsl,
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
    InvalidTrustedRegistration(String),
}

impl fmt::Display for RuntimeAdmissionConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBackends(message) => {
                write!(
                    formatter,
                    "general-compute backend registry is invalid: {message}"
                )
            }
            Self::InvalidWorkerCapabilities(message) => write!(
                formatter,
                "general-compute worker capabilities are invalid: {message}"
            ),
            Self::InvalidTrustedRegistration(message) => write!(
                formatter,
                "general-compute trusted registration is invalid: {message}"
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
                write!(
                    formatter,
                    "general-compute-v1alpha1 request manifest is malformed: {message}"
                )
            }
            Self::ManifestRejected { code, message } => {
                write!(
                    formatter,
                    "general-compute-v1alpha1 request was rejected ({code:?}): {message}"
                )
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
    trusted_registration: TrustedWorkerCapabilityRegistration,
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
            trusted_registration: TrustedWorkerCapabilityRegistration {
                worker: WorkerCapabilities {
                    guest_image_digests: Vec::new(),
                    capabilities: Vec::new(),
                    max_threads: 0,
                    gpu_available: false,
                },
                gpu_capabilities: Vec::new(),
                backends: Vec::new(),
            },
        }
    }
}

impl WorkerRuntimeAdmission {
    #[must_use]
    pub fn new(registry: CapabilityMatrix, worker: WorkerCapabilities) -> Self {
        Self {
            trusted_registration: TrustedWorkerCapabilityRegistration {
                worker: worker.clone(),
                gpu_capabilities: Vec::new(),
                backends: registry.backends.clone(),
            },
            registry,
            worker,
        }
    }

    /// Build admission from the complete operator-owned registration. The
    /// registration is the only source of typed GPU identities; a worker's
    /// boolean `gpu_available` claim is not upgraded into a capability row.
    #[must_use]
    pub fn new_with_trusted_registration(
        trusted_registration: TrustedWorkerCapabilityRegistration,
    ) -> Self {
        let registry = CapabilityMatrix::new(trusted_registration.backends.clone());
        let worker = trusted_registration.worker.clone();
        Self {
            registry,
            worker,
            trusted_registration,
        }
    }

    #[must_use]
    pub fn capability_matrix(&self) -> CapabilityMatrix {
        self.registry.clone()
    }

    #[must_use]
    pub fn worker_capabilities(&self) -> WorkerCapabilities {
        self.worker.clone()
    }

    #[must_use]
    pub fn trusted_registration(&self) -> TrustedWorkerCapabilityRegistration {
        self.trusted_registration.clone()
    }

    /// Public admission advertises only the bounded closed-DSL capability.
    /// Typed GPU/image/backend identities remain operator-owned private data.
    #[must_use]
    pub fn public_capability_report(&self) -> WorkerCapabilityReport {
        WorkerCapabilityReport::public_managed_dsl()
    }

    /// Select the concrete operator-approved device for a request. Typed GPU
    /// requests fail closed when the registration has no compatible identity.
    pub fn select_gpu_for_request(
        &self,
        request: &GeneralComputeRequest,
    ) -> Result<Option<general_compute_runtime::gpu::GpuSelection>, ValidationError> {
        self.trusted_registration.select_gpu_for_request(request)
    }

    /// Load an operator-owned capability registry.  An absent registry keeps
    /// the alpha runtime disabled; malformed configuration fails closed at
    /// worker startup instead of silently widening admission.
    pub fn from_environment() -> Result<Self, RuntimeAdmissionConfigError> {
        if let Ok(trusted) = std::env::var("HIVEMIND_GENERAL_COMPUTE_TRUSTED_REGISTRATION") {
            if !trusted.trim().is_empty() {
                let registration = serde_json::from_str::<TrustedWorkerCapabilityRegistration>(
                    &trusted,
                )
                .map_err(|error| {
                    RuntimeAdmissionConfigError::InvalidTrustedRegistration(error.to_string())
                })?;
                return Ok(Self::new_with_trusted_registration(registration));
            }
        }
        let backends = std::env::var("HIVEMIND_GENERAL_COMPUTE_BACKENDS").unwrap_or_default();
        let worker =
            std::env::var("HIVEMIND_GENERAL_COMPUTE_WORKER_CAPABILITIES").unwrap_or_default();
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
            "production_sandboxed_dsl" => Ok(RuntimeRoute::ProductionSandboxedDsl),
            GENERAL_COMPUTE_RUNTIME_VERSION => {
                if manifest_json.is_empty() {
                    return Err(RuntimeAdmissionError::ManifestRequired);
                }
                let request = serde_json::from_slice::<GeneralComputeRequest>(manifest_json)
                    .map_err(|error| RuntimeAdmissionError::ManifestMalformed(error.to_string()))?;
                self.registry
                    .validate_request(&request, &self.worker)
                    .map_err(rejected)?;
                self.trusted_registration
                    .select_gpu_for_request(&request)
                    .map_err(rejected)?;
                Ok(RuntimeRoute::GeneralComputeV1Alpha1(request))
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

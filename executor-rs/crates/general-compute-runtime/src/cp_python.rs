use crate::differential::ReferenceObservation;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonBackendRegistration {
    pub backend_id: String,
    pub executable: String,
    pub runtime_version: String,
    pub guest_image_digest: String,
    pub protocol_version: String,
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonRegistryError {
    EmptyBackendId,
    EmptyExecutable,
    InvalidImageDigest,
    EmptyProtocolVersion,
    ZeroOutputLimit,
    DuplicateBackend(String),
}

impl fmt::Display for PythonRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBackendId => formatter.write_str("python backend id must not be empty"),
            Self::EmptyExecutable => formatter.write_str("python executable must not be empty"),
            Self::InvalidImageDigest => formatter.write_str("python image digest must be sha256 pinned"),
            Self::EmptyProtocolVersion => formatter.write_str("python protocol version must not be empty"),
            Self::ZeroOutputLimit => formatter.write_str("python output limit must be positive"),
            Self::DuplicateBackend(id) => write!(formatter, "duplicate python backend {id}"),
        }
    }
}

impl std::error::Error for PythonRegistryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonBackendRegistry {
    backends: BTreeMap<String, PythonBackendRegistration>,
}

impl PythonBackendRegistry {
    pub fn new(registrations: Vec<PythonBackendRegistration>) -> Result<Self, PythonRegistryError> {
        let mut backends = BTreeMap::new();
        for registration in registrations {
            validate_registration(&registration)?;
            if backends.insert(registration.backend_id.clone(), registration).is_some() {
                let id = backends.keys().next_back().cloned().unwrap_or_default();
                return Err(PythonRegistryError::DuplicateBackend(id));
            }
        }
        Ok(Self { backends })
    }

    fn get(&self, backend_id: &str) -> Option<&PythonBackendRegistration> {
        self.backends.get(backend_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonAdapterError {
    BackendUnavailable { backend_id: String },
    MalformedObservation(String),
}

impl fmt::Display for PythonAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendUnavailable { backend_id } => write!(formatter, "python backend {backend_id} is unavailable"),
            Self::MalformedObservation(message) => write!(formatter, "malformed python observation: {message}"),
        }
    }
}

impl std::error::Error for PythonAdapterError {}

#[derive(Debug, Clone)]
pub struct PinnedPythonAdapter {
    registration: PythonBackendRegistration,
}

impl PinnedPythonAdapter {
    pub fn from_registry(registry: &PythonBackendRegistry, backend_id: &str) -> Result<Self, PythonAdapterError> {
        let Some(registration) = registry.get(backend_id) else {
            return Err(PythonAdapterError::BackendUnavailable {
                backend_id: backend_id.into(),
            });
        };
        Ok(Self {
            registration: registration.clone(),
        })
    }

    pub fn registration(&self) -> &PythonBackendRegistration {
        &self.registration
    }

    pub fn parse_observation(&self, bytes: &[u8]) -> Result<ReferenceObservation, PythonAdapterError> {
        let observation: StrictObservation = serde_json::from_slice(bytes)
            .map_err(|error| PythonAdapterError::MalformedObservation(error.to_string()))?;
        if !matches!(
            observation.status.as_str(),
            "halted" | "exception" | "exited" | "resource_exhausted" | "cancelled"
        ) {
            return Err(PythonAdapterError::MalformedObservation("unknown status".into()));
        }
        if observation.output.len() > self.registration.max_output_bytes {
            return Err(PythonAdapterError::MalformedObservation(
                "output exceeds registered cap".into(),
            ));
        }
        Ok(ReferenceObservation {
            status: observation.status,
            steps: observation.steps,
            output: observation.output,
        })
    }
}

fn validate_registration(registration: &PythonBackendRegistration) -> Result<(), PythonRegistryError> {
    if registration.backend_id.trim().is_empty() {
        return Err(PythonRegistryError::EmptyBackendId);
    }
    if registration.executable.trim().is_empty() {
        return Err(PythonRegistryError::EmptyExecutable);
    }
    if !registration.guest_image_digest.starts_with("sha256:")
        || registration.guest_image_digest.len() != "sha256:".len() + 64
        || !registration.guest_image_digest["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(PythonRegistryError::InvalidImageDigest);
    }
    if registration.protocol_version.trim().is_empty() {
        return Err(PythonRegistryError::EmptyProtocolVersion);
    }
    if registration.max_output_bytes == 0 {
        return Err(PythonRegistryError::ZeroOutputLimit);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictObservation {
    status: String,
    steps: u64,
    output: String,
}

//! Versioned optimized-backend identity pins.
//!
//! This module records the exact backend/version/CPU feature/thread tuple that
//! a future optimized implementation must report before it can be compared
//! with the bounded reference vectors. It does not load native libraries or
//! claim that an optimized backend is installed.

use std::fmt;

use crate::differential::{
    DifferentialCase, DifferentialError, DifferentialRunner, ReferenceObservation,
};
use crate::sha256_digest;
use serde::{Deserialize, Serialize};

pub const BACKEND_PIN_PROTOCOL_VERSION: &str = "general-compute-backend-pin-v1";
pub const MAX_BACKEND_TOKEN_LENGTH: usize = 128;
pub const MAX_BACKEND_CPU_FEATURES: usize = 128;
pub const MAX_BACKEND_CPU_FEATURE_LENGTH: usize = 64;
pub const MAX_BACKEND_THREADS: u32 = 4096;
pub const MAX_REFERENCE_VECTOR_COUNT: usize = 128;
pub const MAX_REFERENCE_VECTOR_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_REFERENCE_VECTOR_INPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendPinError {
    InvalidField(&'static str),
    InvalidThreadCount,
    TooManyCpuFeatures,
    CpuFeatureTooLong,
    FeaturesNotCanonical,
    InvalidReferenceDigest,
    InvalidImageDigest,
    IdentityMismatch(&'static str),
}

impl fmt::Display for BackendPinError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField(field) => write!(formatter, "backend pin field {field} is invalid"),
            Self::InvalidThreadCount => formatter.write_str(
                "backend pin thread count must be between one and the reference maximum",
            ),
            Self::TooManyCpuFeatures => {
                formatter.write_str("backend pin has too many CPU features")
            }
            Self::CpuFeatureTooLong => formatter.write_str("backend CPU feature name is too long"),
            Self::FeaturesNotCanonical => {
                formatter.write_str("backend CPU features must be strictly sorted and unique")
            }
            Self::InvalidReferenceDigest => {
                formatter.write_str("backend reference-vector digest must be a SHA-256 digest")
            }
            Self::InvalidImageDigest => {
                formatter.write_str("backend guest image digest must be a SHA-256 digest")
            }
            Self::IdentityMismatch(field) => {
                write!(formatter, "backend runtime identity differs in {field}")
            }
        }
    }
}

impl std::error::Error for BackendPinError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendRuntimeIdentity {
    pub protocol_version: String,
    pub backend_id: String,
    pub backend_version: String,
    pub cpu_features: Vec<String>,
    pub thread_count: u32,
    pub reference_vector_sha256: String,
    pub guest_image_digest: Option<String>,
}

impl BackendRuntimeIdentity {
    pub fn new(
        backend_id: impl Into<String>,
        backend_version: impl Into<String>,
        cpu_features: Vec<String>,
        thread_count: u32,
        reference_vector_sha256: impl Into<String>,
    ) -> Result<Self, BackendPinError> {
        Self::new_internal(
            backend_id.into(),
            backend_version.into(),
            cpu_features,
            thread_count,
            reference_vector_sha256.into(),
            None,
        )
    }

    pub fn new_with_image(
        backend_id: impl Into<String>,
        backend_version: impl Into<String>,
        cpu_features: Vec<String>,
        thread_count: u32,
        reference_vector_sha256: impl Into<String>,
        guest_image_digest: impl Into<String>,
    ) -> Result<Self, BackendPinError> {
        Self::new_internal(
            backend_id.into(),
            backend_version.into(),
            cpu_features,
            thread_count,
            reference_vector_sha256.into(),
            Some(guest_image_digest.into()),
        )
    }

    fn new_internal(
        backend_id: String,
        backend_version: String,
        cpu_features: Vec<String>,
        thread_count: u32,
        reference_vector_sha256: String,
        guest_image_digest: Option<String>,
    ) -> Result<Self, BackendPinError> {
        let identity = Self {
            protocol_version: BACKEND_PIN_PROTOCOL_VERSION.into(),
            backend_id,
            backend_version,
            cpu_features,
            thread_count,
            reference_vector_sha256,
            guest_image_digest,
        };
        identity.validate()?;
        Ok(identity)
    }

    fn validate(&self) -> Result<(), BackendPinError> {
        validate_token(&self.backend_id, "backend_id")?;
        validate_token(&self.backend_version, "backend_version")?;
        validate_features(&self.cpu_features)?;
        validate_thread_count(self.thread_count)?;
        if !is_sha256_digest(&self.reference_vector_sha256) {
            return Err(BackendPinError::InvalidReferenceDigest);
        }
        if self
            .guest_image_digest
            .as_deref()
            .is_some_and(|digest| !is_sha256_digest(digest))
        {
            return Err(BackendPinError::InvalidImageDigest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptimizedBackendPin {
    pub protocol_version: String,
    pub backend_id: String,
    pub backend_version: String,
    pub cpu_features: Vec<String>,
    pub thread_count: u32,
    pub reference_vector_sha256: String,
    pub guest_image_digest: Option<String>,
}

impl OptimizedBackendPin {
    pub fn new(
        backend_id: impl Into<String>,
        backend_version: impl Into<String>,
        cpu_features: Vec<String>,
        thread_count: u32,
        reference_vector_sha256: impl Into<String>,
    ) -> Result<Self, BackendPinError> {
        let identity = BackendRuntimeIdentity::new(
            backend_id,
            backend_version,
            cpu_features,
            thread_count,
            reference_vector_sha256,
        )?;
        Ok(Self::from_identity(identity))
    }

    pub fn new_with_image(
        backend_id: impl Into<String>,
        backend_version: impl Into<String>,
        cpu_features: Vec<String>,
        thread_count: u32,
        reference_vector_sha256: impl Into<String>,
        guest_image_digest: impl Into<String>,
    ) -> Result<Self, BackendPinError> {
        let identity = BackendRuntimeIdentity::new_with_image(
            backend_id,
            backend_version,
            cpu_features,
            thread_count,
            reference_vector_sha256,
            guest_image_digest,
        )?;
        Ok(Self::from_identity(identity))
    }

    fn from_identity(identity: BackendRuntimeIdentity) -> Self {
        Self {
            protocol_version: identity.protocol_version,
            backend_id: identity.backend_id,
            backend_version: identity.backend_version,
            cpu_features: identity.cpu_features,
            thread_count: identity.thread_count,
            reference_vector_sha256: identity.reference_vector_sha256,
            guest_image_digest: identity.guest_image_digest,
        }
    }

    /// Require exact identity equality before an optimized result can be
    /// compared against the pinned reference vectors.
    pub fn verify(&self, identity: &BackendRuntimeIdentity) -> Result<(), BackendPinError> {
        if self.protocol_version != BACKEND_PIN_PROTOCOL_VERSION {
            return Err(BackendPinError::IdentityMismatch("protocol_version"));
        }
        self.validate()?;
        identity.validate()?;
        if self.backend_id != identity.backend_id {
            return Err(BackendPinError::IdentityMismatch("backend_id"));
        }
        if self.backend_version != identity.backend_version {
            return Err(BackendPinError::IdentityMismatch("backend_version"));
        }
        if self.cpu_features != identity.cpu_features {
            return Err(BackendPinError::IdentityMismatch("cpu_features"));
        }
        if self.thread_count != identity.thread_count {
            return Err(BackendPinError::IdentityMismatch("thread_count"));
        }
        if self.reference_vector_sha256 != identity.reference_vector_sha256 {
            return Err(BackendPinError::IdentityMismatch("reference_vector_sha256"));
        }
        if self.guest_image_digest != identity.guest_image_digest {
            return Err(BackendPinError::IdentityMismatch("guest_image_digest"));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), BackendPinError> {
        validate_token(&self.backend_id, "backend_id")?;
        validate_token(&self.backend_version, "backend_version")?;
        validate_features(&self.cpu_features)?;
        validate_thread_count(self.thread_count)?;
        if !is_sha256_digest(&self.reference_vector_sha256) {
            return Err(BackendPinError::InvalidReferenceDigest);
        }
        if self
            .guest_image_digest
            .as_deref()
            .is_some_and(|digest| !is_sha256_digest(digest))
        {
            return Err(BackendPinError::InvalidImageDigest);
        }
        Ok(())
    }
}

/// An operator-approved optimized backend and the bounded suite used to
/// validate claims from that backend.
///
/// This is deliberately a registration and reference gate. It does not load
/// a native library, execute an OCI image, or establish hardware attestation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptimizedBackendRegistration {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub pin: OptimizedBackendPin,
    pub reference_vectors: Vec<DifferentialCase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptimizedBackendRegistrationError {
    Pin(BackendPinError),
    ReferenceVector(DifferentialError),
    ObservationCount,
    VectorDigest,
    EmptyReferenceVectors,
    TooManyReferenceVectors,
    Canonicalization,
}

impl fmt::Display for OptimizedBackendRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pin(error) => write!(formatter, "optimized backend pin rejected: {error}"),
            Self::ReferenceVector(error) => {
                write!(
                    formatter,
                    "optimized backend reference vector rejected: {error}"
                )
            }
            Self::ObservationCount => {
                formatter.write_str("optimized backend observation count does not match vectors")
            }
            Self::VectorDigest => {
                formatter.write_str("optimized backend reference-vector digest drifted")
            }
            Self::EmptyReferenceVectors => {
                formatter.write_str("optimized backend reference-vector suite must not be empty")
            }
            Self::TooManyReferenceVectors => write!(
                formatter,
                "optimized backend reference-vector suite exceeds {MAX_REFERENCE_VECTOR_COUNT} vectors"
            ),
            Self::Canonicalization => formatter
                .write_str("optimized backend reference vectors could not be canonicalized"),
        }
    }
}

impl std::error::Error for OptimizedBackendRegistrationError {}

/// The bounded evidence produced after replaying a registration's reference
/// suite through the trusted reference interpreter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceVectorReport {
    pub vector_count: usize,
    pub reference_vector_sha256: String,
}

impl OptimizedBackendRegistration {
    /// Create a registration whose backend id, image digest, and vector suite
    /// are all bound to the supplied operator pin.
    pub fn new(
        backend_id: impl Into<String>,
        guest_image_digest: impl Into<String>,
        pin: OptimizedBackendPin,
        reference_vectors: Vec<DifferentialCase>,
    ) -> Result<Self, OptimizedBackendRegistrationError> {
        let registration = Self {
            backend_id: backend_id.into(),
            guest_image_digest: guest_image_digest.into(),
            pin,
            reference_vectors,
        };
        registration.validate_binding()?;
        Ok(registration)
    }

    /// Return the canonical SHA-256 digest for a reference-vector suite.
    ///
    /// Serde struct field order is the protocol order for
    /// `DifferentialCase`; vector order is significant and is preserved.
    pub fn reference_vector_digest(
        reference_vectors: &[DifferentialCase],
    ) -> Result<String, OptimizedBackendRegistrationError> {
        if reference_vectors.is_empty() {
            return Err(OptimizedBackendRegistrationError::EmptyReferenceVectors);
        }
        if reference_vectors.len() > MAX_REFERENCE_VECTOR_COUNT {
            return Err(OptimizedBackendRegistrationError::TooManyReferenceVectors);
        }
        for reference_vector in reference_vectors {
            if reference_vector.source.len() > MAX_REFERENCE_VECTOR_SOURCE_BYTES
                || reference_vector.input_json.len() > MAX_REFERENCE_VECTOR_INPUT_BYTES
            {
                return Err(OptimizedBackendRegistrationError::ReferenceVector(
                    DifferentialError::InvalidCase(
                        "reference vector source or input exceeds the bounded limit".into(),
                    ),
                ));
            }
        }
        let bytes = serde_json::to_vec(reference_vectors)
            .map_err(|_| OptimizedBackendRegistrationError::Canonicalization)?;
        Ok(sha256_digest(&bytes))
    }

    /// Require exact identity equality with the operator-approved pin and
    /// registration image before accepting any backend claim.
    pub fn verify_identity(
        &self,
        identity: &BackendRuntimeIdentity,
    ) -> Result<(), OptimizedBackendRegistrationError> {
        self.validate_binding()?;
        self.pin
            .verify(identity)
            .map_err(OptimizedBackendRegistrationError::Pin)
    }

    /// Replay the bounded reference suite and return its digest/count report.
    pub fn execute_reference_vectors(
        &self,
    ) -> Result<ReferenceVectorReport, OptimizedBackendRegistrationError> {
        self.validate_binding()?;
        let observations = self
            .reference_vectors
            .iter()
            .cloned()
            .map(|reference_vector| {
                DifferentialRunner::new(reference_vector)
                    .run_reference()
                    .map_err(OptimizedBackendRegistrationError::ReferenceVector)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.verify_observations(&observations)?;
        Ok(ReferenceVectorReport {
            vector_count: observations.len(),
            reference_vector_sha256: self.pin.reference_vector_sha256.clone(),
        })
    }

    /// Compare observations from an approved backend with every pinned
    /// reference vector, including suite digest and observation-count gates.
    pub fn verify_observations(
        &self,
        observations: &[ReferenceObservation],
    ) -> Result<(), OptimizedBackendRegistrationError> {
        self.validate_binding()?;
        if observations.len() != self.reference_vectors.len() {
            return Err(OptimizedBackendRegistrationError::ObservationCount);
        }
        for (reference_vector, observed) in self.reference_vectors.iter().zip(observations) {
            DifferentialRunner::new(reference_vector.clone())
                .compare(observed)
                .map_err(OptimizedBackendRegistrationError::ReferenceVector)?;
        }
        Ok(())
    }

    fn validate_binding(&self) -> Result<(), OptimizedBackendRegistrationError> {
        self.pin
            .validate()
            .map_err(OptimizedBackendRegistrationError::Pin)?;
        if self.backend_id != self.pin.backend_id {
            return Err(OptimizedBackendRegistrationError::Pin(
                BackendPinError::IdentityMismatch("backend_id"),
            ));
        }
        if !is_sha256_digest(&self.guest_image_digest) {
            return Err(OptimizedBackendRegistrationError::Pin(
                BackendPinError::InvalidImageDigest,
            ));
        }
        if self.pin.guest_image_digest.as_deref() != Some(self.guest_image_digest.as_str()) {
            return Err(OptimizedBackendRegistrationError::Pin(
                BackendPinError::IdentityMismatch("guest_image_digest"),
            ));
        }
        let digest = Self::reference_vector_digest(&self.reference_vectors)?;
        if digest != self.pin.reference_vector_sha256 {
            return Err(OptimizedBackendRegistrationError::VectorDigest);
        }
        Ok(())
    }
}

fn validate_token(value: &str, field: &'static str) -> Result<(), BackendPinError> {
    if value.is_empty()
        || value.len() > MAX_BACKEND_TOKEN_LENGTH
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
    {
        return Err(BackendPinError::InvalidField(field));
    }
    Ok(())
}

fn validate_features(features: &[String]) -> Result<(), BackendPinError> {
    if features.len() > MAX_BACKEND_CPU_FEATURES {
        return Err(BackendPinError::TooManyCpuFeatures);
    }
    if features.iter().any(|feature| {
        feature.is_empty()
            || feature.len() > MAX_BACKEND_CPU_FEATURE_LENGTH
            || !feature.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
            })
    }) {
        return Err(BackendPinError::CpuFeatureTooLong);
    }
    if features.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(BackendPinError::FeaturesNotCanonical);
    }
    Ok(())
}

fn validate_thread_count(thread_count: u32) -> Result<(), BackendPinError> {
    if thread_count == 0 || thread_count > MAX_BACKEND_THREADS {
        return Err(BackendPinError::InvalidThreadCount);
    }
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

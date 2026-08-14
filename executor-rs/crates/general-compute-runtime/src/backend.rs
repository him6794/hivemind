//! Versioned optimized-backend identity pins.
//!
//! This module records the exact backend/version/CPU feature/thread tuple that
//! a future optimized implementation must report before it can be compared
//! with the bounded reference vectors. It does not load native libraries or
//! claim that an optimized backend is installed.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const BACKEND_PIN_PROTOCOL_VERSION: &str = "general-compute-backend-pin-v1";
pub const MAX_BACKEND_TOKEN_LENGTH: usize = 128;
pub const MAX_BACKEND_CPU_FEATURES: usize = 128;
pub const MAX_BACKEND_CPU_FEATURE_LENGTH: usize = 64;
pub const MAX_BACKEND_THREADS: u32 = 4096;

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

//! Pre-dispatch, single-chunk probe capabilities. These cannot authorize an
//! execution read. Peers must additionally validate the persisted probe live.

use crate::{
    artifact_read::{timestamp, validate_token_length, ArtifactReadScope, ArtifactReadTokenError},
    worker_execution::normalize_pem,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub const MAX_ARTIFACT_PROBE_TTL_SECONDS: u64 = 15;
const TOKEN_TYPE: &str = "hcode-artifact-probe+jwt";

#[derive(Debug)]
pub enum ArtifactProbeTokenError {
    Scope(ArtifactReadTokenError),
    Credential(jsonwebtoken::errors::Error),
    Invalid(&'static str),
}

impl fmt::Display for ArtifactProbeTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scope(error) => write!(f, "invalid artifact probe scope: {error}"),
            Self::Credential(_) => f.write_str("invalid artifact probe credential"),
            Self::Invalid(reason) => write!(f, "invalid artifact probe: {reason}"),
        }
    }
}
impl std::error::Error for ArtifactProbeTokenError {}
impl From<ArtifactReadTokenError> for ArtifactProbeTokenError {
    fn from(value: ArtifactReadTokenError) -> Self {
        Self::Scope(value)
    }
}
type Result<T> = std::result::Result<T, ArtifactProbeTokenError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ProbePurpose {
    #[serde(rename = "hcode-artifact-probe-v1")]
    Probe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactProbeClaims {
    purpose: ProbePurpose,
    pub aud: Uuid,
    pub lease_id: Uuid,
    pub scope: ArtifactReadScope,
    pub iat: u64,
    pub exp: u64,
}

impl ArtifactProbeClaims {
    fn validate(&self, now: u64) -> Result<()> {
        self.scope.validate()?;
        if self.aud.is_nil() || self.lease_id.is_nil() {
            return Err(ArtifactProbeTokenError::Invalid("missing source or probe"));
        }
        if self.iat > now
            || self.exp <= now
            || self.exp <= self.iat
            || self.exp - self.iat > MAX_ARTIFACT_PROBE_TTL_SECONDS
        {
            return Err(ArtifactProbeTokenError::Invalid("lifetime"));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ArtifactProbeTokenSigner {
    key: EncodingKey,
}
impl ArtifactProbeTokenSigner {
    pub fn from_pem(private: &str) -> Result<Self> {
        Ok(Self {
            key: EncodingKey::from_ed_pem(normalize_pem(private).as_bytes())
                .map_err(ArtifactProbeTokenError::Credential)?,
        })
    }
    pub fn sign(
        &self,
        source: Uuid,
        lease: Uuid,
        scope: ArtifactReadScope,
        valid_until: u64,
    ) -> Result<String> {
        let now = timestamp()?;
        let claims = ArtifactProbeClaims {
            purpose: ProbePurpose::Probe,
            aud: source,
            lease_id: lease,
            scope,
            iat: now,
            exp: now
                .saturating_add(MAX_ARTIFACT_PROBE_TTL_SECONDS)
                .min(valid_until),
        };
        claims.validate(now)?;
        let mut header = Header::new(Algorithm::EdDSA);
        header.typ = Some(TOKEN_TYPE.into());
        let token =
            encode(&header, &claims, &self.key).map_err(ArtifactProbeTokenError::Credential)?;
        validate_token_length(&token)?;
        Ok(token)
    }
}

#[derive(Clone)]
pub struct ArtifactProbeTokenVerifier {
    key: DecodingKey,
}
impl ArtifactProbeTokenVerifier {
    pub fn from_pem(public: &str) -> Result<Self> {
        Ok(Self {
            key: DecodingKey::from_ed_pem(normalize_pem(public).as_bytes())
                .map_err(ArtifactProbeTokenError::Credential)?,
        })
    }
    pub fn verify(&self, token: &str, source: Uuid) -> Result<ArtifactProbeClaims> {
        validate_token_length(token)?;
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.leeway = 0;
        validation.set_required_spec_claims(&["exp", "aud"]);
        validation.set_audience(&[source.to_string()]);
        let decoded = decode::<ArtifactProbeClaims>(token, &self.key, &validation)
            .map_err(ArtifactProbeTokenError::Credential)?;
        if decoded.header.typ.as_deref() != Some(TOKEN_TYPE) {
            return Err(ArtifactProbeTokenError::Invalid("token type"));
        }
        decoded.claims.validate(timestamp()?)?;
        Ok(decoded.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact_read::{ArtifactReadTokenSigner, ArtifactReadTokenVerifier};
    use crate::worker_execution::{WorkerExecutionSigner, WorkerExecutionVerifier};
    use hivemind_models::Claims;

    fn scope() -> ArtifactReadScope {
        ArtifactReadScope {
            task_id: "probe-task".into(),
            artifact_id: "model".into(),
            artifact_sha256: format!("sha256:{}", "a".repeat(64)),
            artifact_size_bytes: 123,
            offset: 0,
            chunk_sha256: format!("sha256:{}", "a".repeat(64)),
            chunk_size_bytes: 123,
        }
    }

    #[test]
    fn probe_token_is_bounded_and_disjoint_from_read_and_execution() {
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        let signer = ArtifactProbeTokenSigner::from_pem(&private).unwrap();
        let verifier = ArtifactProbeTokenVerifier::from_pem(&public).unwrap();
        let source = Uuid::new_v4();
        let lease = Uuid::new_v4();
        let token = signer.sign(source, lease, scope(), u64::MAX).unwrap();
        let claims = verifier.verify(&token, source).unwrap();
        assert_eq!(claims.exp - claims.iat, 15);
        assert_eq!(claims.lease_id, lease);
        assert_eq!(claims.scope, scope());
        assert!(verifier.verify(&token, Uuid::new_v4()).is_err());
        assert!(ArtifactReadTokenVerifier::from_pem(&public)
            .unwrap()
            .verify(&token, source)
            .is_err());
        assert!(WorkerExecutionVerifier::from_pem(&public)
            .unwrap()
            .decode_execution_claims(&token)
            .is_err());
        let read = ArtifactReadTokenSigner::from_pem(&private)
            .unwrap()
            .sign(source, lease, scope(), u64::MAX)
            .unwrap();
        assert!(verifier.verify(&read, source).is_err());
        let execution = WorkerExecutionSigner::from_pem(&private)
            .unwrap()
            .encode_claims(&Claims {
                sub: "owner".into(),
                user_id: "owner".into(),
                role: Some("worker-execution".into()),
                task_id: Some("probe-task".into()),
                worker_id: Some("worker".into()),
                iat: claims.iat as usize,
                exp: claims.exp as usize,
            })
            .unwrap();
        assert!(verifier.verify(&execution, source).is_err());
        let cap = claims.iat + 5;
        assert_eq!(
            verifier
                .verify(&signer.sign(source, lease, scope(), cap).unwrap(), source)
                .unwrap()
                .exp,
            cap
        );
        assert!(signer.sign(source, lease, scope(), 0).is_err());
        assert!(signer.sign(Uuid::nil(), lease, scope(), cap).is_err());
        assert!(signer.sign(source, Uuid::nil(), scope(), cap).is_err());
        assert!(verifier.verify(&"x".repeat(4097), source).is_err());
    }

    #[test]
    fn probe_token_rejects_signed_scope_purpose_header_and_lifetime_mutations() {
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        let signer = ArtifactProbeTokenSigner::from_pem(&private).unwrap();
        let verifier = ArtifactProbeTokenVerifier::from_pem(&public).unwrap();
        let source = Uuid::new_v4();
        let original = verifier
            .verify(
                &signer
                    .sign(source, Uuid::new_v4(), scope(), u64::MAX)
                    .unwrap(),
                source,
            )
            .unwrap();
        // Explicit payload types let each case change exactly one signed field.
        #[derive(Serialize)]
        struct RawScope {
            task_id: String,
            artifact_id: String,
            artifact_sha256: String,
            artifact_size_bytes: u64,
            offset: u64,
            chunk_sha256: String,
            chunk_size_bytes: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            url: Option<String>,
        }
        #[derive(Serialize)]
        struct RawClaims {
            purpose: String,
            aud: Uuid,
            lease_id: Uuid,
            scope: RawScope,
            iat: u64,
            exp: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            role: Option<String>,
        }
        let base = || RawClaims {
            purpose: "hcode-artifact-probe-v1".into(),
            aud: original.aud,
            lease_id: original.lease_id,
            scope: RawScope {
                task_id: original.scope.task_id.clone(),
                artifact_id: original.scope.artifact_id.clone(),
                artifact_sha256: original.scope.artifact_sha256.clone(),
                artifact_size_bytes: original.scope.artifact_size_bytes,
                offset: original.scope.offset,
                chunk_sha256: original.scope.chunk_sha256.clone(),
                chunk_size_bytes: original.scope.chunk_size_bytes,
                url: None,
            },
            iat: original.iat,
            exp: original.exp,
            role: None,
        };
        let probe_header = || {
            let mut header = Header::new(Algorithm::EdDSA);
            header.typ = Some(TOKEN_TYPE.into());
            header
        };
        for case in 0..12 {
            let mut claims = base();
            let mut header = probe_header();
            match case {
                0 => claims.purpose = "hcode-artifact-read-v1".into(),
                1 => claims.role = Some("user".into()),
                2 => claims.scope.url = Some("http://localhost".into()),
                3 => claims.scope.task_id = "../task".into(),
                4 => claims.scope.offset = 124,
                5 => claims.scope.chunk_size_bytes = 1_048_577,
                6 => claims.exp = original.iat,
                7 => claims.exp = original.iat + 16,
                8 => claims.iat = original.exp + 1,
                9 => claims.scope.chunk_sha256 = "bad".into(),
                10 => header.typ = Some("hcode-artifact-read+jwt".into()),
                _ => claims.lease_id = Uuid::nil(),
            }
            let token = encode(&header, &claims, &signer.key).unwrap();
            assert!(verifier.verify(&token, source).is_err(), "case {case}");
        }
        // The unmodified payload must pass, so each rejection above comes from its mutation.
        let token = encode(&probe_header(), &base(), &signer.key).unwrap();
        assert!(verifier.verify(&token, source).is_ok());
    }
}

//! Purpose-separated credentials for reading one P2P artifact chunk.
//!
//! Signature verification is not authorization to serve bytes. Nodepool must
//! resolve `lease_id` and recheck the live source, task, replica, generation,
//! expiry and manifest coordinates for every read. Never send an execution or
//! account token to an untrusted source peer.

use crate::worker_execution::normalize_pem;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const MAX_ARTIFACT_READ_TOKEN_BYTES: usize = 4096;
pub const MAX_ARTIFACT_READ_TOKEN_TTL_SECONDS: u64 = 60;
/// The v1 peer-read wire contract permits at most 1 MiB per capability.
pub const MAX_ARTIFACT_READ_CHUNK_BYTES: u64 = 1024 * 1024;
const TOKEN_TYPE: &str = "hcode-artifact-read+jwt";
const IDENTIFIER_MAX_BYTES: usize = 255;

#[derive(Debug)]
pub enum ArtifactReadTokenError {
    InvalidKey(jsonwebtoken::errors::Error),
    InvalidToken(jsonwebtoken::errors::Error),
    InvalidScope(&'static str),
    InvalidLifetime,
    InvalidHeader,
    InvalidTokenLength,
    ClockUnavailable,
}

impl fmt::Display for ArtifactReadTokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey(_) => formatter.write_str("invalid artifact-read Ed25519 key"),
            Self::InvalidToken(_) => formatter.write_str("invalid artifact-read token"),
            Self::InvalidScope(reason) => {
                write!(formatter, "invalid artifact-read scope: {reason}")
            }
            Self::InvalidLifetime => formatter.write_str("invalid artifact-read token lifetime"),
            Self::InvalidHeader => formatter.write_str("invalid artifact-read token type"),
            Self::InvalidTokenLength => formatter.write_str("invalid artifact-read token length"),
            Self::ClockUnavailable => formatter.write_str("artifact-read clock is unavailable"),
        }
    }
}

impl std::error::Error for ArtifactReadTokenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidKey(error) | Self::InvalidToken(error) => Some(error),
            _ => None,
        }
    }
}

type Result<T> = std::result::Result<T, ArtifactReadTokenError>;

/// Immutable coordinates copied from Nodepool's persisted manifest, not from
/// a peer's assertion. No path, URL, execution identity or account authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReadScope {
    pub task_id: String,
    pub artifact_id: String,
    pub artifact_sha256: String,
    pub artifact_size_bytes: u64,
    pub offset: u64,
    pub chunk_sha256: String,
    pub chunk_size_bytes: u64,
}

impl ArtifactReadScope {
    fn validate(&self) -> Result<()> {
        for (identifier, is_task) in [(&self.task_id, true), (&self.artifact_id, false)] {
            if identifier.is_empty()
                || identifier.len() > IDENTIFIER_MAX_BYTES
                || identifier == "."
                || identifier == ".."
                || (is_task && identifier.contains(".."))
                || !identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            {
                return Err(ArtifactReadTokenError::InvalidScope("unsafe identifier"));
            }
        }
        for digest in [&self.artifact_sha256, &self.chunk_sha256] {
            let valid = digest.strip_prefix("sha256:").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            });
            if !valid {
                return Err(ArtifactReadTokenError::InvalidScope("noncanonical SHA-256"));
            }
        }
        if self.artifact_size_bytes == 0
            || self.artifact_size_bytes > i64::MAX as u64
            || self.chunk_size_bytes == 0
            || self.chunk_size_bytes > MAX_ARTIFACT_READ_CHUNK_BYTES
            || self
                .offset
                .checked_add(self.chunk_size_bytes)
                .is_none_or(|end| end > self.artifact_size_bytes)
        {
            return Err(ArtifactReadTokenError::InvalidScope("invalid chunk range"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ReadPurpose {
    #[serde(rename = "hcode-artifact-read-v1")]
    ArtifactRead,
}

/// Authenticated claims only: the caller still needs the current Nodepool lease.
/// In particular, this is not a reusable offline authorization result.
///
/// Deliberately lacks `sub`, `user_id`, `role` and worker execution claims, so
/// existing account, enrollment and execution decoders cannot accept it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReadClaims {
    purpose: ReadPurpose,
    pub aud: Uuid,
    pub lease_id: Uuid,
    pub scope: ArtifactReadScope,
    pub iat: u64,
    pub exp: u64,
}

impl ArtifactReadClaims {
    fn validate(&self, now: u64) -> Result<()> {
        if self.aud.is_nil() || self.lease_id.is_nil() {
            return Err(ArtifactReadTokenError::InvalidScope(
                "missing source or lease",
            ));
        }
        self.scope.validate()?;
        if self.iat > now
            || self.exp <= now
            || self.exp <= self.iat
            || self.exp - self.iat > MAX_ARTIFACT_READ_TOKEN_TTL_SECONDS
        {
            return Err(ArtifactReadTokenError::InvalidLifetime);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ArtifactReadTokenSigner {
    key: EncodingKey,
}

impl ArtifactReadTokenSigner {
    pub fn from_pem(private_key: &str) -> Result<Self> {
        Ok(Self {
            key: EncodingKey::from_ed_pem(normalize_pem(private_key).as_bytes())
                .map_err(ArtifactReadTokenError::InvalidKey)?,
        })
    }

    /// `valid_until` must be the earliest live lease/assignment/artifact deadline
    /// established by Nodepool. The signer additionally caps it at 60 seconds.
    pub fn sign(
        &self,
        source_id: Uuid,
        lease_id: Uuid,
        scope: ArtifactReadScope,
        valid_until: u64,
    ) -> Result<String> {
        let now = timestamp()?;
        let exp = now
            .checked_add(MAX_ARTIFACT_READ_TOKEN_TTL_SECONDS)
            .ok_or(ArtifactReadTokenError::InvalidLifetime)?
            .min(valid_until);
        let claims = ArtifactReadClaims {
            purpose: ReadPurpose::ArtifactRead,
            aud: source_id,
            lease_id,
            scope,
            iat: now,
            exp,
        };
        claims.validate(now)?;
        let mut header = Header::new(Algorithm::EdDSA);
        header.typ = Some(TOKEN_TYPE.into());
        let token =
            encode(&header, &claims, &self.key).map_err(ArtifactReadTokenError::InvalidToken)?;
        validate_token_length(&token)?;
        Ok(token)
    }
}

#[derive(Clone)]
pub struct ArtifactReadTokenVerifier {
    key: DecodingKey,
}

impl ArtifactReadTokenVerifier {
    pub fn from_pem(public_key: &str) -> Result<Self> {
        Ok(Self {
            key: DecodingKey::from_ed_pem(normalize_pem(public_key).as_bytes())
                .map_err(ArtifactReadTokenError::InvalidKey)?,
        })
    }

    /// Verify provenance and scope, not live authorization. `source_id` is the
    /// source resolved by Nodepool, never a peer-supplied replacement audience.
    pub fn verify(&self, token: &str, source_id: Uuid) -> Result<ArtifactReadClaims> {
        validate_token_length(token)?;
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.leeway = 0;
        validation.set_required_spec_claims(&["exp", "aud"]);
        validation.set_audience(&[source_id.to_string()]);
        let decoded = decode::<ArtifactReadClaims>(token, &self.key, &validation)
            .map_err(ArtifactReadTokenError::InvalidToken)?;
        if decoded.header.typ.as_deref() != Some(TOKEN_TYPE) {
            return Err(ArtifactReadTokenError::InvalidHeader);
        }
        decoded.claims.validate(timestamp()?)?;
        Ok(decoded.claims)
    }
}

fn timestamp() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| ArtifactReadTokenError::ClockUnavailable)
}

fn validate_token_length(token: &str) -> Result<()> {
    if token.is_empty() || token.len() > MAX_ARTIFACT_READ_TOKEN_BYTES {
        return Err(ArtifactReadTokenError::InvalidTokenLength);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_execution::{WorkerExecutionSigner, WorkerExecutionVerifier};
    use hivemind_models::Claims;

    fn scope() -> ArtifactReadScope {
        ArtifactReadScope {
            task_id: "task-1".into(),
            artifact_id: "model".into(),
            artifact_sha256: format!("sha256:{}", "a".repeat(64)),
            artifact_size_bytes: MAX_ARTIFACT_READ_CHUNK_BYTES + 17,
            offset: MAX_ARTIFACT_READ_CHUNK_BYTES,
            chunk_sha256: format!("sha256:{}", "b".repeat(64)),
            chunk_size_bytes: 17,
        }
    }

    fn claims(source_id: Uuid) -> ArtifactReadClaims {
        let now = timestamp().unwrap();
        ArtifactReadClaims {
            purpose: ReadPurpose::ArtifactRead,
            aud: source_id,
            lease_id: Uuid::new_v4(),
            scope: scope(),
            iat: now,
            exp: now + MAX_ARTIFACT_READ_TOKEN_TTL_SECONDS,
        }
    }

    fn pair() -> (ArtifactReadTokenSigner, ArtifactReadTokenVerifier) {
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        (
            ArtifactReadTokenSigner::from_pem(&private).unwrap(),
            ArtifactReadTokenVerifier::from_pem(&public).unwrap(),
        )
    }

    fn raw_token(signer: &ArtifactReadTokenSigner, claims: &impl Serialize) -> String {
        let mut header = Header::new(Algorithm::EdDSA);
        header.typ = Some(TOKEN_TYPE.into());
        encode(&header, claims, &signer.key).unwrap()
    }

    #[test]
    fn artifact_read_token_roundtrip_binds_source_lease_and_all_coordinates() {
        let (signer, verifier) = pair();
        let source = Uuid::new_v4();
        let lease = Uuid::new_v4();
        let token = signer.sign(source, lease, scope(), u64::MAX).unwrap();
        let decoded = verifier.verify(&token, source).unwrap();
        assert_eq!(decoded.aud, source);
        assert_eq!(decoded.lease_id, lease);
        assert_eq!(decoded.scope, scope());
        assert_eq!(
            decoded.exp - decoded.iat,
            MAX_ARTIFACT_READ_TOKEN_TTL_SECONDS
        );
        assert!(token.len() <= MAX_ARTIFACT_READ_TOKEN_BYTES);
        assert!(verifier.verify(&token, Uuid::new_v4()).is_err());
        assert!(verifier.verify(&token, Uuid::nil()).is_err());
    }

    #[test]
    fn artifact_read_token_deadline_is_capped_and_cannot_be_already_expired() {
        let (signer, verifier) = pair();
        let source = Uuid::new_v4();
        let lease = Uuid::new_v4();
        let deadline = timestamp().unwrap() + 30;
        let token = signer.sign(source, lease, scope(), deadline).unwrap();
        assert_eq!(verifier.verify(&token, source).unwrap().exp, deadline);
        assert!(signer.sign(source, lease, scope(), 0).is_err());
        assert!(signer.sign(Uuid::nil(), lease, scope(), deadline).is_err());
        assert!(signer.sign(source, Uuid::nil(), scope(), deadline).is_err());
    }

    #[test]
    fn artifact_read_token_rejects_bad_ranges_ids_and_digests_on_sign_and_verify() {
        let (signer, verifier) = pair();
        let source = Uuid::new_v4();
        let good = claims(source);
        for invalid in [
            ArtifactReadScope {
                task_id: "../task".into(),
                ..scope()
            },
            ArtifactReadScope {
                task_id: " ".into(),
                ..scope()
            },
            ArtifactReadScope {
                task_id: "x".repeat(256),
                ..scope()
            },
            ArtifactReadScope {
                artifact_id: ".".into(),
                ..scope()
            },
            ArtifactReadScope {
                artifact_id: "模型".into(),
                ..scope()
            },
            ArtifactReadScope {
                artifact_id: String::new(),
                ..scope()
            },
            ArtifactReadScope {
                artifact_sha256: "sha256:bad".into(),
                ..scope()
            },
            ArtifactReadScope {
                chunk_sha256: format!("sha256:{}", "A".repeat(64)),
                ..scope()
            },
            ArtifactReadScope {
                chunk_size_bytes: 0,
                ..scope()
            },
            ArtifactReadScope {
                chunk_size_bytes: MAX_ARTIFACT_READ_CHUNK_BYTES + 1,
                ..scope()
            },
            ArtifactReadScope {
                artifact_size_bytes: 0,
                ..scope()
            },
            ArtifactReadScope {
                artifact_size_bytes: u64::MAX,
                ..scope()
            },
            ArtifactReadScope {
                offset: u64::MAX,
                ..scope()
            },
            ArtifactReadScope {
                offset: MAX_ARTIFACT_READ_CHUNK_BYTES + 1,
                ..scope()
            },
        ] {
            assert!(signer
                .sign(source, good.lease_id, invalid.clone(), u64::MAX)
                .is_err());
            let malformed = ArtifactReadClaims {
                scope: invalid,
                ..good.clone()
            };
            assert!(verifier
                .verify(&raw_token(&signer, &malformed), source)
                .is_err());
        }
        let max_ids = ArtifactReadScope {
            task_id: "x".repeat(IDENTIFIER_MAX_BYTES),
            artifact_id: "y".repeat(IDENTIFIER_MAX_BYTES),
            ..scope()
        };
        let token = signer
            .sign(source, good.lease_id, max_ids.clone(), u64::MAX)
            .unwrap();
        assert_eq!(verifier.verify(&token, source).unwrap().scope, max_ids);
    }

    #[test]
    fn artifact_read_token_verifier_rejects_invalid_lifetimes_without_grace() {
        let (signer, verifier) = pair();
        let source = Uuid::new_v4();
        let good = claims(source);
        let now = timestamp().unwrap();
        for (iat, exp) in [
            (now - 1, now),
            (now - 120, now - 60),
            (now, now + MAX_ARTIFACT_READ_TOKEN_TTL_SECONDS + 1),
            (now + 300, now + 301),
            (u64::MAX, u64::MAX),
            (now + 1, now),
        ] {
            let malformed = ArtifactReadClaims {
                iat,
                exp,
                ..good.clone()
            };
            assert!(verifier
                .verify(&raw_token(&signer, &malformed), source)
                .is_err());
        }
    }

    #[test]
    fn artifact_read_token_rejects_wrong_key_algorithm_header_and_tampering() {
        let (signer, verifier) = pair();
        let source = Uuid::new_v4();
        let good = claims(source);
        let token = raw_token(&signer, &good);
        let (_, other_verifier) = pair();
        assert!(other_verifier.verify(&token, source).is_err());
        let mut tampered = token.as_bytes().to_vec();
        let signature_start = token.rfind('.').unwrap() + 1;
        tampered[signature_start] = if tampered[signature_start] == b'A' {
            b'B'
        } else {
            b'A'
        };
        assert!(verifier
            .verify(&String::from_utf8(tampered).unwrap(), source)
            .is_err());
        for typ in [None, Some("JWT".into()), Some("worker-execution".into())] {
            let mut header = Header::new(Algorithm::EdDSA);
            header.typ = typ;
            let wrong = encode(&header, &good, &signer.key).unwrap();
            assert!(verifier.verify(&wrong, source).is_err());
        }
        let hmac = encode(
            &Header::default(),
            &good,
            &EncodingKey::from_secret(b"test-only-key"),
        )
        .unwrap();
        assert!(verifier.verify(&hmac, source).is_err());
        assert!(matches!(
            verifier.verify("", source),
            Err(ArtifactReadTokenError::InvalidTokenLength)
        ));
        assert!(matches!(
            verifier.verify(&"x".repeat(MAX_ARTIFACT_READ_TOKEN_BYTES + 1), source),
            Err(ArtifactReadTokenError::InvalidTokenLength)
        ));
    }

    #[derive(Serialize)]
    struct WireClaims<S> {
        purpose: &'static str,
        aud: Uuid,
        lease_id: Uuid,
        scope: S,
        iat: u64,
        exp: u64,
    }

    #[derive(Serialize)]
    struct ExtraClaim<T> {
        #[serde(flatten)]
        claims: T,
        sub: &'static str,
    }

    #[test]
    fn artifact_read_token_rejects_wrong_purpose_and_extra_authority_fields() {
        let (signer, verifier) = pair();
        let source = Uuid::new_v4();
        let good = claims(source);
        let wrong_purpose = WireClaims {
            purpose: "worker-execution",
            aud: source,
            lease_id: good.lease_id,
            scope: scope(),
            iat: good.iat,
            exp: good.exp,
        };
        assert!(verifier
            .verify(&raw_token(&signer, &wrong_purpose), source)
            .is_err());
        let extra = ExtraClaim {
            claims: good.clone(),
            sub: "owner",
        };
        assert!(verifier
            .verify(&raw_token(&signer, &extra), source)
            .is_err());
        let nested_extra = WireClaims {
            purpose: "hcode-artifact-read-v1",
            aud: source,
            lease_id: good.lease_id,
            scope: ExtraClaim {
                claims: scope(),
                sub: "owner",
            },
            iat: good.iat,
            exp: good.exp,
        };
        assert!(verifier
            .verify(&raw_token(&signer, &nested_extra), source)
            .is_err());
    }

    #[test]
    fn artifact_read_token_rejects_nil_wire_identities_and_missing_claims() {
        let (signer, verifier) = pair();
        let source = Uuid::new_v4();
        let good = claims(source);
        for malformed in [
            ArtifactReadClaims {
                aud: Uuid::nil(),
                ..good.clone()
            },
            ArtifactReadClaims {
                lease_id: Uuid::nil(),
                ..good.clone()
            },
        ] {
            assert!(verifier
                .verify(&raw_token(&signer, &malformed), malformed.aud)
                .is_err());
        }
        #[derive(Serialize)]
        struct IncompleteClaims {
            aud: Uuid,
            exp: u64,
        }
        let missing = IncompleteClaims {
            aud: source,
            exp: good.exp,
        };
        assert!(verifier
            .verify(&raw_token(&signer, &missing), source)
            .is_err());
    }

    #[test]
    fn artifact_read_token_accepts_valid_manifest_ids_and_full_size_chunks() {
        let (signer, verifier) = pair();
        let source = Uuid::new_v4();
        // Artifact IDs allow internal double dots; task IDs do not.
        let first = ArtifactReadScope {
            artifact_id: "model..weights".into(),
            offset: 0,
            chunk_size_bytes: MAX_ARTIFACT_READ_CHUNK_BYTES,
            ..scope()
        };
        let token = signer
            .sign(source, Uuid::new_v4(), first.clone(), u64::MAX)
            .unwrap();
        assert_eq!(verifier.verify(&token, source).unwrap().scope, first);
    }

    #[test]
    fn artifact_read_token_validates_keys_and_normalizes_escaped_pem() {
        assert!(ArtifactReadTokenSigner::from_pem("not a private key").is_err());
        assert!(ArtifactReadTokenVerifier::from_pem("not a public key").is_err());
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        let signer = ArtifactReadTokenSigner::from_pem(&private.replace('\n', "\\n")).unwrap();
        let verifier = ArtifactReadTokenVerifier::from_pem(&public.replace('\n', "\\n")).unwrap();
        let source = Uuid::new_v4();
        let token = signer
            .sign(source, Uuid::new_v4(), scope(), u64::MAX)
            .unwrap();
        assert_eq!(verifier.verify(&token, source).unwrap().scope, scope());
    }

    #[test]
    fn artifact_read_tokens_and_execution_tokens_are_not_interchangeable() {
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        let signer = ArtifactReadTokenSigner::from_pem(&private).unwrap();
        let verifier = ArtifactReadTokenVerifier::from_pem(&public).unwrap();
        let source = Uuid::new_v4();
        let token = signer
            .sign(source, Uuid::new_v4(), scope(), u64::MAX)
            .unwrap();
        let worker_verifier = WorkerExecutionVerifier::from_pem(&public).unwrap();
        assert!(worker_verifier.decode(&token).is_err());
        assert!(worker_verifier.decode_execution_claims(&token).is_err());
        // Payload isolation remains necessary even if an unrelated decoder stops
        // requiring audience validation in the future.
        let mut signature_only = Validation::new(Algorithm::EdDSA);
        signature_only.validate_aud = false;
        let key = DecodingKey::from_ed_pem(public.as_bytes()).unwrap();
        assert!(decode::<Claims>(&token, &key, &signature_only).is_err());
        assert!(decode::<crate::worker_execution::WorkerExecutionClaims>(
            &token,
            &key,
            &signature_only
        )
        .is_err());
        let jwt = crate::jwt_service::JwtService::new("artifact-isolation-test-secret", 1);
        assert!(jwt.validate(&token).is_err());
        assert!(crate::enrollment::parse_credential_id(&token).is_err());
        let now = timestamp().unwrap() as usize;
        for role in ["worker-execution", "worker", "user", "admin"] {
            let claims = Claims {
                sub: "owner".into(),
                user_id: "owner".into(),
                role: Some(role.into()),
                task_id: Some("task-1".into()),
                worker_id: Some("worker-1".into()),
                iat: now,
                exp: now + 300,
            };
            let execution_signer = WorkerExecutionSigner::from_pem(&private).unwrap();
            let identity = crate::worker_execution::WorkerExecutionIdentity {
                execution_id: "execution-1".into(),
                attempt_id: "round:replica".into(),
                idempotency_key: "request-1".into(),
                request_digest: "sha256:test".into(),
                transfer_generation: 1,
            };
            for other in [
                execution_signer.encode_claims(&claims).unwrap(),
                execution_signer
                    .encode_execution_claims(&claims, &identity)
                    .unwrap(),
                execution_signer
                    .encode_consensus_claims(&claims, &identity, "round", "replica", 1)
                    .unwrap(),
                jwt.encode_claims(&claims).unwrap(),
            ] {
                assert!(verifier.verify(&other, source).is_err());
            }
        }
        let enrollment = crate::enrollment::issue_credential(
            "owner",
            crate::enrollment::EnrollmentRole::Worker,
            "client-1",
        )
        .unwrap();
        assert!(verifier.verify(&enrollment.token, source).is_err());
    }
}

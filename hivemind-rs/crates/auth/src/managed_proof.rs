use anyhow::{Context, Result};
use chrono::Utc;
use hivemind_managed_prover_protocol::RemoteManagedProofRequest;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

pub const MANAGED_PROOF_AUTH_ISSUER: &str = "hivemind-nodepool";
pub const MANAGED_PROOF_AUTH_AUDIENCE: &str = "hivemind-managed-prover-service";
pub const MANAGED_PROOF_AUTH_ROLE: &str = "managed-proof-provider";
pub const MANAGED_PROOF_AUTH_PURPOSE: &str = "produce-managed-proof";
pub const MANAGED_PROOF_AUTH_TOKEN_MAX_BYTES: usize = 8 * 1024;

/// Nodepool-issued authorization for one proof request. This is intentionally
/// a separate signing domain from Worker execution tokens: it authorizes only
/// proof production, never task completion, settlement, or artifact access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedProofAuthorizationClaims {
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub role: String,
    pub purpose: String,
    pub protocol_version: u16,
    pub task_id: String,
    pub proof_task_id: String,
    pub worker_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub lease_generation: i64,
    pub runtime: String,
    pub backend_id: String,
    pub semantics_manifest_sha256: String,
    pub proof_scheme: String,
    pub image_id: Vec<u32>,
    pub deadline_unix_ms: i64,
    pub jti: String,
    pub exp: usize,
    pub iat: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedProofAuthorizationBinding {
    pub protocol_version: u16,
    pub task_id: String,
    pub proof_task_id: String,
    pub owner: String,
    pub worker_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub lease_generation: i64,
    pub runtime: String,
    pub backend_id: String,
    pub semantics_manifest_sha256: String,
    pub proof_scheme: String,
    pub image_id: [u32; 8],
    pub deadline_unix_ms: i64,
}

impl From<&RemoteManagedProofRequest> for ManagedProofAuthorizationBinding {
    fn from(request: &RemoteManagedProofRequest) -> Self {
        Self {
            protocol_version: request.protocol_version,
            task_id: request.task_id.clone(),
            proof_task_id: request.proof_task_id.clone(),
            owner: request.owner.clone(),
            worker_id: request.worker_id.clone(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            lease_generation: request.lease_generation,
            runtime: request.runtime.clone(),
            backend_id: request.backend_id.clone(),
            semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
            proof_scheme: request.proof_scheme.clone(),
            image_id: request.image_id,
            deadline_unix_ms: request.deadline_unix_ms,
        }
    }
}

impl ManagedProofAuthorizationClaims {
    pub fn validate_shape(&self) -> Result<()> {
        if self.iss != MANAGED_PROOF_AUTH_ISSUER {
            anyhow::bail!("managed proof authorization issuer is invalid");
        }
        if self.aud != MANAGED_PROOF_AUTH_AUDIENCE {
            anyhow::bail!("managed proof authorization audience is invalid");
        }
        if self.role != MANAGED_PROOF_AUTH_ROLE || self.purpose != MANAGED_PROOF_AUTH_PURPOSE {
            anyhow::bail!("managed proof authorization purpose is invalid");
        }
        if self.task_id.trim().is_empty()
            || self.proof_task_id.trim().is_empty()
            || self.sub.trim().is_empty()
            || self.worker_id.trim().is_empty()
            || self.execution_id.trim().is_empty()
            || self.attempt_id.trim().is_empty()
            || self.idempotency_key.trim().is_empty()
            || self.jti.trim().is_empty()
        {
            anyhow::bail!("managed proof authorization identity is incomplete");
        }
        if self.request_digest.trim().is_empty()
            || self.runtime.trim().is_empty()
            || self.proof_scheme.trim().is_empty()
            || self.deadline_unix_ms <= 0
            || self.lease_generation <= 0
            || self.image_id.len() != 8
        {
            anyhow::bail!("managed proof authorization binding is invalid");
        }
        if self.exp == 0 || self.iat == 0 || self.exp < self.iat {
            anyhow::bail!("managed proof authorization lifetime is invalid");
        }
        Ok(())
    }

    pub fn binds_request(&self, request: &RemoteManagedProofRequest) -> Result<()> {
        self.validate_shape()?;
        request.validate().map_err(|error| anyhow::anyhow!(error))?;
        if self.protocol_version != request.protocol_version
            || self.task_id != request.task_id
            || self.proof_task_id != request.proof_task_id
            || self.sub != request.owner
            || self.worker_id != request.worker_id
            || self.execution_id != request.execution_id
            || self.attempt_id != request.attempt_id
            || self.idempotency_key != request.idempotency_key
            || self.request_digest != request.request_digest
            || self.lease_generation != request.lease_generation
            || self.runtime != request.runtime
            || self.backend_id != request.backend_id
            || self.semantics_manifest_sha256 != request.semantics_manifest_sha256
            || self.proof_scheme != request.proof_scheme
            || self.image_id != request.image_id.to_vec()
            || self.deadline_unix_ms != request.deadline_unix_ms
        {
            anyhow::bail!("managed proof authorization does not match the request");
        }
        let expiry_ms = i64::try_from(self.exp)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1_000))
            .ok_or_else(|| anyhow::anyhow!("managed proof authorization expiry is invalid"))?;
        if expiry_ms < request.deadline_unix_ms {
            anyhow::bail!("managed proof authorization expires before the request deadline");
        }
        Ok(())
    }

    pub fn binds_binding(&self, binding: &ManagedProofAuthorizationBinding) -> Result<()> {
        self.validate_shape()?;
        if self.protocol_version != binding.protocol_version
            || self.task_id != binding.task_id
            || self.proof_task_id != binding.proof_task_id
            || self.sub != binding.owner
            || self.worker_id != binding.worker_id
            || self.execution_id != binding.execution_id
            || self.attempt_id != binding.attempt_id
            || self.idempotency_key != binding.idempotency_key
            || self.request_digest != binding.request_digest
            || self.lease_generation != binding.lease_generation
            || self.runtime != binding.runtime
            || self.backend_id != binding.backend_id
            || self.semantics_manifest_sha256 != binding.semantics_manifest_sha256
            || self.proof_scheme != binding.proof_scheme
            || self.image_id != binding.image_id.to_vec()
            || self.deadline_unix_ms != binding.deadline_unix_ms
        {
            anyhow::bail!(
                "managed proof authorization does not match the persisted request binding"
            );
        }
        let expiry_ms = i64::try_from(self.exp)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1_000))
            .ok_or_else(|| anyhow::anyhow!("managed proof authorization expiry is invalid"))?;
        if expiry_ms < binding.deadline_unix_ms {
            anyhow::bail!("managed proof authorization expires before the request deadline");
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ManagedProofAuthorizationSigner {
    encoding_key: EncodingKey,
}

impl ManagedProofAuthorizationSigner {
    pub fn from_pem(private_key_pem: &str) -> Result<Self> {
        let pem = normalize_pem(private_key_pem);
        let encoding_key = EncodingKey::from_ed_pem(pem.as_bytes())
            .context("managed proof authorization private key is not valid Ed25519 PEM")?;
        Ok(Self { encoding_key })
    }

    pub fn encode(&self, claims: &ManagedProofAuthorizationClaims) -> Result<String> {
        claims.validate_shape()?;
        let mut header = Header::new(Algorithm::EdDSA);
        header.typ = Some("JWT".into());
        let token = encode(&header, claims, &self.encoding_key)
            .context("failed to encode managed proof authorization")?;
        if token.len() > MANAGED_PROOF_AUTH_TOKEN_MAX_BYTES {
            anyhow::bail!("managed proof authorization exceeds the token limit");
        }
        Ok(token)
    }
}

#[derive(Clone)]
pub struct ManagedProofAuthorizationVerifier {
    decoding_key: DecodingKey,
}

impl ManagedProofAuthorizationVerifier {
    pub fn from_pem(public_key_pem: &str) -> Result<Self> {
        let pem = normalize_pem(public_key_pem);
        let decoding_key = DecodingKey::from_ed_pem(pem.as_bytes())
            .context("managed proof authorization public key is not valid Ed25519 PEM")?;
        Ok(Self { decoding_key })
    }

    pub fn decode(&self, token: &str) -> Result<ManagedProofAuthorizationClaims> {
        if token.len() > MANAGED_PROOF_AUTH_TOKEN_MAX_BYTES {
            anyhow::bail!("managed proof authorization exceeds the token limit");
        }
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.validate_exp = true;
        validation.set_audience(&[MANAGED_PROOF_AUTH_AUDIENCE]);
        validation.set_issuer(&[MANAGED_PROOF_AUTH_ISSUER]);
        let token_data =
            decode::<ManagedProofAuthorizationClaims>(token, &self.decoding_key, &validation)
                .context("failed to decode managed proof authorization")?;
        token_data.claims.validate_shape()?;
        Ok(token_data.claims)
    }

    pub fn decode_for_request(
        &self,
        token: &str,
        request: &RemoteManagedProofRequest,
    ) -> Result<ManagedProofAuthorizationClaims> {
        let claims = self.decode(token)?;
        claims.binds_request(request)?;
        Ok(claims)
    }
}

pub fn new_claims(
    request: &RemoteManagedProofRequest,
    jti: impl Into<String>,
    now: chrono::DateTime<Utc>,
    lifetime: chrono::Duration,
) -> Result<ManagedProofAuthorizationClaims> {
    request.validate().map_err(|error| anyhow::anyhow!(error))?;
    let iat = usize::try_from(now.timestamp()).context("authorization issue time is invalid")?;
    let exp_time = now
        .checked_add_signed(lifetime)
        .context("authorization lifetime is invalid")?;
    let exp = usize::try_from(exp_time.timestamp()).context("authorization expiry is invalid")?;
    let claims = ManagedProofAuthorizationClaims {
        iss: MANAGED_PROOF_AUTH_ISSUER.into(),
        aud: MANAGED_PROOF_AUTH_AUDIENCE.into(),
        sub: request.owner.clone(),
        role: MANAGED_PROOF_AUTH_ROLE.into(),
        purpose: MANAGED_PROOF_AUTH_PURPOSE.into(),
        protocol_version: request.protocol_version,
        task_id: request.task_id.clone(),
        proof_task_id: request.proof_task_id.clone(),
        worker_id: request.worker_id.clone(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        lease_generation: request.lease_generation,
        runtime: request.runtime.clone(),
        backend_id: request.backend_id.clone(),
        semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
        proof_scheme: request.proof_scheme.clone(),
        image_id: request.image_id.to_vec(),
        deadline_unix_ms: request.deadline_unix_ms,
        jti: jti.into(),
        exp,
        iat,
    };
    claims.validate_shape()?;
    Ok(claims)
}

fn normalize_pem(value: &str) -> String {
    value.trim().replace("\\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_config::generate_worker_execution_test_key_pair;

    fn request() -> RemoteManagedProofRequest {
        RemoteManagedProofRequest {
            protocol_version: 1,
            task_id: "task-1".into(),
            proof_task_id: "task-1".into(),
            owner: "owner-1".into(),
            worker_id: "worker-1".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "proof-1".into(),
            request_digest: String::new(),
            lease_generation: 1,
            runtime: "managed-function-v0".into(),
            backend_id: String::new(),
            semantics_manifest_sha256: String::new(),
            source: "return input;".into(),
            input: r#"{"value":42}"#.into(),
            max_usage_units: 100,
            proof_scheme: "risc0-zkvm-3.0.6".into(),
            image_id: [1; 8],
            deadline_unix_ms: Utc::now().timestamp_millis() + 300_000,
        }
        .with_computed_digest()
        .unwrap()
    }

    #[test]
    fn dedicated_token_round_trip_binds_request() {
        let request = request();
        let (private_key, public_key) = generate_worker_execution_test_key_pair();
        let signer = ManagedProofAuthorizationSigner::from_pem(&private_key).unwrap();
        let verifier = ManagedProofAuthorizationVerifier::from_pem(&public_key).unwrap();
        let claims = new_claims(&request, "jti-1", Utc::now(), chrono::Duration::hours(1)).unwrap();
        let token = signer.encode(&claims).unwrap();
        let decoded = verifier.decode_for_request(&token, &request).unwrap();
        assert_eq!(decoded.jti, "jti-1");
        assert_eq!(decoded.purpose, MANAGED_PROOF_AUTH_PURPOSE);
    }

    #[test]
    fn token_rejects_wrong_audience_and_request_binding() {
        let request = request();
        let (private_key, public_key) = generate_worker_execution_test_key_pair();
        let signer = ManagedProofAuthorizationSigner::from_pem(&private_key).unwrap();
        let verifier = ManagedProofAuthorizationVerifier::from_pem(&public_key).unwrap();
        let mut claims =
            new_claims(&request, "jti-1", Utc::now(), chrono::Duration::hours(1)).unwrap();
        claims.aud = "wrong-audience".into();
        assert!(signer.encode(&claims).is_err());

        let claims = new_claims(&request, "jti-2", Utc::now(), chrono::Duration::hours(1)).unwrap();
        let token = signer.encode(&claims).unwrap();
        let mut changed = request.clone();
        changed.worker_id = "worker-2".into();
        changed.request_digest = changed.compute_digest().unwrap();
        assert!(verifier.decode_for_request(&token, &changed).is_err());
    }
}

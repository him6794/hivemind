use anyhow::{Context, Result};
use hivemind_models::Claims;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};

/// Sample/dev platform public key embedded in official worker packages so end
/// users can verify nodepool-issued execution tokens without pasting a secret.
///
/// Production platforms should replace the matching private key via
/// `WORKER_EXECUTION_PRIVATE_KEY_PEM`. Self-hosted platforms can override the
/// public key on workers with `WORKER_EXECUTION_PUBLIC_KEY_PEM`.
pub const DEFAULT_WORKER_EXECUTION_PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\n\
MCowBQYDK2VwAyEAfG12U4EBcWCj7yKaZUhlUmPvRtLEAZshKvN2WyL7EPs=\n\
-----END PUBLIC KEY-----\n";

/// Matching sample/dev private key for local compose and unit tests only.
pub const SAMPLE_WORKER_EXECUTION_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MC4CAQAwBQYDK2VwBCIEICKHh+VEGAfiiOPJJzI7afT5yro9vY5hldaNtGSXSDhY\n\
-----END PRIVATE KEY-----\n";

/// Signs worker-execution JWTs with the platform Ed25519 private key.
#[derive(Clone)]
pub struct WorkerExecutionSigner {
    encoding_key: EncodingKey,
}

impl WorkerExecutionSigner {
    pub fn from_pem(private_key_pem: &str) -> Result<Self> {
        let pem = normalize_pem(private_key_pem);
        let encoding_key = EncodingKey::from_ed_pem(pem.as_bytes())
            .context("WORKER_EXECUTION_PRIVATE_KEY_PEM is not a valid Ed25519 private key PEM")?;
        Ok(Self { encoding_key })
    }

    pub fn encode_claims(&self, claims: &Claims) -> Result<String> {
        let mut header = Header::new(Algorithm::EdDSA);
        header.typ = Some("JWT".into());
        encode(&header, claims, &self.encoding_key)
            .context("Failed to encode worker execution token")
    }
}

/// Verifies worker-execution JWTs with the platform Ed25519 public key.
#[derive(Clone)]
pub struct WorkerExecutionVerifier {
    decoding_key: DecodingKey,
}

impl WorkerExecutionVerifier {
    pub fn from_pem(public_key_pem: &str) -> Result<Self> {
        let pem = normalize_pem(public_key_pem);
        let decoding_key = DecodingKey::from_ed_pem(pem.as_bytes())
            .context("WORKER_EXECUTION_PUBLIC_KEY_PEM is not a valid Ed25519 public key PEM")?;
        Ok(Self { decoding_key })
    }

    pub fn default_platform() -> Result<Self> {
        Self::from_pem(DEFAULT_WORKER_EXECUTION_PUBLIC_KEY_PEM)
    }

    pub fn decode(&self, token: &str) -> Result<Claims> {
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.validate_exp = true;
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)
            .context("Failed to decode worker execution token")?;
        Ok(token_data.claims)
    }
}

/// Accept either real newlines or `\n`-escaped PEM blobs from env files.
pub fn normalize_pem(value: &str) -> String {
    value.trim().replace("\\n", "\n")
}

pub fn validate_private_key_pem(private_key_pem: &str) -> Result<()> {
    let pem = private_key_pem.trim();
    if pem.is_empty() {
        anyhow::bail!("WORKER_EXECUTION_PRIVATE_KEY_PEM must be set to a non-default value");
    }
    WorkerExecutionSigner::from_pem(pem).map(|_| ())
}

pub fn validate_public_key_pem(public_key_pem: &str) -> Result<()> {
    let pem = public_key_pem.trim();
    if pem.is_empty() {
        anyhow::bail!("WORKER_EXECUTION_PUBLIC_KEY_PEM must be set to a non-default value");
    }
    WorkerExecutionVerifier::from_pem(pem).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_claims() -> Claims {
        let now = Utc::now().timestamp() as usize;
        Claims {
            sub: "task-owner".into(),
            user_id: "task-owner".into(),
            role: Some("worker-execution".into()),
            task_id: Some("task-1".into()),
            worker_id: Some("worker-7".into()),
            exp: now + 300,
            iat: now,
        }
    }

    #[test]
    fn ed25519_roundtrip_binds_task_and_worker() {
        let signer =
            WorkerExecutionSigner::from_pem(SAMPLE_WORKER_EXECUTION_PRIVATE_KEY_PEM).unwrap();
        let verifier =
            WorkerExecutionVerifier::from_pem(DEFAULT_WORKER_EXECUTION_PUBLIC_KEY_PEM).unwrap();
        let token = signer.encode_claims(&sample_claims()).unwrap();
        let claims = verifier.decode(&token).unwrap();
        assert_eq!(claims.role.as_deref(), Some("worker-execution"));
        assert_eq!(claims.task_id.as_deref(), Some("task-1"));
        assert_eq!(claims.worker_id.as_deref(), Some("worker-7"));
    }

    #[test]
    fn hmac_token_is_rejected_by_public_key_verifier() {
        let claims = sample_claims();
        let hmac_token = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(b"unit-test-worker-execution-secret-32-bytes"),
        )
        .unwrap();
        let verifier = WorkerExecutionVerifier::default_platform().unwrap();
        assert!(verifier.decode(&hmac_token).is_err());
    }
}

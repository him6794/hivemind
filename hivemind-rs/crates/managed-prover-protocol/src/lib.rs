use serde::{de::IgnoredAny, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const MANAGED_PROVER_PROTOCOL_VERSION: u16 = 1;

pub const MAX_TASK_ID_BYTES: usize = 255;
pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_USAGE_UNITS: u64 = 1_000_000;

pub const MAX_PROOF_SCHEME_BYTES: usize = 64;
pub const MAX_JOURNAL_BYTES: usize = 4 * 1024;
pub const MAX_RECEIPT_JSON_BYTES: usize = 2 * 1024 * 1024;

/// Conservative cap for the encoded request. A source byte may require a
/// six-byte JSON escape, so this bound is intentionally higher than the sum of
/// the decoded field limits.
pub const MAX_REQUEST_JSON_BYTES: usize =
    512 + 6 * (MAX_TASK_ID_BYTES + MAX_SOURCE_BYTES + MAX_INPUT_BYTES);

/// Cap for the encoded response. Nested valid JSON may double in size when it
/// is escaped as a string, while each journal byte needs at most four JSON
/// bytes (three digits and a comma).
pub const MAX_RESPONSE_JSON_BYTES: usize =
    1024 + 6 * MAX_PROOF_SCHEME_BYTES + 4 * MAX_JOURNAL_BYTES + 2 * MAX_RECEIPT_JSON_BYTES;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedProverRequest {
    pub protocol_version: u16,
    pub task_id: String,
    pub source: String,
    pub input: String,
    pub max_usage_units: u64,
}

impl ManagedProverRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version)?;

        if self.task_id.len() > MAX_TASK_ID_BYTES {
            return Err(ProtocolError::TaskIdTooLarge {
                received: self.task_id.len(),
                limit: MAX_TASK_ID_BYTES,
            });
        }
        if !is_safe_task_id(&self.task_id) {
            return Err(ProtocolError::UnsafeTaskId);
        }

        if self.source.trim().is_empty() {
            return Err(ProtocolError::EmptySource);
        }
        if self.source.len() > MAX_SOURCE_BYTES {
            return Err(ProtocolError::SourceTooLarge {
                received: self.source.len(),
                limit: MAX_SOURCE_BYTES,
            });
        }

        if self.input.trim().is_empty() {
            return Err(ProtocolError::EmptyInput);
        }
        if self.input.len() > MAX_INPUT_BYTES {
            return Err(ProtocolError::InputTooLarge {
                received: self.input.len(),
                limit: MAX_INPUT_BYTES,
            });
        }
        if !is_valid_json(&self.input) {
            return Err(ProtocolError::InvalidInputJson);
        }

        if !(1..=MAX_USAGE_UNITS).contains(&self.max_usage_units) {
            return Err(ProtocolError::InvalidUsageBudget);
        }

        Ok(())
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|_| ProtocolError::InvalidRequestJson)?;
        if encoded.len() > MAX_REQUEST_JSON_BYTES {
            return Err(ProtocolError::RequestJsonTooLarge {
                received: encoded.len(),
                limit: MAX_REQUEST_JSON_BYTES,
            });
        }
        Ok(encoded)
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_REQUEST_JSON_BYTES {
            return Err(ProtocolError::RequestJsonTooLarge {
                received: bytes.len(),
                limit: MAX_REQUEST_JSON_BYTES,
            });
        }
        let request: Self =
            serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidRequestJson)?;
        request.validate()?;
        Ok(request)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedProverResponse {
    pub protocol_version: u16,
    pub proof_scheme: String,
    pub image_id: [u32; 8],
    pub journal: Vec<u8>,
    pub receipt_json: String,
}

impl ManagedProverResponse {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version)?;

        if self.proof_scheme.trim().is_empty() {
            return Err(ProtocolError::EmptyProofScheme);
        }
        if self.proof_scheme.len() > MAX_PROOF_SCHEME_BYTES {
            return Err(ProtocolError::ProofSchemeTooLarge {
                received: self.proof_scheme.len(),
                limit: MAX_PROOF_SCHEME_BYTES,
            });
        }

        if self.journal.is_empty() {
            return Err(ProtocolError::EmptyJournal);
        }
        if self.journal.len() > MAX_JOURNAL_BYTES {
            return Err(ProtocolError::JournalTooLarge {
                received: self.journal.len(),
                limit: MAX_JOURNAL_BYTES,
            });
        }

        if self.receipt_json.trim().is_empty() {
            return Err(ProtocolError::EmptyReceiptJson);
        }
        if self.receipt_json.len() > MAX_RECEIPT_JSON_BYTES {
            return Err(ProtocolError::ReceiptJsonTooLarge {
                received: self.receipt_json.len(),
                limit: MAX_RECEIPT_JSON_BYTES,
            });
        }
        if !is_valid_json(&self.receipt_json) {
            return Err(ProtocolError::InvalidReceiptJson);
        }

        Ok(())
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|_| ProtocolError::InvalidResponseJson)?;
        if encoded.len() > MAX_RESPONSE_JSON_BYTES {
            return Err(ProtocolError::ResponseJsonTooLarge {
                received: encoded.len(),
                limit: MAX_RESPONSE_JSON_BYTES,
            });
        }
        Ok(encoded)
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_RESPONSE_JSON_BYTES {
            return Err(ProtocolError::ResponseJsonTooLarge {
                received: bytes.len(),
                limit: MAX_RESPONSE_JSON_BYTES,
            });
        }
        let response: Self =
            serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidResponseJson)?;
        response.validate()?;
        Ok(response)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ProtocolError {
    #[error("unsupported managed prover protocol version {received}")]
    UnsupportedVersion { received: u16 },
    #[error("managed prover task id is not safe")]
    UnsafeTaskId,
    #[error("managed prover task id is too large: {received} bytes exceeds {limit}")]
    TaskIdTooLarge { received: usize, limit: usize },
    #[error("managed prover source is empty")]
    EmptySource,
    #[error("managed prover source is too large: {received} bytes exceeds {limit}")]
    SourceTooLarge { received: usize, limit: usize },
    #[error("managed prover input is empty")]
    EmptyInput,
    #[error("managed prover input is too large: {received} bytes exceeds {limit}")]
    InputTooLarge { received: usize, limit: usize },
    #[error("managed prover input is not valid JSON")]
    InvalidInputJson,
    #[error("managed prover usage budget is outside the supported range")]
    InvalidUsageBudget,
    #[error("managed prover proof scheme is empty")]
    EmptyProofScheme,
    #[error("managed prover proof scheme is too large: {received} bytes exceeds {limit}")]
    ProofSchemeTooLarge { received: usize, limit: usize },
    #[error("managed prover journal is empty")]
    EmptyJournal,
    #[error("managed prover journal is too large: {received} bytes exceeds {limit}")]
    JournalTooLarge { received: usize, limit: usize },
    #[error("managed prover receipt JSON is empty")]
    EmptyReceiptJson,
    #[error("managed prover receipt JSON is too large: {received} bytes exceeds {limit}")]
    ReceiptJsonTooLarge { received: usize, limit: usize },
    #[error("managed prover receipt is not valid JSON")]
    InvalidReceiptJson,
    #[error("managed prover request JSON is invalid")]
    InvalidRequestJson,
    #[error("managed prover request JSON is too large: {received} bytes exceeds {limit}")]
    RequestJsonTooLarge { received: usize, limit: usize },
    #[error("managed prover response JSON is invalid")]
    InvalidResponseJson,
    #[error("managed prover response JSON is too large: {received} bytes exceeds {limit}")]
    ResponseJsonTooLarge { received: usize, limit: usize },
    #[error("remote managed proof protocol version is unsupported: {received}")]
    UnsupportedRemoteVersion { received: u16 },
    #[error("remote managed proof identity field is empty: {field}")]
    EmptyRemoteIdentity { field: &'static str },
    #[error("remote managed proof identity field is too large: {field}")]
    RemoteIdentityTooLarge { field: &'static str },
    #[error("remote managed proof request has an invalid digest")]
    InvalidRemoteDigest,
    #[error("remote managed proof request digest does not match its canonical fields")]
    RemoteDigestMismatch,
    #[error("remote managed proof request has an invalid lease generation")]
    InvalidRemoteLeaseGeneration,
    #[error("remote managed proof request has an invalid runtime binding")]
    InvalidRemoteRuntime,
    #[error("remote managed proof production DSL binding is incomplete")]
    InvalidRemoteDslBinding,
    #[error("remote managed proof image id must contain eight words")]
    InvalidRemoteImageId,
    #[error("remote managed proof deadline is invalid")]
    InvalidRemoteDeadline,
    #[error("remote managed proof request JSON is too large: {received} bytes exceeds {limit}")]
    RemoteRequestJsonTooLarge { received: usize, limit: usize },
}

fn validate_version(protocol_version: u16) -> Result<(), ProtocolError> {
    if protocol_version != MANAGED_PROVER_PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion {
            received: protocol_version,
        });
    }
    Ok(())
}

fn is_safe_task_id(task_id: &str) -> bool {
    if let Some(digest) = task_id.strip_prefix("dsl-proof-v1:") {
        return digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit());
    }

    !task_id.is_empty()
        && task_id != "."
        && !task_id.contains("..")
        && task_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn is_valid_json(json: &str) -> bool {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    IgnoredAny::deserialize(&mut deserializer).is_ok() && deserializer.end().is_ok()
}

/// Versioned, authenticated-provider payload layered around the local v1
/// sidecar request. The authorization token is deliberately not part of this
/// structure; it is transported in gRPC metadata and binds this digest.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteManagedProofRequest {
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
    pub source: String,
    pub input: String,
    pub max_usage_units: u64,
    pub proof_scheme: String,
    pub image_id: [u32; 8],
    pub deadline_unix_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UnsignedRemoteManagedProofRequest<'a> {
    protocol_version: u16,
    task_id: &'a str,
    proof_task_id: &'a str,
    owner: &'a str,
    worker_id: &'a str,
    execution_id: &'a str,
    attempt_id: &'a str,
    idempotency_key: &'a str,
    lease_generation: i64,
    runtime: &'a str,
    backend_id: &'a str,
    semantics_manifest_sha256: &'a str,
    source: &'a str,
    input: &'a str,
    max_usage_units: u64,
    proof_scheme: &'a str,
    image_id: [u32; 8],
    deadline_unix_ms: i64,
}

pub const REMOTE_MANAGED_PROOF_PROTOCOL_VERSION: u16 = 1;
pub const REMOTE_MANAGED_PROOF_DOMAIN: &str = "hivemind-managed-proof-remote-request-v1";
pub const MAX_REMOTE_IDENTITY_BYTES: usize = 255;
pub const MAX_REMOTE_RUNTIME_BYTES: usize = 64;
pub const MAX_REMOTE_BACKEND_BYTES: usize = 255;
pub const MAX_REMOTE_SEMANTICS_DIGEST_BYTES: usize = 71;
pub const MAX_REMOTE_REQUEST_JSON_BYTES: usize = 8 * 1024 * 1024;

impl RemoteManagedProofRequest {
    /// Fill the digest after all unsigned fields have been constructed.
    pub fn with_computed_digest(mut self) -> Result<Self, ProtocolError> {
        self.request_digest = self.compute_digest()?;
        self.validate()?;
        Ok(self)
    }

    pub fn compute_digest(&self) -> Result<String, ProtocolError> {
        let unsigned = UnsignedRemoteManagedProofRequest {
            protocol_version: self.protocol_version,
            task_id: &self.task_id,
            proof_task_id: &self.proof_task_id,
            owner: &self.owner,
            worker_id: &self.worker_id,
            execution_id: &self.execution_id,
            attempt_id: &self.attempt_id,
            idempotency_key: &self.idempotency_key,
            lease_generation: self.lease_generation,
            runtime: &self.runtime,
            backend_id: &self.backend_id,
            semantics_manifest_sha256: &self.semantics_manifest_sha256,
            source: &self.source,
            input: &self.input,
            max_usage_units: self.max_usage_units,
            proof_scheme: &self.proof_scheme,
            image_id: self.image_id,
            deadline_unix_ms: self.deadline_unix_ms,
        };
        let encoded =
            serde_json::to_vec(&unsigned).map_err(|_| ProtocolError::InvalidRequestJson)?;
        let mut hasher = Sha256::new();
        hasher.update(REMOTE_MANAGED_PROOF_DOMAIN.as_bytes());
        hasher.update([0]);
        hasher.update(encoded);
        Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_version != REMOTE_MANAGED_PROOF_PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedRemoteVersion {
                received: self.protocol_version,
            });
        }
        for (field, value) in [
            ("task_id", self.task_id.as_str()),
            ("proof_task_id", self.proof_task_id.as_str()),
            ("owner", self.owner.as_str()),
            ("worker_id", self.worker_id.as_str()),
            ("execution_id", self.execution_id.as_str()),
            ("attempt_id", self.attempt_id.as_str()),
            ("idempotency_key", self.idempotency_key.as_str()),
        ] {
            validate_remote_identity(field, value)?;
        }
        if !is_safe_task_id(&self.task_id) || !is_safe_task_id(&self.proof_task_id) {
            return Err(ProtocolError::UnsafeTaskId);
        }
        if self.runtime.len() > MAX_REMOTE_RUNTIME_BYTES
            || self.runtime.trim().is_empty()
            || !matches!(
                self.runtime.as_str(),
                "managed-function-v0" | "production_sandboxed_dsl"
            )
        {
            return Err(ProtocolError::InvalidRemoteRuntime);
        }
        if self.runtime == "production_sandboxed_dsl" {
            if self.backend_id.trim().is_empty()
                || self.semantics_manifest_sha256.trim().is_empty()
                || !is_sha256_digest(&self.semantics_manifest_sha256)
            {
                return Err(ProtocolError::InvalidRemoteDslBinding);
            }
        } else if !self.backend_id.is_empty() || !self.semantics_manifest_sha256.is_empty() {
            return Err(ProtocolError::InvalidRemoteDslBinding);
        }
        if self.backend_id.len() > MAX_REMOTE_BACKEND_BYTES
            || self.semantics_manifest_sha256.len() > MAX_REMOTE_SEMANTICS_DIGEST_BYTES
        {
            return Err(ProtocolError::RemoteIdentityTooLarge {
                field: "backend/semantics",
            });
        }
        if self.lease_generation <= 0 {
            return Err(ProtocolError::InvalidRemoteLeaseGeneration);
        }
        if self.deadline_unix_ms <= 0 {
            return Err(ProtocolError::InvalidRemoteDeadline);
        }
        let sidecar_request = ManagedProverRequest {
            protocol_version: MANAGED_PROVER_PROTOCOL_VERSION,
            task_id: self.proof_task_id.clone(),
            source: self.source.clone(),
            input: self.input.clone(),
            max_usage_units: self.max_usage_units,
        };
        sidecar_request.validate()?;
        if self.proof_scheme.trim().is_empty() || self.proof_scheme.len() > MAX_PROOF_SCHEME_BYTES {
            return Err(ProtocolError::EmptyProofScheme);
        }
        if self.image_id.len() != 8 {
            return Err(ProtocolError::InvalidRemoteImageId);
        }
        if !is_sha256_digest(&self.request_digest) {
            return Err(ProtocolError::InvalidRemoteDigest);
        }
        if self.request_digest != self.compute_digest()? {
            return Err(ProtocolError::RemoteDigestMismatch);
        }
        Ok(())
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|_| ProtocolError::InvalidRequestJson)?;
        if encoded.len() > MAX_REMOTE_REQUEST_JSON_BYTES {
            return Err(ProtocolError::RemoteRequestJsonTooLarge {
                received: encoded.len(),
                limit: MAX_REMOTE_REQUEST_JSON_BYTES,
            });
        }
        Ok(encoded)
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_REMOTE_REQUEST_JSON_BYTES {
            return Err(ProtocolError::RemoteRequestJsonTooLarge {
                received: bytes.len(),
                limit: MAX_REMOTE_REQUEST_JSON_BYTES,
            });
        }
        let request: Self =
            serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidRequestJson)?;
        request.validate()?;
        Ok(request)
    }
}

fn validate_remote_identity(field: &'static str, value: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(ProtocolError::EmptyRemoteIdentity { field });
    }
    if value.len() > MAX_REMOTE_IDENTITY_BYTES {
        return Err(ProtocolError::RemoteIdentityTooLarge { field });
    }
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex_value) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex_value.len() == 64 && hex_value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod remote_request_tests {
    use super::{RemoteManagedProofRequest, REMOTE_MANAGED_PROOF_PROTOCOL_VERSION};

    fn valid_request() -> RemoteManagedProofRequest {
        RemoteManagedProofRequest {
            protocol_version: REMOTE_MANAGED_PROOF_PROTOCOL_VERSION,
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
            deadline_unix_ms: 4_000_000_000_000,
        }
    }

    #[test]
    fn canonical_digest_round_trip_binds_all_fields() {
        let request = valid_request()
            .with_computed_digest()
            .expect("valid request");
        let encoded = request.to_json_bytes().expect("request encodes");
        let decoded =
            RemoteManagedProofRequest::from_json_bytes(&encoded).expect("request decodes");
        assert_eq!(decoded, request);

        let mut changed = request.clone();
        changed.worker_id = "worker-2".into();
        assert!(changed.validate().is_err());
    }

    #[test]
    fn digest_cannot_be_self_declared_or_unknown_fields_added() {
        let request = valid_request()
            .with_computed_digest()
            .expect("valid request");
        let mut value = serde_json::to_value(request).expect("value");
        value["request_digest"] =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
        assert!(RemoteManagedProofRequest::from_json_bytes(
            &serde_json::to_vec(&value).expect("json")
        )
        .is_err());

        let request = valid_request()
            .with_computed_digest()
            .expect("valid request");
        let mut value = serde_json::to_value(request).expect("value");
        value["unexpected"] = true.into();
        assert!(RemoteManagedProofRequest::from_json_bytes(
            &serde_json::to_vec(&value).expect("json")
        )
        .is_err());
    }

    #[test]
    fn production_dsl_requires_backend_and_semantics_binding() {
        let mut request = valid_request();
        request.runtime = "production_sandboxed_dsl".into();
        assert!(request.clone().with_computed_digest().is_err());

        request.backend_id = "managed-default".into();
        request.semantics_manifest_sha256 =
            "sha256:8ed716dc07c7bc9abcfc5338b1888e71dd041c3fb397c45d0efb1ff76af1deee".into();
        request.proof_task_id =
            "dsl-proof-v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
        assert!(request.with_computed_digest().is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ManagedProverRequest, ManagedProverResponse, ProtocolError,
        MANAGED_PROVER_PROTOCOL_VERSION, MAX_INPUT_BYTES, MAX_JOURNAL_BYTES,
        MAX_PROOF_SCHEME_BYTES, MAX_RECEIPT_JSON_BYTES, MAX_REQUEST_JSON_BYTES,
        MAX_RESPONSE_JSON_BYTES, MAX_SOURCE_BYTES, MAX_TASK_ID_BYTES, MAX_USAGE_UNITS,
    };

    fn valid_request() -> ManagedProverRequest {
        ManagedProverRequest {
            protocol_version: MANAGED_PROVER_PROTOCOL_VERSION,
            task_id: "task-123".into(),
            source: "return input;".into(),
            input: r#"{"value":42}"#.into(),
            max_usage_units: 1_000,
        }
    }

    fn valid_response() -> ManagedProverResponse {
        ManagedProverResponse {
            protocol_version: MANAGED_PROVER_PROTOCOL_VERSION,
            proof_scheme: "backend-v1".into(),
            image_id: [1, 2, 3, 4, 5, 6, 7, 8],
            journal: vec![1, 2, 3],
            receipt_json: r#"{"receipt":true}"#.into(),
        }
    }

    #[test]
    fn request_json_round_trip_preserves_the_versioned_contract() {
        let request = valid_request();

        let encoded = request.to_json_bytes().expect("request encodes");
        let decoded = ManagedProverRequest::from_json_bytes(&encoded).expect("request decodes");

        assert_eq!(decoded, request);
        assert!(!encoded.ends_with(b"\n"));
    }

    #[test]
    fn request_accepts_exact_field_and_budget_limits() {
        let request = ManagedProverRequest {
            task_id: "a".repeat(MAX_TASK_ID_BYTES),
            source: "s".repeat(MAX_SOURCE_BYTES),
            input: format!(r#"{{"padding":"{}"}}"#, "i".repeat(MAX_INPUT_BYTES - 14)),
            max_usage_units: MAX_USAGE_UNITS,
            ..valid_request()
        };

        assert_eq!(request.input.len(), MAX_INPUT_BYTES);
        assert_eq!(request.validate(), Ok(()));
    }

    #[test]
    fn request_rejects_wrong_version_and_out_of_range_budget() {
        let mut request = valid_request();
        request.protocol_version += 1;
        assert_eq!(
            request.validate(),
            Err(ProtocolError::UnsupportedVersion {
                received: MANAGED_PROVER_PROTOCOL_VERSION + 1,
            })
        );

        request = valid_request();
        request.max_usage_units = 0;
        assert_eq!(request.validate(), Err(ProtocolError::InvalidUsageBudget));
        request.max_usage_units = MAX_USAGE_UNITS + 1;
        assert_eq!(request.validate(), Err(ProtocolError::InvalidUsageBudget));
    }

    #[test]
    fn request_rejects_unsafe_or_oversized_task_ids() {
        for task_id in ["", ".", "..", "../escape", "has/slash", "not ascii"] {
            let mut request = valid_request();
            request.task_id = task_id.into();
            assert_eq!(request.validate(), Err(ProtocolError::UnsafeTaskId));
        }

        let mut request = valid_request();
        request.task_id = "a".repeat(MAX_TASK_ID_BYTES + 1);
        assert_eq!(
            request.validate(),
            Err(ProtocolError::TaskIdTooLarge {
                received: MAX_TASK_ID_BYTES + 1,
                limit: MAX_TASK_ID_BYTES,
            })
        );

        request.task_id = format!("dsl-proof-v1:{}", "a".repeat(64));
        assert!(request.validate().is_ok());
    }

    #[test]
    fn request_rejects_blank_or_oversized_source() {
        let mut request = valid_request();
        request.source = " \r\n\t".into();
        assert_eq!(request.validate(), Err(ProtocolError::EmptySource));

        request.source = "s".repeat(MAX_SOURCE_BYTES + 1);
        assert_eq!(
            request.validate(),
            Err(ProtocolError::SourceTooLarge {
                received: MAX_SOURCE_BYTES + 1,
                limit: MAX_SOURCE_BYTES,
            })
        );
    }

    #[test]
    fn request_rejects_blank_oversized_or_invalid_json_input() {
        let mut request = valid_request();
        request.input = " \r\n\t".into();
        assert_eq!(request.validate(), Err(ProtocolError::EmptyInput));

        request.input = "i".repeat(MAX_INPUT_BYTES + 1);
        assert_eq!(
            request.validate(),
            Err(ProtocolError::InputTooLarge {
                received: MAX_INPUT_BYTES + 1,
                limit: MAX_INPUT_BYTES,
            })
        );

        request.input = "not-json".into();
        assert_eq!(request.validate(), Err(ProtocolError::InvalidInputJson));
    }

    #[test]
    fn request_decoder_is_bounded_and_denies_unknown_fields() {
        let oversized = vec![b' '; MAX_REQUEST_JSON_BYTES + 1];
        assert_eq!(
            ManagedProverRequest::from_json_bytes(&oversized),
            Err(ProtocolError::RequestJsonTooLarge {
                received: MAX_REQUEST_JSON_BYTES + 1,
                limit: MAX_REQUEST_JSON_BYTES,
            })
        );

        let mut value = serde_json::to_value(valid_request()).unwrap();
        value["unexpected"] = true.into();
        assert_eq!(
            ManagedProverRequest::from_json_bytes(&serde_json::to_vec(&value).unwrap()),
            Err(ProtocolError::InvalidRequestJson)
        );
    }

    #[test]
    fn response_json_round_trip_preserves_the_versioned_contract() {
        let response = valid_response();

        let encoded = response.to_json_bytes().expect("response encodes");
        let decoded = ManagedProverResponse::from_json_bytes(&encoded).expect("response decodes");

        assert_eq!(decoded, response);
        assert!(!encoded.ends_with(b"\n"));
    }

    #[test]
    fn response_accepts_exact_field_limits() {
        let response = ManagedProverResponse {
            proof_scheme: "s".repeat(MAX_PROOF_SCHEME_BYTES),
            journal: vec![0; MAX_JOURNAL_BYTES],
            receipt_json: format!(
                r#"{{"padding":"{}"}}"#,
                "r".repeat(MAX_RECEIPT_JSON_BYTES - 14)
            ),
            ..valid_response()
        };

        assert_eq!(response.receipt_json.len(), MAX_RECEIPT_JSON_BYTES);
        assert_eq!(response.validate(), Ok(()));
    }

    #[test]
    fn response_rejects_wrong_version_and_invalid_proof_scheme() {
        let mut response = valid_response();
        response.protocol_version += 1;
        assert_eq!(
            response.validate(),
            Err(ProtocolError::UnsupportedVersion {
                received: MANAGED_PROVER_PROTOCOL_VERSION + 1,
            })
        );

        response = valid_response();
        response.proof_scheme = " \r\n".into();
        assert_eq!(response.validate(), Err(ProtocolError::EmptyProofScheme));
        response.proof_scheme = "s".repeat(MAX_PROOF_SCHEME_BYTES + 1);
        assert_eq!(
            response.validate(),
            Err(ProtocolError::ProofSchemeTooLarge {
                received: MAX_PROOF_SCHEME_BYTES + 1,
                limit: MAX_PROOF_SCHEME_BYTES,
            })
        );
    }

    #[test]
    fn response_rejects_empty_or_oversized_journal() {
        let mut response = valid_response();
        response.journal.clear();
        assert_eq!(response.validate(), Err(ProtocolError::EmptyJournal));

        response.journal = vec![0; MAX_JOURNAL_BYTES + 1];
        assert_eq!(
            response.validate(),
            Err(ProtocolError::JournalTooLarge {
                received: MAX_JOURNAL_BYTES + 1,
                limit: MAX_JOURNAL_BYTES,
            })
        );
    }

    #[test]
    fn response_rejects_empty_oversized_or_invalid_receipt_json() {
        let mut response = valid_response();
        response.receipt_json = " \r\n".into();
        assert_eq!(response.validate(), Err(ProtocolError::EmptyReceiptJson));

        response.receipt_json = "r".repeat(MAX_RECEIPT_JSON_BYTES + 1);
        assert_eq!(
            response.validate(),
            Err(ProtocolError::ReceiptJsonTooLarge {
                received: MAX_RECEIPT_JSON_BYTES + 1,
                limit: MAX_RECEIPT_JSON_BYTES,
            })
        );

        response.receipt_json = "not-json".into();
        assert_eq!(response.validate(), Err(ProtocolError::InvalidReceiptJson));
    }

    #[test]
    fn response_decoder_is_bounded_and_denies_unknown_fields() {
        let oversized = vec![b' '; MAX_RESPONSE_JSON_BYTES + 1];
        assert_eq!(
            ManagedProverResponse::from_json_bytes(&oversized),
            Err(ProtocolError::ResponseJsonTooLarge {
                received: MAX_RESPONSE_JSON_BYTES + 1,
                limit: MAX_RESPONSE_JSON_BYTES,
            })
        );

        let mut value = serde_json::to_value(valid_response()).unwrap();
        value["unexpected"] = true.into();
        assert_eq!(
            ManagedProverResponse::from_json_bytes(&serde_json::to_vec(&value).unwrap()),
            Err(ProtocolError::InvalidResponseJson)
        );
    }
}

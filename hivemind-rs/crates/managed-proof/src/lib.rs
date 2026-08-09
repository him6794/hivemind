use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PROOF_PROTOCOL_VERSION: u16 = 1;
pub const MANAGED_RUNTIME_ID: &str = "managed-function-v0";
pub const COST_MODEL_ID: &str = "managed-function-v0-metering-v1";
#[cfg(feature = "risc0-verifier")]
pub const RISC0_PROOF_SCHEME: &str = "risc0-zkvm-3.0.6";
const RISC0_IMAGE_ID_WORDS: usize = 8;
#[cfg(feature = "risc0-verifier")]
pub const RISC0_MAX_JOURNAL_BYTES: usize = 4 * 1024;
#[cfg(feature = "risc0-verifier")]
pub const RISC0_MAX_RECEIPT_JSON_BYTES: usize = 2 * 1024 * 1024;
#[cfg(feature = "risc0-verifier")]
pub const RISC0_MAX_COMPOSITE_SEGMENTS: usize = 1;
#[cfg(feature = "risc0-verifier")]
pub const RISC0_MAX_SEGMENT_SEAL_WORDS: usize = 131_072;
#[cfg(feature = "risc0-verifier")]
const RISC0_SEGMENT_HASH_FUNCTION: &str = "poseidon2";
pub const RISC0_MANAGED_GUEST_ID: [u32; RISC0_IMAGE_ID_WORDS] = [
    3_606_400_121,
    4_250_889_949,
    2_277_454_476,
    3_430_793_801,
    2_111_044_864,
    2_713_379_816,
    851_522_248,
    2_751_351_423,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionMetrics {
    pub usage_units: u64,
    pub executed_ops: u64,
    pub function_calls: u64,
    pub loop_iterations: u64,
    pub max_call_depth: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionClaim {
    pub protocol_version: u16,
    pub runtime_id: String,
    pub cost_model_id: String,
    pub task_id: String,
    pub source_sha256: [u8; 32],
    pub input_sha256: [u8; 32],
    pub output_sha256: [u8; 32],
    pub max_usage_units: u64,
    pub usage_units: u64,
    pub executed_ops: u64,
    pub function_calls: u64,
    pub loop_iterations: u64,
    pub max_call_depth: u64,
    pub output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClaimError {
    #[error(
        "execution used {usage_units} units, exceeding the bound budget of {max_usage_units} units"
    )]
    UsageExceedsBudget {
        usage_units: u64,
        max_usage_units: u64,
    },
}

#[cfg(feature = "risc0-verifier")]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Risc0VerificationError {
    #[error("unsupported proof scheme")]
    UnsupportedProofScheme,
    #[error("proof image id must contain 8 words, got {received}")]
    InvalidImageIdLength { received: usize },
    #[error("proof image id does not match the trusted managed guest")]
    UntrustedImageId,
    #[error("proof journal is too large: {received} bytes exceeds {limit} bytes")]
    JournalTooLarge { received: usize, limit: usize },
    #[error("proof receipt is too large: {received} bytes exceeds {limit} bytes")]
    ReceiptTooLarge { received: usize, limit: usize },
    #[error("proof receipt is not valid RISC Zero receipt JSON")]
    InvalidReceipt,
    #[error("proof receipt kind is not supported")]
    UnsupportedReceiptKind,
    #[error("proof receipt must contain a composite segment")]
    MissingCompositeSegments,
    #[error("proof receipt assumptions are not supported")]
    UnsupportedAssumptions,
    #[error("proof receipt has too many segments: {received} exceeds {limit}")]
    TooManyCompositeSegments { received: usize, limit: usize },
    #[error("proof segment at position {position} has unexpected index {received}")]
    InvalidSegmentIndex { position: usize, received: u32 },
    #[error("proof segment hash function is not supported")]
    UnsupportedSegmentHashFunction,
    #[error("proof segment seal at position {position} is too large: {received} exceeds {limit}")]
    SegmentSealTooLarge {
        position: usize,
        received: usize,
        limit: usize,
    },
    #[error("proof envelope journal does not match the receipt journal")]
    JournalMismatch,
    #[error("RISC Zero receipt failed cryptographic verification")]
    InvalidProof,
    #[error("verified receipt journal is not a valid managed execution claim")]
    InvalidClaim,
}

#[cfg(feature = "risc0-verifier")]
fn decode_claim_candidate(journal: &[u8]) -> Result<ExecutionClaim, Risc0VerificationError> {
    ExecutionClaim::from_journal_bytes(journal).map_err(|_| Risc0VerificationError::InvalidClaim)
}

#[cfg(feature = "risc0-verifier")]
fn build_production_risc0_verifier_context() -> risc0_zkvm::VerifierContext {
    use risc0_zkvm::{SegmentReceiptVerifierParameters, VerifierContext};

    VerifierContext::empty()
        .with_segment_verifier_parameters(SegmentReceiptVerifierParameters::default())
}

#[cfg(feature = "risc0-verifier")]
thread_local! {
    static PRODUCTION_RISC0_VERIFIER_CONTEXT: risc0_zkvm::VerifierContext =
        build_production_risc0_verifier_context();
}

#[cfg(feature = "risc0-verifier")]
fn with_production_risc0_verifier_context<T>(
    operation: impl FnOnce(&risc0_zkvm::VerifierContext) -> T,
) -> T {
    PRODUCTION_RISC0_VERIFIER_CONTEXT.with(operation)
}

#[cfg(feature = "risc0-verifier")]
fn validate_risc0_receipt_shape(
    receipt: &risc0_zkvm::Receipt,
) -> Result<(), Risc0VerificationError> {
    let risc0_zkvm::InnerReceipt::Composite(composite) = &receipt.inner else {
        return Err(Risc0VerificationError::UnsupportedReceiptKind);
    };
    if composite.segments.is_empty() {
        return Err(Risc0VerificationError::MissingCompositeSegments);
    }
    if composite.segments.len() > RISC0_MAX_COMPOSITE_SEGMENTS {
        return Err(Risc0VerificationError::TooManyCompositeSegments {
            received: composite.segments.len(),
            limit: RISC0_MAX_COMPOSITE_SEGMENTS,
        });
    }
    for (position, segment) in composite.segments.iter().enumerate() {
        let expected = u32::try_from(position).expect("segment limit fits in u32");
        if segment.index != expected {
            return Err(Risc0VerificationError::InvalidSegmentIndex {
                position,
                received: segment.index,
            });
        }
        if segment.hashfn != RISC0_SEGMENT_HASH_FUNCTION {
            return Err(Risc0VerificationError::UnsupportedSegmentHashFunction);
        }
        if segment.seal.len() > RISC0_MAX_SEGMENT_SEAL_WORDS {
            return Err(Risc0VerificationError::SegmentSealTooLarge {
                position,
                received: segment.seal.len(),
                limit: RISC0_MAX_SEGMENT_SEAL_WORDS,
            });
        }
    }
    if !composite.assumption_receipts.is_empty() {
        return Err(Risc0VerificationError::UnsupportedAssumptions);
    }
    let output = composite
        .segments
        .last()
        .and_then(|segment| segment.claim.output.as_value().ok())
        .and_then(Option::as_ref)
        .ok_or(Risc0VerificationError::UnsupportedAssumptions)?;
    let assumptions = output
        .assumptions
        .as_value()
        .map_err(|_| Risc0VerificationError::UnsupportedAssumptions)?;
    if !assumptions.0.is_empty() {
        return Err(Risc0VerificationError::UnsupportedAssumptions);
    }
    Ok(())
}

#[cfg(feature = "risc0-verifier")]
pub fn verify_risc0_proof_envelope(
    envelope: &hivemind_proto::ManagedProofEnvelope,
) -> Result<ExecutionClaim, Risc0VerificationError> {
    if envelope.proof_scheme != RISC0_PROOF_SCHEME {
        return Err(Risc0VerificationError::UnsupportedProofScheme);
    }
    if envelope.image_id.len() != RISC0_IMAGE_ID_WORDS {
        return Err(Risc0VerificationError::InvalidImageIdLength {
            received: envelope.image_id.len(),
        });
    }
    if envelope.image_id.as_slice() != RISC0_MANAGED_GUEST_ID {
        return Err(Risc0VerificationError::UntrustedImageId);
    }
    if envelope.journal.len() > RISC0_MAX_JOURNAL_BYTES {
        return Err(Risc0VerificationError::JournalTooLarge {
            received: envelope.journal.len(),
            limit: RISC0_MAX_JOURNAL_BYTES,
        });
    }
    if envelope.receipt_json.len() > RISC0_MAX_RECEIPT_JSON_BYTES {
        return Err(Risc0VerificationError::ReceiptTooLarge {
            received: envelope.receipt_json.len(),
            limit: RISC0_MAX_RECEIPT_JSON_BYTES,
        });
    }
    let receipt: risc0_zkvm::Receipt = serde_json::from_slice(&envelope.receipt_json)
        .map_err(|_| Risc0VerificationError::InvalidReceipt)?;
    validate_risc0_receipt_shape(&receipt)?;
    if envelope.journal != receipt.journal.bytes {
        return Err(Risc0VerificationError::JournalMismatch);
    }
    let claim = decode_claim_candidate(&envelope.journal)?;
    with_production_risc0_verifier_context(|context| {
        receipt.verify_with_context(context, RISC0_MANAGED_GUEST_ID)
    })
    .map_err(|_| Risc0VerificationError::InvalidProof)?;
    Ok(claim)
}

impl ExecutionClaim {
    pub fn new(
        task_id: impl Into<String>,
        source: &[u8],
        input: &[u8],
        output: &[u8],
        max_usage_units: u64,
        metrics: ExecutionMetrics,
    ) -> Result<Self, ClaimError> {
        if metrics.usage_units > max_usage_units {
            return Err(ClaimError::UsageExceedsBudget {
                usage_units: metrics.usage_units,
                max_usage_units,
            });
        }

        Ok(Self {
            protocol_version: PROOF_PROTOCOL_VERSION,
            runtime_id: MANAGED_RUNTIME_ID.to_owned(),
            cost_model_id: COST_MODEL_ID.to_owned(),
            task_id: task_id.into(),
            source_sha256: sha256(source),
            input_sha256: sha256(input),
            output_sha256: sha256(output),
            max_usage_units,
            usage_units: metrics.usage_units,
            executed_ops: metrics.executed_ops,
            function_calls: metrics.function_calls,
            loop_iterations: metrics.loop_iterations,
            max_call_depth: metrics.max_call_depth,
            output_bytes: u64::try_from(output.len()).expect("output length fits in u64"),
        })
    }

    pub fn to_journal_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    pub fn from_journal_bytes(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }
}

fn sha256(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

#[cfg(test)]
mod tests {
    use super::{ClaimError, ExecutionClaim, ExecutionMetrics, PROOF_PROTOCOL_VERSION};

    fn metrics() -> ExecutionMetrics {
        ExecutionMetrics {
            usage_units: 17,
            executed_ops: 17,
            function_calls: 2,
            loop_iterations: 3,
            max_call_depth: 1,
        }
    }

    fn claim(task_id: &str) -> ExecutionClaim {
        ExecutionClaim::new(
            task_id,
            b"fn main(input) { return len(input); }",
            br#"{"value":"secret"}"#,
            br#"{"result":6}"#,
            100,
            metrics(),
        )
        .expect("valid execution claim")
    }

    #[test]
    fn claim_binds_task_program_input_output_and_protocol() {
        let original = claim("task-a");

        assert_eq!(original.protocol_version, PROOF_PROTOCOL_VERSION);
        assert_ne!(original.source_sha256, [0; 32]);
        assert_ne!(original.input_sha256, [0; 32]);
        assert_ne!(original.output_sha256, [0; 32]);
        assert_eq!(original.output_bytes, 12);

        assert_ne!(original, claim("task-b"));
        assert_ne!(
            original,
            ExecutionClaim::new(
                "task-a",
                b"fn main(input) { return 0; }",
                br#"{"value":"secret"}"#,
                br#"{"result":6}"#,
                100,
                metrics(),
            )
            .unwrap()
        );
        assert_ne!(
            original,
            ExecutionClaim::new(
                "task-a",
                b"fn main(input) { return len(input); }",
                br#"{"value":"changed"}"#,
                br#"{"result":6}"#,
                100,
                metrics(),
            )
            .unwrap()
        );
        assert_ne!(
            original,
            ExecutionClaim::new(
                "task-a",
                b"fn main(input) { return len(input); }",
                br#"{"value":"secret"}"#,
                br#"{"result":7}"#,
                100,
                metrics(),
            )
            .unwrap()
        );
    }

    #[test]
    fn claim_rejects_usage_above_the_bound_budget() {
        let error =
            ExecutionClaim::new("task-a", b"return 1;", b"null", b"1", 16, metrics()).unwrap_err();

        assert_eq!(
            error,
            ClaimError::UsageExceedsBudget {
                usage_units: 17,
                max_usage_units: 16,
            }
        );
    }

    #[test]
    fn journal_encoding_is_deterministic_and_round_trips() {
        let claim = claim("task-a");
        let first = claim.to_journal_bytes().unwrap();
        let second = claim.to_journal_bytes().unwrap();

        assert_eq!(first, second);
        assert_eq!(ExecutionClaim::from_journal_bytes(&first).unwrap(), claim);
    }
}

#[cfg(all(test, feature = "risc0-verifier"))]
mod risc0_verifier_tests {
    use hivemind_proto::ManagedProofEnvelope;
    use risc0_zkvm::{Assumptions, FakeReceipt, InnerReceipt, MaybePruned, Receipt, ReceiptClaim};
    use serde::Deserialize;

    use super::{
        decode_claim_candidate, verify_risc0_proof_envelope, ExecutionClaim, Risc0VerificationError,
    };

    #[derive(Deserialize)]
    struct ProofFixture {
        proof_scheme: String,
        image_id: [u32; 8],
        journal: Vec<u8>,
        receipt: serde_json::Value,
    }

    #[test]
    fn verifier_rejects_untrusted_scheme_before_receipt_decode() {
        let envelope = ManagedProofEnvelope {
            proof_scheme: "worker-selected-verifier".into(),
            image_id: vec![0; 8],
            journal: b"not-a-claim".to_vec(),
            receipt_json: b"not-a-receipt".to_vec(),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::UnsupportedProofScheme);
    }

    #[test]
    fn verifier_rejects_invalid_image_id_length_before_receipt_decode() {
        let envelope = ManagedProofEnvelope {
            proof_scheme: super::RISC0_PROOF_SCHEME.into(),
            image_id: vec![0; 7],
            journal: b"not-a-claim".to_vec(),
            receipt_json: b"not-a-receipt".to_vec(),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(
            error,
            Risc0VerificationError::InvalidImageIdLength { received: 7 }
        );
    }

    #[test]
    fn verifier_rejects_untrusted_image_id_before_receipt_decode() {
        let envelope = ManagedProofEnvelope {
            proof_scheme: super::RISC0_PROOF_SCHEME.into(),
            image_id: vec![0; 8],
            journal: b"not-a-claim".to_vec(),
            receipt_json: b"not-a-receipt".to_vec(),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::UntrustedImageId);
    }

    #[test]
    fn verifier_rejects_oversized_journal_before_receipt_decode() {
        let envelope = ManagedProofEnvelope {
            proof_scheme: super::RISC0_PROOF_SCHEME.into(),
            image_id: super::RISC0_MANAGED_GUEST_ID.to_vec(),
            journal: vec![0; 4_097],
            receipt_json: b"not-a-receipt".to_vec(),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(
            error,
            Risc0VerificationError::JournalTooLarge {
                received: 4_097,
                limit: 4_096,
            }
        );
    }

    #[test]
    fn verifier_rejects_oversized_receipt_before_json_decode() {
        let envelope = ManagedProofEnvelope {
            proof_scheme: super::RISC0_PROOF_SCHEME.into(),
            image_id: super::RISC0_MANAGED_GUEST_ID.to_vec(),
            journal: Vec::new(),
            receipt_json: vec![0; 2 * 1024 * 1024 + 1],
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(
            error,
            Risc0VerificationError::ReceiptTooLarge {
                received: 2 * 1024 * 1024 + 1,
                limit: 2 * 1024 * 1024,
            }
        );
    }

    #[test]
    fn verifier_rejects_invalid_receipt_json() {
        let envelope = ManagedProofEnvelope {
            proof_scheme: super::RISC0_PROOF_SCHEME.into(),
            image_id: super::RISC0_MANAGED_GUEST_ID.to_vec(),
            journal: b"not-a-claim".to_vec(),
            receipt_json: b"not-a-receipt".to_vec(),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::InvalidReceipt);
    }

    #[test]
    fn verifier_rejects_envelope_journal_mismatch() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut journal = fixture.journal;
        journal[0] ^= 1;
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal,
            receipt_json: serde_json::to_vec(&fixture.receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::JournalMismatch);
    }

    #[test]
    fn verifier_rejects_fake_receipt_when_dev_mode_is_disabled() {
        let journal = b"not-a-claim-yet".to_vec();
        let claim = ReceiptClaim::ok(super::RISC0_MANAGED_GUEST_ID, journal.clone());
        let receipt = Receipt::new(InnerReceipt::Fake(FakeReceipt::new(claim)), journal.clone());
        let envelope = ManagedProofEnvelope {
            proof_scheme: super::RISC0_PROOF_SCHEME.into(),
            image_id: super::RISC0_MANAGED_GUEST_ID.to_vec(),
            journal,
            receipt_json: serde_json::to_vec(&receipt).expect("fake receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::UnsupportedReceiptKind);
    }

    #[test]
    fn verifier_rejects_fake_receipt_without_panicking_when_dev_mode_env_is_set() {
        let journal = b"not-a-claim-yet".to_vec();
        let claim = ReceiptClaim::ok(super::RISC0_MANAGED_GUEST_ID, journal.clone());
        let receipt = Receipt::new(InnerReceipt::Fake(FakeReceipt::new(claim)), journal.clone());
        let envelope = ManagedProofEnvelope {
            proof_scheme: super::RISC0_PROOF_SCHEME.into(),
            image_id: super::RISC0_MANAGED_GUEST_ID.to_vec(),
            journal,
            receipt_json: serde_json::to_vec(&receipt).expect("fake receipt serializes"),
        };
        let previous = std::env::var_os("RISC0_DEV_MODE");
        std::env::set_var("RISC0_DEV_MODE", "1");

        let verification = std::panic::catch_unwind(|| verify_risc0_proof_envelope(&envelope));

        match previous {
            Some(value) => std::env::set_var("RISC0_DEV_MODE", value),
            None => std::env::remove_var("RISC0_DEV_MODE"),
        }
        let error = verification
            .expect("production verifier must not panic on a dev-mode environment variable")
            .unwrap_err();
        assert_eq!(error, Risc0VerificationError::UnsupportedReceiptKind);
    }

    #[test]
    fn verifier_rejects_too_many_composite_segments_before_crypto() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let InnerReceipt::Composite(composite) = &mut receipt.inner else {
            panic!("fixture receipt must be composite");
        };
        composite.segments.push(composite.segments[0].clone());
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(
            error,
            Risc0VerificationError::TooManyCompositeSegments {
                received: 2,
                limit: 1,
            }
        );
    }

    #[test]
    fn verifier_rejects_empty_composite_before_crypto() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let InnerReceipt::Composite(composite) = &mut receipt.inner else {
            panic!("fixture receipt must be composite");
        };
        composite.segments.clear();
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::MissingCompositeSegments);
    }

    #[test]
    fn verifier_rejects_composite_assumption_receipts_before_crypto() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let assumption = receipt.inner.clone().into();
        let InnerReceipt::Composite(composite) = &mut receipt.inner else {
            panic!("fixture receipt must be composite");
        };
        composite.assumption_receipts.push(assumption);
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::UnsupportedAssumptions);
    }

    #[test]
    fn verifier_rejects_final_claim_assumptions_before_crypto() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let InnerReceipt::Composite(composite) = &mut receipt.inner else {
            panic!("fixture receipt must be composite");
        };
        let Some(output) = composite.segments[0]
            .claim
            .output
            .as_value_mut()
            .expect("fixture output is open")
        else {
            panic!("fixture output must be present");
        };
        output.assumptions = MaybePruned::Value(Assumptions(vec![MaybePruned::Pruned(
            risc0_zkvm::sha::Digest::ZERO,
        )]));
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::UnsupportedAssumptions);
    }

    #[test]
    fn verifier_rejects_unexpected_segment_index_before_crypto() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let InnerReceipt::Composite(composite) = &mut receipt.inner else {
            panic!("fixture receipt must be composite");
        };
        composite.segments[0].index = 1;
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(
            error,
            Risc0VerificationError::InvalidSegmentIndex {
                position: 0,
                received: 1,
            }
        );
    }

    #[test]
    fn verifier_rejects_unsupported_segment_hash_function_before_crypto() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let InnerReceipt::Composite(composite) = &mut receipt.inner else {
            panic!("fixture receipt must be composite");
        };
        composite.segments[0].hashfn = "worker-selected-hash".into();
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(
            error,
            Risc0VerificationError::UnsupportedSegmentHashFunction
        );
    }

    #[test]
    fn verifier_rejects_oversized_segment_seal_before_crypto() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let InnerReceipt::Composite(composite) = &mut receipt.inner else {
            panic!("fixture receipt must be composite");
        };
        composite.segments[0].seal.resize(131_073, 0);
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(
            error,
            Risc0VerificationError::SegmentSealTooLarge {
                position: 0,
                received: 131_073,
                limit: 131_072,
            }
        );
    }

    #[test]
    fn verifier_rejects_within_cap_tampered_composite_seal_at_crypto_gate() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let InnerReceipt::Composite(composite) = &mut receipt.inner else {
            panic!("fixture receipt must be composite");
        };
        composite.segments[0].seal[0] ^= 1;
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::InvalidProof);
    }

    #[test]
    fn verifier_rejects_invalid_verified_claim_journal() {
        let error = decode_claim_candidate(b"not-an-execution-claim").unwrap_err();

        assert_eq!(error, Risc0VerificationError::InvalidClaim);
    }

    #[test]
    fn verifier_rejects_invalid_claim_before_crypto_on_public_path() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let mut receipt: Receipt =
            serde_json::from_value(fixture.receipt).expect("fixture receipt parses");
        let journal = b"not-an-execution-claim".to_vec();
        receipt.journal.bytes = journal.clone();
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal,
            receipt_json: serde_json::to_vec(&receipt).expect("receipt serializes"),
        };

        let error = verify_risc0_proof_envelope(&envelope).unwrap_err();

        assert_eq!(error, Risc0VerificationError::InvalidClaim);
    }

    #[test]
    fn verifier_reuses_production_context_on_the_same_thread() {
        let first = super::with_production_risc0_verifier_context(|context| {
            context as *const risc0_zkvm::VerifierContext as usize
        });
        let second = super::with_production_risc0_verifier_context(|context| {
            context as *const risc0_zkvm::VerifierContext as usize
        });

        assert_eq!(first, second);
    }

    #[test]
    fn real_fixture_stays_within_phase_one_resource_budget() {
        const FIXTURE_BYTES: &[u8] =
            include_bytes!("../tests/fixtures/risc0-managed-proof-v1.json");
        let fixture: ProofFixture =
            serde_json::from_slice(FIXTURE_BYTES).expect("real proof fixture parses");
        let receipt_json = serde_json::to_vec(&fixture.receipt).expect("receipt serializes");
        let receipt: Receipt =
            serde_json::from_slice(&receipt_json).expect("fixture receipt parses");
        let InnerReceipt::Composite(composite) = &receipt.inner else {
            panic!("fixture receipt must be composite");
        };

        assert_eq!(FIXTURE_BYTES.len(), 664_026);
        assert_eq!(receipt_json.len(), 661_720);
        assert!(receipt_json.len() <= super::RISC0_MAX_RECEIPT_JSON_BYTES);
        assert_eq!(fixture.journal.len(), 656);
        assert!(fixture.journal.len() <= super::RISC0_MAX_JOURNAL_BYTES);
        assert_eq!(composite.segments.len(), 1);
        assert!(composite.segments.len() <= super::RISC0_MAX_COMPOSITE_SEGMENTS);
        assert!(composite.assumption_receipts.is_empty());
        assert_eq!(composite.segments[0].index, 0);
        assert_eq!(composite.segments[0].hashfn, "poseidon2");
        assert_eq!(composite.segments[0].seal.len(), 63_914);
        assert!(composite.segments[0].seal.len() <= super::RISC0_MAX_SEGMENT_SEAL_WORDS);
        let output = composite.segments[0]
            .claim
            .output
            .as_value()
            .expect("fixture output is open")
            .as_ref()
            .expect("fixture output is present");
        assert!(output
            .assumptions
            .as_value()
            .expect("fixture assumptions are open")
            .0
            .is_empty());
    }

    #[test]
    fn verifier_accepts_pinned_real_receipt_and_returns_claim() {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("real proof fixture parses");
        let envelope = ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&fixture.receipt).expect("receipt serializes"),
        };

        let claim = verify_risc0_proof_envelope(&envelope).expect("real receipt verifies");

        assert_eq!(
            claim,
            ExecutionClaim {
                protocol_version: 1,
                runtime_id: "managed-function-v0".into(),
                cost_model_id: "managed-function-v0-metering-v1".into(),
                task_id: "task-zk-golden".into(),
                source_sha256: [
                    154, 22, 110, 56, 121, 24, 26, 112, 209, 234, 88, 94, 168, 148, 139, 255, 74,
                    36, 89, 238, 17, 65, 50, 226, 231, 183, 47, 134, 76, 29, 43, 121,
                ],
                input_sha256: [
                    245, 190, 80, 49, 225, 145, 97, 223, 71, 5, 7, 200, 49, 63, 233, 243, 224, 122,
                    141, 236, 168, 84, 129, 22, 36, 152, 145, 12, 68, 84, 163, 96,
                ],
                output_sha256: [
                    187, 20, 83, 199, 254, 200, 223, 199, 201, 210, 196, 1, 54, 32, 95, 124, 239,
                    106, 29, 155, 182, 62, 30, 232, 92, 3, 143, 4, 231, 48, 186, 168,
                ],
                max_usage_units: 1_000,
                usage_units: 29,
                executed_ops: 29,
                function_calls: 1,
                loop_iterations: 0,
                max_call_depth: 1,
                output_bytes: 12,
            }
        );
    }
}

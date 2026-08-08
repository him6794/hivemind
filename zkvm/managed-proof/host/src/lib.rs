use anyhow::{ensure, Context, Result};
use hivemind_managed_proof::ExecutionClaim;
use hivemind_managed_proof_methods::{
    HIVEMIND_MANAGED_PROOF_GUEST_ELF, HIVEMIND_MANAGED_PROOF_GUEST_ID,
};
use risc0_zkvm::{default_executor, default_prover, ExecutorEnv, Receipt};
use serde::{Deserialize, Serialize};

pub const RISC0_PROOF_SCHEME: &str = "risc0-zkvm-3.0.6";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkerProofEnvelope {
    pub proof_scheme: String,
    pub image_id: [u32; 8],
    pub journal: Vec<u8>,
    pub receipt: Receipt,
}

impl WorkerProofEnvelope {
    pub fn from_receipt(receipt: Receipt) -> Self {
        Self {
            proof_scheme: RISC0_PROOF_SCHEME.to_owned(),
            image_id: HIVEMIND_MANAGED_PROOF_GUEST_ID,
            journal: receipt.journal.bytes.clone(),
            receipt,
        }
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("serialize RISC Zero proof envelope")
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("parse RISC Zero proof envelope")
    }
}

pub fn execute_guest_claim(
    task_id: &str,
    source: &str,
    input: &str,
    max_usage_units: u64,
) -> Result<ExecutionClaim> {
    let env = ExecutorEnv::builder()
        .write(&task_id.to_owned())?
        .write(&source.to_owned())?
        .write(&input.to_owned())?
        .write(&max_usage_units)?
        .build()?;
    let session = default_executor().execute(env, HIVEMIND_MANAGED_PROOF_GUEST_ELF)?;

    Ok(ExecutionClaim::from_journal_bytes(&session.journal.bytes)?)
}

pub fn prove_guest_execution(
    task_id: &str,
    source: &str,
    input: &str,
    max_usage_units: u64,
) -> Result<Receipt> {
    let env = ExecutorEnv::builder()
        .write(&task_id.to_owned())?
        .write(&source.to_owned())?
        .write(&input.to_owned())?
        .write(&max_usage_units)?
        .build()?;
    let prove_info = default_prover().prove(env, HIVEMIND_MANAGED_PROOF_GUEST_ELF)?;

    Ok(prove_info.receipt)
}

pub fn prove_guest_envelope(
    task_id: &str,
    source: &str,
    input: &str,
    max_usage_units: u64,
) -> Result<WorkerProofEnvelope> {
    Ok(WorkerProofEnvelope::from_receipt(prove_guest_execution(
        task_id,
        source,
        input,
        max_usage_units,
    )?))
}

pub fn verify_proof_envelope(envelope: &WorkerProofEnvelope) -> Result<ExecutionClaim> {
    ensure!(
        envelope.proof_scheme == RISC0_PROOF_SCHEME,
        "unsupported proof scheme: {}",
        envelope.proof_scheme
    );
    ensure!(
        envelope.image_id == HIVEMIND_MANAGED_PROOF_GUEST_ID,
        "proof envelope image id does not match the pinned guest"
    );
    ensure!(
        envelope.journal == envelope.receipt.journal.bytes,
        "proof envelope journal does not match the receipt journal"
    );

    envelope
        .receipt
        .verify(HIVEMIND_MANAGED_PROOF_GUEST_ID)
        .context("verify RISC Zero receipt")?;

    ExecutionClaim::from_journal_bytes(&envelope.journal)
        .context("parse verified execution claim from receipt journal")
}

#[cfg(test)]
mod tests {
    use hivemind_managed_proof::{ExecutionClaim, ExecutionMetrics};
    use managed_function_runtime::{render_output, ExecutionLimits, ManagedExecutor};
    use risc0_zkvm::{FakeReceipt, InnerReceipt, Receipt, ReceiptClaim};

    use hivemind_managed_proof_methods::HIVEMIND_MANAGED_PROOF_GUEST_ID;

    use super::{
        execute_guest_claim, prove_guest_envelope, verify_proof_envelope, WorkerProofEnvelope,
        RISC0_PROOF_SCHEME,
    };

    const SOURCE: &str = r#"
fn add(a, b) { return a + b; }
let total = add(get(input, "left"), get(input, "right"));
return {"total": total};
"#;
    const INPUT: &str = r#"{"left":20,"right":22}"#;
    const TASK_ID: &str = "task-zk-golden";
    const MAX_USAGE_UNITS: u64 = 1_000;

    fn native_claim() -> ExecutionClaim {
        let execution = ManagedExecutor
            .execute_json_input(
                SOURCE,
                ExecutionLimits {
                    max_usage_units: Some(MAX_USAGE_UNITS),
                    ..ExecutionLimits::unlimited()
                },
                INPUT,
            )
            .expect("native golden-vector execution succeeds");
        let output = if execution.output.is_empty() {
            render_output(&execution.value)
        } else {
            execution.output
        };

        ExecutionClaim::new(
            TASK_ID,
            SOURCE.as_bytes(),
            INPUT.as_bytes(),
            output.as_bytes(),
            MAX_USAGE_UNITS,
            ExecutionMetrics {
                usage_units: execution.receipt.usage_units,
                executed_ops: execution.receipt.executed_ops,
                function_calls: execution.receipt.function_calls,
                loop_iterations: execution.receipt.loop_iterations,
                max_call_depth: u64::try_from(execution.receipt.max_call_depth)
                    .expect("call depth fits in u64"),
            },
        )
        .expect("native execution is within budget")
    }

    fn fake_receipt() -> Receipt {
        let journal = native_claim()
            .to_journal_bytes()
            .expect("native claim serializes");
        let claim = ReceiptClaim::ok(HIVEMIND_MANAGED_PROOF_GUEST_ID, journal.clone());

        Receipt::new(InnerReceipt::Fake(FakeReceipt::new(claim)), journal)
    }

    #[test]
    fn proof_envelope_json_round_trips_receipt_and_metadata() {
        let envelope = WorkerProofEnvelope::from_receipt(fake_receipt());
        let encoded = envelope.to_json_bytes().expect("envelope serializes");
        let decoded = WorkerProofEnvelope::from_json_bytes(&encoded).expect("envelope parses");

        assert_eq!(decoded.proof_scheme, RISC0_PROOF_SCHEME);
        assert_eq!(decoded.image_id, HIVEMIND_MANAGED_PROOF_GUEST_ID);
        assert_eq!(decoded.journal, decoded.receipt.journal.bytes);
        assert_eq!(
            ExecutionClaim::from_journal_bytes(&decoded.journal).unwrap(),
            native_claim()
        );
    }

    #[test]
    fn verifier_rejects_envelope_metadata_tampering_before_receipt_verification() {
        let envelope = WorkerProofEnvelope::from_receipt(fake_receipt());

        let mut wrong_scheme = envelope.clone();
        wrong_scheme.proof_scheme = "untrusted-proof-scheme".to_owned();
        assert!(verify_proof_envelope(&wrong_scheme).is_err());

        let mut wrong_image = envelope.clone();
        wrong_image.image_id[0] ^= 1;
        assert!(verify_proof_envelope(&wrong_image).is_err());

        let mut wrong_journal = envelope;
        wrong_journal.journal[0] ^= 1;
        assert!(verify_proof_envelope(&wrong_journal).is_err());
    }

    #[test]
    fn guest_journal_matches_native_runtime_claim() {
        let guest = execute_guest_claim(TASK_ID, SOURCE, INPUT, MAX_USAGE_UNITS)
            .expect("guest golden-vector execution succeeds");

        assert_eq!(guest, native_claim());
    }

    #[test]
    fn receipt_verifies_guest_image_and_commits_native_claim() {
        let envelope = prove_guest_envelope(TASK_ID, SOURCE, INPUT, MAX_USAGE_UNITS)
            .expect("guest proof envelope succeeds");
        let receipt = &envelope.receipt;

        receipt
            .verify(HIVEMIND_MANAGED_PROOF_GUEST_ID)
            .expect("receipt verifies against the pinned guest image");
        let claim = ExecutionClaim::from_journal_bytes(&receipt.journal.bytes)
            .expect("receipt journal contains an execution claim");
        assert_eq!(claim, native_claim());

        let mut wrong_image_id = HIVEMIND_MANAGED_PROOF_GUEST_ID;
        wrong_image_id[0] ^= 1;
        assert!(receipt.verify(wrong_image_id).is_err());

        let mut tampered_journal = receipt.clone();
        tampered_journal.journal.bytes[0] ^= 1;
        assert!(tampered_journal
            .verify(HIVEMIND_MANAGED_PROOF_GUEST_ID)
            .is_err());

        let encoded = envelope.to_json_bytes().expect("envelope serializes");
        let decoded = WorkerProofEnvelope::from_json_bytes(&encoded).expect("envelope parses");
        assert_eq!(
            verify_proof_envelope(&decoded).expect("enveloped receipt verifies"),
            native_claim()
        );
    }
}

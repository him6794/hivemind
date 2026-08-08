use anyhow::Result;
use hivemind_managed_proof::ExecutionClaim;
use hivemind_managed_proof_methods::HIVEMIND_MANAGED_PROOF_GUEST_ELF;
use risc0_zkvm::{default_executor, default_prover, ExecutorEnv, Receipt};

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

#[cfg(test)]
mod tests {
    use hivemind_managed_proof::{ExecutionClaim, ExecutionMetrics};
    use managed_function_runtime::{render_output, ExecutionLimits, ManagedExecutor};

    use hivemind_managed_proof_methods::HIVEMIND_MANAGED_PROOF_GUEST_ID;

    use super::{execute_guest_claim, prove_guest_execution};

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

    #[test]
    fn guest_journal_matches_native_runtime_claim() {
        let guest = execute_guest_claim(TASK_ID, SOURCE, INPUT, MAX_USAGE_UNITS)
            .expect("guest golden-vector execution succeeds");

        assert_eq!(guest, native_claim());
    }

    #[test]
    fn receipt_verifies_guest_image_and_commits_native_claim() {
        let receipt = prove_guest_execution(TASK_ID, SOURCE, INPUT, MAX_USAGE_UNITS)
            .expect("guest proof succeeds");

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
    }
}

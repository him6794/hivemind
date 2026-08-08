#![no_main]

use hivemind_managed_proof::{ExecutionClaim, ExecutionMetrics};
use managed_function_runtime::{render_output, ExecutionLimits, ManagedExecutor};
use risc0_zkvm::guest::env;

risc0_zkvm::guest::entry!(main);

fn main() {
    let task_id: String = env::read();
    let source: String = env::read();
    let input: String = env::read();
    let max_usage_units: u64 = env::read();
    let execution = ManagedExecutor
        .execute_json_input(
            &source,
            ExecutionLimits {
                max_usage_units: Some(max_usage_units),
                ..ExecutionLimits::unlimited()
            },
            &input,
        )
        .expect("managed execution succeeds");
    let output = if execution.output.is_empty() {
        render_output(&execution.value)
    } else {
        execution.output
    };
    let claim = ExecutionClaim::new(
        task_id,
        source.as_bytes(),
        input.as_bytes(),
        output.as_bytes(),
        max_usage_units,
        ExecutionMetrics {
            usage_units: execution.receipt.usage_units,
            executed_ops: execution.receipt.executed_ops,
            function_calls: execution.receipt.function_calls,
            loop_iterations: execution.receipt.loop_iterations,
            max_call_depth: execution.receipt.max_call_depth as u64,
        },
    )
    .expect("guest execution is within budget");
    let journal = claim
        .to_journal_bytes()
        .expect("execution claim serializes");

    env::commit_slice(&journal);
}

//! One metered local run of hcode source, for previews and price quotes.
//!
//! The run uses the same parser, evaluator, and managed-function limits that a
//! Worker applies to a single replica, so its usage is the usage one replica
//! reports. The result is an estimate: Nodepool settles on what the Workers
//! report, and a Worker built from a different interpreter can differ.
//!
//! [`DryRunLimits`] protect the machine doing the run. They are not the task's
//! own budget, so a run that reaches them says so instead of calling the task
//! unaffordable.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use crate::{
    ExecutionLimits, ExecutionReceipt, ExecutionResult, ManagedExecutor, RuntimeError,
    TensorArtifacts, render_output_bounded,
};

/// Default usage a dry run may spend before it is stopped.
pub const DEFAULT_MAX_USAGE_UNITS: u64 = 20_000_000;

/// Default wall-clock time a dry run may take on this machine.
pub const DEFAULT_WALL_TIME: Duration = Duration::from_secs(10);

/// Bounds on one dry run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DryRunLimits {
    /// Usage units the run may consume before it stops with `budget_exhausted`.
    pub max_usage_units: u64,
    /// Longest the run may take before it is cancelled.
    pub wall_time: Duration,
}

impl Default for DryRunLimits {
    fn default() -> Self {
        Self {
            max_usage_units: DEFAULT_MAX_USAGE_UNITS,
            wall_time: DEFAULT_WALL_TIME,
        }
    }
}

/// A run that completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunSuccess {
    /// What a Worker would report as the task output: the printed text when the
    /// program printed anything, otherwise the final value.
    pub output: String,
    /// Metering for one replica. `usage_units` is what that replica is charged.
    pub receipt: ExecutionReceipt,
}

/// Why a run stopped without a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCause {
    /// The program or its input is wrong. A Worker would fail the same way.
    Program,
    /// The run reached [`DryRunLimits::max_usage_units`]. The task may still be
    /// affordable; this run was only not allowed to find out.
    UsageLimit,
    /// The run reached [`DryRunLimits::wall_time`] and was cancelled.
    TimedOut,
}

/// A run that stopped without a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunFailure {
    pub code: String,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Usage consumed before the failure, when the runtime recorded it.
    pub usage_units: Option<u64>,
    pub cause: FailureCause,
}

impl DryRunFailure {
    fn from_error(error: &RuntimeError, fallback_usage: Option<u64>) -> Self {
        let cause = match error.code() {
            "budget_exhausted" => FailureCause::UsageLimit,
            "cancelled" => FailureCause::TimedOut,
            _ => FailureCause::Program,
        };
        Self {
            code: error.code().to_owned(),
            message: error.message().to_owned(),
            line: error.line(),
            column: error.column(),
            usage_units: error
                .partial_receipt()
                .map(|receipt| receipt.usage_units)
                .or(fallback_usage),
            cause,
        }
    }
}

pub type DryRunOutcome = Result<DryRunSuccess, DryRunFailure>;

/// Runs `source` once against `input_json` as a CPU hcode replica would.
pub fn dry_run(source: &str, input_json: &str, limits: DryRunLimits) -> DryRunOutcome {
    execute(limits, |execution_limits, cancelled| {
        ManagedExecutor.execute_json_input_with_cancel(
            source,
            execution_limits,
            input_json,
            cancelled,
        )
    })
}

/// Like [`dry_run`], with digest-verified tensor artifacts visible to the program.
pub fn dry_run_with_artifacts(
    source: &str,
    input_json: &str,
    artifacts: &TensorArtifacts,
    limits: DryRunLimits,
) -> DryRunOutcome {
    execute(limits, |execution_limits, cancelled| {
        ManagedExecutor.execute_json_input_with_artifacts(
            source,
            execution_limits,
            input_json,
            artifacts,
            Some(cancelled),
        )
    })
}

fn execute(
    limits: DryRunLimits,
    evaluate: impl FnOnce(ExecutionLimits, &AtomicBool) -> Result<ExecutionResult, RuntimeError>,
) -> DryRunOutcome {
    let execution_limits = ExecutionLimits::for_managed_function_budget(limits.max_usage_units);
    let max_output_bytes = execution_limits.max_output_bytes;
    let execution = with_deadline(limits.wall_time, |cancelled| {
        evaluate(execution_limits, cancelled)
    })
    .map_err(|error| DryRunFailure::from_error(&error, None))?;
    // A Worker reports printed text when there is any, and renders the final
    // value otherwise. A value that cannot be rendered fails the task after the
    // work was done, so its usage still counts.
    let output = if execution.output.is_empty() {
        render_output_bounded(&execution.value, max_output_bytes).map_err(|error| {
            DryRunFailure::from_error(&error, Some(execution.receipt.usage_units))
        })?
    } else {
        execution.output
    };
    Ok(DryRunSuccess {
        output,
        receipt: execution.receipt,
    })
}

/// Runs `run` with a flag that a watchdog thread raises after `wall_time`.
/// The evaluator checks the flag on every charged operation.
fn with_deadline<T>(wall_time: Duration, run: impl FnOnce(&AtomicBool) -> T) -> T {
    let cancelled = AtomicBool::new(false);
    let (finished, finished_receiver) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let flag = &cancelled;
        scope.spawn(move || {
            if finished_receiver.recv_timeout(wall_time) == Err(RecvTimeoutError::Timeout) {
                flag.store(true, Ordering::Release);
            }
        });
        let result = run(&cancelled);
        // Dropping the sender wakes the watchdog so the scope can end at once.
        drop(finished);
        result
    })
}

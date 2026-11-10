//! Budget arithmetic for hcode tasks that run under managed consensus.
//!
//! A task carries one fee-inclusive `max_cpt`. Nodepool splits it into an equal
//! allowance for every replica, chosen so that the allowances plus the platform
//! fee on their sum still fit under `max_cpt`. Settlement later charges the usage
//! that the accepted replicas actually report.
//!
//! The scheduler, the Master's dry run, and the CLI all call these functions, so
//! they cannot disagree about what a given usage costs.

use std::fmt;

/// Platform fee in basis points of the replica usage (10%).
pub const PLATFORM_FEE_BPS: i64 = 1000;
const BPS_DENOMINATOR: i64 = 10_000;

/// A budget calculation that has no valid answer. The message is shown to users
/// when a task is rejected, so it names the rule that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetError(&'static str);

impl BudgetError {
    #[must_use]
    pub fn message(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for BudgetError {}

/// Platform fee on `amount_cpt` of replica usage, rounded down.
pub fn fee(amount_cpt: i64) -> Result<i64, BudgetError> {
    if amount_cpt < 0 {
        return Err(BudgetError(
            "managed v1 settlement amount cannot be negative",
        ));
    }
    let whole = amount_cpt / BPS_DENOMINATOR;
    let remainder = amount_cpt % BPS_DENOMINATOR;
    whole
        .checked_mul(PLATFORM_FEE_BPS)
        .and_then(|fee| fee.checked_add(remainder * PLATFORM_FEE_BPS / BPS_DENOMINATOR))
        .ok_or(BudgetError("managed v1 fee calculation overflowed"))
}

/// Worst-case hold for a per-replica allowance: every replica spends all of it.
/// Returns (reserved usage, reserved fee, total hold).
pub fn worst_case_hold(
    replica_allowance_cpt: i64,
    replica_count: u16,
) -> Result<(i64, i64, i64), BudgetError> {
    if replica_allowance_cpt <= 0 {
        return Err(BudgetError(
            "managed v1 settlement allowance must be positive",
        ));
    }
    if replica_count == 0 {
        return Err(BudgetError(
            "managed v1 settlement replica count must be positive",
        ));
    }
    let reserved_usage_cpt = replica_allowance_cpt
        .checked_mul(i64::from(replica_count))
        .ok_or(BudgetError(
            "managed v1 reserved usage calculation overflowed",
        ))?;
    let reserved_fee_cpt = fee(reserved_usage_cpt)?;
    let held_total_cpt = reserved_usage_cpt
        .checked_add(reserved_fee_cpt)
        .ok_or(BudgetError("managed v1 hold calculation overflowed"))?;
    Ok((reserved_usage_cpt, reserved_fee_cpt, held_total_cpt))
}

/// Largest equal per-replica allowance that fits a task-total budget, counting
/// the platform fee on the aggregate replica usage.
/// Returns (per-replica allowance, reserved usage, reserved fee, total hold).
pub fn total_budget_hold(
    task_max_cpt: i64,
    replica_count: u16,
) -> Result<(i64, i64, i64, i64), BudgetError> {
    if task_max_cpt <= 0 {
        return Err(BudgetError("managed v1 task-total budget must be positive"));
    }
    if replica_count == 0 {
        return Err(BudgetError(
            "managed v1 settlement replica count must be positive",
        ));
    }
    let cannot_fund =
        BudgetError("managed v1 task-total budget cannot fund a positive replica allowance");
    let replica_count_i64 = i64::from(replica_count);
    let mut low = 1_i64;
    let mut high = task_max_cpt / replica_count_i64;
    if high == 0 {
        return Err(cannot_fund);
    }
    let mut allowance = 0_i64;
    while low <= high {
        let candidate = low + (high - low) / 2;
        let reserved_usage_cpt = candidate.checked_mul(replica_count_i64).ok_or(BudgetError(
            "managed v1 reserved usage calculation overflowed",
        ))?;
        let reserved_fee_cpt = fee(reserved_usage_cpt)?;
        let Some(held_total_cpt) = reserved_usage_cpt.checked_add(reserved_fee_cpt) else {
            high = candidate - 1;
            continue;
        };
        if held_total_cpt <= task_max_cpt {
            allowance = candidate;
            low = candidate + 1;
        } else {
            high = candidate - 1;
        }
    }
    if allowance <= 0 {
        return Err(cannot_fund);
    }
    let (reserved_usage_cpt, reserved_fee_cpt, held_total_cpt) =
        worst_case_hold(allowance, replica_count)?;
    if held_total_cpt > task_max_cpt {
        return Err(BudgetError("managed v1 task-total hold exceeds its budget"));
    }
    Ok((
        allowance,
        reserved_usage_cpt,
        reserved_fee_cpt,
        held_total_cpt,
    ))
}

/// What a task whose replicas each use `replica_usage` units must carry as `max_cpt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskBudget {
    /// Usage each replica is allowed, and the usage this budget was computed for.
    pub replica_usage: i64,
    pub replica_count: u16,
    /// `replica_usage × replica_count`.
    pub reserved_usage: i64,
    /// Platform fee on `reserved_usage`.
    pub fee: i64,
    /// Smallest `max_cpt` that gives every replica `replica_usage`. It is also
    /// the charge when every replica runs and is accepted.
    pub task_budget: i64,
}

/// The smallest task-total budget that lets each of `replica_count` replicas use
/// `replica_usage` units. A program that reports no usage still needs the smallest
/// positive allowance, so zero is treated as one unit.
pub fn required_task_budget(
    replica_usage: u64,
    replica_count: u16,
) -> Result<TaskBudget, BudgetError> {
    let usage = i64::try_from(replica_usage.max(1))
        .map_err(|_| BudgetError("managed v1 reserved usage calculation overflowed"))?;
    let (reserved_usage, fee, task_budget) = worst_case_hold(usage, replica_count)?;
    Ok(TaskBudget {
        replica_usage: usage,
        replica_count,
        reserved_usage,
        fee,
        task_budget,
    })
}

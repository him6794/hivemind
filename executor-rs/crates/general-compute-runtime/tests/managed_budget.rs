use general_compute_runtime::managed_budget::{
    BudgetError, PLATFORM_FEE_BPS, fee, required_task_budget, total_budget_hold, worst_case_hold,
};

#[test]
fn fee_is_ten_percent_rounded_down() {
    assert_eq!(PLATFORM_FEE_BPS, 1000);
    assert_eq!(fee(0), Ok(0));
    assert_eq!(fee(9), Ok(0));
    assert_eq!(fee(10), Ok(1));
    assert_eq!(fee(99), Ok(9));
    assert_eq!(fee(100), Ok(10));
    assert_eq!(fee(9_999), Ok(999));
    assert_eq!(fee(10_000), Ok(1_000));
    assert_eq!(fee(10_001), Ok(1_000));
    assert!(fee(i64::MAX).is_ok());
    assert_eq!(
        fee(-1).unwrap_err().message(),
        "managed v1 settlement amount cannot be negative"
    );
}

#[test]
fn worst_case_hold_charges_every_replica_the_full_allowance() {
    assert_eq!(worst_case_hold(100, 3), Ok((300, 30, 330)));
    assert_eq!(worst_case_hold(1, 2), Ok((2, 0, 2)));
    assert!(worst_case_hold(0, 3).is_err());
    assert!(worst_case_hold(-5, 3).is_err());
    assert!(worst_case_hold(100, 0).is_err());
    assert!(worst_case_hold(i64::MAX, 3).is_err());
}

#[test]
fn total_budget_hold_matches_the_scheduler_vectors() {
    // These vectors are the ones the scheduler's own tests pin.
    assert_eq!(total_budget_hold(100, 3), Ok((30, 90, 9, 99)));
    assert!(total_budget_hold(2, 3).is_err());
    assert!(total_budget_hold(i64::MAX, 3).is_ok());
    assert!(total_budget_hold(0, 3).is_err());
    assert!(total_budget_hold(100, 0).is_err());
}

#[test]
fn total_budget_hold_never_exceeds_the_budget_and_is_maximal() {
    for replicas in 2_u16..=7 {
        for budget in (1_i64..2_000).chain([9_999, 10_000, 10_001, 123_456]) {
            let Ok((allowance, usage, fee_cpt, held)) = total_budget_hold(budget, replicas) else {
                continue;
            };
            assert!(held <= budget, "{budget} CPT, {replicas} replicas");
            assert_eq!(usage + fee_cpt, held);
            // One more unit per replica must not fit.
            let (_, _, bigger) = worst_case_hold(allowance + 1, replicas).unwrap();
            assert!(bigger > budget, "{budget} CPT, {replicas} replicas");
        }
    }
}

#[test]
fn required_budget_for_a_published_example() {
    // `sum-list` in the docs examples uses 25 units per replica.
    let quote = required_task_budget(25, 3).unwrap();
    assert_eq!(quote.replica_usage, 25);
    assert_eq!(quote.reserved_usage, 75);
    assert_eq!(quote.fee, 7);
    assert_eq!(quote.task_budget, 82);
    assert_eq!(total_budget_hold(82, 3).unwrap().0, 25);
}

#[test]
fn required_budget_is_the_smallest_that_funds_the_usage() {
    for replicas in 2_u16..=7 {
        for usage in (1_u64..400).chain([9_999, 10_000, 1_110, 1_000_000]) {
            let quote = required_task_budget(usage, replicas).unwrap();
            let funded = total_budget_hold(quote.task_budget, replicas).unwrap().0;
            assert_eq!(
                u64::try_from(funded).unwrap(),
                usage,
                "{usage} units, {replicas} replicas"
            );
            // One CPT less cannot fund the usage.
            let short = total_budget_hold(quote.task_budget - 1, replicas)
                .map_or(0, |hold| u64::try_from(hold.0).unwrap());
            assert!(short < usage, "{usage} units, {replicas} replicas");
        }
    }
}

#[test]
fn zero_usage_still_needs_one_unit_per_replica() {
    let quote = required_task_budget(0, 3).unwrap();
    assert_eq!(quote.replica_usage, 1);
    assert_eq!(quote.task_budget, 3);
}

#[test]
fn required_budget_rejects_values_that_overflow() {
    assert!(required_task_budget(u64::MAX, 3).is_err());
    assert!(required_task_budget(u64::try_from(i64::MAX).unwrap(), 3).is_err());
    assert!(required_task_budget(10, 0).is_err());
}

#[test]
fn budget_errors_name_the_rule() {
    let error: BudgetError = total_budget_hold(2, 3).unwrap_err();
    assert_eq!(
        error.to_string(),
        "managed v1 task-total budget cannot fund a positive replica allowance"
    );
}

#[test]
fn the_ten_by_ten_matmul_does_not_fit_the_welcome_grant() {
    // `matmul` costs m + m*n + m*k*n: 1,110 units for 10x10 matrices.
    let quote = required_task_budget(1_110, 3).unwrap();
    assert_eq!(quote.task_budget, 3_663);
    assert!(quote.task_budget > 1_000);
}

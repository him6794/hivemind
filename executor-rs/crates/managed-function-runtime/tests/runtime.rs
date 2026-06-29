use managed_function_runtime::{ExecutionLimits, ManagedExecutor, Status, Value};

#[test]
fn executes_function_with_branch_and_receipt_metering() {
    let source = r#"
fn price(units) {
    return if units > 10 { units * 2 } else { units * 3 };
}

let subtotal = price(12);
print("priced");
subtotal + 1;
"#;

    let result = ManagedExecutor.execute(source, ExecutionLimits::default()).unwrap();

    assert_eq!(result.status, Status::Completed);
    assert_eq!(result.value, Value::Int(25));
    assert_eq!(result.output, "priced\n");
    assert!(result.receipt.executed_ops > 0);
    assert_eq!(result.receipt.function_calls, 1);
    assert_eq!(result.receipt.output_bytes, 7);
}

#[test]
fn stops_before_exceeding_operation_budget() {
    let source = r"
fn add(a, b) { return a + b; }
add(add(1, 2), add(3, 4));
";

    let err = ManagedExecutor
        .execute(
            source,
            ExecutionLimits {
                max_ops: 8,
                ..ExecutionLimits::default()
            },
        )
        .unwrap_err();

    assert_eq!(err.code(), "op_limit_exceeded");
}

#[test]
fn rejects_imports_as_unsupported_syntax() {
    let err = ManagedExecutor
        .execute("import fs;", ExecutionLimits::default())
        .unwrap_err();

    assert_eq!(err.code(), "parse_error");
}

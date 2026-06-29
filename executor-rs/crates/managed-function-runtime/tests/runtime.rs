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

#[test]
fn executes_bounded_for_over_json_input_and_collections() {
    let source = r#"
let total = 0;
for n in get(input, "numbers") {
    let total = total + n;
}
let details = {"count": len(get(input, "numbers")), "total": total};
details;
"#;

    let result = ManagedExecutor
        .execute_json_input(source, ExecutionLimits::default(), r#"{"numbers":[1,2,3,4]}"#)
        .unwrap();

    assert_eq!(
        result.value,
        Value::Dict(
            [
                ("count".to_string(), Value::Int(4)),
                ("total".to_string(), Value::Int(10)),
            ]
            .into()
        )
    );
    assert_eq!(result.receipt.loop_iterations, 4);
}

#[test]
fn bounded_for_stops_before_exceeding_loop_budget() {
    let source = r"
for n in [1, 2, 3] {
    print(n);
}
";

    let err = ManagedExecutor
        .execute(
            source,
            ExecutionLimits {
                max_loop_iterations: 2,
                ..ExecutionLimits::default()
            },
        )
        .unwrap_err();

    assert_eq!(err.code(), "loop_limit_exceeded");
}

#[test]
fn stdlib_rejects_unsupported_host_access() {
    let err = ManagedExecutor
        .execute(r#"read_file("secret.txt");"#, ExecutionLimits::default())
        .unwrap_err();

    assert_eq!(err.code(), "name_error");
}

#[test]
fn parse_errors_include_source_location() {
    let err = ManagedExecutor
        .execute("let ok = 1;\nlet broken = ;\n", ExecutionLimits::default())
        .unwrap_err();

    assert_eq!(err.code(), "parse_error");
    assert_eq!(err.line(), Some(2));
    assert_eq!(err.column(), Some(14));
}

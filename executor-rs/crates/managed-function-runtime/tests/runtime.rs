use std::collections::BTreeMap;

use managed_function_runtime::{ExecutionLimits, ManagedExecutor, Status, Value};

#[test]
fn executes_function_with_branch_and_receipt_metering() {
    // Test null literal
    let result = ManagedExecutor
        .execute("return null;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Null);

    // Test unary minus
    let result = ManagedExecutor
        .execute("return -5;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Int(-5));

    // Test unary plus
    let result = ManagedExecutor
        .execute("return +5;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Int(5));

    // Test unary not
    let result = ManagedExecutor
        .execute("return not true;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(false));

    // Test logical and (short-circuit)
    let result = ManagedExecutor
        .execute("return true and false;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(false));

    let result = ManagedExecutor
        .execute("return true and true;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(true));

    // Test logical or (short-circuit)
    let result = ManagedExecutor
        .execute("return false or true;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(true));

    let result = ManagedExecutor
        .execute("return false or false;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(false));

    // Test indexing - list
    let result = ManagedExecutor
        .execute("return [1, 2, 3][1];", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Int(2));

    // Test indexing - string
    let result = ManagedExecutor
        .execute(r#"return "abc"[1];"#, ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::String("b".to_string()));

    // Test indexing - dict
    let result = ManagedExecutor
        .execute(r#"return {"a": 1, "b": 2}["b"];"#, ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Int(2));

    // Test multi-statement function body
    let source = r"
    fn foo(x) {
        let y = x + 1;
        let z = y * 2;
        return z;
    }
    return foo(5);
    ";
    let result = ManagedExecutor.execute(source, ExecutionLimits::default()).unwrap();
    assert_eq!(result.value, Value::Int(12));

    // Test assignment with indexing
    let source = r"
    let arr = [1, 2, 3];
    arr[1] = 99;
    return arr[1];
    ";
    let result = ManagedExecutor.execute(source, ExecutionLimits::default()).unwrap();
    assert_eq!(result.value, Value::Int(99));

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
    assert!(result.receipt.usage_units >= result.receipt.executed_ops);
    assert_eq!(result.receipt.output_bytes, 7);
}

#[test]
fn charges_builtin_and_user_calls_against_user_selected_budget() {
    let source = r"
fn add(a, b) { return a + b; }
return len([add(1, 2), add(3, 4)]);
";

    let result = ManagedExecutor
        .execute(
            source,
            ExecutionLimits {
                max_usage_units: Some(30),
                ..ExecutionLimits::unlimited()
            },
        )
        .expect("budget should cover the program");

    assert_eq!(result.value, Value::Int(2));
    assert!(result.receipt.usage_units > result.receipt.function_calls);
}

#[test]
fn stops_with_structured_budget_exhaustion_when_user_budget_is_spent() {
    let err = ManagedExecutor
        .execute(
            "return 1 + 2 + 3;",
            ExecutionLimits {
                max_usage_units: Some(2),
                ..ExecutionLimits::unlimited()
            },
        )
        .expect_err("the selected budget should be exhausted");

    assert_eq!(err.code(), "budget_exhausted");
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

#[test]
fn managed_function_templates_execute_successfully() {
    let cases = [
        (
            include_str!("../../../../templates/managed-function-v0/01_policy_gate.hmf"),
            include_str!("../../../../templates/managed-function-v0/01_policy_gate.input.json"),
            dict([
                ("allowed", Value::Bool(true)),
                ("risk_score", Value::Int(21)),
                ("spend_cpt", Value::Int(12)),
            ]),
        ),
        (
            include_str!("../../../../templates/managed-function-v0/02_weighted_score.hmf"),
            include_str!("../../../../templates/managed-function-v0/02_weighted_score.input.json"),
            dict([("band", Value::String("gold".into())), ("score", Value::Int(860))]),
        ),
        (
            include_str!("../../../../templates/managed-function-v0/03_batch_sum.hmf"),
            include_str!("../../../../templates/managed-function-v0/03_batch_sum.input.json"),
            dict([
                ("input_count", Value::Int(3)),
                ("paid_count", Value::Int(2)),
                ("paid_total", Value::Int(35)),
            ]),
        ),
        (
            include_str!("../../../../templates/managed-function-v0/04_price_quote.hmf"),
            include_str!("../../../../templates/managed-function-v0/04_price_quote.input.json"),
            dict([
                ("per_host_cpt", Value::Int(29)),
                ("total_cpt", Value::Int(58)),
                ("within_budget", Value::Bool(true)),
            ]),
        ),
        (
            include_str!("../../../../templates/managed-function-v0/05_route_task.hmf"),
            include_str!("../../../../templates/managed-function-v0/05_route_task.input.json"),
            dict([("pool", Value::String("cpu_pool".into())), ("priority", Value::Int(10))]),
        ),
    ];

    for (source, input, expected) in cases {
        let result = ManagedExecutor
            .execute_json_input(source, ExecutionLimits::default(), input)
            .unwrap();

        assert_eq!(result.status, Status::Completed);
        assert_eq!(result.value, expected);
        assert!(result.receipt.executed_ops > 0);
    }
}

fn dict<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Dict(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect::<BTreeMap<_, _>>(),
    )
}

#[test]
fn test_null_literal_and_unary_ops() {
    let result = ManagedExecutor
        .execute("return null;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Null);

    let result = ManagedExecutor
        .execute("return -5;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Int(-5));

    let result = ManagedExecutor
        .execute("return +5;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Int(5));

    let result = ManagedExecutor
        .execute("return not true;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(false));

    let result = ManagedExecutor
        .execute("return not false;", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(true));
}

#[test]
fn test_logical_and_or_short_circuit() {
    // and short-circuit: false and <expr> should not evaluate <expr>
    let result = ManagedExecutor
        .execute("return false and (1/0);", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(false));

    // or short-circuit: true or <expr> should not evaluate <expr>
    let result = ManagedExecutor
        .execute("return true or (1/0);", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Bool(true));
}

#[test]
fn test_indexing() {
    let result = ManagedExecutor
        .execute("return [1, 2, 3][1];", ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Int(2));

    let result = ManagedExecutor
        .execute(r#"return "abc"[1];"#, ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::String("b".to_string()));

    let result = ManagedExecutor
        .execute(r#"return {"a": 1, "b": 2}["b"];"#, ExecutionLimits::default())
        .unwrap();
    assert_eq!(result.value, Value::Int(2));
}

#[test]
fn test_multi_statement_function_body() {
    let source = r"
    fn foo(x) {
        let y = x + 1;
        let z = y * 2;
        return z;
    }
    return foo(5);
    ";
    let result = ManagedExecutor.execute(source, ExecutionLimits::default()).unwrap();
    assert_eq!(result.value, Value::Int(12));
}

#[test]
fn test_assignment_with_indexing() {
    let source = r"
    let arr = [1, 2, 3];
    arr[1] = 99;
    return arr[1];
    ";
    let result = ManagedExecutor.execute(source, ExecutionLimits::default()).unwrap();
    assert_eq!(result.value, Value::Int(99));
}

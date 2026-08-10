use std::collections::BTreeMap;

use managed_function_runtime::{ExecutionLimits, ManagedExecutor, Status, Value, render_output, render_output_bounded};

#[test]
fn renders_canonical_output_for_host_and_zk_guest() {
    let value = Value::Dict(
        [
            ("answer".to_string(), Value::Int(42)),
            ("nested".to_string(), Value::List(vec![Value::Bool(true), Value::Null])),
        ]
        .into(),
    );

    assert_eq!(render_output(&value), r#"{"answer":42,"nested":[true,null]}"#);
    assert_eq!(render_output(&Value::String("raw".into())), "raw");
}

#[test]
fn default_limits_reject_oversized_intermediate_string_concatenation() {
    let operand = "x".repeat(600 * 1024);
    let source = format!("return \"{operand}\" + \"{operand}\";");

    let result = ManagedExecutor.execute(&source, ExecutionLimits::default());
    assert!(
        result.is_err(),
        "the concatenated value must not exceed the default value limit"
    );
    let err = result.unwrap_err();

    assert_eq!(err.code(), "value_limit_exceeded");
}

#[test]
fn bounded_canonical_renderer_preserves_output_and_stops_at_its_limit() {
    let value = Value::Dict(
        [
            (
                "escaped\nkey".to_string(),
                Value::String("quote: \"; snowman: ☃".into()),
            ),
            ("nested".to_string(), Value::List(vec![Value::Bool(true), Value::Null])),
        ]
        .into(),
    );

    let expected = r#"{"escaped\nkey":"quote: \"; snowman: ☃","nested":[true,null]}"#;
    assert_eq!(render_output_bounded(&value, 1024).unwrap(), expected);
    assert_eq!(render_output(&value), expected);

    let err = render_output_bounded(&Value::String("abcdef".into()), 5).unwrap_err();
    assert_eq!(err.code(), "output_limit_exceeded");

    let err = render_output_bounded(&Value::List(vec![Value::String("abcdef".into())]), 8).unwrap_err();
    assert_eq!(err.code(), "output_limit_exceeded");
}

#[test]
fn default_value_safety_limits_are_finite_but_unlimited_disables_them() {
    let default = ExecutionLimits::default();
    assert!(default.max_value_bytes < u64::MAX);
    assert!(default.max_collection_items < u64::MAX);
    assert!(default.max_value_depth < u64::MAX);
    assert!(default.max_value_materialization_bytes < u64::MAX);

    let unlimited = ExecutionLimits::unlimited();
    assert_eq!(unlimited.max_value_bytes, u64::MAX);
    assert_eq!(unlimited.max_collection_items, u64::MAX);
    assert_eq!(unlimited.max_value_depth, u64::MAX);
    assert_eq!(unlimited.max_value_materialization_bytes, u64::MAX);
}

#[test]
fn value_limits_reject_oversized_collections_and_cumulative_clones() {
    let list_err = ManagedExecutor
        .execute(
            "return [1, 2, 3];",
            ExecutionLimits {
                max_collection_items: 2,
                ..ExecutionLimits::unlimited()
            },
        )
        .unwrap_err();
    assert_eq!(list_err.code(), "value_limit_exceeded");

    let dict_err = ManagedExecutor
        .execute(
            r#"return {"a": 1, "b": 2};"#,
            ExecutionLimits {
                max_collection_items: 1,
                ..ExecutionLimits::unlimited()
            },
        )
        .unwrap_err();
    assert_eq!(dict_err.code(), "value_limit_exceeded");

    let clone_err = ManagedExecutor
        .execute(
            r#"
let item = "abc";
let first = item;
let second = item;
return second;
"#,
            ExecutionLimits {
                max_value_bytes: 16,
                max_value_materialization_bytes: 8,
                ..ExecutionLimits::unlimited()
            },
        )
        .unwrap_err();
    assert_eq!(clone_err.code(), "value_limit_exceeded");
}

#[test]
fn value_limits_validate_json_input_before_evaluation_clones_it() {
    let err = ManagedExecutor
        .execute_json_input(
            "return input;",
            ExecutionLimits {
                max_value_bytes: 14,
                ..ExecutionLimits::unlimited()
            },
            r#"{"key":"value"}"#,
        )
        .unwrap_err();

    assert_eq!(err.code(), "value_limit_exceeded");
}

#[test]
fn value_limits_reject_deep_values_and_bound_debug_print_output() {
    let depth_err = ManagedExecutor
        .execute(
            "return [[[0]]];",
            ExecutionLimits {
                max_value_depth: 2,
                ..ExecutionLimits::unlimited()
            },
        )
        .unwrap_err();
    assert_eq!(depth_err.code(), "value_limit_exceeded");

    let print_err = ManagedExecutor
        .execute(
            r#"print(["abc", "def"]);"#,
            ExecutionLimits {
                max_output_bytes: 8,
                ..ExecutionLimits::unlimited()
            },
        )
        .unwrap_err();
    assert_eq!(print_err.code(), "output_limit_exceeded");
}

#[test]
fn print_preserves_existing_scalar_and_collection_representation() {
    let result = ManagedExecutor
        .execute(
            r#"
print("raw");
print([1, true, null]);
print({"a": 1});
"#,
            ExecutionLimits::default(),
        )
        .unwrap();

    assert_eq!(result.output, "raw\n[Int(1), Bool(true), Null]\n{\"a\": Int(1)}\n");
}

#[test]
fn cumulative_value_limit_covers_builtin_index_and_assignment_paths() {
    for source in [
        r#"
let values = ["abc"];
return get(values, 0);
"#,
        r#"
let values = ["abc"];
return values[0];
"#,
    ] {
        let err = ManagedExecutor
            .execute(
                source,
                ExecutionLimits {
                    max_value_bytes: 16,
                    max_value_materialization_bytes: 24,
                    ..ExecutionLimits::unlimited()
                },
            )
            .unwrap_err();
        assert_eq!(err.code(), "value_limit_exceeded");
    }

    let err = ManagedExecutor
        .execute(
            r#"
let data = {};
data["key"] = "value";
return data;
"#,
            ExecutionLimits {
                max_value_bytes: 32,
                max_value_materialization_bytes: 28,
                ..ExecutionLimits::unlimited()
            },
        )
        .unwrap_err();
    assert_eq!(err.code(), "value_limit_exceeded");
}

#[test]
fn unlimited_limits_allow_large_values_without_changing_usage_accounting() {
    let operand = "x".repeat(600 * 1024);
    let source = format!("return \"{operand}\" + \"{operand}\";");
    let result = ManagedExecutor
        .execute(&source, ExecutionLimits::unlimited())
        .expect("unlimited value limits must not reject the large concatenation");
    assert_eq!(result.value, Value::String(operand.repeat(2)));

    let source = "return [1, 2, 3][1];";
    let default = ManagedExecutor.execute(source, ExecutionLimits::default()).unwrap();
    let unlimited = ManagedExecutor.execute(source, ExecutionLimits::unlimited()).unwrap();
    assert_eq!(default.receipt.executed_ops, unlimited.receipt.executed_ops);
    assert_eq!(default.receipt.usage_units, unlimited.receipt.usage_units);
}

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

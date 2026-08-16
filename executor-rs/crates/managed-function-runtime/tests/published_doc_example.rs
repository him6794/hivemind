//! The official site publishes this exact program, its input, and its result.
//!
//! Documentation goes stale silently: nothing fails when a published example
//! stops producing the number next to it. This test pins the example so a
//! runtime change that would invalidate the docs shows up here instead of in
//! front of a user.
//!
//! Published in `frontend/src/lib/hivemind-site-data.mjs` as MANAGED_EXAMPLE,
//! MANAGED_EXAMPLE_INPUT, and the `language.exampleNote` copy.

use managed_function_runtime::{ExecutionLimits, ManagedExecutor, Status, Value};

const PUBLISHED_SOURCE: &str = r#"let rows = get(input, "rows");
let total = 0;

fn score(row) {
  return get(row, "hits") * 2 - get(row, "misses");
}

for row in rows {
  let total = total + score(row);
}

print("rows scanned");
total;"#;

const PUBLISHED_INPUT: &str = r#"{"rows": [{"hits": 12, "misses": 3}, {"hits": 8, "misses": 1}]}"#;

const README_TASK_SOURCE: &str = r#"fn sum(values) {
  return get(values, "a") + get(values, "b");
}
sum(input);"#;

const README_TASK_SOURCE_JSON: &str = r#""task_source": "fn sum(values) {\n  return get(values, \"a\") + get(values, \"b\");\n}\nsum(input);\n""#;

#[test]
fn published_doc_example_still_produces_the_documented_result() {
    let result = ManagedExecutor
        .execute_json_input(
            PUBLISHED_SOURCE,
            ExecutionLimits::default(),
            PUBLISHED_INPUT,
        )
        .expect("the published example must still run");

    assert_eq!(result.status, Status::Completed);
    assert_eq!(
        result.value,
        Value::Int(36),
        "the docs publish 36 as the result"
    );
    assert_eq!(result.output, "rows scanned\n");
    assert_eq!(
        result.receipt.usage_units, 80,
        "the docs publish 80 usage units, and settle the job at 81 CPT"
    );
}

#[test]
fn readme_task_submission_publishes_an_executable_managed_function() {
    let readme = include_str!("../../../../README.md");
    assert!(
        readme.contains(README_TASK_SOURCE_JSON),
        "README task_source must exactly match the executable managed-function DSL example"
    );

    let result = ManagedExecutor
        .execute_json_input(
            README_TASK_SOURCE,
            ExecutionLimits::default(),
            r#"{"a": 1, "b": 2}"#,
        )
        .expect("the README managed-function example must run");
    assert_eq!(result.status, Status::Completed);
    assert_eq!(result.value, Value::Int(3));
}

#[test]
fn bare_assignment_is_a_parse_error_as_the_docs_state() {
    let error = ManagedExecutor
        .execute(
            "let total = 0; total = total + 1; total;",
            ExecutionLimits::default(),
        )
        .expect_err("a bare name = value assignment must not parse");

    assert_eq!(error.code(), "parse_error");
}

#[test]
fn published_function_level_examples_match_receipts() {
    let cases = [
        ("len([1, 2, 3]);", 11),
        (r#"get({"hits": 4}, "hits");"#, 10),
        ("fn double(n) { return n * 2; } double(4);", 11),
        (r#"print("ok");"#, 7),
    ];

    for (source, expected_usage_units) in cases {
        let result = ManagedExecutor
            .execute(source, ExecutionLimits::default())
            .expect("the published pricing example must run");
        assert_eq!(
            result.receipt.usage_units, expected_usage_units,
            "unexpected receipt for {source:?}"
        );
    }
}

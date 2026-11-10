use std::time::{Duration, Instant};

use managed_function_runtime::{
    ExecutionLimits, ManagedExecutor, TensorArtifacts,
    dry_run::{DryRunLimits, FailureCause, dry_run, dry_run_with_artifacts},
    render_output,
};
use sha2::{Digest, Sha256};

const SUM_LIST: &str = r#"let total = 0;
for value in get(input, "values") {
  let total = total + value;
}
total;"#;

fn limits(max_usage_units: u64) -> DryRunLimits {
    DryRunLimits {
        max_usage_units,
        wall_time: Duration::from_secs(30),
    }
}

#[test]
fn usage_matches_the_published_examples() {
    // (source, input, output, usage) as the documentation pages show them.
    let examples: &[(&str, &str, &str, u64)] = &[
        (SUM_LIST, r#"{"values": [10, 20, 30]}"#, "60", 25),
        (SUM_LIST, r#"{"values": [1, 2, 3, 4, 5, 6]}"#, "21", 37),
        (r#"get(input, "name");"#, r#"{"name": "Ada"}"#, "Ada", 9),
        (
            r#""Hello, " + get(input, "name") + "!";"#,
            r#"{"name": "小明"}"#,
            "Hello, 小明!",
            13,
        ),
        (
            "print(\"step 1\");\nprint(\"step 2\");\n3;",
            "{}",
            "step 1\nstep 2\n",
            16,
        ),
    ];
    for (source, input, output, usage) in examples {
        let report = dry_run(source, input, limits(1_000)).unwrap();
        assert_eq!(&report.output, output, "{source}");
        assert_eq!(report.receipt.usage_units, *usage, "{source}");
    }
}

#[test]
fn a_run_reports_what_a_replica_would() {
    let input = r#"{"values": [10, 20, 30]}"#;
    let report = dry_run(SUM_LIST, input, limits(1_000)).unwrap();
    let replica = ManagedExecutor
        .execute_json_input(
            SUM_LIST,
            ExecutionLimits::for_managed_function_budget(1_000),
            input,
        )
        .unwrap();
    assert_eq!(report.receipt, replica.receipt);
    assert_eq!(report.output, render_output(&replica.value));
}

#[test]
fn the_budget_boundary_is_exact() {
    let input = r#"{"values": [10, 20, 30]}"#;
    assert!(dry_run(SUM_LIST, input, limits(25)).is_ok());
    let failure = dry_run(SUM_LIST, input, limits(24)).unwrap_err();
    assert_eq!(failure.code, "budget_exhausted");
    assert_eq!(failure.cause, FailureCause::UsageLimit);
    assert!(failure.usage_units.is_some_and(|usage| usage <= 24));
}

#[test]
fn syntax_errors_carry_a_position() {
    let failure = dry_run("let x = 1;\nlet y = ;", "{}", limits(1_000)).unwrap_err();
    assert_eq!(failure.code, "parse_error");
    assert_eq!(failure.cause, FailureCause::Program);
    assert_eq!(failure.line, Some(2));
    assert!(failure.column.is_some());
    assert!(
        !failure.message.starts_with("parse_error"),
        "{}",
        failure.message
    );
}

#[test]
fn runtime_errors_report_the_usage_spent_before_them() {
    let failure = dry_run("let a = 1;\nlet b = 0;\na / b;", "{}", limits(1_000)).unwrap_err();
    assert_eq!(failure.cause, FailureCause::Program);
    // Only syntax errors carry a position today; evaluation errors do not.
    assert_eq!(failure.line, None);
    assert!(failure.usage_units.is_some_and(|usage| usage > 0));
}

#[test]
fn invalid_input_json_is_a_program_failure() {
    let failure = dry_run(SUM_LIST, "{not json", limits(1_000)).unwrap_err();
    assert_eq!(failure.code, "input_error");
    assert_eq!(failure.cause, FailureCause::Program);
}

#[test]
fn a_slow_program_is_cancelled_at_the_wall_time() {
    // Far more work than the wall time allows, with a usage limit that would not stop it first.
    let source = "let total = 0;\nfor i in range(100000) {\n  for j in range(100000) {\n    let total = total + j;\n  }\n}\ntotal;";
    let started = Instant::now();
    let failure = dry_run(
        source,
        "{}",
        DryRunLimits {
            max_usage_units: u64::MAX / 4,
            wall_time: Duration::from_millis(150),
        },
    )
    .unwrap_err();
    assert_eq!(failure.cause, FailureCause::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn a_fast_program_does_not_wait_for_the_wall_time() {
    let started = Instant::now();
    let report = dry_run(
        r#"get(input, "name");"#,
        r#"{"name": "Ada"}"#,
        DryRunLimits {
            max_usage_units: 1_000,
            wall_time: Duration::from_secs(600),
        },
    )
    .unwrap();
    assert_eq!(report.output, "Ada");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_failure_after_work_reports_the_usage_spent() {
    // The string doubles until it passes the value size limit.
    let source =
        "let text = \"0123456789\";\nfor i in range(18) {\n  let text = text + text;\n}\ntext;";
    let failure = dry_run(source, "{}", limits(100_000_000)).unwrap_err();
    assert_eq!(failure.cause, FailureCause::Program);
    assert!(failure.usage_units.is_some_and(|usage| usage > 0));
}

fn bundle(header: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = u64::try_from(header.len()).unwrap().to_le_bytes().to_vec();
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn weights() -> TensorArtifacts {
    let data: Vec<u8> = [1.0_f32, 2.0, 3.0, 4.0]
        .iter()
        .flat_map(|n| n.to_le_bytes())
        .collect();
    let bytes = bundle(
        r#"{"weights":{"dtype":"F32","shape":[2,2],"data_offsets":[0,16]}}"#,
        &data,
    );
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let mut inputs = TensorArtifacts::default();
    inputs.insert("model".to_string(), bytes, &digest).unwrap();
    inputs
}

#[test]
fn artifacts_are_visible_and_metered_like_a_replica_run() {
    let source = r#"return tolist(matmul(load_tensor("model", "weights"), tensor([1.0, 2.0])));"#;
    let report = dry_run_with_artifacts(source, "null", &weights(), limits(1_000)).unwrap();
    assert_eq!(report.output, "[5.0,11.0]");
    let replica = ManagedExecutor
        .execute_json_input_with_artifacts(
            source,
            ExecutionLimits::for_managed_function_budget(1_000),
            "null",
            &weights(),
            None,
        )
        .unwrap();
    assert_eq!(report.receipt.usage_units, replica.receipt.usage_units);
}

#[test]
fn a_program_that_needs_artifacts_fails_without_them() {
    let source = r#"return tolist(load_tensor("model", "weights"));"#;
    let failure = dry_run(source, "null", limits(1_000)).unwrap_err();
    assert_eq!(failure.cause, FailureCause::Program);
}

use managed_function_runtime::{
    CpuGpuBackend, ExecutionLimits, ManagedExecutor, ManagedGpuExecutor, Value,
    render_output_bounded,
};

const INPUT: &str = "-1.2392947103274559";

#[test]
fn cpu_json_preserves_the_binary64_value_rendered_by_a_previous_task() {
    let expected: f64 = INPUT.parse().unwrap();
    let limits = ExecutionLimits::for_managed_function_budget(100);
    let result = ManagedExecutor
        .execute_json_input("return input;", limits.clone(), INPUT)
        .unwrap();
    let Value::Float(actual) = result.value else {
        panic!("expected a float");
    };
    assert_eq!(actual.to_bits(), expected.to_bits());
    let rendered = render_output_bounded(&Value::Float(actual), 100).unwrap();
    let second = ManagedExecutor
        .execute_json_input("return input;", limits, &rendered)
        .unwrap();
    assert_eq!(second.value, Value::Float(expected));
}

#[test]
fn cpu_json_roundtrips_finite_extremes_and_subnormal_values() {
    for expected in [
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::MAX,
        -f64::MAX,
        9_007_199_254_740_992.0,
    ] {
        let input = render_output_bounded(&Value::Float(expected), 4096).unwrap();
        let result = ManagedExecutor
            .execute_json_input(
                "return input;",
                ExecutionLimits::for_managed_function_budget(100),
                &input,
            )
            .unwrap();
        let Value::Float(actual) = result.value else {
            panic!("expected float");
        };
        assert_eq!(actual.to_bits(), expected.to_bits(), "{input}");
    }
}

#[test]
fn gpu_json_scalar_input_uses_the_same_exact_binary64_parser() {
    let expected: f64 = INPUT.parse().unwrap();
    let mut backend = CpuGpuBackend;
    let result = ManagedGpuExecutor::new(&mut backend)
        .execute_json_input("return input;", ExecutionLimits::default(), INPUT)
        .unwrap();
    let Value::Float(actual) = result.value else {
        panic!("expected a float");
    };
    assert_eq!(actual.to_bits(), expected.to_bits());
}

#[test]
fn frozen_v0_still_rejects_floats_and_preserves_large_integers() {
    let error = ManagedExecutor
        .execute_json_input("return input;", ExecutionLimits::default(), INPUT)
        .unwrap_err();
    assert_eq!(error.code(), "input_error");
    for value in [i64::MIN, i64::MAX, 9_007_199_254_740_993] {
        let result = ManagedExecutor
            .execute_json_input(
                "return input;",
                ExecutionLimits::default(),
                &value.to_string(),
            )
            .unwrap();
        assert_eq!(result.value, Value::Int(value));
    }
}

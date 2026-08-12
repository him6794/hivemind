use general_compute_runtime::differential::{
    DifferentialCase, DifferentialError, DifferentialRunner, ReferenceObservation,
};

fn halt_case() -> DifferentialCase {
    DifferentialCase {
        source: "minsky:inc(0);halt".into(),
        input_json: r#"{"value": 4}"#.into(),
        seed: 7,
        expected: ReferenceObservation {
            status: "halted".into(),
            steps: 2,
            output: "1".into(),
        },
    }
}

#[test]
fn differential_case_replays_with_fixed_source_input_and_seed() {
    let case = halt_case();
    let runner = DifferentialRunner::new(case.clone());

    assert_eq!(runner.case(), &case);
    assert_eq!(runner.run_reference().expect("case should run"), case.expected);
    assert_eq!(runner.run_reference().expect("replay should run"), case.expected);
}

#[test]
fn differential_runner_rejects_mismatched_backend_observation() {
    let runner = DifferentialRunner::new(halt_case());
    let observed = ReferenceObservation {
        status: "halted".into(),
        steps: 3,
        output: "1".into(),
    };

    assert!(matches!(
        runner.compare(&observed),
        Err(DifferentialError::Mismatch { .. })
    ));
}

#[test]
fn differential_runner_rejects_unregistered_source_input_or_seed() {
    let mut case = halt_case();
    case.input_json = r#"{"value": 5}"#.into();
    assert!(matches!(
        DifferentialRunner::new(case).run_reference(),
        Err(DifferentialError::InvalidCase(_))
    ));

    let mut case = halt_case();
    case.seed = 8;
    assert!(matches!(
        DifferentialRunner::new(case).run_reference(),
        Err(DifferentialError::InvalidCase(_))
    ));
}

use general_compute_runtime::ode::{
    AdaptiveRk4Config, MAX_RK4_STEPS, OdeError, OdeStatus, Rk4Config,
};

#[test]
fn rk4_integrates_exponential_growth_with_bounded_fixed_steps() {
    let config = Rk4Config::new(0.25, 1e-6, 16).expect("fixed-step config should validate");
    let result = config
        .integrate(0.0, 1.0, 1.0, |_, value| value)
        .expect("bounded exponential solve should complete");

    assert_eq!(result.status, OdeStatus::Completed);
    assert_eq!(result.steps, 4);
    assert_eq!(result.final_time, 1.0);
    assert_eq!(result.tolerance, 1e-6);
    assert!((result.final_value - std::f64::consts::E).abs() < 1e-4);
}

#[test]
fn rk4_rejects_invalid_configuration_step_budget_and_nonfinite_derivatives() {
    assert_eq!(
        Rk4Config::new(0.0, 1e-6, 16),
        Err(OdeError::InvalidStepSize)
    );
    assert_eq!(
        Rk4Config::new(0.25, -1.0, 16),
        Err(OdeError::InvalidTolerance)
    );
    assert_eq!(
        Rk4Config::new(0.25, 1e-6, MAX_RK4_STEPS + 1),
        Err(OdeError::StepLimitExceeded {
            requested: MAX_RK4_STEPS + 1,
            max: MAX_RK4_STEPS,
        })
    );

    let config = Rk4Config::new(0.25, 1e-6, 2).unwrap();
    assert_eq!(
        config.integrate(0.0, 1.0, 1.0, |_, value| value),
        Err(OdeError::StepLimitExceeded {
            requested: 3,
            max: 2,
        })
    );
    assert_eq!(
        config.integrate(1.0, 1.0, 0.0, |_, value| value),
        Err(OdeError::TargetBeforeStart)
    );
    assert_eq!(
        config.integrate(0.0, 1.0, 0.25, |_, _| f64::NAN),
        Err(OdeError::NonFiniteDerivative { step: 0 })
    );

    let wide_step = Rk4Config::new(4.0, 1e-6, 1).unwrap();
    assert_eq!(
        wide_step.integrate(0.0, 1.0, 4.0, |_, value| {
            if value.is_finite() { f64::MAX } else { 0.0 }
        }),
        Err(OdeError::NonFiniteState { step: 0 })
    );
}

#[test]
fn adaptive_rk4_reaches_target_and_reports_error_control_metadata() {
    let config = AdaptiveRk4Config::new(0.5, 1e-8, 1e-6, 128)
        .expect("adaptive configuration should validate");
    let result = config
        .integrate(0.0, 1.0, 1.0, |_, value| value)
        .expect("adaptive exponential solve should complete");

    assert_eq!(result.status, OdeStatus::Completed);
    assert_eq!(result.final_time, 1.0);
    assert_eq!(result.tolerance, 1e-8);
    assert!(result.accepted_steps > 0);
    assert!(result.attempted_steps >= result.accepted_steps);
    assert!(result.last_step_size >= 1e-6);
    assert!((result.final_value - std::f64::consts::E).abs() < 2e-7);
}

#[test]
fn adaptive_rk4_rejects_invalid_limits_and_unresolvable_steps() {
    assert_eq!(
        AdaptiveRk4Config::new(0.0, 1e-8, 1e-6, 128),
        Err(OdeError::InvalidStepSize)
    );
    assert_eq!(
        AdaptiveRk4Config::new(0.5, 0.0, 1e-6, 128),
        Err(OdeError::InvalidTolerance)
    );
    assert_eq!(
        AdaptiveRk4Config::new(0.5, 1e-8, 1e-6, 0),
        Err(OdeError::StepLimitExceeded {
            requested: 0,
            max: MAX_RK4_STEPS,
        })
    );

    let config = AdaptiveRk4Config::new(1.0, 1e-12, 0.5, 16).unwrap();
    assert_eq!(
        config.integrate(0.0, 1.0, 1.0, |_, value| 1_000.0 * value),
        Err(OdeError::AdaptiveStepTooSmall)
    );
}

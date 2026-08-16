use general_compute_runtime::statistics::{
    MAX_STATISTICS_SAMPLES, StatisticsError, mean, population_variance, quantile, sample_variance,
};

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} is not within {tolerance} of {expected}"
    );
}

#[test]
fn statistics_compute_deterministic_moments() {
    let values = [1.0, 2.0, 3.0, 4.0];
    assert_close(mean(&values).expect("mean should compute"), 2.5, 1e-15);
    assert_close(
        population_variance(&values).expect("population variance should compute"),
        1.25,
        1e-15,
    );
    assert_close(
        sample_variance(&values).expect("sample variance should compute"),
        5.0 / 3.0,
        1e-15,
    );
    assert_eq!(population_variance(&[7.0]).unwrap(), 0.0);
}

#[test]
fn statistics_compute_linear_interpolated_quantiles_without_mutating_input() {
    let values = [4.0, 1.0, 3.0, 2.0];
    assert_close(quantile(&values, 0.0).unwrap(), 1.0, 1e-15);
    assert_close(quantile(&values, 0.25).unwrap(), 1.75, 1e-15);
    assert_close(quantile(&values, 0.5).unwrap(), 2.5, 1e-15);
    assert_close(quantile(&values, 0.75).unwrap(), 3.25, 1e-15);
    assert_close(quantile(&values, 1.0).unwrap(), 4.0, 1e-15);
    assert_eq!(values, [4.0, 1.0, 3.0, 2.0]);
}

#[test]
fn statistics_reject_empty_nonfinite_invalid_and_over_budget_inputs() {
    assert_eq!(mean(&[]), Err(StatisticsError::EmptyInput));
    assert_eq!(
        sample_variance(&[1.0]),
        Err(StatisticsError::InsufficientSamplesForVariance)
    );
    assert_eq!(mean(&[1.0, f64::NAN]), Err(StatisticsError::NonFiniteValue));
    assert_eq!(
        quantile(&[1.0, 2.0], -0.1),
        Err(StatisticsError::InvalidProbability)
    );
    assert_eq!(
        quantile(&[1.0, 2.0], f64::NAN),
        Err(StatisticsError::InvalidProbability)
    );
    let over_budget = vec![0.0; MAX_STATISTICS_SAMPLES + 1];
    assert_eq!(
        mean(&over_budget),
        Err(StatisticsError::SampleCountExceeded {
            requested: MAX_STATISTICS_SAMPLES + 1,
            max: MAX_STATISTICS_SAMPLES,
        })
    );
}

use std::f64::consts::PI;

use general_compute_runtime::monte_carlo::{estimate_unit_circle_pi, ConfidenceLevel};
use general_compute_runtime::monte_carlo::{MonteCarloError, MAX_MONTE_CARLO_SAMPLES};

#[test]
fn unit_circle_pi_estimate_replays_a_pinned_confidence_fixture() {
    let estimate = estimate_unit_circle_pi(42, 7, 3, 10_000, ConfidenceLevel::NinetyFive)
        .expect("bounded Monte Carlo estimate should succeed");

    assert_eq!(estimate.samples, 10_000);
    assert_eq!(estimate.hits, 7_813);
    assert_eq!(estimate.estimate, 3.1252);
    assert!(estimate.confidence_interval.0 <= PI);
    assert!(PI <= estimate.confidence_interval.1);
    assert!(estimate.standard_error.is_finite());

    let replay = estimate_unit_circle_pi(42, 7, 3, 10_000, ConfidenceLevel::NinetyFive)
        .expect("replay should succeed");
    assert_eq!(estimate, replay);
}

#[test]
fn monte_carlo_rejects_empty_and_over_budget_requests() {
    assert_eq!(
        estimate_unit_circle_pi(42, 7, 3, 0, ConfidenceLevel::NinetyFive),
        Err(MonteCarloError::SampleCountZero)
    );
    assert_eq!(
        estimate_unit_circle_pi(
            42,
            7,
            3,
            MAX_MONTE_CARLO_SAMPLES + 1,
            ConfidenceLevel::NinetyFive
        ),
        Err(MonteCarloError::SampleCountExceeded {
            requested: MAX_MONTE_CARLO_SAMPLES + 1,
            max: MAX_MONTE_CARLO_SAMPLES,
        })
    );
}

#[test]
fn wider_confidence_levels_have_no_narrower_intervals() {
    let ninety = estimate_unit_circle_pi(1, 2, 3, 2_000, ConfidenceLevel::Ninety)
        .expect("ninety percent estimate should succeed");
    let ninety_nine = estimate_unit_circle_pi(1, 2, 3, 2_000, ConfidenceLevel::NinetyNine)
        .expect("ninety-nine percent estimate should succeed");

    assert_eq!(ConfidenceLevel::NinetyFive.probability(), 0.95);
    let ninety_width = ninety.confidence_interval.1 - ninety.confidence_interval.0;
    let ninety_nine_width = ninety_nine.confidence_interval.1 - ninety_nine.confidence_interval.0;
    assert!(ninety_nine_width >= ninety_width);
}

//! Deterministic, bounded Monte Carlo reference estimators.
//!
//! The estimator in this module is intentionally small and auditable: it uses
//! the pinned [`crate::rng::DeterministicRng`] stream to estimate the area of a
//! quarter unit circle, then reports a normal-approximation confidence
//! interval for the resulting estimate of π. It is a reference fixture, not a
//! production statistical backend.

use std::fmt;

use crate::rng::{DeterministicRng, MAX_RNG_SAMPLES};

/// Each trial consumes two uniform samples, so this cap preserves the RNG's
/// one-million-sample budget while keeping the trial count explicit.
pub const MAX_MONTE_CARLO_SAMPLES: usize = MAX_RNG_SAMPLES / 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceLevel {
    Ninety,
    NinetyFive,
    NinetyNine,
}

impl ConfidenceLevel {
    #[must_use]
    pub const fn probability(self) -> f64 {
        match self {
            Self::Ninety => 0.90,
            Self::NinetyFive => 0.95,
            Self::NinetyNine => 0.99,
        }
    }

    const fn z_score(self) -> f64 {
        match self {
            Self::Ninety => 1.644_853_626_951_472_2,
            Self::NinetyFive => 1.959_963_984_540_054,
            Self::NinetyNine => 2.575_829_303_548_900_4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonteCarloError {
    SampleCountZero,
    SampleCountExceeded { requested: usize, max: usize },
}

impl fmt::Display for MonteCarloError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SampleCountZero => {
                formatter.write_str("Monte Carlo sample count must be nonzero")
            }
            Self::SampleCountExceeded { requested, max } => {
                write!(
                    formatter,
                    "requested {requested} Monte Carlo samples, maximum is {max}"
                )
            }
        }
    }
}

impl std::error::Error for MonteCarloError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonteCarloEstimate {
    pub samples: usize,
    pub hits: usize,
    pub estimate: f64,
    pub variance: f64,
    pub standard_error: f64,
    pub confidence_level: ConfidenceLevel,
    pub confidence_interval: (f64, f64),
}

/// Estimate π by sampling the quarter unit square with a pinned RNG stream.
pub fn estimate_unit_circle_pi(
    seed: u64,
    stream: u64,
    subsequence: u64,
    samples: usize,
    confidence_level: ConfidenceLevel,
) -> Result<MonteCarloEstimate, MonteCarloError> {
    if samples == 0 {
        return Err(MonteCarloError::SampleCountZero);
    }
    if samples > MAX_MONTE_CARLO_SAMPLES {
        return Err(MonteCarloError::SampleCountExceeded {
            requested: samples,
            max: MAX_MONTE_CARLO_SAMPLES,
        });
    }

    let mut rng = DeterministicRng::new(seed, stream, subsequence);
    let mut hits = 0usize;
    for _ in 0..samples {
        let x = rng.next_f64();
        let y = rng.next_f64();
        let radius_squared = x * x + y * y;
        if radius_squared <= 1.0 {
            hits += 1;
        }
    }

    let sample_count = samples as f64;
    let proportion = hits as f64 / sample_count;
    let estimate = 4.0 * proportion;
    let variance = 16.0 * proportion * (1.0 - proportion) / sample_count;
    let standard_error = variance.sqrt();
    let margin = confidence_level.z_score() * standard_error;

    Ok(MonteCarloEstimate {
        samples,
        hits,
        estimate,
        variance,
        standard_error,
        confidence_level,
        confidence_interval: ((estimate - margin).max(0.0), (estimate + margin).min(4.0)),
    })
}

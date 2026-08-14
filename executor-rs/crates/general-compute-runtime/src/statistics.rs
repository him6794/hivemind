//! Deterministic bounded descriptive statistics for the reference runtime.
//!
//! All reductions use a sequential Welford update and quantiles use sorted
//! linear interpolation. This is a replayable CPU reference layer, not a
//! production streaming or distributed statistics backend.

use std::cmp::Ordering;
use std::fmt;

pub const MAX_STATISTICS_SAMPLES: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatisticsError {
    EmptyInput,
    SampleCountExceeded { requested: usize, max: usize },
    NonFiniteValue,
    InvalidProbability,
    InsufficientSamplesForVariance,
}

impl fmt::Display for StatisticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => formatter.write_str("statistics input must be non-empty"),
            Self::SampleCountExceeded { requested, max } => {
                write!(
                    formatter,
                    "requested {requested} statistics samples, maximum is {max}"
                )
            }
            Self::NonFiniteValue => {
                formatter.write_str("statistics input contains a non-finite value")
            }
            Self::InvalidProbability => {
                formatter.write_str("quantile probability must be finite and in [0, 1]")
            }
            Self::InsufficientSamplesForVariance => {
                formatter.write_str("sample variance requires at least two values")
            }
        }
    }
}

impl std::error::Error for StatisticsError {}

/// Compute a deterministic arithmetic mean using sequential Welford updates.
pub fn mean(values: &[f64]) -> Result<f64, StatisticsError> {
    let (average, _) = moments(values)?;
    Ok(average)
}

/// Compute population variance with a denominator of `n`.
pub fn population_variance(values: &[f64]) -> Result<f64, StatisticsError> {
    let (average, second_central_moment) = moments(values)?;
    let variance = second_central_moment / values.len() as f64;
    if !average.is_finite() || !variance.is_finite() {
        return Err(StatisticsError::NonFiniteValue);
    }
    Ok(variance.max(0.0))
}

/// Compute unbiased sample variance with a denominator of `n - 1`.
pub fn sample_variance(values: &[f64]) -> Result<f64, StatisticsError> {
    validate_values(values)?;
    if values.len() < 2 {
        return Err(StatisticsError::InsufficientSamplesForVariance);
    }
    let (_, second_central_moment) = moments_checked(values);
    let variance = second_central_moment / (values.len() - 1) as f64;
    if !variance.is_finite() {
        return Err(StatisticsError::NonFiniteValue);
    }
    Ok(variance.max(0.0))
}

/// Compute a quantile using sorted linear interpolation between observations.
pub fn quantile(values: &[f64], probability: f64) -> Result<f64, StatisticsError> {
    validate_values(values)?;
    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
        return Err(StatisticsError::InvalidProbability);
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let maximum_index =
        u32::try_from(sorted.len() - 1).map_err(|_| StatisticsError::SampleCountExceeded {
            requested: sorted.len(),
            max: MAX_STATISTICS_SAMPLES,
        })?;
    let position = probability * f64::from(maximum_index);
    let upper_u32 = (0..=maximum_index)
        .find(|&index| f64::from(index) >= position)
        .unwrap_or(maximum_index);
    let lower_u32 =
        if (f64::from(upper_u32) - position).abs() <= f64::EPSILON * position.abs().max(1.0) {
            upper_u32
        } else {
            upper_u32.saturating_sub(1)
        };
    let lower = usize::try_from(lower_u32).map_err(|_| StatisticsError::SampleCountExceeded {
        requested: sorted.len(),
        max: MAX_STATISTICS_SAMPLES,
    })?;
    let upper = usize::try_from(upper_u32).map_err(|_| StatisticsError::SampleCountExceeded {
        requested: sorted.len(),
        max: MAX_STATISTICS_SAMPLES,
    })?;
    let fraction = position - f64::from(lower_u32);
    let result = sorted[lower] + fraction * (sorted[upper] - sorted[lower]);
    if !result.is_finite() {
        return Err(StatisticsError::NonFiniteValue);
    }
    Ok(result)
}

fn moments(values: &[f64]) -> Result<(f64, f64), StatisticsError> {
    validate_values(values)?;
    let moments = moments_checked(values);
    if !moments.0.is_finite() || !moments.1.is_finite() {
        return Err(StatisticsError::NonFiniteValue);
    }
    Ok(moments)
}

fn moments_checked(values: &[f64]) -> (f64, f64) {
    let mut average = 0.0;
    let mut second_central_moment = 0.0;
    for (index, &value) in values.iter().enumerate() {
        let count = (index + 1) as f64;
        let delta = value - average;
        average += delta / count;
        let updated_delta = value - average;
        second_central_moment += delta * updated_delta;
    }
    (average, second_central_moment)
}

fn validate_values(values: &[f64]) -> Result<(), StatisticsError> {
    if values.is_empty() {
        return Err(StatisticsError::EmptyInput);
    }
    if values.len() > MAX_STATISTICS_SAMPLES {
        return Err(StatisticsError::SampleCountExceeded {
            requested: values.len(),
            max: MAX_STATISTICS_SAMPLES,
        });
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(StatisticsError::NonFiniteValue);
    }
    Ok(())
}

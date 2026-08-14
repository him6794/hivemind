//! Bounded fixed-step Runge–Kutta reference integration.
//!
//! This is intentionally a scalar RK4 primitive. It records the fixed-step
//! configuration and result metadata so a later adaptive solver can add its
//! own status and failure semantics without changing this contract.

use std::fmt;

pub const MAX_RK4_STEPS: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OdeStatus {
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OdeError {
    InvalidStepSize,
    InvalidTolerance,
    TargetBeforeStart,
    StepLimitExceeded { requested: usize, max: usize },
    AdaptiveStepTooSmall,
    NonFiniteDerivative { step: usize },
    NonFiniteState { step: usize },
}

impl fmt::Display for OdeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStepSize => {
                formatter.write_str("RK4 step size must be finite and positive")
            }
            Self::InvalidTolerance => {
                formatter.write_str("ODE tolerance must be finite and non-negative")
            }
            Self::TargetBeforeStart => {
                formatter.write_str("ODE target time must not precede start time")
            }
            Self::StepLimitExceeded { requested, max } => {
                write!(
                    formatter,
                    "ODE requires {requested} steps, maximum is {max}"
                )
            }
            Self::AdaptiveStepTooSmall => {
                formatter.write_str("adaptive ODE step cannot satisfy tolerance at minimum size")
            }
            Self::NonFiniteDerivative { step } => {
                write!(formatter, "ODE derivative became non-finite at step {step}")
            }
            Self::NonFiniteState { step } => {
                write!(formatter, "ODE state became non-finite after step {step}")
            }
        }
    }
}

impl std::error::Error for OdeError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rk4Config {
    pub step_size: f64,
    pub tolerance: f64,
    pub max_steps: usize,
}

impl Rk4Config {
    pub fn new(step_size: f64, tolerance: f64, max_steps: usize) -> Result<Self, OdeError> {
        if !step_size.is_finite() || step_size <= 0.0 {
            return Err(OdeError::InvalidStepSize);
        }
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(OdeError::InvalidTolerance);
        }
        if max_steps == 0 || max_steps > MAX_RK4_STEPS {
            return Err(OdeError::StepLimitExceeded {
                requested: max_steps,
                max: MAX_RK4_STEPS,
            });
        }
        Ok(Self {
            step_size,
            tolerance,
            max_steps,
        })
    }

    pub fn integrate<F>(
        &self,
        start_time: f64,
        initial_value: f64,
        target_time: f64,
        mut derivative: F,
    ) -> Result<Rk4Result, OdeError>
    where
        F: FnMut(f64, f64) -> f64,
    {
        if !start_time.is_finite() || !initial_value.is_finite() || !target_time.is_finite() {
            return Err(OdeError::NonFiniteState { step: 0 });
        }
        if target_time < start_time {
            return Err(OdeError::TargetBeforeStart);
        }

        let mut time = start_time;
        let mut value = initial_value;
        let mut steps = 0usize;
        while time < target_time {
            if steps == self.max_steps {
                return Err(OdeError::StepLimitExceeded {
                    requested: steps.saturating_add(1),
                    max: self.max_steps,
                });
            }
            let step = (target_time - time).min(self.step_size);
            let k1 = evaluate(&mut derivative, time, value, steps)?;
            let k2 = evaluate(
                &mut derivative,
                time + step / 2.0,
                value + step * k1 / 2.0,
                steps,
            )?;
            let k3 = evaluate(
                &mut derivative,
                time + step / 2.0,
                value + step * k2 / 2.0,
                steps,
            )?;
            let k4 = evaluate(&mut derivative, time + step, value + step * k3, steps)?;
            let next_value = value + step * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0;
            if !next_value.is_finite() {
                return Err(OdeError::NonFiniteState { step: steps + 1 });
            }
            value = next_value;
            time = (time + step).min(target_time);
            steps += 1;
        }

        Ok(Rk4Result {
            status: OdeStatus::Completed,
            final_time: time,
            final_value: value,
            steps,
            step_size: self.step_size,
            tolerance: self.tolerance,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rk4Result {
    pub status: OdeStatus,
    pub final_time: f64,
    pub final_value: f64,
    pub steps: usize,
    pub step_size: f64,
    pub tolerance: f64,
}

/// Bounded scalar RK4 step-doubling controller. The full-step versus two
/// half-step estimate is deterministic and records both accepted and
/// attempted steps for replay/audit consumers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveRk4Config {
    pub initial_step: f64,
    pub tolerance: f64,
    pub min_step: f64,
    pub max_steps: usize,
}

impl AdaptiveRk4Config {
    pub fn new(
        initial_step: f64,
        tolerance: f64,
        min_step: f64,
        max_steps: usize,
    ) -> Result<Self, OdeError> {
        if !initial_step.is_finite() || initial_step <= 0.0 {
            return Err(OdeError::InvalidStepSize);
        }
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(OdeError::InvalidTolerance);
        }
        if !min_step.is_finite() || min_step <= 0.0 || min_step > initial_step {
            return Err(OdeError::InvalidStepSize);
        }
        if max_steps == 0 || max_steps > MAX_RK4_STEPS {
            return Err(OdeError::StepLimitExceeded {
                requested: max_steps,
                max: MAX_RK4_STEPS,
            });
        }
        Ok(Self {
            initial_step,
            tolerance,
            min_step,
            max_steps,
        })
    }

    pub fn integrate<F>(
        &self,
        start_time: f64,
        initial_value: f64,
        target_time: f64,
        mut derivative: F,
    ) -> Result<AdaptiveRk4Result, OdeError>
    where
        F: FnMut(f64, f64) -> f64,
    {
        if !start_time.is_finite() || !initial_value.is_finite() || !target_time.is_finite() {
            return Err(OdeError::NonFiniteState { step: 0 });
        }
        if target_time < start_time {
            return Err(OdeError::TargetBeforeStart);
        }

        let mut time = start_time;
        let mut value = initial_value;
        let mut step_size = self.initial_step;
        let mut accepted_steps = 0usize;
        let mut attempted_steps = 0usize;
        let mut last_step_size = 0.0;

        while time < target_time {
            if attempted_steps == self.max_steps {
                return Err(OdeError::StepLimitExceeded {
                    requested: attempted_steps.saturating_add(1),
                    max: self.max_steps,
                });
            }
            let candidate = (target_time - time).min(step_size);
            let half = candidate / 2.0;
            let full_value = rk4_step(&mut derivative, time, value, candidate, attempted_steps)?;
            let midpoint_value = rk4_step(&mut derivative, time, value, half, attempted_steps)?;
            let half_value = rk4_step(
                &mut derivative,
                time + half,
                midpoint_value,
                half,
                attempted_steps,
            )?;
            let error = (half_value - full_value).abs() / 15.0;
            let allowed_error = self.tolerance * half_value.abs().max(1.0);
            attempted_steps += 1;
            if !error.is_finite() || !allowed_error.is_finite() {
                return Err(OdeError::NonFiniteState {
                    step: attempted_steps,
                });
            }

            if error <= allowed_error {
                let next_time = (time + candidate).min(target_time);
                if next_time <= time {
                    return Err(OdeError::NonFiniteState {
                        step: attempted_steps,
                    });
                }
                time = next_time;
                value = half_value;
                accepted_steps += 1;
                last_step_size = candidate;
                let factor = if error == 0.0 {
                    2.0
                } else {
                    (0.9 * (allowed_error / error).powf(0.2)).clamp(0.2, 2.0)
                };
                step_size = (candidate * factor)
                    .min(self.initial_step)
                    .max(self.min_step);
            } else {
                if candidate <= self.min_step {
                    return Err(OdeError::AdaptiveStepTooSmall);
                }
                let factor = (0.9 * (allowed_error / error).powf(0.25)).clamp(0.1, 0.5);
                let next_step = candidate * factor;
                if next_step < self.min_step {
                    return Err(OdeError::AdaptiveStepTooSmall);
                }
                step_size = next_step;
            }
        }

        Ok(AdaptiveRk4Result {
            status: OdeStatus::Completed,
            final_time: time,
            final_value: value,
            accepted_steps,
            attempted_steps,
            tolerance: self.tolerance,
            last_step_size,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveRk4Result {
    pub status: OdeStatus,
    pub final_time: f64,
    pub final_value: f64,
    pub accepted_steps: usize,
    pub attempted_steps: usize,
    pub tolerance: f64,
    pub last_step_size: f64,
}

fn rk4_step<F>(
    derivative: &mut F,
    time: f64,
    value: f64,
    step: f64,
    step_index: usize,
) -> Result<f64, OdeError>
where
    F: FnMut(f64, f64) -> f64,
{
    let k1 = evaluate(derivative, time, value, step_index)?;
    let k2 = evaluate(
        derivative,
        time + step / 2.0,
        value + step * k1 / 2.0,
        step_index,
    )?;
    let k3 = evaluate(
        derivative,
        time + step / 2.0,
        value + step * k2 / 2.0,
        step_index,
    )?;
    let k4 = evaluate(derivative, time + step, value + step * k3, step_index)?;
    let next_value = value + step * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0;
    if next_value.is_finite() {
        Ok(next_value)
    } else {
        Err(OdeError::NonFiniteState {
            step: step_index + 1,
        })
    }
}

fn evaluate<F>(derivative: &mut F, time: f64, value: f64, step: usize) -> Result<f64, OdeError>
where
    F: FnMut(f64, f64) -> f64,
{
    if !time.is_finite() || !value.is_finite() {
        return Err(OdeError::NonFiniteState { step });
    }
    let result = derivative(time, value);
    if result.is_finite() {
        Ok(result)
    } else {
        Err(OdeError::NonFiniteDerivative { step })
    }
}

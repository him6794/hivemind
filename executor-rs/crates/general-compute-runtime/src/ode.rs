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

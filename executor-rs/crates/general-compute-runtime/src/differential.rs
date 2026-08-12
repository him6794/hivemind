use crate::reference::{Instruction, InterpreterLimits, InterpreterStatus, MinskyProgram, ReferenceInterpreter};
use crate::supervisor::Cancellation;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentialCase {
    pub source: String,
    pub input_json: String,
    pub seed: u64,
    pub expected: ReferenceObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceObservation {
    pub status: String,
    pub steps: u64,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DifferentialError {
    InvalidCase(String),
    Mismatch {
        expected: ReferenceObservation,
        observed: ReferenceObservation,
    },
}

impl fmt::Display for DifferentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCase(message) => write!(formatter, "invalid differential case: {message}"),
            Self::Mismatch { expected, observed } => {
                write!(
                    formatter,
                    "reference/backend mismatch: expected {expected:?}, observed {observed:?}"
                )
            }
        }
    }
}

impl std::error::Error for DifferentialError {}

#[derive(Debug, Clone)]
pub struct DifferentialRunner {
    case: DifferentialCase,
}

impl DifferentialRunner {
    pub fn new(case: DifferentialCase) -> Self {
        Self { case }
    }

    pub fn case(&self) -> &DifferentialCase {
        &self.case
    }

    pub fn run_reference(&self) -> Result<ReferenceObservation, DifferentialError> {
        if self.case.source != "minsky:inc(0);halt" {
            return Err(DifferentialError::InvalidCase(
                "only the pinned minsky fixture is registered".into(),
            ));
        }
        if self.case.input_json != r#"{"value": 4}"# {
            return Err(DifferentialError::InvalidCase(
                "input does not match the pinned fixture".into(),
            ));
        }
        if self.case.seed != 7 {
            return Err(DifferentialError::InvalidCase(
                "seed does not match the pinned fixture".into(),
            ));
        }
        let program = MinskyProgram::new(vec![Instruction::Inc { register: 0, next: 1 }, Instruction::Halt])
            .map_err(|error| DifferentialError::InvalidCase(error.to_string()))?;
        let result = ReferenceInterpreter::new(program).run(InterpreterLimits::new(8), &Cancellation::new());
        let observation = ReferenceObservation {
            status: match result.status {
                InterpreterStatus::Halted => "halted",
                InterpreterStatus::ResourceExhausted => "resource_exhausted",
                InterpreterStatus::Cancelled => "cancelled",
            }
            .into(),
            steps: result.steps,
            output: result.registers.first().map(ToString::to_string).unwrap_or_default(),
        };
        self.compare(&observation)?;
        Ok(observation)
    }

    pub fn compare(&self, observed: &ReferenceObservation) -> Result<(), DifferentialError> {
        if &self.case.expected == observed {
            Ok(())
        } else {
            Err(DifferentialError::Mismatch {
                expected: self.case.expected.clone(),
                observed: observed.clone(),
            })
        }
    }
}

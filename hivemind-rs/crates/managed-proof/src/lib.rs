use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PROOF_PROTOCOL_VERSION: u16 = 1;
pub const MANAGED_RUNTIME_ID: &str = "managed-function-v0";
pub const COST_MODEL_ID: &str = "managed-function-v0-metering-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionMetrics {
    pub usage_units: u64,
    pub executed_ops: u64,
    pub function_calls: u64,
    pub loop_iterations: u64,
    pub max_call_depth: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionClaim {
    pub protocol_version: u16,
    pub runtime_id: String,
    pub cost_model_id: String,
    pub task_id: String,
    pub source_sha256: [u8; 32],
    pub input_sha256: [u8; 32],
    pub output_sha256: [u8; 32],
    pub max_usage_units: u64,
    pub usage_units: u64,
    pub executed_ops: u64,
    pub function_calls: u64,
    pub loop_iterations: u64,
    pub max_call_depth: u64,
    pub output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClaimError {
    #[error(
        "execution used {usage_units} units, exceeding the bound budget of {max_usage_units} units"
    )]
    UsageExceedsBudget {
        usage_units: u64,
        max_usage_units: u64,
    },
}

impl ExecutionClaim {
    pub fn new(
        task_id: impl Into<String>,
        source: &[u8],
        input: &[u8],
        output: &[u8],
        max_usage_units: u64,
        metrics: ExecutionMetrics,
    ) -> Result<Self, ClaimError> {
        if metrics.usage_units > max_usage_units {
            return Err(ClaimError::UsageExceedsBudget {
                usage_units: metrics.usage_units,
                max_usage_units,
            });
        }

        Ok(Self {
            protocol_version: PROOF_PROTOCOL_VERSION,
            runtime_id: MANAGED_RUNTIME_ID.to_owned(),
            cost_model_id: COST_MODEL_ID.to_owned(),
            task_id: task_id.into(),
            source_sha256: sha256(source),
            input_sha256: sha256(input),
            output_sha256: sha256(output),
            max_usage_units,
            usage_units: metrics.usage_units,
            executed_ops: metrics.executed_ops,
            function_calls: metrics.function_calls,
            loop_iterations: metrics.loop_iterations,
            max_call_depth: metrics.max_call_depth,
            output_bytes: u64::try_from(output.len()).expect("output length fits in u64"),
        })
    }

    pub fn to_journal_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    pub fn from_journal_bytes(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }
}

fn sha256(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

#[cfg(test)]
mod tests {
    use super::{ClaimError, ExecutionClaim, ExecutionMetrics, PROOF_PROTOCOL_VERSION};

    fn metrics() -> ExecutionMetrics {
        ExecutionMetrics {
            usage_units: 17,
            executed_ops: 17,
            function_calls: 2,
            loop_iterations: 3,
            max_call_depth: 1,
        }
    }

    fn claim(task_id: &str) -> ExecutionClaim {
        ExecutionClaim::new(
            task_id,
            b"fn main(input) { return len(input); }",
            br#"{"value":"secret"}"#,
            br#"{"result":6}"#,
            100,
            metrics(),
        )
        .expect("valid execution claim")
    }

    #[test]
    fn claim_binds_task_program_input_output_and_protocol() {
        let original = claim("task-a");

        assert_eq!(original.protocol_version, PROOF_PROTOCOL_VERSION);
        assert_ne!(original.source_sha256, [0; 32]);
        assert_ne!(original.input_sha256, [0; 32]);
        assert_ne!(original.output_sha256, [0; 32]);
        assert_eq!(original.output_bytes, 12);

        assert_ne!(original, claim("task-b"));
        assert_ne!(
            original,
            ExecutionClaim::new(
                "task-a",
                b"fn main(input) { return 0; }",
                br#"{"value":"secret"}"#,
                br#"{"result":6}"#,
                100,
                metrics(),
            )
            .unwrap()
        );
        assert_ne!(
            original,
            ExecutionClaim::new(
                "task-a",
                b"fn main(input) { return len(input); }",
                br#"{"value":"changed"}"#,
                br#"{"result":6}"#,
                100,
                metrics(),
            )
            .unwrap()
        );
        assert_ne!(
            original,
            ExecutionClaim::new(
                "task-a",
                b"fn main(input) { return len(input); }",
                br#"{"value":"secret"}"#,
                br#"{"result":7}"#,
                100,
                metrics(),
            )
            .unwrap()
        );
    }

    #[test]
    fn claim_rejects_usage_above_the_bound_budget() {
        let error =
            ExecutionClaim::new("task-a", b"return 1;", b"null", b"1", 16, metrics()).unwrap_err();

        assert_eq!(
            error,
            ClaimError::UsageExceedsBudget {
                usage_units: 17,
                max_usage_units: 16,
            }
        );
    }

    #[test]
    fn journal_encoding_is_deterministic_and_round_trips() {
        let claim = claim("task-a");
        let first = claim.to_journal_bytes().unwrap();
        let second = claim.to_journal_bytes().unwrap();

        assert_eq!(first, second);
        assert_eq!(ExecutionClaim::from_journal_bytes(&first).unwrap(), claim);
    }
}

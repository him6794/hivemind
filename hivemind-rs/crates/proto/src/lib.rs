tonic::include_proto!("nodepool");

/// Maximum UTF-8 byte length accepted for any task identifier at an admission boundary.
pub const TASK_ID_MAX_BYTES: usize = 255;

/// Maximum byte length accepted for `managed-function-v0` source code.
pub const MANAGED_TASK_SOURCE_MAX_BYTES: usize = 64 * 1024;

/// Maximum byte length accepted for `managed-function-v0` JSON input.
pub const MANAGED_JSON_INPUT_MAX_BYTES: usize = 1024 * 1024;

/// Maximum signed admission budget accepted for `managed-function-v0` execution.
pub const MANAGED_BUDGET_MAX_USAGE_UNITS: i64 = 1_000_000;

/// Maximum status/output byte length accepted in a Worker execution response.
pub const WORKER_STATUS_MESSAGE_MAX_BYTES: usize = 1024 * 1024;

/// Maximum legacy managed-receipt byte length accepted in a Worker execution response.
pub const LEGACY_MANAGED_RECEIPT_MAX_BYTES: usize = 64 * 1024;

/// Maximum encoded managed-proof protobuf message accepted across the verifier RPC boundary.
pub const MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES: usize = 2_166_784;

/// Maximum gRPC message size for Worker RPCs.
///
/// A managed `ExecuteTaskResponse` can include a 2,166,784-byte proof envelope,
/// a 1 MiB status/output payload, a legacy receipt, and protobuf field overhead.
/// This explicit 4 MiB cap matches tonic's default whole-message ceiling while
/// keeping the Worker client symmetric for request encoding and response decoding.
pub const WORKER_RPC_MESSAGE_MAX_BYTES: usize = 4 * 1024 * 1024;

pub use batch_runtime_service_client::BatchRuntimeServiceClient;
pub use batch_runtime_service_server::{BatchRuntimeService, BatchRuntimeServiceServer};
pub use master_node_service_client::MasterNodeServiceClient;
pub use master_node_service_server::{MasterNodeService, MasterNodeServiceServer};
pub use node_manager_service_client::NodeManagerServiceClient;
pub use node_manager_service_server::{NodeManagerService, NodeManagerServiceServer};
pub use user_service_client::UserServiceClient;
pub use user_service_server::{UserService, UserServiceServer};
pub use vpn_service_client::VpnServiceClient;
pub use vpn_service_server::{VpnService, VpnServiceServer};
pub use worker_node_service_client::WorkerNodeServiceClient;
pub use worker_node_service_server::{WorkerNodeService, WorkerNodeServiceServer};

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::{
        ExecuteTaskResponse, ManagedProofEnvelope, LEGACY_MANAGED_RECEIPT_MAX_BYTES,
        MANAGED_BUDGET_MAX_USAGE_UNITS, MANAGED_JSON_INPUT_MAX_BYTES,
        MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES, MANAGED_TASK_SOURCE_MAX_BYTES, TASK_ID_MAX_BYTES,
        WORKER_RPC_MESSAGE_MAX_BYTES, WORKER_STATUS_MESSAGE_MAX_BYTES,
    };

    #[test]
    fn admission_contract_limits_are_stable() {
        assert_eq!(TASK_ID_MAX_BYTES, 255);
        assert_eq!(MANAGED_TASK_SOURCE_MAX_BYTES, 64 * 1024);
        assert_eq!(MANAGED_JSON_INPUT_MAX_BYTES, 1024 * 1024);
        assert_eq!(MANAGED_BUDGET_MAX_USAGE_UNITS, 1_000_000);
        assert_eq!(MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES, 2_166_784);
        assert_eq!(WORKER_STATUS_MESSAGE_MAX_BYTES, 1024 * 1024);
        assert_eq!(LEGACY_MANAGED_RECEIPT_MAX_BYTES, 64 * 1024);
    }

    #[test]
    fn worker_rpc_message_cap_covers_managed_execution_response() {
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "x".repeat(WORKER_STATUS_MESSAGE_MAX_BYTES),
            managed_executed_ops: i64::MAX,
            managed_output_bytes: i64::MAX,
            managed_receipt_json: "x".repeat(LEGACY_MANAGED_RECEIPT_MAX_BYTES),
            managed_proof: Some(ManagedProofEnvelope {
                receipt_json: vec![0; MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES - 16],
                ..ManagedProofEnvelope::default()
            }),
        };
        let worker_rpc_message_max_bytes = std::hint::black_box(WORKER_RPC_MESSAGE_MAX_BYTES);

        assert!(
            worker_rpc_message_max_bytes
                >= MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES
                    + WORKER_STATUS_MESSAGE_MAX_BYTES
                    + LEGACY_MANAGED_RECEIPT_MAX_BYTES
        );
        assert!(worker_rpc_message_max_bytes <= 4 * WORKER_STATUS_MESSAGE_MAX_BYTES);
        assert!(response.encoded_len() <= worker_rpc_message_max_bytes);
    }

    #[test]
    fn managed_proof_envelope_round_trips_on_execute_response() {
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "42".into(),
            managed_executed_ops: 17,
            managed_output_bytes: 2,
            managed_receipt_json: "{}".into(),
            managed_proof: Some(ManagedProofEnvelope {
                proof_scheme: "risc0-zkvm-3.0.6".into(),
                image_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                journal: br#"{"usage_units":17}"#.to_vec(),
                receipt_json: br#"{"inner":{}}"#.to_vec(),
            }),
        };

        let decoded = ExecuteTaskResponse::decode(response.encode_to_vec().as_slice()).unwrap();
        let proof = decoded.managed_proof.expect("proof envelope is present");

        assert_eq!(proof.proof_scheme, "risc0-zkvm-3.0.6");
        assert_eq!(proof.image_id, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(proof.journal, br#"{"usage_units":17}"#);
        assert_eq!(proof.receipt_json, br#"{"inner":{}}"#);
    }
}

tonic::include_proto!("nodepool");

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

    use super::{ExecuteTaskResponse, ManagedProofEnvelope};

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

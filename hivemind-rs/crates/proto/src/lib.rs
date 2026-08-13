tonic::include_proto!("nodepool");

/// Maximum UTF-8 byte length accepted for any task identifier at an admission boundary.
pub const TASK_ID_MAX_BYTES: usize = 255;

/// Maximum byte length accepted for `managed-function-v0` source code.
pub const MANAGED_TASK_SOURCE_MAX_BYTES: usize = 64 * 1024;

/// Maximum byte length accepted for `managed-function-v0` JSON input.
pub const MANAGED_JSON_INPUT_MAX_BYTES: usize = 1024 * 1024;

/// Maximum byte length accepted for a general-compute-v1alpha1 request manifest.
pub const GENERAL_COMPUTE_MANIFEST_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Maximum serialized size accepted for a general-compute-v1alpha1 result envelope.
pub const GENERAL_COMPUTE_RESULT_MAX_BYTES: usize = 2 * 1024 * 1024;

/// Maximum raw payload accepted in one general-compute chunk transfer.
pub const GENERAL_COMPUTE_CHUNK_UPLOAD_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Maximum encoded message size for the dedicated general-compute chunk RPCs.
/// This is intentionally larger than one raw chunk and is not used for the
/// existing ExecuteTask RPC contract.
pub const GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES: usize =
    GENERAL_COMPUTE_CHUNK_UPLOAD_MAX_BYTES + 64 * 1024;

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

/// Validate the identity, digest syntax, and size binding of one typed upload.
pub fn validate_general_compute_chunk_upload(
    upload: &GeneralComputeChunkUpload,
) -> Result<(), &'static str> {
    validate_chunk_identity(
        &upload.token,
        &upload.execution_id,
        &upload.attempt_id,
        &upload.idempotency_key,
        &upload.request_digest,
        &upload.artifact_id,
    )?;
    if upload.offset < 0 {
        return Err("chunk offset must not be negative");
    }
    if upload.size_bytes <= 0 {
        return Err("chunk size must be positive");
    }
    let size = usize::try_from(upload.size_bytes).map_err(|_| "chunk size is too large")?;
    if size > GENERAL_COMPUTE_CHUNK_UPLOAD_MAX_BYTES {
        return Err("chunk exceeds the upload byte limit");
    }
    if upload.bytes.len() != size {
        return Err("chunk size does not match payload bytes");
    }
    if !is_sha256_digest(&upload.sha256) {
        return Err("chunk digest must be a sha256 digest");
    }
    Ok(())
}

/// Validate the identity and completed-digest list of a resume request.
pub fn validate_general_compute_chunk_resume_request(
    request: &GeneralComputeChunkResumeRequest,
) -> Result<(), &'static str> {
    validate_chunk_identity(
        &request.token,
        &request.execution_id,
        &request.attempt_id,
        &request.idempotency_key,
        &request.request_digest,
        &request.artifact_id,
    )?;
    for digest in &request.completed_sha256 {
        if !is_sha256_digest(digest) {
            return Err("completed chunk digest must be a sha256 digest");
        }
    }
    Ok(())
}

fn validate_chunk_identity(
    token: &str,
    execution_id: &str,
    attempt_id: &str,
    idempotency_key: &str,
    request_digest: &str,
    artifact_id: &str,
) -> Result<(), &'static str> {
    if token.trim().is_empty()
        || execution_id.trim().is_empty()
        || attempt_id.trim().is_empty()
        || idempotency_key.trim().is_empty()
        || artifact_id.trim().is_empty()
    {
        return Err("chunk transfer identity fields are required");
    }
    if !is_sha256_digest(request_digest) {
        return Err("request digest must be a sha256 digest");
    }
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub use batch_runtime_service_client::BatchRuntimeServiceClient;
pub use batch_runtime_service_server::{BatchRuntimeService, BatchRuntimeServiceServer};
pub use general_compute_chunk_service_client::GeneralComputeChunkServiceClient;
pub use general_compute_chunk_service_server::{
    GeneralComputeChunkService, GeneralComputeChunkServiceServer,
};
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
        validate_general_compute_chunk_resume_request, validate_general_compute_chunk_upload,
        ExecuteTaskResponse, GeneralComputeChunkDescriptor, GeneralComputeChunkResumeRequest,
        GeneralComputeChunkResumeResponse, GeneralComputeChunkUpload,
        GeneralComputeChunkUploadResponse, ManagedProofEnvelope,
        GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES, GENERAL_COMPUTE_CHUNK_UPLOAD_MAX_BYTES,
        GENERAL_COMPUTE_MANIFEST_MAX_BYTES, GENERAL_COMPUTE_RESULT_MAX_BYTES,
        LEGACY_MANAGED_RECEIPT_MAX_BYTES, MANAGED_BUDGET_MAX_USAGE_UNITS,
        MANAGED_JSON_INPUT_MAX_BYTES, MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES,
        MANAGED_TASK_SOURCE_MAX_BYTES, TASK_ID_MAX_BYTES, WORKER_RPC_MESSAGE_MAX_BYTES,
        WORKER_STATUS_MESSAGE_MAX_BYTES,
    };

    #[test]
    fn admission_contract_limits_are_stable() {
        assert_eq!(TASK_ID_MAX_BYTES, 255);
        assert_eq!(MANAGED_TASK_SOURCE_MAX_BYTES, 64 * 1024);
        assert_eq!(MANAGED_JSON_INPUT_MAX_BYTES, 1024 * 1024);
        assert_eq!(GENERAL_COMPUTE_MANIFEST_MAX_BYTES, 4 * 1024 * 1024);
        assert_eq!(GENERAL_COMPUTE_RESULT_MAX_BYTES, 2 * 1024 * 1024);
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
            ..ExecuteTaskResponse::default()
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
            ..ExecuteTaskResponse::default()
        };

        let decoded = ExecuteTaskResponse::decode(response.encode_to_vec().as_slice()).unwrap();
        let proof = decoded.managed_proof.expect("proof envelope is present");

        assert_eq!(proof.proof_scheme, "risc0-zkvm-3.0.6");
        assert_eq!(proof.image_id, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(proof.journal, br#"{"usage_units":17}"#);
        assert_eq!(proof.receipt_json, br#"{"inner":{}}"#);
    }

    #[test]
    fn general_compute_result_round_trips_with_a_bounded_payload() {
        let payload = br#"{"status":"completed","output":"ok"}"#.to_vec();
        assert!(payload.len() <= GENERAL_COMPUTE_RESULT_MAX_BYTES);
        let response = ExecuteTaskResponse {
            success: true,
            general_compute_result_json: payload.clone(),
            ..ExecuteTaskResponse::default()
        };

        let decoded = ExecuteTaskResponse::decode(response.encode_to_vec().as_slice()).unwrap();

        assert_eq!(decoded.general_compute_result_json, payload);
    }

    #[test]
    fn worker_rpc_message_cap_covers_the_general_compute_result_payload() {
        let response = ExecuteTaskResponse {
            general_compute_result_json: vec![0; GENERAL_COMPUTE_RESULT_MAX_BYTES],
            ..ExecuteTaskResponse::default()
        };

        assert!(response.encoded_len() <= WORKER_RPC_MESSAGE_MAX_BYTES);
    }

    #[test]
    fn general_compute_chunk_upload_round_trips_all_binding_fields() {
        let upload = GeneralComputeChunkUpload {
            token: "nodepool-token".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
            artifact_id: "source".into(),
            offset: 16,
            size_bytes: 4,
            sha256: "sha256:3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7"
                .into(),
            bytes: b"data".to_vec(),
        };

        validate_general_compute_chunk_upload(&upload).expect("valid chunk upload should pass");
        let decoded = GeneralComputeChunkUpload::decode(upload.encode_to_vec().as_slice())
            .expect("chunk upload should decode");
        assert_eq!(decoded, upload);
        assert_eq!(decoded.bytes.len(), decoded.size_bytes as usize);
    }

    #[test]
    fn general_compute_chunk_wire_validation_rejects_unbounded_or_unbound_payloads() {
        let mut upload = GeneralComputeChunkUpload {
            token: "nodepool-token".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
            artifact_id: "source".into(),
            offset: 0,
            size_bytes: 4,
            sha256: "sha256:3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7"
                .into(),
            bytes: b"data".to_vec(),
        };

        upload.size_bytes = -1;
        assert!(validate_general_compute_chunk_upload(&upload).is_err());
        upload.size_bytes = 4;
        upload.bytes = b"short".to_vec();
        assert!(validate_general_compute_chunk_upload(&upload).is_err());
        upload.bytes = vec![0; GENERAL_COMPUTE_CHUNK_UPLOAD_MAX_BYTES + 1];
        upload.size_bytes = upload.bytes.len() as i64;
        assert!(validate_general_compute_chunk_upload(&upload).is_err());
        upload.bytes.clear();
        upload.size_bytes = 0;
        upload.execution_id.clear();
        assert!(validate_general_compute_chunk_upload(&upload).is_err());
    }

    #[test]
    fn general_compute_chunk_resume_validation_binds_identity_and_digest_list() {
        let request = GeneralComputeChunkResumeRequest {
            token: "nodepool-token".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
            artifact_id: "source".into(),
            completed_sha256: vec![
                "sha256:3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7".into(),
            ],
        };

        validate_general_compute_chunk_resume_request(&request)
            .expect("valid resume request should pass");
        let decoded = GeneralComputeChunkResumeRequest::decode(request.encode_to_vec().as_slice())
            .expect("resume request should decode");
        assert_eq!(decoded, request);

        let mut invalid = request;
        invalid.completed_sha256[0] = "not-a-sha256".into();
        assert!(validate_general_compute_chunk_resume_request(&invalid).is_err());
    }

    #[test]
    fn general_compute_chunk_rpc_response_round_trips_with_a_separate_message_cap() {
        let response = GeneralComputeChunkUploadResponse {
            success: true,
            status_message: "accepted".into(),
            accepted_chunks: 2,
        };
        let decoded =
            GeneralComputeChunkUploadResponse::decode(response.encode_to_vec().as_slice())
                .expect("chunk RPC response should decode");

        assert_eq!(decoded, response);
        assert!(GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES > WORKER_RPC_MESSAGE_MAX_BYTES);
        assert!(
            GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES > GENERAL_COMPUTE_CHUNK_UPLOAD_MAX_BYTES
        );
    }

    #[test]
    fn general_compute_chunk_resume_response_round_trips_missing_descriptors() {
        let response = GeneralComputeChunkResumeResponse {
            success: true,
            status_message: "resume".into(),
            missing_chunks: vec![GeneralComputeChunkDescriptor {
                offset: 8,
                size_bytes: 4,
                sha256: "sha256:3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7"
                    .into(),
            }],
        };
        let decoded =
            GeneralComputeChunkResumeResponse::decode(response.encode_to_vec().as_slice())
                .expect("chunk resume response should decode");
        assert_eq!(decoded, response);
    }
}

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

/// Maximum encoded size accepted for one transfer-lease authority request.
pub const GENERAL_COMPUTE_TRANSFER_LEASE_RPC_MESSAGE_MAX_BYTES: usize = 16 * 1024;

/// Maximum byte length of a Nodepool-issued Worker execution JWT in an
/// authority request.
pub const WORKER_EXECUTION_TOKEN_MAX_BYTES: usize = 8 * 1024;

/// Maximum byte length of task, Worker, execution, attempt, or idempotency
/// identity fields. This matches the persistent control-plane identifiers.
pub const GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES: usize = 255;

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
    if upload.transfer_generation <= 0 {
        return Err("transfer generation must be positive");
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
    if request.transfer_generation <= 0 {
        return Err("transfer generation must be positive");
    }
    for digest in &request.completed_sha256 {
        if !is_sha256_digest(digest) {
            return Err("completed chunk digest must be a sha256 digest");
        }
    }
    Ok(())
}

/// Validate the bounded identity and manifest envelope used to prepare a
/// general-compute transfer on a Worker.
pub fn validate_general_compute_prepare_request(
    request: &GeneralComputePrepareRequest,
) -> Result<(), &'static str> {
    if request.task_id.trim().is_empty() {
        return Err("task id is required");
    }
    if request.task_id.len() > TASK_ID_MAX_BYTES {
        return Err("task id exceeds the byte limit");
    }
    if request.token.trim().is_empty() {
        return Err("worker execution token is required");
    }
    if request.token.len() > WORKER_EXECUTION_TOKEN_MAX_BYTES {
        return Err("worker execution token exceeds the byte limit");
    }
    if request.runtime != "general-compute-v1alpha1" {
        return Err("PrepareGeneralCompute requires general-compute-v1alpha1");
    }
    if request.general_compute_manifest_json.is_empty() {
        return Err("general-compute manifest is required");
    }
    if request.general_compute_manifest_json.len() > GENERAL_COMPUTE_MANIFEST_MAX_BYTES {
        return Err("general-compute manifest exceeds the byte limit");
    }
    for value in [
        request.execution_id.as_str(),
        request.attempt_id.as_str(),
        request.idempotency_key.as_str(),
    ] {
        if value.trim().is_empty() {
            return Err("general-compute transfer identity fields are required");
        }
        if value.len() > GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES {
            return Err("general-compute transfer identity field exceeds the byte limit");
        }
    }
    if !is_sha256_digest(&request.request_digest) {
        return Err("request digest must be a sha256 digest");
    }
    if request.transfer_generation <= 0 {
        return Err("transfer generation must be positive");
    }
    if prost::Message::encoded_len(request) > GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES {
        return Err("general-compute preparation exceeds the message limit");
    }
    Ok(())
}

/// Validate a Worker-to-Nodepool transfer-lease authority envelope.
pub fn validate_general_compute_transfer_lease_request(
    request: &ValidateGeneralComputeTransferLeaseRequest,
) -> Result<(), &'static str> {
    if request.token.trim().is_empty() {
        return Err("worker execution token is required");
    }
    if request.token.len() > WORKER_EXECUTION_TOKEN_MAX_BYTES {
        return Err("worker execution token exceeds the byte limit");
    }
    for value in [
        request.worker_id.as_str(),
        request.task_id.as_str(),
        request.execution_id.as_str(),
        request.attempt_id.as_str(),
        request.idempotency_key.as_str(),
    ] {
        if value.trim().is_empty() {
            return Err("transfer lease identity fields are required");
        }
        if value.len() > GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES {
            return Err("transfer lease identity field exceeds the byte limit");
        }
    }
    if request.transfer_generation <= 0 {
        return Err("transfer generation must be positive");
    }
    if !is_sha256_digest(&request.request_digest) {
        return Err("request digest must be a sha256 digest");
    }
    if prost::Message::encoded_len(request) > GENERAL_COMPUTE_TRANSFER_LEASE_RPC_MESSAGE_MAX_BYTES {
        return Err("transfer lease authority request exceeds the message limit");
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
        validate_general_compute_prepare_request, validate_general_compute_transfer_lease_request,
        ExecuteTaskResponse, GeneralComputeChunkDescriptor, GeneralComputeChunkResumeRequest,
        GeneralComputeChunkResumeResponse, GeneralComputeChunkUpload,
        GeneralComputeChunkUploadResponse, GeneralComputePrepareRequest,
        GeneralComputePrepareResponse, ManagedProofEnvelope,
        ValidateGeneralComputeTransferLeaseRequest, ValidateGeneralComputeTransferLeaseResponse,
        GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES, GENERAL_COMPUTE_CHUNK_UPLOAD_MAX_BYTES,
        GENERAL_COMPUTE_MANIFEST_MAX_BYTES, GENERAL_COMPUTE_RESULT_MAX_BYTES,
        GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES,
        GENERAL_COMPUTE_TRANSFER_LEASE_RPC_MESSAGE_MAX_BYTES, LEGACY_MANAGED_RECEIPT_MAX_BYTES,
        MANAGED_BUDGET_MAX_USAGE_UNITS, MANAGED_JSON_INPUT_MAX_BYTES,
        MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES, MANAGED_TASK_SOURCE_MAX_BYTES, TASK_ID_MAX_BYTES,
        WORKER_EXECUTION_TOKEN_MAX_BYTES, WORKER_RPC_MESSAGE_MAX_BYTES,
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
        assert_eq!(WORKER_EXECUTION_TOKEN_MAX_BYTES, 8 * 1024);
        assert_eq!(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES, 255);
        assert_eq!(
            GENERAL_COMPUTE_TRANSFER_LEASE_RPC_MESSAGE_MAX_BYTES,
            16 * 1024
        );
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
    fn transfer_lease_authority_envelope_round_trips_full_identity() {
        let request = ValidateGeneralComputeTransferLeaseRequest {
            token: "nodepool-signed-worker-execution-token".into(),
            worker_id: "worker-7".into(),
            task_id: "task-1".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-2".into(),
            transfer_generation: 3,
            idempotency_key: "idempotency-4".into(),
            request_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        };

        validate_general_compute_transfer_lease_request(&request)
            .expect("fully bound transfer lease request should pass");
        let decoded =
            ValidateGeneralComputeTransferLeaseRequest::decode(request.encode_to_vec().as_slice())
                .expect("transfer lease request should decode");
        assert_eq!(decoded, request);

        let response = ValidateGeneralComputeTransferLeaseResponse {
            success: true,
            status_message: "active".into(),
        };
        let decoded = ValidateGeneralComputeTransferLeaseResponse::decode(
            response.encode_to_vec().as_slice(),
        )
        .expect("transfer lease response should decode");
        assert_eq!(decoded, response);
    }

    #[test]
    fn transfer_lease_authority_validation_rejects_unbound_or_unbounded_identity() {
        let valid = ValidateGeneralComputeTransferLeaseRequest {
            token: "nodepool-signed-worker-execution-token".into(),
            worker_id: "worker-7".into(),
            task_id: "task-1".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-2".into(),
            transfer_generation: 3,
            idempotency_key: "idempotency-4".into(),
            request_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        };
        let invalid = [
            ValidateGeneralComputeTransferLeaseRequest {
                token: " ".into(),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                worker_id: String::new(),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                task_id: String::new(),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                execution_id: String::new(),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                attempt_id: String::new(),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                idempotency_key: String::new(),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                request_digest: "not-a-digest".into(),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                transfer_generation: 0,
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                transfer_generation: -1,
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                token: "x".repeat(WORKER_EXECUTION_TOKEN_MAX_BYTES + 1),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                worker_id: "x".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES + 1),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                task_id: "x".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES + 1),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                execution_id: "x".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES + 1),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                attempt_id: "x".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES + 1),
                ..valid.clone()
            },
            ValidateGeneralComputeTransferLeaseRequest {
                idempotency_key: "x".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES + 1),
                ..valid.clone()
            },
        ];

        for request in invalid {
            assert!(
                validate_general_compute_transfer_lease_request(&request).is_err(),
                "unbound or unbounded transfer identity must be rejected: {request:?}"
            );
        }

        let maximum = ValidateGeneralComputeTransferLeaseRequest {
            token: "x".repeat(WORKER_EXECUTION_TOKEN_MAX_BYTES),
            worker_id: "w".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES),
            task_id: "t".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES),
            execution_id: "e".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES),
            attempt_id: "a".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES),
            idempotency_key: "i".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES),
            ..valid
        };
        validate_general_compute_transfer_lease_request(&maximum)
            .expect("maximum bounded transfer identity should pass");
        assert!(maximum.encoded_len() <= GENERAL_COMPUTE_TRANSFER_LEASE_RPC_MESSAGE_MAX_BYTES);
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
            transfer_generation: 7,
        };

        validate_general_compute_chunk_upload(&upload).expect("valid chunk upload should pass");
        let decoded = GeneralComputeChunkUpload::decode(upload.encode_to_vec().as_slice())
            .expect("chunk upload should decode");
        assert_eq!(decoded, upload);
        assert_eq!(decoded.transfer_generation, 7);
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
            transfer_generation: 7,
        };

        upload.transfer_generation = 0;
        assert!(validate_general_compute_chunk_upload(&upload).is_err());
        upload.transfer_generation = 7;
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
            transfer_generation: 7,
        };

        validate_general_compute_chunk_resume_request(&request)
            .expect("valid resume request should pass");
        let decoded = GeneralComputeChunkResumeRequest::decode(request.encode_to_vec().as_slice())
            .expect("resume request should decode");
        assert_eq!(decoded, request);

        let mut invalid = request;
        invalid.transfer_generation = 0;
        assert!(validate_general_compute_chunk_resume_request(&invalid).is_err());
        invalid.transfer_generation = 7;
        invalid.completed_sha256[0] = "not-a-sha256".into();
        assert!(validate_general_compute_chunk_resume_request(&invalid).is_err());
    }

    #[test]
    fn general_compute_prepare_envelope_round_trips_full_transfer_identity() {
        let request = GeneralComputePrepareRequest {
            task_id: "task-1".into(),
            token: "nodepool-signed-worker-execution-token".into(),
            runtime: "general-compute-v1alpha1".into(),
            general_compute_manifest_json: br#"{"execution_id":"execution-1"}"#.to_vec(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-2".into(),
            idempotency_key: "idempotency-3".into(),
            request_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            transfer_generation: 7,
        };
        let decoded = GeneralComputePrepareRequest::decode(request.encode_to_vec().as_slice())
            .expect("prepare request should decode");
        assert_eq!(decoded, request);

        let response = GeneralComputePrepareResponse {
            success: true,
            status_message: "prepared".into(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            transfer_generation: request.transfer_generation,
        };
        let decoded = GeneralComputePrepareResponse::decode(response.encode_to_vec().as_slice())
            .expect("prepare response should decode");
        assert_eq!(decoded, response);
    }

    #[test]
    fn general_compute_prepare_validator_rejects_unbound_or_oversized_fields() {
        let valid = GeneralComputePrepareRequest {
            task_id: "task-1".into(),
            token: "nodepool-signed-worker-execution-token".into(),
            runtime: "general-compute-v1alpha1".into(),
            general_compute_manifest_json: br#"{"execution_id":"execution-1"}"#.to_vec(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-2".into(),
            idempotency_key: "idempotency-3".into(),
            request_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            transfer_generation: 7,
        };
        assert_eq!(validate_general_compute_prepare_request(&valid), Ok(()));

        let mut invalid = valid.clone();
        invalid.task_id.clear();
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid.clone();
        invalid.task_id = "x".repeat(TASK_ID_MAX_BYTES + 1);
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid.clone();
        invalid.token = "x".repeat(WORKER_EXECUTION_TOKEN_MAX_BYTES + 1);
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid.clone();
        invalid.runtime = "managed-function-v0".into();
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid.clone();
        invalid.general_compute_manifest_json.clear();
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid.clone();
        invalid.general_compute_manifest_json = vec![0; GENERAL_COMPUTE_MANIFEST_MAX_BYTES + 1];
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid.clone();
        invalid.execution_id = " ".into();
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid.clone();
        invalid.attempt_id = "x".repeat(GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES + 1);
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid.clone();
        invalid.request_digest = "not-a-sha256".into();
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
        invalid = valid;
        invalid.transfer_generation = 0;
        assert!(validate_general_compute_prepare_request(&invalid).is_err());
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

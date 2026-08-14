use chrono::Utc;
use general_compute_runtime::artifact::CasChunkStore;
use general_compute_runtime::transport::ChunkTransportError;
use general_compute_runtime::{
    sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRole, ExecutionPolicy,
    GeneralComputeRequest, GENERAL_COMPUTE_RUNTIME_VERSION,
};
use hivemind_auth::worker_execution::{WorkerExecutionSigner, WorkerExecutionVerifier};
use hivemind_models::Claims;
use hivemind_proto::{GeneralComputeChunkResumeRequest, GeneralComputeChunkUpload};
use hivemind_worker_executor::chunk_transport::{
    ingest_general_compute_chunk, resume_general_compute_chunks, VerifiedWorkerExecution,
    WorkerChunkIngestError,
};
use std::sync::OnceLock;
use tempfile::TempDir;

const TASK_ID: &str = "task-1";
const WORKER_ID: &str = "worker-1";
const BYTES: &[u8] = b"print(42)";

fn request() -> GeneralComputeRequest {
    let source = ArtifactManifest {
        artifact_id: "source".into(),
        role: ArtifactRole::Source,
        size_bytes: BYTES.len() as u64,
        mime_type: "text/plain".into(),
        sha256: sha256_digest(BYTES),
        chunks: vec![ArtifactChunk {
            offset: 0,
            size_bytes: BYTES.len() as u64,
            sha256: sha256_digest(BYTES),
        }],
        inline_bytes: None,
    };
    let mut request = GeneralComputeRequest {
        execution_id: "execution-1".into(),
        attempt_id: "attempt-1".into(),
        idempotency_key: "idempotency-1".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: format!("sha256:{}", "a".repeat(64)),
        backend_id: "python-reference".into(),
        entrypoint: "main".into(),
        source_artifact: source,
        input_artifacts: vec![],
        execution_policy: ExecutionPolicy::default(),
        determinism: Default::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn token() -> String {
    let (private_key, _public_key) = test_key_pair();
    WorkerExecutionSigner::from_pem(private_key)
        .expect("test signer should parse")
        .encode_claims(&Claims {
            sub: "owner-1".into(),
            user_id: "owner-1".into(),
            role: Some("worker-execution".into()),
            task_id: Some(TASK_ID.into()),
            worker_id: Some(WORKER_ID.into()),
            exp: (Utc::now().timestamp() + 300) as usize,
            iat: Utc::now().timestamp() as usize,
        })
        .expect("test token should encode")
}

fn test_key_pair() -> (&'static str, &'static str) {
    static KEY_PAIR: OnceLock<(String, String)> = OnceLock::new();
    let pair = KEY_PAIR.get_or_init(hivemind_config::generate_worker_execution_test_key_pair);
    (pair.0.as_str(), pair.1.as_str())
}

fn verified_authorization(token: &str) -> VerifiedWorkerExecution {
    let (_private_key, public_key) = test_key_pair();
    let verifier =
        WorkerExecutionVerifier::from_pem(public_key).expect("test verifier should parse");
    VerifiedWorkerExecution::from_token(&verifier, token, TASK_ID, WORKER_ID)
        .expect("test token should verify")
}

fn upload(request: &GeneralComputeRequest, token: &str) -> GeneralComputeChunkUpload {
    GeneralComputeChunkUpload {
        token: token.into(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        artifact_id: "source".into(),
        offset: 0,
        size_bytes: BYTES.len() as i64,
        sha256: sha256_digest(BYTES),
        bytes: BYTES.to_vec(),
        transfer_generation: 1,
    }
}

fn resume(request: &GeneralComputeRequest, token: &str) -> GeneralComputeChunkResumeRequest {
    GeneralComputeChunkResumeRequest {
        token: token.into(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        artifact_id: "source".into(),
        completed_sha256: vec![],
        transfer_generation: 1,
    }
}

fn store() -> (TempDir, CasChunkStore) {
    let root = TempDir::new().expect("temporary CAS root should be created");
    let store = CasChunkStore::new(root.path()).expect("temporary CAS root should be valid");
    (root, store)
}

#[test]
fn adapter_rejects_a_proto_token_that_differs_from_the_verified_token() {
    let request = request();
    let (_root, store) = store();
    let token = token();
    let upload = upload(&request, "forged-token");

    let result =
        ingest_general_compute_chunk(&store, &request, &upload, &verified_authorization(&token));

    assert_eq!(result, Err(WorkerChunkIngestError::TokenMismatch));
}

#[test]
fn adapter_rejects_stale_attempts_before_cas_ingest() {
    let request = request();
    let (_root, store) = store();
    let token = token();
    let mut upload = upload(&request, &token);
    upload.attempt_id = "attempt-previous".into();

    let result =
        ingest_general_compute_chunk(&store, &request, &upload, &verified_authorization(&token));

    assert_eq!(
        result,
        Err(WorkerChunkIngestError::Transport(
            ChunkTransportError::IdentityMismatch
        ))
    );
}

#[test]
fn adapter_rejects_a_wrong_request_digest_before_cas_ingest() {
    let request = request();
    let (_root, store) = store();
    let token = token();
    let mut upload = upload(&request, &token);
    upload.request_digest = format!("sha256:{}", "b".repeat(64));

    let result =
        ingest_general_compute_chunk(&store, &request, &upload, &verified_authorization(&token));

    assert_eq!(
        result,
        Err(WorkerChunkIngestError::Transport(
            ChunkTransportError::IdentityMismatch
        ))
    );
}

#[test]
fn adapter_rejects_a_manifest_mismatch_before_cas_ingest() {
    let request = request();
    let (_root, store) = store();
    let token = token();
    let mut upload = upload(&request, &token);
    upload.offset = 1;

    let result =
        ingest_general_compute_chunk(&store, &request, &upload, &verified_authorization(&token));

    assert_eq!(
        result,
        Err(WorkerChunkIngestError::Transport(
            ChunkTransportError::ManifestChunkMismatch
        ))
    );
}

#[test]
fn adapter_rejects_payload_bytes_that_do_not_match_the_manifest_digest() {
    let request = request();
    let (_root, store) = store();
    let token = token();
    let mut upload = upload(&request, &token);
    upload.bytes = b"print(43)".to_vec();

    let result =
        ingest_general_compute_chunk(&store, &request, &upload, &verified_authorization(&token));

    assert_eq!(
        result,
        Err(WorkerChunkIngestError::Transport(
            ChunkTransportError::ChunkDigestMismatch
        ))
    );
}

#[test]
fn adapter_allows_an_identical_replay_but_not_unverified_bytes() {
    let request = request();
    let (_root, store) = store();
    let token = token();
    let upload = upload(&request, &token);
    let auth = verified_authorization(&token);

    ingest_general_compute_chunk(&store, &request, &upload, &auth)
        .expect("first verified upload should succeed");
    ingest_general_compute_chunk(&store, &request, &upload, &auth)
        .expect("identical verified replay should be idempotent");
}

#[test]
fn adapter_returns_only_manifest_chunks_missing_from_a_verified_resume_request() {
    let request = request();
    let (_root, store) = store();
    let token = token();
    let resume = resume(&request, &token);
    let auth = verified_authorization(&token);

    let missing = resume_general_compute_chunks(&store, &request, &resume, &auth)
        .expect("verified resume request should return missing chunks");

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].offset, 0);
    assert_eq!(missing[0].sha256, sha256_digest(BYTES));
}

#[test]
fn adapter_does_not_trust_a_completed_digest_when_the_local_cas_is_missing_it() {
    let request = request();
    let (_root, store) = store();
    let token = token();
    let mut resume = resume(&request, &token);
    resume.completed_sha256 = vec![sha256_digest(BYTES)];
    let auth = verified_authorization(&token);

    let missing = resume_general_compute_chunks(&store, &request, &resume, &auth)
        .expect("resume should recompute local CAS state");

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].sha256, sha256_digest(BYTES));
}

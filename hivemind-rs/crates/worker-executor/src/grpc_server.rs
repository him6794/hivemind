#![allow(clippy::result_large_err)]

use hivemind_auth::worker_execution::{WorkerExecutionClaims, WorkerExecutionVerifier};
use hivemind_models::Claims;
use hivemind_proto::{
    general_compute_chunk_service_server::GeneralComputeChunkService,
    node_manager_service_client::NodeManagerServiceClient, validate_managed_consensus_request,
    validate_managed_consensus_response, worker_node_service_server::WorkerNodeService,
    ExecuteTaskRequest, ExecuteTaskResponse, GeneralComputeChunkDescriptor,
    GeneralComputeChunkResumeRequest, GeneralComputeChunkResumeResponse, GeneralComputeChunkUpload,
    GeneralComputeChunkUploadResponse, GeneralComputePrepareRequest, GeneralComputePrepareResponse,
    ManagedConsensusResult, StopTaskExecutionRequest, StopTaskExecutionResponse, TaskOutputRequest,
    TaskOutputResponse, TaskOutputUploadRequest, TaskOutputUploadResponse, TaskResultUploadRequest,
    TaskResultUploadResponse, TaskUsageRequest, TaskUsageResponse,
    GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES, GENERAL_COMPUTE_RESULT_MAX_BYTES,
    LEGACY_MANAGED_RECEIPT_MAX_BYTES, MANAGED_GPU_RESULT_MAX_BYTES, WORKER_RPC_MESSAGE_MAX_BYTES,
    WORKER_STATUS_MESSAGE_MAX_BYTES,
};
use prost::Message;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tonic::{Request, Response, Status};

use crate::{
    runtime_admission::WorkerRuntimeAdmission, StopTaskOutcome, TaskResult, WorkerExecutor,
};
use general_compute_runtime::artifact::CasChunkStore;
use general_compute_runtime::managed_gpu::{ManagedGpuRequest, MANAGED_GPU_RUNTIME_VERSION};
use general_compute_runtime::GeneralComputeRequest;
use hivemind_config::HivemindConfig;
use hivemind_managed_consensus::digest_hex;
use hivemind_models::{Task, TaskStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferLeaseAuthorityError {
    Denied(String),
    Unavailable(String),
}

#[tonic::async_trait]
pub trait TransferLeaseAuthority: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn validate(
        &self,
        token: &str,
        worker_id: &str,
        task_id: &str,
        execution_id: &str,
        attempt_id: &str,
        transfer_generation: i64,
        idempotency_key: &str,
        request_digest: &str,
    ) -> Result<(), TransferLeaseAuthorityError>;
}

/// Nodepool-backed lease authority used by production Workers. The execution
/// token is presented to Nodepool for every transfer operation; no user JWT or
/// Worker-local revocation cache is trusted.
pub struct NodepoolTransferLeaseAuthority {
    endpoint: Arc<Mutex<String>>,
}

impl NodepoolTransferLeaseAuthority {
    #[must_use]
    pub fn new(endpoint: impl Into<String>) -> Arc<Self> {
        Self::new_shared(Arc::new(Mutex::new(endpoint.into())))
    }

    #[must_use]
    pub fn new_shared(endpoint: Arc<Mutex<String>>) -> Arc<Self> {
        Arc::new(Self { endpoint })
    }
}

#[tonic::async_trait]
impl TransferLeaseAuthority for NodepoolTransferLeaseAuthority {
    async fn validate(
        &self,
        token: &str,
        worker_id: &str,
        task_id: &str,
        execution_id: &str,
        attempt_id: &str,
        transfer_generation: i64,
        idempotency_key: &str,
        request_digest: &str,
    ) -> Result<(), TransferLeaseAuthorityError> {
        let endpoint = self
            .endpoint
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let mut client =
            NodeManagerServiceClient::connect(crate::nodepool_client::nodepool_endpoint(&endpoint))
                .await
                .map_err(|error| TransferLeaseAuthorityError::Unavailable(error.to_string()))?;
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            client.validate_general_compute_transfer_lease(
                hivemind_proto::ValidateGeneralComputeTransferLeaseRequest {
                    token: token.to_owned(),
                    worker_id: worker_id.to_owned(),
                    task_id: task_id.to_owned(),
                    execution_id: execution_id.to_owned(),
                    attempt_id: attempt_id.to_owned(),
                    transfer_generation,
                    idempotency_key: idempotency_key.to_owned(),
                    request_digest: request_digest.to_owned(),
                },
            ),
        )
        .await
        .map_err(|_| {
            TransferLeaseAuthorityError::Unavailable(
                "Nodepool transfer lease validation timed out".into(),
            )
        })?
        .map_err(|error| TransferLeaseAuthorityError::Unavailable(error.to_string()))?
        .into_inner();
        if response.success {
            Ok(())
        } else {
            Err(TransferLeaseAuthorityError::Denied(
                if response.status_message.trim().is_empty() {
                    "transfer lease is no longer active".into()
                } else {
                    response.status_message
                },
            ))
        }
    }
}

/// Shared state for the Worker gRPC surfaces.
///
/// Production callers must construct this state with an explicit Nodepool
/// lease authority. The legacy no-authority constructor is test-only.
///
/// ```compile_fail
/// # use hivemind_worker_executor::grpc_server::WorkerGrpcState;
/// # use hivemind_config::HivemindConfig;
/// # use hivemind_worker_executor::WorkerExecutor;
/// # use std::sync::Arc;
/// let config = HivemindConfig::default();
/// let _ = WorkerGrpcState::new(
///     config.clone(),
///     Arc::new(WorkerExecutor::new(config)),
///     "worker".into(),
/// );
/// ```
pub type WorkerIdentityHandle = Arc<Mutex<Option<String>>>;

pub struct WorkerGrpcState {
    pub config: HivemindConfig,
    pub executor: Arc<WorkerExecutor>,
    worker_id: WorkerIdentityHandle,
    cas_store: Option<Arc<CasChunkStore>>,
    reports: Mutex<HashMap<WorkerTaskKey, WorkerTaskReport>>,
    completed_consensus_results:
        Arc<Mutex<HashMap<CompletedConsensusResultKey, CompletedConsensusResult>>>,
    transfer_lease_authority: Arc<Mutex<Option<Arc<dyn TransferLeaseAuthority>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CompletedConsensusResultKey {
    task_id: String,
    execution_id: String,
    attempt_id: String,
    idempotency_key: String,
    request_digest: String,
    consensus_round_id: String,
    replica_id: String,
    consensus_protocol_version: u32,
    payload_digest: String,
}

struct CompletedConsensusResult {
    response: ExecuteTaskResponse,
    expires_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WorkerTaskKey {
    task_id: String,
    attempt_id: String,
}

impl WorkerTaskKey {
    fn new(task_id: &str, attempt_id: Option<&str>) -> Self {
        Self {
            task_id: task_id.to_owned(),
            attempt_id: attempt_id.unwrap_or_default().to_owned(),
        }
    }
}

#[derive(Clone)]
struct WorkerTaskReport {
    owner: String,
    worker_id: Option<String>,
    attempt_id: Option<String>,
    output: Option<String>,
    result_torrent: Option<String>,
    usage: Option<hivemind_proto::ResourceUsage>,
    general_compute_request: Option<GeneralComputeRequest>,
    managed_gpu_request: Option<ManagedGpuRequest>,
    transfer_generation: Option<i64>,
}

impl WorkerGrpcState {
    /// Construct a Worker state without an authority for in-process tests.
    #[cfg(test)]
    pub fn new(config: HivemindConfig, executor: Arc<WorkerExecutor>, worker_id: String) -> Self {
        Self::new_without_transfer_lease_authority(config, executor, worker_id)
    }

    fn new_without_transfer_lease_authority(
        config: HivemindConfig,
        executor: Arc<WorkerExecutor>,
        worker_id: String,
    ) -> Self {
        Self {
            config,
            executor,
            worker_id: Arc::new(Mutex::new(Some(worker_id))),
            cas_store: crate::executor::cas_store_from_environment(),
            reports: Mutex::new(HashMap::new()),
            completed_consensus_results: Arc::new(Mutex::new(HashMap::new())),
            transfer_lease_authority: Arc::new(Mutex::new(None)),
        }
    }

    /// Return the shared identity handle used by the gRPC and control APIs.
    #[must_use]
    pub fn worker_identity_handle(&self) -> WorkerIdentityHandle {
        self.worker_id.clone()
    }

    /// Read the currently registered Worker identity.
    #[must_use]
    pub fn current_worker_id(&self) -> Option<String> {
        self.worker_id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Update the identity after Nodepool accepts a registration.
    pub fn set_worker_id(&self, worker_id: impl Into<String>) {
        let mut identity = self
            .worker_id
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *identity = Some(worker_id.into());
    }

    pub fn new_with_transfer_lease_authority(
        config: HivemindConfig,
        executor: Arc<WorkerExecutor>,
        worker_id: String,
        authority: Arc<dyn TransferLeaseAuthority>,
    ) -> Self {
        Self::new_without_transfer_lease_authority(config, executor, worker_id)
            .with_transfer_lease_authority(authority)
    }

    pub fn with_transfer_lease_authority(self, authority: Arc<dyn TransferLeaseAuthority>) -> Self {
        if let Ok(mut slot) = self.transfer_lease_authority.lock() {
            *slot = Some(authority);
        }
        self
    }

    #[allow(clippy::too_many_arguments)]
    async fn validate_transfer_lease(
        &self,
        token: &str,
        task_id: &str,
        execution_id: &str,
        attempt_id: &str,
        transfer_generation: i64,
        idempotency_key: &str,
        request_digest: &str,
    ) -> Result<(), Status> {
        let worker_id = self
            .current_worker_id()
            .ok_or_else(|| Status::failed_precondition("worker identity is unavailable"))?;
        let authority = self
            .transfer_lease_authority
            .lock()
            .map_err(|_| Status::internal("transfer lease authority store poisoned"))?
            .clone();
        let Some(authority) = authority else {
            // Unit/in-process callers that do not configure a control-plane
            // client retain the local admission behavior. The production
            // binary installs the Nodepool-backed authority before serving.
            return Ok(());
        };
        authority
            .validate(
                token,
                &worker_id,
                task_id,
                execution_id,
                attempt_id,
                transfer_generation,
                idempotency_key,
                request_digest,
            )
            .await
            .map_err(|error| match error {
                TransferLeaseAuthorityError::Denied(message) => Status::permission_denied(message),
                TransferLeaseAuthorityError::Unavailable(message) => Status::unavailable(message),
            })
    }

    fn completed_consensus_result_key(request: &ExecuteTaskRequest) -> CompletedConsensusResultKey {
        let mut payload = request.clone();
        payload.token.clear();
        let payload_digest = digest_hex(&payload.encode_to_vec());
        CompletedConsensusResultKey {
            task_id: request.task_id.clone(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            consensus_round_id: request.consensus_round_id.clone(),
            replica_id: request.replica_id.clone(),
            consensus_protocol_version: request.consensus_protocol_version,
            payload_digest,
        }
    }

    fn cached_consensus_result(
        &self,
        request: &ExecuteTaskRequest,
    ) -> Result<Option<ExecuteTaskResponse>, Status> {
        let mut cache = self
            .completed_consensus_results
            .lock()
            .map_err(|_| Status::internal("completed result cache is unavailable"))?;
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        let key = Self::completed_consensus_result_key(request);
        Ok(cache.get(&key).map(|entry| entry.response.clone()))
    }

    fn cache_consensus_result(
        &self,
        request: &ExecuteTaskRequest,
        response: &ExecuteTaskResponse,
    ) -> Result<(), Status> {
        if response.encoded_len() > hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES * 2 {
            return Err(Status::resource_exhausted(
                "managed consensus cached result exceeds the Worker cache limit",
            ));
        }
        let mut cache = self
            .completed_consensus_results
            .lock()
            .map_err(|_| Status::internal("completed result cache is unavailable"))?;
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        let key = Self::completed_consensus_result_key(request);
        if let Some(existing) = cache.get(&key) {
            if existing.response != *response {
                return Err(Status::failed_precondition(
                    "conflicting managed consensus result is already cached",
                ));
            }
            return Ok(());
        }
        let mut total_bytes: usize = cache
            .values()
            .map(|entry| entry.response.encoded_len())
            .sum();
        while (!cache.is_empty() && cache.len() >= COMPLETED_CONSENSUS_RESULT_CACHE_MAX_ENTRIES)
            || (!cache.is_empty()
                && total_bytes.saturating_add(response.encoded_len())
                    > COMPLETED_CONSENSUS_RESULT_CACHE_MAX_BYTES)
        {
            let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = cache.remove(&oldest_key) {
                total_bytes = total_bytes.saturating_sub(entry.response.encoded_len());
            }
        }
        cache.insert(
            key,
            CompletedConsensusResult {
                response: response.clone(),
                expires_at: now + COMPLETED_CONSENSUS_RESULT_CACHE_TTL,
            },
        );
        Ok(())
    }
}

const MAX_TASK_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_RESULT_REFERENCE_BYTES: usize = 4096;
const COMPLETED_CONSENSUS_RESULT_CACHE_MAX_ENTRIES: usize = 256;
const COMPLETED_CONSENSUS_RESULT_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;
const COMPLETED_CONSENSUS_RESULT_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct GrpcWorkerNodeService {
    state: Arc<WorkerGrpcState>,
    runtime_admission: WorkerRuntimeAdmission,
}

/// Dedicated authenticated CAS/chunk service. This is intentionally a
/// separate gRPC service from `WorkerNodeService::ExecuteTask`, whose message
/// cap is too small for a bounded 16 MiB chunk.
pub struct GrpcGeneralComputeChunkService {
    state: Arc<WorkerGrpcState>,
    runtime_admission: WorkerRuntimeAdmission,
}

impl Clone for WorkerGrpcState {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            executor: self.executor.clone(),
            worker_id: self.worker_id.clone(),
            cas_store: self.cas_store.clone(),
            reports: Mutex::new(
                self.reports
                    .lock()
                    .map(|reports| reports.clone())
                    .unwrap_or_default(),
            ),
            completed_consensus_results: self.completed_consensus_results.clone(),
            transfer_lease_authority: self.transfer_lease_authority.clone(),
        }
    }
}

impl GrpcGeneralComputeChunkService {
    pub fn new(state: Arc<WorkerGrpcState>, runtime_admission: WorkerRuntimeAdmission) -> Self {
        Self {
            state,
            runtime_admission,
        }
    }

    #[allow(clippy::result_large_err)]
    fn verifier(&self) -> Result<WorkerExecutionVerifier, Status> {
        WorkerExecutionVerifier::from_pem(&self.state.config.auth.worker_execution_public_key_pem)
            .map_err(|_| Status::internal("Worker execution public key is invalid"))
    }

    #[allow(clippy::result_large_err)]
    fn assignment(
        &self,
        token: &str,
        execution_id: &str,
        attempt_id: &str,
        idempotency_key: &str,
        request_digest: &str,
        transfer_generation: i64,
    ) -> Result<
        (
            crate::chunk_transport::VerifiedWorkerExecution,
            GeneralComputeRequest,
        ),
        Status,
    > {
        let verifier = self.verifier()?;
        let claims = verifier
            .decode(token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if claims.role.as_deref() != Some("worker-execution") {
            return Err(Status::permission_denied("Worker execution token required"));
        }
        let task_id = claims
            .task_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| Status::permission_denied("Token is not bound to a task"))?;
        let worker_id = claims
            .worker_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| Status::permission_denied("Token is not bound to a worker"))?;
        if self.state.current_worker_id().as_deref() != Some(worker_id) {
            return Err(Status::permission_denied(
                "Token is not bound to this worker",
            ));
        }
        let execution_claims = verifier
            .decode_execution_claims(token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        let reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Status::internal("task report store poisoned"))?;
        let key = WorkerTaskKey::new(task_id, execution_claims.attempt_id.as_deref());
        let report = reports.get(&key).ok_or_else(|| {
            Status::permission_denied("Token is not authorized for task assignment")
        })?;
        if report.owner != claims.sub || report.worker_id.as_deref() != Some(worker_id) {
            return Err(Status::permission_denied(
                "Token is not authorized for task assignment",
            ));
        }
        let request = report.general_compute_request.clone().ok_or_else(|| {
            Status::failed_precondition("general-compute request is not assigned")
        })?;
        if report.transfer_generation != Some(transfer_generation) {
            return Err(Status::permission_denied(
                "transfer lease generation does not match the admitted attempt",
            ));
        }
        if request.execution_id != execution_id
            || request.attempt_id != attempt_id
            || request.idempotency_key != idempotency_key
            || request.request_digest != request_digest
        {
            return Err(Status::permission_denied(
                "Chunk identity is not bound to the assigned attempt",
            ));
        }
        let verified = crate::chunk_transport::VerifiedWorkerExecution::from_token(
            &verifier, token, task_id, worker_id,
        )
        .map_err(chunk_auth_status)?;
        verified
            .require_identity(&verifier, &request)
            .map_err(chunk_auth_status)?;
        Ok((verified, request))
    }

    async fn prepare_request(
        &self,
        request: &GeneralComputePrepareRequest,
    ) -> Result<GeneralComputeRequest, Status> {
        if !crate::sandbox::is_safe_task_id(&request.task_id) {
            return Err(Status::invalid_argument("unsafe task id"));
        }
        let verifier = self.verifier()?;
        let claims = verifier
            .decode(&request.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if claims.role.as_deref() != Some("worker-execution") {
            return Err(Status::permission_denied("Worker execution token required"));
        }
        if claims.task_id.as_deref() != Some(request.task_id.as_str())
            || claims.worker_id.as_deref() != self.state.current_worker_id().as_deref()
        {
            return Err(Status::permission_denied(
                "Token is not authorized for this worker assignment",
            ));
        }
        let token_identity = WorkerExecutionVerifier::from_pem(
            &self.state.config.auth.worker_execution_public_key_pem,
        )
        .map_err(|_| Status::internal("Worker execution public key is invalid"))?
        .decode_execution_claims(&request.token)
        .map_err(|_| Status::unauthenticated("Invalid worker execution token"))?;
        let admitted = self
            .runtime_admission
            .admit(&request.runtime, &request.general_compute_manifest_json)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        validate_execute_task_contract(&ExecuteTaskRequest {
            runtime: request.runtime.clone(),
            general_compute_manifest_json: request.general_compute_manifest_json.clone(),
            ..ExecuteTaskRequest::default()
        })
        .map_err(Status::invalid_argument)?;
        let admitted_request = match admitted {
            crate::runtime_admission::RuntimeRoute::GeneralComputeV1Alpha1(request) => request,
            _ => {
                return Err(Status::invalid_argument(
                    "PrepareGeneralCompute requires general-compute-v1alpha1",
                ));
            }
        };
        if admitted_request.execution_id != request.execution_id
            || admitted_request.attempt_id != request.attempt_id
            || admitted_request.idempotency_key != request.idempotency_key
            || admitted_request.request_digest != request.request_digest
        {
            return Err(Status::permission_denied(
                "prepare identity does not match the request manifest",
            ));
        }
        if token_identity.execution_id.as_deref() != Some(request.execution_id.as_str())
            || token_identity.attempt_id.as_deref() != Some(request.attempt_id.as_str())
            || token_identity.idempotency_key.as_deref() != Some(request.idempotency_key.as_str())
            || token_identity.request_digest.as_deref() != Some(request.request_digest.as_str())
        {
            return Err(Status::permission_denied(
                "worker execution token is not bound to the prepared attempt",
            ));
        }
        if token_identity.transfer_generation != Some(request.transfer_generation)
            || request.transfer_generation <= 0
        {
            return Err(Status::permission_denied(
                "worker execution token is not bound to the transfer lease generation",
            ));
        }
        self.state
            .validate_transfer_lease(
                &request.token,
                &request.task_id,
                &request.execution_id,
                &request.attempt_id,
                request.transfer_generation,
                &request.idempotency_key,
                &request.request_digest,
            )
            .await?;
        let key = WorkerTaskKey::new(&request.task_id, Some(&admitted_request.attempt_id));
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Status::internal("task report store poisoned"))?;
        if reports.iter().any(|(existing_key, report)| {
            existing_key.task_id == request.task_id && report.owner != claims.sub
        }) {
            return Err(*task_assignment_denied());
        }
        if reports.keys().any(|existing_key| {
            existing_key.task_id == request.task_id && existing_key.attempt_id.is_empty()
        }) {
            return Err(Status::permission_denied(
                "typed execution cannot replace a legacy task attempt",
            ));
        }
        if let Some(report) = reports.get_mut(&key) {
            if report.owner != claims.sub {
                return Err(*task_assignment_denied());
            }
            if report.managed_gpu_request.is_some() {
                return Err(Status::permission_denied(
                    "general-compute preparation cannot replace a managed GPU attempt",
                ));
            }
            if report.general_compute_request.as_ref() != Some(&admitted_request) {
                if report
                    .transfer_generation
                    .is_some_and(|generation| request.transfer_generation <= generation)
                {
                    return Err(Status::permission_denied(
                        "stale transfer lease generation cannot replace the admitted attempt",
                    ));
                }
                // A task retry rotates its attempt identity while retaining
                // the same task id. Re-preparation is the explicit lifecycle
                // boundary that replaces the previous pending assignment.
                report.attempt_id = Some(admitted_request.attempt_id.clone());
                report.output = None;
                report.result_torrent = None;
                report.usage = None;
                report.general_compute_request = Some(admitted_request.clone());
                report.transfer_generation = Some(request.transfer_generation);
            } else if report.transfer_generation != Some(request.transfer_generation) {
                return Err(Status::permission_denied(
                    "transfer lease generation does not match the admitted attempt",
                ));
            }
        } else {
            reports.insert(
                key,
                WorkerTaskReport {
                    owner: claims.sub,
                    worker_id: self.state.current_worker_id(),
                    attempt_id: Some(admitted_request.attempt_id.clone()),
                    output: None,
                    result_torrent: None,
                    usage: None,
                    general_compute_request: Some(admitted_request.clone()),
                    managed_gpu_request: None,
                    transfer_generation: Some(request.transfer_generation),
                },
            );
        }
        Ok(admitted_request)
    }

    async fn validate_transfer_lease_for_token(
        &self,
        token: &str,
        execution_id: &str,
        attempt_id: &str,
        transfer_generation: i64,
        idempotency_key: &str,
        request_digest: &str,
    ) -> Result<(), Status> {
        let verifier = self.verifier()?;
        let task_id = verifier
            .decode(token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?
            .task_id
            .ok_or_else(|| Status::permission_denied("Token is not bound to a task"))?;
        self.state
            .validate_transfer_lease(
                token,
                &task_id,
                execution_id,
                attempt_id,
                transfer_generation,
                idempotency_key,
                request_digest,
            )
            .await
    }
}

impl GrpcWorkerNodeService {
    pub fn new(state: Arc<WorkerGrpcState>) -> Self {
        Self {
            state,
            runtime_admission: WorkerRuntimeAdmission::default(),
        }
    }

    #[must_use]
    pub fn with_runtime_admission(mut self, runtime_admission: WorkerRuntimeAdmission) -> Self {
        self.runtime_admission = runtime_admission;
        self
    }

    fn validate_rpc_token(&self, token: &str) -> Result<Claims, Box<Status>> {
        WorkerExecutionVerifier::from_pem(&self.state.config.auth.worker_execution_public_key_pem)
            .map_err(|_| Box::new(Status::internal("Worker execution public key is invalid")))?
            .decode(token)
            .map_err(|_| Box::new(Status::unauthenticated("Invalid token")))
    }

    fn validate_worker_execution_token(&self, token: &str) -> Result<Claims, Box<Status>> {
        let claims = self.validate_rpc_token(token)?;
        if claims.role.as_deref() != Some("worker-execution") {
            return Err(Box::new(Status::permission_denied(
                "Worker execution token required",
            )));
        }
        Ok(claims)
    }

    #[allow(dead_code)]
    fn record_task_assignment(&self, task_id: &str, owner: &str) -> Result<(), Box<Status>> {
        self.record_task_assignment_for_attempt(task_id, owner, None)
    }

    fn record_task_assignment_for_attempt(
        &self,
        task_id: &str,
        owner: &str,
        attempt_id: Option<&str>,
    ) -> Result<(), Box<Status>> {
        let key = WorkerTaskKey::new(task_id, attempt_id);
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        if let Some(report) = reports.get(&key) {
            if report.owner != owner {
                return Err(task_assignment_denied());
            }
            return Ok(());
        }

        if reports
            .iter()
            .any(|(existing_key, report)| existing_key.task_id == task_id && report.owner != owner)
        {
            return Err(task_assignment_denied());
        }
        let incoming_is_typed = !key.attempt_id.is_empty();
        if reports.keys().any(|existing_key| {
            existing_key.task_id == task_id
                && existing_key.attempt_id.is_empty() == incoming_is_typed
        }) {
            return Err(Box::new(Status::permission_denied(if incoming_is_typed {
                "typed execution cannot replace a legacy task attempt"
            } else {
                "legacy execution cannot replace a typed task attempt"
            })));
        }

        reports.insert(
            key,
            WorkerTaskReport {
                owner: owner.to_string(),
                worker_id: self.state.current_worker_id(),
                attempt_id: attempt_id.map(str::to_owned),
                output: None,
                result_torrent: None,
                usage: None,
                general_compute_request: None,
                managed_gpu_request: None,
                transfer_generation: None,
            },
        );
        Ok(())
    }

    fn validate_task_assignment(
        &self,
        token: &str,
        task_id: &str,
        worker_id: Option<&str>,
    ) -> Result<WorkerTaskKey, Box<Status>> {
        let claims = self.validate_worker_execution_token(token)?;
        let execution_claims = WorkerExecutionVerifier::from_pem(
            &self.state.config.auth.worker_execution_public_key_pem,
        )
        .map_err(|_| Box::new(Status::internal("Worker execution public key is invalid")))?
        .decode_execution_claims(token)
        .map_err(|_| Box::new(Status::unauthenticated("Invalid token")))?;
        if !crate::sandbox::is_safe_task_id(task_id) {
            return Err(Box::new(Status::invalid_argument("unsafe task id")));
        }
        let key = WorkerTaskKey::new(task_id, execution_claims.attempt_id.as_deref());
        let reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        let report = reports.get(&key).ok_or_else(task_assignment_denied)?;
        if report.owner != claims.sub {
            return Err(task_assignment_denied());
        }
        if claims.task_id.as_deref() != Some(task_id) {
            return Err(task_assignment_denied());
        }
        if report
            .attempt_id
            .as_deref()
            .is_some_and(|attempt_id| execution_claims.attempt_id.as_deref() != Some(attempt_id))
        {
            return Err(task_assignment_denied());
        }
        if report.worker_id.as_deref() != claims.worker_id.as_deref() {
            return Err(task_assignment_denied());
        }
        if let Some(worker_id) = worker_id {
            if report.worker_id.as_deref() != Some(worker_id) {
                return Err(task_assignment_denied());
            }
        }
        Ok(key)
    }

    fn validate_task_attempt_assignment(
        &self,
        token: &str,
        task_id: &str,
        attempt_id: &str,
    ) -> Result<WorkerTaskKey, Box<Status>> {
        let key = self.validate_task_assignment(token, task_id, None)?;
        if key.attempt_id != attempt_id {
            return Err(task_assignment_denied());
        }
        Ok(key)
    }

    fn validate_task_attempt_assignment_with_idempotency(
        &self,
        token: &str,
        task_id: &str,
        attempt_id: &str,
        idempotency_key: &str,
    ) -> Result<WorkerTaskKey, Box<Status>> {
        let key = self.validate_task_attempt_assignment(token, task_id, attempt_id)?;
        if idempotency_key.trim().is_empty() {
            return Err(task_assignment_denied());
        }
        let execution_claims = WorkerExecutionVerifier::from_pem(
            &self.state.config.auth.worker_execution_public_key_pem,
        )
        .map_err(|_| Box::new(Status::internal("Worker execution public key is invalid")))?
        .decode_execution_claims(token)
        .map_err(|_| Box::new(Status::unauthenticated("Invalid token")))?;
        if execution_claims.idempotency_key.as_deref() != Some(idempotency_key) {
            return Err(task_assignment_denied());
        }
        Ok(key)
    }

    fn report_for_update_for_key<F>(
        &self,
        key: &WorkerTaskKey,
        update: F,
    ) -> Result<(), Box<Status>>
    where
        F: FnOnce(&mut WorkerTaskReport),
    {
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        let report = reports.get_mut(key).ok_or_else(task_assignment_denied)?;
        update(report);
        Ok(())
    }

    #[cfg(test)]
    fn report_for_update<F>(&self, task_id: &str, update: F) -> Result<(), Box<Status>>
    where
        F: FnOnce(&mut WorkerTaskReport),
    {
        self.report_for_update_for_key(&WorkerTaskKey::new(task_id, None), update)
    }

    fn report_for_task_for_key(
        &self,
        key: &WorkerTaskKey,
    ) -> Result<Option<WorkerTaskReport>, Box<Status>> {
        self.state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))
            .map(|reports| reports.get(key).cloned())
    }

    #[cfg(test)]
    fn report_for_task(&self, task_id: &str) -> Result<Option<WorkerTaskReport>, Box<Status>> {
        self.report_for_task_for_key(&WorkerTaskKey::new(task_id, None))
    }

    fn record_managed_gpu_request(
        &self,
        task_id: &str,
        request: ManagedGpuRequest,
    ) -> Result<(), Box<Status>> {
        let key = WorkerTaskKey::new(task_id, Some(&request.attempt_id));
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        let report = reports.get_mut(&key).ok_or_else(task_assignment_denied)?;
        if report.transfer_generation.is_some() {
            return Err(Box::new(Status::permission_denied(
                "managed GPU execution cannot use a general-compute transfer lease",
            )));
        }
        if let Some(existing) = report.managed_gpu_request.as_ref() {
            if existing != &request {
                return Err(Box::new(Status::permission_denied(
                    "ExecuteTask request does not match the admitted managed GPU attempt",
                )));
            }
        }
        if report.general_compute_request.is_some() {
            return Err(Box::new(Status::permission_denied(
                "managed GPU execution cannot replace a general-compute attempt",
            )));
        }
        report.managed_gpu_request = Some(request);
        Ok(())
    }
    fn record_general_compute_request(
        &self,
        task_id: &str,
        request: GeneralComputeRequest,
        transfer_generation: i64,
    ) -> Result<(), Box<Status>> {
        let key = WorkerTaskKey::new(task_id, Some(&request.attempt_id));
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        let report = reports.get_mut(&key).ok_or_else(task_assignment_denied)?;
        if report
            .transfer_generation
            .is_some_and(|generation| generation != transfer_generation)
        {
            return Err(Box::new(Status::permission_denied(
                "stale transfer lease generation cannot replace the admitted attempt",
            )));
        }
        if report.managed_gpu_request.is_some() {
            return Err(Box::new(Status::permission_denied(
                "general-compute execution cannot replace a managed GPU attempt",
            )));
        }
        report.general_compute_request = Some(request);
        report.transfer_generation = Some(transfer_generation);
        Ok(())
    }
}

fn task_assignment_denied() -> Box<Status> {
    Box::new(Status::permission_denied(
        "Token is not authorized for task assignment",
    ))
}

fn validate_managed_consensus_token_identity(
    claims: &WorkerExecutionClaims,
    request: &ExecuteTaskRequest,
) -> Result<(), Status> {
    let matches = claims.execution_id.as_deref() == Some(request.execution_id.as_str())
        && claims.attempt_id.as_deref() == Some(request.attempt_id.as_str())
        && claims.idempotency_key.as_deref() == Some(request.idempotency_key.as_str())
        && claims.request_digest.as_deref() == Some(request.request_digest.as_str())
        && claims.consensus_round_id.as_deref() == Some(request.consensus_round_id.as_str())
        && claims.replica_id.as_deref() == Some(request.replica_id.as_str())
        && claims.consensus_protocol_version == Some(request.consensus_protocol_version as u16);
    if !matches {
        return Err(Status::permission_denied(
            "worker execution token is not bound to the complete managed consensus identity",
        ));
    }
    Ok(())
}

fn validate_execute_task_contract(request: &ExecuteTaskRequest) -> Result<(), &'static str> {
    match request.runtime.trim() {
        "" => Ok(()),
        "managed-function-v0" => {
            if request.task_source.trim().is_empty() {
                return Err("managed-function-v0 requires non-empty task_source");
            }
            if request.task_source.len() > hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES {
                return Err("managed-function-v0 task_source exceeds the byte limit");
            }
            if request.torrent.trim().is_empty() {
                return Err("managed-function-v0 requires non-empty JSON input");
            }
            if request.torrent.len() > hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES {
                return Err("managed-function-v0 JSON input exceeds the byte limit");
            }
            if request.managed_budget_units <= 0 {
                return Err("managed-function-v0 budget must be positive");
            }
            if request.managed_budget_units > hivemind_proto::MANAGED_BUDGET_MAX_USAGE_UNITS {
                return Err("managed-function-v0 budget exceeds the usage-unit limit");
            }
            Ok(())
        }
        "production_sandboxed_dsl" => {
            if request.task_source.trim().is_empty() {
                return Err("production_sandboxed_dsl requires non-empty task_source");
            }
            if request.task_source.len() > hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES {
                return Err("production_sandboxed_dsl task_source exceeds the byte limit");
            }
            if request.torrent.trim().is_empty() {
                return Err("production_sandboxed_dsl requires non-empty JSON input");
            }
            if request.torrent.len() > hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES {
                return Err("production_sandboxed_dsl JSON input exceeds the byte limit");
            }
            if request.managed_budget_units <= 0 {
                return Err("production_sandboxed_dsl budget must be positive");
            }
            if request.managed_budget_units > hivemind_proto::MANAGED_BUDGET_MAX_USAGE_UNITS {
                return Err("production_sandboxed_dsl budget exceeds the usage-unit limit");
            }
            if !request.general_compute_manifest_json.is_empty() {
                return Err("production_sandboxed_dsl must not carry a general-compute manifest");
            }
            if request.managed_dsl_backend_id.trim().is_empty() {
                return Err("production_sandboxed_dsl requires managed_dsl_backend_id");
            }
            if request.managed_dsl_semantics_manifest_sha256
                != general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256
            {
                return Err("production_sandboxed_dsl requires the canonical semantics digest");
            }
            Ok(())
        }
        general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION => {
            if !request.managed_dsl_backend_id.is_empty()
                || !request.managed_dsl_semantics_manifest_sha256.is_empty()
            {
                return Err("managed DSL identity requires production_sandboxed_dsl");
            }
            if request.general_compute_manifest_json.is_empty() {
                return Err("general-compute-v1alpha1 requires a non-empty request manifest");
            }
            if request.general_compute_manifest_json.len()
                > hivemind_proto::GENERAL_COMPUTE_MANIFEST_MAX_BYTES
            {
                return Err("general-compute-v1alpha1 request manifest exceeds the byte limit");
            }
            Ok(())
        }
        MANAGED_GPU_RUNTIME_VERSION => {
            if !request.general_compute_manifest_json.is_empty() {
                return Err("managed-function-gpu-v1 must not carry a general-compute manifest");
            }
            if !request.managed_dsl_backend_id.is_empty()
                || !request.managed_dsl_semantics_manifest_sha256.is_empty()
            {
                return Err("managed-function-gpu-v1 must not carry managed DSL identity");
            }
            if !request.task_source.trim().is_empty() || !request.torrent.trim().is_empty() {
                return Err(
                    "managed-function-gpu-v1 source and input belong to the request manifest",
                );
            }
            if request.managed_budget_units != 0 {
                return Err("managed-function-gpu-v1 must not carry a managed budget field");
            }
            if request.managed_gpu_manifest_json.is_empty() {
                return Err("managed-function-gpu-v1 requires a non-empty request manifest");
            }
            if request.managed_gpu_manifest_json.len()
                > hivemind_proto::MANAGED_GPU_MANIFEST_MAX_BYTES
            {
                return Err("managed-function-gpu-v1 request manifest exceeds the byte limit");
            }
            let manifest =
                serde_json::from_slice::<ManagedGpuRequest>(&request.managed_gpu_manifest_json)
                    .map_err(|_| "managed-function-gpu-v1 request manifest is malformed")?;
            manifest
                .validate()
                .map_err(|_| "managed-function-gpu-v1 request manifest is invalid")?;
            for (actual, expected) in [
                (&request.execution_id, &manifest.execution_id),
                (&request.attempt_id, &manifest.attempt_id),
                (&request.idempotency_key, &manifest.idempotency_key),
                (&request.request_digest, &manifest.request_digest),
            ] {
                if !actual.is_empty() && actual != expected {
                    return Err(
                        "managed-function-gpu-v1 top-level identity does not match manifest",
                    );
                }
            }
            Ok(())
        }
        _ => Err("unsupported task runtime"),
    }
}

#[tonic::async_trait]
impl WorkerNodeService for GrpcWorkerNodeService {
    async fn execute_task(
        &self,
        request: Request<ExecuteTaskRequest>,
    ) -> Result<Response<ExecuteTaskResponse>, Status> {
        let req = request.into_inner();
        let consensus_mode = req.consensus_protocol_version > 0;
        validate_managed_consensus_request(&req).map_err(Status::invalid_argument)?;
        let consensus_request = req.clone();
        let mut request_identity = ExecuteTaskIdentity::from_request(&req);
        let claims = self
            .validate_worker_execution_token(&req.token)
            .map_err(|status| *status)?;
        if !crate::sandbox::is_safe_task_id(&req.task_id) {
            return Err(Status::invalid_argument("unsafe task id"));
        }
        if claims.task_id.as_deref() != Some(req.task_id.as_str())
            || claims.worker_id.as_deref() != self.state.current_worker_id().as_deref()
        {
            return Err(*task_assignment_denied());
        }
        if consensus_mode {
            let verifier = WorkerExecutionVerifier::from_pem(
                &self.state.config.auth.worker_execution_public_key_pem,
            )
            .map_err(|_| Status::internal("Worker execution public key is invalid"))?;
            let execution_claims = verifier
                .decode_execution_claims(&req.token)
                .map_err(|_| Status::unauthenticated("Invalid worker execution token"))?;
            validate_managed_consensus_token_identity(&execution_claims, &req)?;
        }
        let admitted = self
            .runtime_admission
            .admit_with_manifests(
                &req.runtime,
                &req.general_compute_manifest_json,
                &req.managed_gpu_manifest_json,
            )
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        validate_execute_task_contract(&req).map_err(Status::invalid_argument)?;
        if !consensus_mode
            && matches!(
                &admitted,
                crate::runtime_admission::RuntimeRoute::ManagedFunctionV0
                    | crate::runtime_admission::RuntimeRoute::ProductionSandboxedDsl
            )
        {
            return Err(Status::failed_precondition(
                "managed execution requires a consensus request",
            ));
        }
        if consensus_mode {
            if let Some(cached) = self.state.cached_consensus_result(&req)? {
                validate_managed_consensus_response(&cached).map_err(|error| {
                    Status::internal(format!("cached consensus result is invalid: {error}"))
                })?;
                validate_managed_consensus_result_config_limit(&self.state.config, &cached)?;
                validate_managed_consensus_response_identity(&cached, &req)?;
                return Ok(Response::new(cached));
            }
        }
        let managed_gpu_request = match &admitted {
            crate::runtime_admission::RuntimeRoute::ManagedFunctionGpuV1(request) => {
                Some(request.clone())
            }
            _ => None,
        };
        let is_managed_gpu = managed_gpu_request.is_some();
        if let crate::runtime_admission::RuntimeRoute::GeneralComputeV1Alpha1(request) = &admitted {
            let token_identity = WorkerExecutionVerifier::from_pem(
                &self.state.config.auth.worker_execution_public_key_pem,
            )
            .map_err(|_| Status::internal("Worker execution public key is invalid"))?
            .decode_execution_claims(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid worker execution token"))?;
            if token_identity.execution_id.as_deref() != Some(request.execution_id.as_str())
                || token_identity.attempt_id.as_deref() != Some(request.attempt_id.as_str())
                || token_identity.idempotency_key.as_deref()
                    != Some(request.idempotency_key.as_str())
                || token_identity.request_digest.as_deref() != Some(request.request_digest.as_str())
            {
                return Err(Status::permission_denied(
                    "worker execution token is not bound to the general-compute attempt",
                ));
            }
            let reports = self
                .state
                .reports
                .lock()
                .map_err(|_| Status::internal("task report store poisoned"))?;
            if let Some(report) =
                reports.get(&WorkerTaskKey::new(&req.task_id, Some(&request.attempt_id)))
            {
                if let Some(existing) = report.general_compute_request.as_ref() {
                    if report.attempt_id.as_deref() == Some(request.attempt_id.as_str())
                        && existing != request
                    {
                        return Err(Status::permission_denied(
                            "ExecuteTask request does not match the prepared general-compute attempt",
                        ));
                    }
                }
            }
        }
        if let crate::runtime_admission::RuntimeRoute::ManagedFunctionGpuV1(request) = &admitted {
            let token_identity = WorkerExecutionVerifier::from_pem(
                &self.state.config.auth.worker_execution_public_key_pem,
            )
            .map_err(|_| Status::internal("Worker execution public key is invalid"))?
            .decode_execution_claims(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid worker execution token"))?;
            if token_identity.execution_id.as_deref() != Some(request.execution_id.as_str())
                || token_identity.attempt_id.as_deref() != Some(request.attempt_id.as_str())
                || token_identity.idempotency_key.as_deref()
                    != Some(request.idempotency_key.as_str())
                || token_identity.request_digest.as_deref() != Some(request.request_digest.as_str())
            {
                return Err(Status::permission_denied(
                    "worker execution token is not bound to the managed GPU attempt",
                ));
            }
            request_identity.execution_id = request.execution_id.clone();
            request_identity.attempt_id = request.attempt_id.clone();
            request_identity.idempotency_key = request.idempotency_key.clone();
            request_identity.request_digest = request.request_digest.clone();
            let reports = self
                .state
                .reports
                .lock()
                .map_err(|_| Status::internal("task report store poisoned"))?;
            if let Some(report) =
                reports.get(&WorkerTaskKey::new(&req.task_id, Some(&request.attempt_id)))
            {
                if let Some(existing) = report.managed_gpu_request.as_ref() {
                    if report.attempt_id.as_deref() == Some(request.attempt_id.as_str())
                        && existing != request
                    {
                        return Err(Status::permission_denied(
                            "ExecuteTask request does not match the admitted managed GPU attempt",
                        ));
                    }
                }
            }
            if reports
                .get(&WorkerTaskKey::new(&req.task_id, Some(&request.attempt_id)))
                .is_some_and(|report| report.general_compute_request.is_some())
            {
                return Err(Status::permission_denied(
                    "managed GPU execution cannot replace a general-compute attempt",
                ));
            }
        }
        if let crate::runtime_admission::RuntimeRoute::GeneralComputeV1Alpha1(request) = admitted {
            request_identity.execution_id = request.execution_id.clone();
            request_identity.attempt_id = request.attempt_id.clone();
            request_identity.idempotency_key = request.idempotency_key.clone();
            request_identity.request_digest = request.request_digest.clone();
            let transfer_generation = WorkerExecutionVerifier::from_pem(
                &self.state.config.auth.worker_execution_public_key_pem,
            )
            .map_err(|_| Status::internal("Worker execution public key is invalid"))?
            .decode_execution_claims(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid worker execution token"))?
            .transfer_generation
            .ok_or_else(|| {
                Status::permission_denied("worker transfer lease generation is missing")
            })?;
            self.state
                .validate_transfer_lease(
                    &req.token,
                    &req.task_id,
                    &request.execution_id,
                    &request.attempt_id,
                    transfer_generation,
                    &request.idempotency_key,
                    &request.request_digest,
                )
                .await?;
            self.record_task_assignment_for_attempt(
                &req.task_id,
                &claims.sub,
                Some(&request_identity.attempt_id),
            )
            .map_err(|status| *status)?;
            self.record_general_compute_request(&req.task_id, request, transfer_generation)
                .map_err(|status| *status)?;
        } else {
            let attempt_id =
                (is_managed_gpu || consensus_mode).then_some(request_identity.attempt_id.as_str());
            self.record_task_assignment_for_attempt(&req.task_id, &claims.sub, attempt_id)
                .map_err(|status| *status)?;
            if let Some(request) = managed_gpu_request.as_ref() {
                self.record_managed_gpu_request(&req.task_id, request.clone())
                    .map_err(|status| *status)?;
            }
        }
        let limits = req.resource_limits.unwrap_or_default();
        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: req.task_id.clone(),
            owner: claims.sub,
            worker_id: self.state.current_worker_id(),
            worker_ip: None,
            status: TaskStatus::Running,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: if is_managed_gpu
                || req.runtime.trim() == general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION
            {
                None
            } else {
                Some(req.torrent)
            },
            runtime: if req.runtime.trim().is_empty() {
                None
            } else {
                Some(req.runtime)
            },
            task_source: if req.task_source.trim().is_empty() {
                None
            } else {
                Some(req.task_source)
            },
            general_compute_manifest_json: if req.general_compute_manifest_json.is_empty() {
                None
            } else {
                Some(req.general_compute_manifest_json)
            },
            managed_gpu_manifest_json: if is_managed_gpu {
                Some(req.managed_gpu_manifest_json)
            } else {
                None
            },
            managed_dsl_backend_id: if req.managed_dsl_backend_id.trim().is_empty() {
                None
            } else {
                Some(req.managed_dsl_backend_id)
            },
            managed_dsl_semantics_manifest_sha256: if req
                .managed_dsl_semantics_manifest_sha256
                .trim()
                .is_empty()
            {
                None
            } else {
                Some(req.managed_dsl_semantics_manifest_sha256)
            },
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: limits.cpu_score,
            req_gpu_score: limits.gpu_score,
            req_memory_gb: (limits.memory_mb / 1024) as i32,
            req_gpu_memory_gb: (limits.vram_mb / 1024) as i32,
            req_storage_gb: limits.storage_total_gb,
            host_count: 1,
            max_cpt: managed_gpu_request
                .as_ref()
                .map_or(req.managed_budget_units, |request| {
                    request.reservation_cpt as i64
                }),
            billing_settled: false,
            billed_amount: 0,
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
            retry_count: 0,
            max_retries: 3,
            deadline: None,
            deterministic: false,
            side_effects: false,
            priority: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            download_bytes: 0,
            cache_hits: 0,
            created_at: chrono::Utc::now(),
            last_update: chrono::Utc::now(),
            completed_at: None,
        };
        tracing::info!("Worker executing task {}", req.task_id);
        match if consensus_mode {
            self.state
                .executor
                .execute_task_with_consensus(&task, &request_identity.attempt_id)
                .await
        } else {
            self.state
                .executor
                .execute_task_with_attempt(&task, &request_identity.attempt_id)
                .await
        } {
            Ok(result) => {
                let mut response = execute_response_from_result_for_runtime(
                    result,
                    is_managed_gpu,
                    &request_identity,
                )?;
                if consensus_mode {
                    attach_managed_consensus_result(&mut response, &consensus_request, &task)?;
                    validate_managed_consensus_response(&response).map_err(|error| {
                        Status::internal(format!("generated consensus result is invalid: {error}"))
                    })?;
                    validate_managed_consensus_result_config_limit(&self.state.config, &response)?;
                    validate_managed_consensus_response_identity(&response, &consensus_request)?;
                    self.state
                        .cache_consensus_result(&consensus_request, &response)?;
                }
                Ok(Response::new(response))
            }
            Err(_error) => {
                if is_managed_gpu {
                    Err(Status::internal(
                        "managed GPU execution ended without a typed result",
                    ))
                } else {
                    let mut response =
                        failed_execute_response("Task execution failed", &request_identity);
                    if consensus_mode {
                        attach_managed_consensus_result(&mut response, &consensus_request, &task)?;
                        validate_managed_consensus_response(&response).map_err(|error| {
                            Status::internal(format!(
                                "generated consensus failure result is invalid: {error}"
                            ))
                        })?;
                        validate_managed_consensus_result_config_limit(
                            &self.state.config,
                            &response,
                        )?;
                        validate_managed_consensus_response_identity(
                            &response,
                            &consensus_request,
                        )?;
                    }
                    Ok(Response::new(response))
                }
            }
        }
    }

    async fn task_output_upload(
        &self,
        request: Request<TaskOutputUploadRequest>,
    ) -> Result<Response<TaskOutputUploadResponse>, Status> {
        let req = request.into_inner();
        let key = self
            .validate_task_assignment(&req.token, &req.task_id, Some(&req.worker_id))
            .map_err(|status| *status)?;
        if req.task_id.trim().is_empty() {
            return Ok(Response::new(TaskOutputUploadResponse {
                success: false,
                status_message: "Task id is required".into(),
            }));
        }
        if req.output.len() > MAX_TASK_OUTPUT_BYTES {
            return Ok(Response::new(TaskOutputUploadResponse {
                success: false,
                status_message: format!("Task output exceeds {} byte limit", MAX_TASK_OUTPUT_BYTES),
            }));
        }
        tracing::info!(
            "Output upload task {} ({} bytes)",
            req.task_id,
            req.output.len()
        );
        self.report_for_update_for_key(&key, |report| {
            report.output = Some(req.output);
        })
        .map_err(|status| *status)?;
        Ok(Response::new(TaskOutputUploadResponse {
            success: true,
            status_message: "OK".into(),
        }))
    }

    async fn task_result_upload(
        &self,
        request: Request<TaskResultUploadRequest>,
    ) -> Result<Response<TaskResultUploadResponse>, Status> {
        let req = request.into_inner();
        let key = self
            .validate_task_assignment(&req.token, &req.task_id, Some(&req.worker_id))
            .map_err(|status| *status)?;
        if req.task_id.trim().is_empty() {
            return Ok(Response::new(TaskResultUploadResponse {
                success: false,
                status_message: "Task id is required".into(),
            }));
        }
        if req.result_torrent.trim().is_empty() {
            return Ok(Response::new(TaskResultUploadResponse {
                success: false,
                status_message: "Result reference is required".into(),
            }));
        }
        if req.result_torrent.len() > MAX_RESULT_REFERENCE_BYTES {
            return Ok(Response::new(TaskResultUploadResponse {
                success: false,
                status_message: format!(
                    "Result reference exceeds {} byte limit",
                    MAX_RESULT_REFERENCE_BYTES
                ),
            }));
        }
        tracing::info!(
            "Result upload task {} torrent={}",
            req.task_id,
            req.result_torrent
        );
        self.report_for_update_for_key(&key, |report| {
            report.result_torrent = Some(req.result_torrent);
        })
        .map_err(|status| *status)?;
        Ok(Response::new(TaskResultUploadResponse {
            success: true,
            status_message: "OK".into(),
        }))
    }

    async fn task_output(
        &self,
        request: Request<TaskOutputRequest>,
    ) -> Result<Response<TaskOutputResponse>, Status> {
        let req = request.into_inner();
        let key = self
            .validate_task_assignment(&req.token, &req.task_id, None)
            .map_err(|status| *status)?;
        let Some(report) = self
            .report_for_task_for_key(&key)
            .map_err(|status| *status)?
        else {
            return Ok(Response::new(TaskOutputResponse {
                success: false,
                status_message: "Task output not found".into(),
                output: String::new(),
            }));
        };
        let Some(output) = report.output else {
            return Ok(Response::new(TaskOutputResponse {
                success: false,
                status_message: "Task output not found".into(),
                output: String::new(),
            }));
        };
        Ok(Response::new(TaskOutputResponse {
            success: true,
            status_message: "OK".into(),
            output,
        }))
    }

    async fn stop_task_execution(
        &self,
        request: Request<StopTaskExecutionRequest>,
    ) -> Result<Response<StopTaskExecutionResponse>, Status> {
        let req = request.into_inner();
        if req.attempt_id.trim().is_empty() {
            self.validate_task_assignment(&req.token, &req.task_id, None)
                .map_err(|status| *status)?;
        } else {
            self.validate_task_attempt_assignment_with_idempotency(
                &req.token,
                &req.task_id,
                &req.attempt_id,
                &req.idempotency_key,
            )
            .map_err(|status| *status)?;
        }
        if !crate::sandbox::is_safe_task_id(&req.task_id) {
            return Err(Status::invalid_argument("unsafe task id"));
        }
        tracing::info!("Stop task {}", req.task_id);
        let (success, status_message) = match self.state.executor.stop_task_execution_for_attempt(
            &req.task_id,
            (!req.attempt_id.trim().is_empty()).then_some(req.attempt_id.as_str()),
        ) {
            StopTaskOutcome::StopRequested => (true, "Stop requested"),
            StopTaskOutcome::AlreadyStopping => (true, "Stop already requested"),
            StopTaskOutcome::NotRunning => (false, "Task not running"),
        };
        Ok(Response::new(StopTaskExecutionResponse {
            success,
            status_message: status_message.into(),
        }))
    }

    async fn task_usage(
        &self,
        request: Request<TaskUsageRequest>,
    ) -> Result<Response<TaskUsageResponse>, Status> {
        let req = request.into_inner();
        let key = self
            .validate_task_assignment(&req.token, &req.task_id, Some(&req.worker_id))
            .map_err(|status| *status)?;
        if req.task_id.trim().is_empty() {
            return Ok(Response::new(TaskUsageResponse {
                success: false,
                status_message: "Task id is required".into(),
            }));
        }
        let Some(usage) = req.usage else {
            return Ok(Response::new(TaskUsageResponse {
                success: false,
                status_message: "Usage payload is required".into(),
            }));
        };
        if !resource_usage_is_finite(&usage) {
            return Ok(Response::new(TaskUsageResponse {
                success: false,
                status_message: "Task usage contains non-finite values".into(),
            }));
        }
        tracing::debug!(
            "Task {} usage: cpu={:.1}% mem={:.1}%",
            req.task_id,
            usage.cpu_percent,
            usage.memory_percent
        );
        self.report_for_update_for_key(&key, |report| {
            report.usage = Some(usage);
        })
        .map_err(|status| *status)?;
        Ok(Response::new(TaskUsageResponse {
            success: true,
            status_message: "OK".into(),
        }))
    }
}

#[tonic::async_trait]
impl GeneralComputeChunkService for GrpcGeneralComputeChunkService {
    async fn prepare_general_compute(
        &self,
        request: Request<GeneralComputePrepareRequest>,
    ) -> Result<Response<GeneralComputePrepareResponse>, Status> {
        let request = request.into_inner();
        let admitted = self.prepare_request(&request).await?;
        Ok(Response::new(GeneralComputePrepareResponse {
            success: true,
            status_message: "prepared".into(),
            execution_id: admitted.execution_id,
            attempt_id: admitted.attempt_id,
            idempotency_key: admitted.idempotency_key,
            request_digest: admitted.request_digest,
            transfer_generation: request.transfer_generation,
        }))
    }

    async fn upload_chunk(
        &self,
        request: Request<GeneralComputeChunkUpload>,
    ) -> Result<Response<GeneralComputeChunkUploadResponse>, Status> {
        let upload = request.into_inner();
        let (verified, request) = self.assignment(
            &upload.token,
            &upload.execution_id,
            &upload.attempt_id,
            &upload.idempotency_key,
            &upload.request_digest,
            upload.transfer_generation,
        )?;
        self.validate_transfer_lease_for_token(
            &upload.token,
            &upload.execution_id,
            &upload.attempt_id,
            upload.transfer_generation,
            &upload.idempotency_key,
            &upload.request_digest,
        )
        .await?;
        let store = self
            .state
            .cas_store
            .as_deref()
            .ok_or_else(|| Status::failed_precondition("general-compute CAS is unavailable"))?;
        crate::chunk_transport::ingest_general_compute_chunk(store, &request, &upload, &verified)
            .map_err(chunk_transport_status)?;
        Ok(Response::new(GeneralComputeChunkUploadResponse {
            success: true,
            status_message: "accepted".into(),
            accepted_chunks: 1,
        }))
    }

    async fn resume_chunks(
        &self,
        request: Request<GeneralComputeChunkResumeRequest>,
    ) -> Result<Response<GeneralComputeChunkResumeResponse>, Status> {
        let resume = request.into_inner();
        let (verified, request) = self.assignment(
            &resume.token,
            &resume.execution_id,
            &resume.attempt_id,
            &resume.idempotency_key,
            &resume.request_digest,
            resume.transfer_generation,
        )?;
        self.validate_transfer_lease_for_token(
            &resume.token,
            &resume.execution_id,
            &resume.attempt_id,
            resume.transfer_generation,
            &resume.idempotency_key,
            &resume.request_digest,
        )
        .await?;
        let store = self
            .state
            .cas_store
            .as_deref()
            .ok_or_else(|| Status::failed_precondition("general-compute CAS is unavailable"))?;
        let missing = crate::chunk_transport::resume_general_compute_chunks(
            store, &request, &resume, &verified,
        )
        .map_err(chunk_transport_status)?;
        let response = GeneralComputeChunkResumeResponse {
            success: true,
            status_message: "resume".into(),
            missing_chunks: missing.into_iter().map(chunk_descriptor).collect(),
        };
        if response.encoded_len() > GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES {
            return Err(Status::resource_exhausted(
                "missing chunk descriptor response is too large",
            ));
        }
        Ok(Response::new(response))
    }
}

fn chunk_descriptor(
    chunk: general_compute_runtime::ArtifactChunk,
) -> GeneralComputeChunkDescriptor {
    GeneralComputeChunkDescriptor {
        offset: chunk.offset as i64,
        size_bytes: chunk.size_bytes as i64,
        sha256: chunk.sha256,
    }
}

fn chunk_auth_status(error: crate::chunk_transport::WorkerChunkIngestError) -> Status {
    match error {
        crate::chunk_transport::WorkerChunkIngestError::AuthorizationInvalid => {
            Status::unauthenticated(error.to_string())
        }
        crate::chunk_transport::WorkerChunkIngestError::AuthorizationMismatch
        | crate::chunk_transport::WorkerChunkIngestError::TokenMismatch => {
            Status::permission_denied(error.to_string())
        }
        _ => Status::permission_denied(error.to_string()),
    }
}

fn chunk_transport_status(error: crate::chunk_transport::WorkerChunkIngestError) -> Status {
    match error {
        crate::chunk_transport::WorkerChunkIngestError::TokenMismatch
        | crate::chunk_transport::WorkerChunkIngestError::AuthorizationMismatch => {
            Status::permission_denied(error.to_string())
        }
        crate::chunk_transport::WorkerChunkIngestError::AuthorizationInvalid => {
            Status::unauthenticated(error.to_string())
        }
        crate::chunk_transport::WorkerChunkIngestError::WireInvalid(_) => {
            Status::invalid_argument(error.to_string())
        }
        crate::chunk_transport::WorkerChunkIngestError::Transport(error) => match error {
            general_compute_runtime::transport::ChunkTransportError::IdentityMismatch => {
                Status::permission_denied(error.to_string())
            }
            general_compute_runtime::transport::ChunkTransportError::ArtifactNotFound
            | general_compute_runtime::transport::ChunkTransportError::ManifestChunkMismatch
            | general_compute_runtime::transport::ChunkTransportError::ManifestInvalid(_)
            | general_compute_runtime::transport::ChunkTransportError::RequestInvalid(_) => {
                Status::invalid_argument(error.to_string())
            }
            _ => Status::failed_precondition(error.to_string()),
        },
    }
}

#[cfg(test)]
fn execute_response_from_result(
    result: TaskResult,
    identity: &ExecuteTaskIdentity,
) -> ExecuteTaskResponse {
    execute_response_from_result_for_runtime(result, false, identity)
        .unwrap_or_else(|status| failed_execute_response(status.message(), identity))
}

#[allow(clippy::result_large_err)]
fn execute_response_from_result_for_runtime(
    result: TaskResult,
    managed_gpu: bool,
    identity: &ExecuteTaskIdentity,
) -> Result<ExecuteTaskResponse, Status> {
    let TaskResult {
        success,
        output,
        error,
        managed_executed_ops,
        managed_output_bytes,
        managed_receipt_json,
        general_compute_result_json,
        managed_gpu_result_json,
        ..
    } = result;
    let has_typed_general_compute_result = general_compute_result_json.is_some();
    let has_typed_managed_gpu_result = managed_gpu_result_json.is_some();
    let response = ExecuteTaskResponse {
        success,
        status_message: if success && has_typed_general_compute_result {
            "general-compute result attached".into()
        } else if success && has_typed_managed_gpu_result {
            "managed GPU result attached".into()
        } else if success {
            output.unwrap_or_default()
        } else {
            error.unwrap_or_else(|| "Task execution failed".into())
        },
        managed_executed_ops,
        managed_output_bytes,
        managed_receipt_json: managed_receipt_json.unwrap_or_default(),
        general_compute_result_json: general_compute_result_json.unwrap_or_default(),
        managed_gpu_result_json: managed_gpu_result_json.unwrap_or_default(),
        managed_consensus_result: None,
        managed_consensus_result_digest: String::new(),
        replica_id: String::new(),
        consensus_round_id: String::new(),
        consensus_protocol_version: 0,
        execution_id: identity.execution_id.clone(),
        attempt_id: identity.attempt_id.clone(),
        idempotency_key: identity.idempotency_key.clone(),
        request_digest: identity.request_digest.clone(),
    };
    if managed_gpu {
        let Some(payload) = (!response.managed_gpu_result_json.is_empty())
            .then_some(response.managed_gpu_result_json.as_slice())
        else {
            return Err(Status::failed_precondition(
                "managed GPU execution did not return a typed result",
            ));
        };
        if !response_fits_worker_rpc_limits(&response) {
            return Err(Status::resource_exhausted(
                "managed GPU typed result exceeds the Worker RPC limit",
            ));
        }
        let typed =
            serde_json::from_slice::<general_compute_runtime::managed_gpu::ManagedGpuResult>(
                payload,
            )
            .map_err(|_| Status::internal("managed GPU typed result is malformed"))?;
        if typed.execution_id != identity.execution_id
            || typed.attempt_id != identity.attempt_id
            || typed.idempotency_key != identity.idempotency_key
            || typed.request_digest != identity.request_digest
        {
            return Err(Status::internal(
                "managed GPU typed result identity does not match the execution request",
            ));
        }
        let typed_completed =
            typed.status == general_compute_runtime::managed_gpu::ManagedGpuStatus::Completed;
        if response.success != typed_completed {
            return Err(Status::internal(
                "managed GPU typed result status does not match the execution response",
            ));
        }
        if !response.managed_receipt_json.is_empty()
            || !response.general_compute_result_json.is_empty()
        {
            return Err(Status::internal(
                "managed GPU execution returned an incompatible result channel",
            ));
        }
    } else if !response_fits_worker_rpc_limits(&response) {
        return Ok(failed_execute_response(
            "Task result exceeds supported response limits",
            identity,
        ));
    }

    Ok(response)
}

fn response_fits_worker_rpc_limits(response: &ExecuteTaskResponse) -> bool {
    response.status_message.len() <= WORKER_STATUS_MESSAGE_MAX_BYTES
        && response.managed_receipt_json.len() <= LEGACY_MANAGED_RECEIPT_MAX_BYTES
        && response.general_compute_result_json.len() <= GENERAL_COMPUTE_RESULT_MAX_BYTES
        && response.managed_gpu_result_json.len() <= MANAGED_GPU_RESULT_MAX_BYTES
        && response
            .managed_consensus_result
            .as_ref()
            .is_none_or(|result| {
                result.encoded_len() <= hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES
            })
        && response.managed_consensus_result_digest.len() <= 71
        && response.replica_id.len() <= hivemind_proto::GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES
        && response.consensus_round_id.len()
            <= hivemind_proto::GENERAL_COMPUTE_TRANSFER_ID_MAX_BYTES
        && response.encoded_len() <= WORKER_RPC_MESSAGE_MAX_BYTES
}

fn validate_managed_consensus_response_identity(
    response: &ExecuteTaskResponse,
    request: &ExecuteTaskRequest,
) -> Result<(), Status> {
    if response.execution_id != request.execution_id
        || response.attempt_id != request.attempt_id
        || response.idempotency_key != request.idempotency_key
        || response.request_digest != request.request_digest
        || response.consensus_round_id != request.consensus_round_id
        || response.replica_id != request.replica_id
        || response.consensus_protocol_version != request.consensus_protocol_version
    {
        return Err(Status::internal(
            "managed consensus response identity does not match the request",
        ));
    }
    Ok(())
}

fn validate_managed_consensus_result_config_limit(
    config: &HivemindConfig,
    response: &ExecuteTaskResponse,
) -> Result<(), Status> {
    if response.consensus_protocol_version > 0
        && response
            .managed_consensus_result
            .as_ref()
            .is_some_and(|result| {
                result.encoded_len() > config.managed_consensus.max_result_bytes
                    || result.output.len() > config.managed_consensus.max_result_bytes
            })
    {
        return Err(Status::resource_exhausted(
            "managed consensus result exceeds the configured byte limit",
        ));
    }
    Ok(())
}

fn managed_consensus_backend_id(task: &Task) -> String {
    task.managed_dsl_backend_id
        .as_deref()
        .filter(|backend_id| !backend_id.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if task.runtime.as_deref().map(str::trim)
                == Some(general_compute_runtime::MANAGED_DSL_RUNTIME_VERSION)
            {
                hivemind_managed_consensus::MANAGED_DSL_DEFAULT_BACKEND_ID.to_owned()
            } else {
                String::new()
            }
        })
}

fn managed_consensus_semantics_digest(task: &Task) -> String {
    task.managed_dsl_semantics_manifest_sha256
        .as_deref()
        .filter(|digest| !digest.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if task.runtime.as_deref().map(str::trim)
                == Some(general_compute_runtime::MANAGED_DSL_RUNTIME_VERSION)
            {
                hivemind_managed_consensus::MANAGED_DSL_DEFAULT_SEMANTICS_DIGEST.to_owned()
            } else {
                String::new()
            }
        })
}

fn attach_managed_consensus_result(
    response: &mut ExecuteTaskResponse,
    request: &ExecuteTaskRequest,
    task: &Task,
) -> Result<(), Status> {
    let output = if response.success {
        response.status_message.as_bytes().to_vec()
    } else {
        Vec::new()
    };
    if output.len() > hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES {
        return Err(Status::resource_exhausted(
            "managed consensus result exceeds the Worker result limit",
        ));
    }
    let result = ManagedConsensusResult {
        protocol_version: request.consensus_protocol_version,
        status: if response.success {
            "completed".into()
        } else {
            "failed".into()
        },
        output: output.clone(),
        error_code: if response.success {
            String::new()
        } else {
            response.status_message.clone()
        },
        usage_units: response.managed_executed_ops.max(0) as u64,
        executed_ops: response.managed_executed_ops.max(0) as u64,
        output_bytes: output.len() as u64,
        runtime: task.runtime.clone().unwrap_or_default(),
        backend_id: managed_consensus_backend_id(task),
        semantics_manifest_sha256: managed_consensus_semantics_digest(task),
        source_sha256: digest_hex(task.task_source.as_deref().unwrap_or_default().as_bytes()),
        input_sha256: digest_hex(
            task.torrent_source
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        ),
        result_digest: digest_hex(&output),
    };
    response.managed_consensus_result_digest =
        hivemind_proto::managed_consensus_result_digest(&result);
    response.managed_consensus_result = Some(result);
    response.replica_id = request.replica_id.clone();
    response.consensus_round_id = request.consensus_round_id.clone();
    response.consensus_protocol_version = request.consensus_protocol_version;
    Ok(())
}

fn failed_execute_response(message: &str, identity: &ExecuteTaskIdentity) -> ExecuteTaskResponse {
    ExecuteTaskResponse {
        success: false,
        status_message: message.into(),
        managed_executed_ops: 0,
        managed_output_bytes: 0,
        managed_receipt_json: String::new(),
        general_compute_result_json: Vec::new(),
        managed_gpu_result_json: Vec::new(),
        managed_consensus_result: None,
        managed_consensus_result_digest: String::new(),
        replica_id: String::new(),
        consensus_round_id: String::new(),
        consensus_protocol_version: 0,
        execution_id: identity.execution_id.clone(),
        attempt_id: identity.attempt_id.clone(),
        idempotency_key: identity.idempotency_key.clone(),
        request_digest: identity.request_digest.clone(),
    }
}

#[derive(Debug, Clone, Default)]
struct ExecuteTaskIdentity {
    execution_id: String,
    attempt_id: String,
    idempotency_key: String,
    request_digest: String,
}

impl ExecuteTaskIdentity {
    fn from_request(request: &ExecuteTaskRequest) -> Self {
        Self {
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
        }
    }
}

fn resource_usage_is_finite(usage: &hivemind_proto::ResourceUsage) -> bool {
    usage.cpu_percent.is_finite()
        && usage.memory_percent.is_finite()
        && usage.gpu_percent.is_finite()
        && usage.vram_percent.is_finite()
        && usage.storage_percent.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use general_compute_runtime::artifact::CasChunkStore;
    use general_compute_runtime::managed_gpu::{
        ManagedGpuBackendRegistration, ManagedGpuCapability, ManagedGpuEvidence, ManagedGpuLimits,
        ManagedGpuProofPolicy, ManagedGpuRequest, ManagedGpuRequirement, ManagedGpuResult,
        ManagedGpuUsage, MANAGED_GPU_BILLING_VERSION, MANAGED_GPU_COST_MODEL_VERSION,
        MANAGED_GPU_OPERATION_COST_UNITS, MANAGED_GPU_OPERATION_REGISTRY_VERSION,
        MANAGED_GPU_REQUEST_PROTOCOL_VERSION, MANAGED_GPU_RESULT_PROTOCOL_VERSION,
        MANAGED_GPU_RUNTIME_VERSION, MANAGED_GPU_SEMANTICS_MANIFEST_SHA256,
        MANAGED_GPU_SETTLEMENT_BASIS,
    };
    use general_compute_runtime::{
        sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRole, ExecutionPolicy,
        GeneralComputeRequest, GENERAL_COMPUTE_RUNTIME_VERSION,
    };
    use hivemind_auth::worker_execution::{WorkerExecutionIdentity, WorkerExecutionSigner};
    use hivemind_models::Claims;
    use hivemind_proto::ResourceSpec;
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;
    use tempfile::TempDir;
    use tonic::{Code, Request};

    const CONTROL_PLANE_SECRET: &str = "unit-test-control-plane-secret-at-least-32-bytes";
    const ASSIGNED_OWNER: &str = "task-owner";
    const OTHER_OWNER: &str = "other-owner";
    const TEST_WORKER_ID: &str = "worker-1";

    fn test_key_pair() -> &'static (String, String) {
        static KEY_PAIR: OnceLock<(String, String)> = OnceLock::new();
        KEY_PAIR.get_or_init(hivemind_config::generate_worker_execution_test_key_pair)
    }

    fn test_private_key_pem() -> &'static str {
        test_key_pair().0.as_str()
    }

    #[test]
    fn managed_consensus_token_identity_requires_every_signed_field() {
        let mut request = execute_request(
            "managed-function-v0",
            "return input".into(),
            "{}".into(),
            10,
        );
        request.execution_id = "execution-1".into();
        request.attempt_id = "attempt-1".into();
        request.idempotency_key = "idempotency-1".into();
        request.request_digest = "sha256:request".into();
        request.consensus_round_id = "round-1".into();
        request.replica_id = "replica-1".into();
        request.consensus_protocol_version = 1;
        let claims = WorkerExecutionClaims {
            claims: Claims {
                sub: ASSIGNED_OWNER.into(),
                user_id: ASSIGNED_OWNER.into(),
                role: Some("worker-execution".into()),
                task_id: Some(request.task_id.clone()),
                worker_id: Some(TEST_WORKER_ID.into()),
                exp: usize::MAX,
                iat: 0,
            },
            execution_id: Some(request.execution_id.clone()),
            attempt_id: Some(request.attempt_id.clone()),
            idempotency_key: Some(request.idempotency_key.clone()),
            request_digest: Some(request.request_digest.clone()),
            transfer_generation: Some(1),
            consensus_round_id: Some(request.consensus_round_id.clone()),
            replica_id: Some(request.replica_id.clone()),
            consensus_protocol_version: Some(1),
        };
        validate_managed_consensus_token_identity(&claims, &request).unwrap();
        let mut forged = request;
        forged.request_digest = "sha256:other".into();
        assert!(validate_managed_consensus_token_identity(&claims, &forged).is_err());
    }

    #[test]
    fn managed_consensus_completed_result_cache_replays_the_exact_response() {
        let temp = TempDir::new().unwrap();
        let service = test_service(temp.path());
        let mut request = execute_request(
            "managed-function-v0",
            "return input".into(),
            "{}".into(),
            10,
        );
        request.execution_id = "execution-cache".into();
        request.attempt_id = "attempt-cache".into();
        request.idempotency_key = "idempotency-cache".into();
        request.request_digest = "sha256:request-cache".into();
        request.consensus_round_id = "round-cache".into();
        request.replica_id = "replica-cache".into();
        request.consensus_protocol_version = 1;
        let output = b"cached".to_vec();
        let result = ManagedConsensusResult {
            protocol_version: 1,
            status: "completed".into(),
            output: output.clone(),
            output_bytes: output.len() as u64,
            runtime: "managed-function-v0".into(),
            backend_id: hivemind_managed_consensus::MANAGED_DSL_DEFAULT_BACKEND_ID.into(),
            semantics_manifest_sha256:
                hivemind_managed_consensus::MANAGED_DSL_DEFAULT_SEMANTICS_DIGEST.into(),
            source_sha256: digest_hex(b"return input"),
            input_sha256: digest_hex(b"{}"),
            result_digest: digest_hex(&output),
            ..ManagedConsensusResult::default()
        };
        let mut response = ExecuteTaskResponse {
            success: true,
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            consensus_round_id: request.consensus_round_id.clone(),
            replica_id: request.replica_id.clone(),
            consensus_protocol_version: 1,
            managed_consensus_result: Some(result),
            ..ExecuteTaskResponse::default()
        };
        response.managed_consensus_result_digest = digest_hex(
            &response
                .managed_consensus_result
                .as_ref()
                .unwrap()
                .encode_to_vec(),
        );
        service
            .state
            .cache_consensus_result(&request, &response)
            .unwrap();
        let mut conflicting = response.clone();
        conflicting.status_message = "different".into();
        assert!(service
            .state
            .cache_consensus_result(&request, &conflicting)
            .is_err());
        assert_eq!(
            service.state.cached_consensus_result(&request).unwrap(),
            Some(response)
        );
        let mut different_replica = request.clone();
        different_replica.replica_id = "replica-other".into();
        assert!(service
            .state
            .cached_consensus_result(&different_replica)
            .unwrap()
            .is_none());
        let mut different_payload = request;
        different_payload.task_source = "return a different value".into();
        assert!(service
            .state
            .cached_consensus_result(&different_payload)
            .unwrap()
            .is_none());
    }

    fn execute_request(
        runtime: &str,
        source: String,
        input: String,
        budget: i64,
    ) -> ExecuteTaskRequest {
        ExecuteTaskRequest {
            task_id: "managed-contract-test".into(),
            torrent: input,
            resource_limits: None,
            runtime: runtime.into(),
            task_source: source,
            token: String::new(),
            managed_budget_units: budget,
            general_compute_manifest_json: Vec::new(),
            execution_id: String::new(),
            attempt_id: String::new(),
            idempotency_key: String::new(),
            request_digest: String::new(),
            managed_dsl_backend_id: String::new(),
            managed_dsl_semantics_manifest_sha256: String::new(),
            managed_gpu_manifest_json: Vec::new(),
            consensus_round_id: String::new(),
            replica_id: String::new(),
            consensus_protocol_version: 0,
        }
    }

    fn managed_gpu_request_for_execute_tests() -> ManagedGpuRequest {
        let image_digest = format!("sha256:{}", "a".repeat(64));
        let gpu_requirement = ManagedGpuRequirement::new(
            "8.9",
            "12.4",
            "550",
            8 * 1024 * 1024 * 1024,
            1,
            image_digest.clone(),
        )
        .unwrap();
        let mut request = ManagedGpuRequest {
            protocol_version: MANAGED_GPU_REQUEST_PROTOCOL_VERSION.into(),
            execution_id: "execution-gpu-service".into(),
            attempt_id: "attempt-gpu-service".into(),
            idempotency_key: "idempotency-gpu-service".into(),
            request_digest: String::new(),
            runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
            semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
            operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
            backend_id: "cuda-service-test".into(),
            guest_image_digest: image_digest,
            source: "gpu_add_f32([1.0], [2.0])".into(),
            input_json: "{}".into(),
            gpu_requirement,
            limits: ManagedGpuLimits::default(),
            reservation_cpt: 10,
            billing_version: MANAGED_GPU_BILLING_VERSION.into(),
            cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
            settlement_basis: MANAGED_GPU_SETTLEMENT_BASIS.into(),
            proof_policy: ManagedGpuProofPolicy::None,
        };
        request.request_digest = request.canonical_request_digest();
        request
    }

    fn managed_gpu_capability_for_execute_tests(
        request: &ManagedGpuRequest,
    ) -> ManagedGpuCapability {
        ManagedGpuCapability::new(
            "cuda-service-test-0",
            request.gpu_requirement.compute_capability.clone(),
            request.gpu_requirement.runtime_version.clone(),
            request.gpu_requirement.driver_abi.clone(),
            16 * 1024 * 1024 * 1024,
            32,
            request.guest_image_digest.clone(),
            0,
            "GPU-0123456789abcdef",
        )
        .unwrap()
    }

    fn managed_gpu_runtime_admission_for_execute_tests(
        request: &ManagedGpuRequest,
        capability: ManagedGpuCapability,
    ) -> WorkerRuntimeAdmission {
        WorkerRuntimeAdmission::new_with_trusted_registration(
            general_compute_runtime::TrustedWorkerCapabilityRegistration {
                worker: general_compute_runtime::WorkerCapabilities {
                    guest_image_digests: vec![request.guest_image_digest.clone()],
                    capabilities: vec![MANAGED_GPU_RUNTIME_VERSION.into()],
                    max_threads: 4,
                    gpu_available: true,
                },
                gpu_capabilities: vec![],
                managed_gpu_backends: vec![ManagedGpuBackendRegistration {
                    backend_id: request.backend_id.clone(),
                    runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
                    semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
                    operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
                    guest_image_digest: request.guest_image_digest.clone(),
                    billing_version: MANAGED_GPU_BILLING_VERSION.into(),
                    cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
                    reservation_cpt: request.reservation_cpt,
                    max_source_bytes: 256 * 1024,
                    max_input_bytes: 16 * 1024 * 1024,
                    max_output_bytes: 16 * 1024 * 1024,
                    max_operations: 1_000_000,
                    max_gpu_time_ms: 120_000,
                    capabilities: vec![capability],
                }],
                backends: vec![],
            },
        )
    }

    fn managed_gpu_result_for_execute_tests(
        request: &ManagedGpuRequest,
        selected_gpu: ManagedGpuCapability,
    ) -> Vec<u8> {
        let output = "42";
        let result = ManagedGpuResult {
            protocol_version: MANAGED_GPU_RESULT_PROTOCOL_VERSION.into(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            runtime_version: request.runtime_version.clone(),
            semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
            operation_registry_version: request.operation_registry_version.clone(),
            backend_id: request.backend_id.clone(),
            guest_image_digest: request.guest_image_digest.clone(),
            source_sha256: request.source_sha256(),
            input_sha256: request.input_sha256(),
            reservation_cpt: request.reservation_cpt,
            status: general_compute_runtime::managed_gpu::ManagedGpuStatus::Completed,
            exit_code: Some(0),
            error_code: None,
            output: output.into(),
            output_sha256: sha256_digest(output.as_bytes()),
            selected_gpu,
            usage: ManagedGpuUsage {
                source_bytes: request.source.len() as u64,
                input_bytes: request.input_json.len() as u64,
                output_bytes: output.len() as u64,
                executed_operations: 1,
                operation_cost_units: MANAGED_GPU_OPERATION_COST_UNITS,
                wall_time_ms: 1,
                gpu_time_ms: 1,
                gpu_memory_bytes: 1,
            },
            evidence: ManagedGpuEvidence::default(),
        };
        serde_json::to_vec(&result).unwrap()
    }
    fn general_compute_request_for_chunk_tests() -> GeneralComputeRequest {
        let bytes = b"print(42)";
        let source = ArtifactManifest {
            artifact_id: "source".into(),
            role: ArtifactRole::Source,
            size_bytes: bytes.len() as u64,
            mime_type: "text/plain".into(),
            sha256: sha256_digest(bytes),
            chunks: vec![ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(bytes),
            }],
            inline_bytes: None,
        };
        let mut request = GeneralComputeRequest {
            execution_id: "execution-service".into(),
            attempt_id: "attempt-service".into(),
            idempotency_key: "idempotency-service".into(),
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

    fn general_compute_upload(
        request: &GeneralComputeRequest,
        token: &str,
        bytes: &[u8],
    ) -> hivemind_proto::GeneralComputeChunkUpload {
        hivemind_proto::GeneralComputeChunkUpload {
            token: token.into(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            transfer_generation: 1,
            artifact_id: "source".into(),
            offset: 0,
            size_bytes: bytes.len() as i64,
            sha256: sha256_digest(bytes),
            bytes: bytes.to_vec(),
        }
    }

    fn chunk_runtime_admission() -> WorkerRuntimeAdmission {
        WorkerRuntimeAdmission::new(
            general_compute_runtime::CapabilityMatrix::new(vec![
                general_compute_runtime::BackendRegistration {
                    backend_id: "python-reference".into(),
                    execution_mode:
                        general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
                    guest_image_digest: format!("sha256:{}", "a".repeat(64)),
                    capabilities: vec!["cpu".into()],
                    max_threads: 2,
                    network_allowed: false,
                    filesystem_read_only: true,
                    gpu_allowed: false,
                },
            ]),
            general_compute_runtime::WorkerCapabilities {
                guest_image_digests: vec![format!("sha256:{}", "a".repeat(64))],
                capabilities: vec!["cpu".into()],
                max_threads: 2,
                gpu_available: false,
            },
        )
    }

    fn chunk_test_components(
        base: &std::path::Path,
        cas_store: Option<Arc<CasChunkStore>>,
    ) -> (GrpcWorkerNodeService, GrpcGeneralComputeChunkService) {
        chunk_test_components_for(
            base,
            cas_store,
            TEST_WORKER_ID,
            Arc::new(AllowLocalTransferLeaseAuthority),
        )
    }

    fn chunk_test_components_for(
        base: &std::path::Path,
        cas_store: Option<Arc<CasChunkStore>>,
        worker_id: &str,
        authority: Arc<dyn TransferLeaseAuthority>,
    ) -> (GrpcWorkerNodeService, GrpcGeneralComputeChunkService) {
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = base.join("sandbox").to_string_lossy().to_string();
        config.auth.jwt_secret = CONTROL_PLANE_SECRET.into();
        config.auth.worker_execution_public_key_pem = test_key_pair().1.clone();
        let executor = Arc::new(WorkerExecutor::new_with_task_runner(
            config.clone(),
            |_task, _cancellation| async move { Ok(successful_task_result()) },
        ));
        let state = Arc::new(WorkerGrpcState {
            config,
            executor,
            worker_id: Arc::new(Mutex::new(Some(worker_id.into()))),
            cas_store,
            reports: Mutex::new(HashMap::new()),
            completed_consensus_results: Arc::new(Mutex::new(HashMap::new())),
            transfer_lease_authority: Arc::new(Mutex::new(Some(authority))),
        });
        let worker = GrpcWorkerNodeService::new(state.clone())
            .with_runtime_admission(chunk_runtime_admission());
        let chunk_service = GrpcGeneralComputeChunkService::new(state, chunk_runtime_admission());
        (worker, chunk_service)
    }

    fn general_compute_prepare_request(
        request: &GeneralComputeRequest,
        token: &str,
        task_id: &str,
    ) -> GeneralComputePrepareRequest {
        general_compute_prepare_request_for(request, token, task_id, 1)
    }

    fn general_compute_prepare_request_for(
        request: &GeneralComputeRequest,
        token: &str,
        task_id: &str,
        transfer_generation: i64,
    ) -> GeneralComputePrepareRequest {
        GeneralComputePrepareRequest {
            task_id: task_id.into(),
            token: token.into(),
            runtime: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            general_compute_manifest_json: serde_json::to_vec(request).unwrap(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            transfer_generation,
        }
    }

    fn general_compute_execute_request(
        request: &GeneralComputeRequest,
        token: &str,
        task_id: &str,
    ) -> ExecuteTaskRequest {
        ExecuteTaskRequest {
            task_id: task_id.into(),
            runtime: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            token: token.into(),
            general_compute_manifest_json: serde_json::to_vec(request).unwrap(),
            ..ExecuteTaskRequest::default()
        }
    }

    async fn admit_general_compute_request(
        worker: &GrpcWorkerNodeService,
        request: &GeneralComputeRequest,
        token: &str,
        task_id: &str,
    ) {
        let response = worker
            .execute_task(Request::new(general_compute_execute_request(
                request, token, task_id,
            )))
            .await
            .expect("general-compute admission should execute")
            .into_inner();
        assert!(response.success, "admission runner should succeed");
    }

    fn managed_gpu_result_json(identity: &ExecuteTaskIdentity) -> Vec<u8> {
        serde_json::json!({
            "protocol_version": "managed-function-gpu-result-v1",
            "execution_id": identity.execution_id.clone(),
            "attempt_id": identity.attempt_id.clone(),
            "idempotency_key": identity.idempotency_key.clone(),
            "request_digest": identity.request_digest.clone(),
            "runtime_version": "managed-function-gpu-v1",
            "semantics_manifest_sha256": "sha256:semantics",
            "operation_registry_version": "managed-function-gpu-ops-v1",
            "backend_id": "cuda-fixed",
            "guest_image_digest": "sha256:image",
            "source_sha256": "sha256:source",
            "input_sha256": "sha256:input",
            "reservation_cpt": 10,
            "status": "completed",
            "exit_code": 0,
            "error_code": null,
            "output": "42",
            "output_sha256": "sha256:output",
            "selected_gpu": {
                "protocol_version": "managed-function-gpu-capability-v1",
                "vendor": "nvidia",
                "device_id": "gpu-0",
                "compute_capability": "8.9",
                "runtime": "cuda",
                "runtime_version": "12.4",
                "driver_abi": "550",
                "vram_bytes": 17179869184u64,
                "max_streams": 32,
                "image_digest": "sha256:image",
                "cuda_device_ordinal": 0,
                "cuda_uuid": "GPU-test"
            },
            "usage": {
                "source_bytes": 1,
                "input_bytes": 1,
                "output_bytes": 2,
                "executed_operations": 1,
                "operation_cost_units": 10,
                "wall_time_ms": 1,
                "gpu_time_ms": 1,
                "gpu_memory_bytes": 1
            },
            "evidence": {
                "level": "unverified",
                "payload_sha256": null
            }
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn managed_gpu_execute_response_requires_a_typed_result() {
        let mut request =
            execute_request("managed-function-gpu-v1", String::new(), String::new(), 0);
        request.execution_id = "execution-gpu-response".into();
        request.attempt_id = "attempt-gpu-response".into();
        request.idempotency_key = "idempotency-gpu-response".into();
        request.request_digest = "sha256:gpu-response".into();
        let error = execute_response_from_result_for_runtime(
            successful_task_result(),
            true,
            &ExecuteTaskIdentity::from_request(&request),
        )
        .expect_err("GPU responses without a typed result must fail closed");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }

    #[test]
    fn managed_gpu_execute_response_rejects_malformed_typed_result() {
        let identity = ExecuteTaskIdentity {
            execution_id: "execution-malformed".into(),
            attempt_id: "attempt-malformed".into(),
            idempotency_key: "idempotency-malformed".into(),
            request_digest: "sha256:malformed".into(),
        };
        let mut result = successful_task_result();
        result.managed_receipt_json = None;
        result.managed_gpu_result_json = Some(b"{".to_vec());
        let error = execute_response_from_result_for_runtime(result, true, &identity)
            .expect_err("malformed GPU JSON must fail closed");
        assert_eq!(error.code(), Code::Internal);
    }

    #[test]
    fn managed_gpu_execute_response_rejects_identity_mismatch() {
        let identity = ExecuteTaskIdentity {
            execution_id: "execution-expected".into(),
            attempt_id: "attempt-expected".into(),
            idempotency_key: "idempotency-expected".into(),
            request_digest: "sha256:expected".into(),
        };
        let payload_identity = ExecuteTaskIdentity {
            execution_id: "execution-other".into(),
            ..identity.clone()
        };
        let mut result = successful_task_result();
        result.managed_receipt_json = None;
        result.managed_gpu_result_json = Some(managed_gpu_result_json(&payload_identity));
        let error = execute_response_from_result_for_runtime(result, true, &identity)
            .expect_err("GPU identity drift must fail closed");
        assert_eq!(error.code(), Code::Internal);
    }

    #[test]
    fn managed_gpu_execute_response_rejects_status_mismatch() {
        let identity = ExecuteTaskIdentity {
            execution_id: "execution-status".into(),
            attempt_id: "attempt-status".into(),
            idempotency_key: "idempotency-status".into(),
            request_digest: "sha256:status".into(),
        };
        let mut result = successful_task_result();
        result.success = false;
        result.output = None;
        result.error = Some("failed".into());
        result.managed_receipt_json = None;
        result.managed_gpu_result_json = Some(managed_gpu_result_json(&identity));
        let error = execute_response_from_result_for_runtime(result, true, &identity)
            .expect_err("outer status must agree with the typed GPU status");
        assert_eq!(error.code(), Code::Internal);
    }

    #[test]
    fn managed_gpu_execute_response_rejects_incompatible_result_channels() {
        let identity = ExecuteTaskIdentity {
            execution_id: "execution-channel".into(),
            attempt_id: "attempt-channel".into(),
            idempotency_key: "idempotency-channel".into(),
            request_digest: "sha256:channel".into(),
        };
        let mut result = successful_task_result();
        result.managed_gpu_result_json = Some(managed_gpu_result_json(&identity));
        let error = execute_response_from_result_for_runtime(result, true, &identity)
            .expect_err("GPU responses must not carry legacy receipt bytes");
        assert_eq!(error.code(), Code::Internal);
    }

    #[test]
    fn managed_gpu_execute_response_rejects_oversized_typed_result() {
        let identity = ExecuteTaskIdentity {
            execution_id: "execution-oversized".into(),
            attempt_id: "attempt-oversized".into(),
            idempotency_key: "idempotency-oversized".into(),
            request_digest: "sha256:oversized".into(),
        };
        let mut payload = managed_gpu_result_json(&identity);
        payload.resize(MANAGED_GPU_RESULT_MAX_BYTES + 1, b'x');
        let mut result = successful_task_result();
        result.managed_receipt_json = None;
        result.managed_gpu_result_json = Some(payload);
        let error = execute_response_from_result_for_runtime(result, true, &identity)
            .expect_err("oversized GPU typed results must fail closed");
        assert_eq!(error.code(), Code::ResourceExhausted);
    }

    #[test]
    fn managed_gpu_execute_response_accepts_typed_failure_without_success() {
        let identity = ExecuteTaskIdentity {
            execution_id: "execution-failure".into(),
            attempt_id: "attempt-failure".into(),
            idempotency_key: "idempotency-failure".into(),
            request_digest: "sha256:failure".into(),
        };
        let mut payload =
            serde_json::from_slice::<serde_json::Value>(&managed_gpu_result_json(&identity))
                .expect("test GPU result JSON is valid");
        payload["status"] = serde_json::Value::String("failed".into());
        payload["exit_code"] = serde_json::Value::Number(1.into());
        payload["error_code"] = serde_json::Value::String("backend_unavailable".into());
        let mut result = successful_task_result();
        result.success = false;
        result.output = None;
        result.error = Some("backend unavailable".into());
        result.managed_receipt_json = None;
        result.managed_gpu_result_json =
            Some(serde_json::to_vec(&payload).expect("test GPU failure JSON serializes"));
        let response = execute_response_from_result_for_runtime(result, true, &identity)
            .expect("typed GPU failures remain typed at the RPC boundary");
        assert!(!response.success);
        assert!(!response.managed_gpu_result_json.is_empty());
    }

    #[test]
    fn managed_gpu_execute_response_accepts_typed_success() {
        let identity = ExecuteTaskIdentity {
            execution_id: "execution-success".into(),
            attempt_id: "attempt-success".into(),
            idempotency_key: "idempotency-success".into(),
            request_digest: "sha256:success".into(),
        };
        let payload = managed_gpu_result_json(&identity);
        let mut result = successful_task_result();
        result.managed_receipt_json = None;
        result.managed_executed_ops = 0;
        result.managed_output_bytes = 0;
        result.managed_gpu_result_json = Some(payload.clone());
        let response = execute_response_from_result_for_runtime(result, true, &identity)
            .expect("typed GPU successes remain typed at the RPC boundary");
        assert!(response.success);
        assert_eq!(response.managed_gpu_result_json, payload);
        assert!(response.managed_receipt_json.is_empty());
        assert_eq!(response.managed_executed_ops, 0);
        assert_eq!(response.managed_output_bytes, 0);
    }

    #[test]
    fn worker_execute_response_echoes_attempt_identity_for_success_and_failure() {
        let mut request =
            execute_request("general-compute-v1alpha1", String::new(), String::new(), 0);
        request.execution_id = "execution-1".into();
        request.attempt_id = "attempt-2".into();
        request.idempotency_key = "idempotency-1".into();
        request.request_digest = "sha256:request-digest".into();

        let success = execute_response_from_result(
            successful_task_result(),
            &ExecuteTaskIdentity::from_request(&request),
        );
        assert!(success.success);
        assert_eq!(success.execution_id, request.execution_id);
        assert_eq!(success.attempt_id, request.attempt_id);
        assert_eq!(success.idempotency_key, request.idempotency_key);
        assert_eq!(success.request_digest, request.request_digest);

        let mut failed_result = successful_task_result();
        failed_result.success = false;
        failed_result.output = None;
        failed_result.error = Some("execution failed".into());
        let failure = execute_response_from_result(
            failed_result,
            &ExecuteTaskIdentity::from_request(&request),
        );
        assert!(!failure.success);
        assert_eq!(failure.execution_id, request.execution_id);
        assert_eq!(failure.attempt_id, request.attempt_id);
        assert_eq!(failure.idempotency_key, request.idempotency_key);
        assert_eq!(failure.request_digest, request.request_digest);
    }

    #[test]
    fn worker_managed_execute_response_echoes_attempt_identity() {
        let mut request =
            execute_request("managed-function-v0", "return 42;".into(), "{}".into(), 10);
        request.execution_id = "execution-managed".into();
        request.attempt_id = "attempt-managed".into();
        request.idempotency_key = "idempotency-managed".into();
        request.request_digest = "sha256:managed-attempt".into();

        let response = execute_response_from_result(
            successful_task_result(),
            &ExecuteTaskIdentity::from_request(&request),
        );

        assert_eq!(response.execution_id, request.execution_id);
        assert_eq!(response.attempt_id, request.attempt_id);
        assert_eq!(response.idempotency_key, request.idempotency_key);
        assert_eq!(response.request_digest, request.request_digest);
    }

    #[test]
    fn worker_response_over_the_shared_output_cap_fails_closed() {
        let mut result = successful_task_result();
        result.output = Some("x".repeat(hivemind_proto::WORKER_STATUS_MESSAGE_MAX_BYTES + 1));

        let response = execute_response_from_result(
            result,
            &ExecuteTaskIdentity::from_request(&execute_request(
                "managed-function-v0",
                "return 42;".into(),
                "{}".into(),
                10,
            )),
        );

        assert!(!response.success);
        assert_eq!(
            response.status_message,
            "Task result exceeds supported response limits"
        );
    }

    #[test]
    fn worker_execute_response_forwards_typed_general_compute_result() {
        let payload = br#"{"status":"completed"}"#.to_vec();
        let mut result = successful_task_result();
        result.general_compute_result_json = Some(payload.clone());

        let response = execute_response_from_result(
            result,
            &ExecuteTaskIdentity::from_request(&execute_request(
                "general-compute-v1alpha1",
                String::new(),
                String::new(),
                0,
            )),
        );

        assert_eq!(response.general_compute_result_json, payload);
    }

    #[test]
    fn managed_execute_contract_enforces_source_input_and_budget_caps() {
        let exact = execute_request(
            "managed-function-v0",
            "s".repeat(hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES),
            "i".repeat(hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES),
            hivemind_proto::MANAGED_BUDGET_MAX_USAGE_UNITS,
        );
        let oversized_source = execute_request(
            "managed-function-v0",
            "s".repeat(hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES + 1),
            "{}".into(),
            1,
        );
        let oversized_input = execute_request(
            "managed-function-v0",
            "return 1;".into(),
            "i".repeat(hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES + 1),
            1,
        );
        let oversized_budget = execute_request(
            "managed-function-v0",
            "return 1;".into(),
            "{}".into(),
            hivemind_proto::MANAGED_BUDGET_MAX_USAGE_UNITS + 1,
        );

        assert_eq!(validate_execute_task_contract(&exact), Ok(()));
        assert_eq!(
            validate_execute_task_contract(&oversized_source),
            Err("managed-function-v0 task_source exceeds the byte limit")
        );
        assert_eq!(
            validate_execute_task_contract(&oversized_input),
            Err("managed-function-v0 JSON input exceeds the byte limit")
        );
        assert_eq!(
            validate_execute_task_contract(&oversized_budget),
            Err("managed-function-v0 budget exceeds the usage-unit limit")
        );
    }

    #[test]
    fn managed_execute_contract_rejects_blank_fields_and_nonpositive_budget() {
        let blank_source = execute_request("managed-function-v0", "".into(), "{}".into(), 1);
        let blank_input = execute_request("managed-function-v0", "return 1;".into(), "".into(), 1);
        let zero_budget =
            execute_request("managed-function-v0", "return 1;".into(), "{}".into(), 0);
        let negative_budget =
            execute_request("managed-function-v0", "return 1;".into(), "{}".into(), -1);

        assert_eq!(
            validate_execute_task_contract(&blank_source),
            Err("managed-function-v0 requires non-empty task_source")
        );
        assert_eq!(
            validate_execute_task_contract(&blank_input),
            Err("managed-function-v0 requires non-empty JSON input")
        );
        assert_eq!(
            validate_execute_task_contract(&zero_budget),
            Err("managed-function-v0 budget must be positive")
        );
        assert_eq!(
            validate_execute_task_contract(&negative_budget),
            Err("managed-function-v0 budget must be positive")
        );
    }

    #[test]
    fn execute_runtime_contract_fails_closed_without_breaking_non_managed_tasks() {
        let unsupported = execute_request("native-v1", String::new(), String::new(), 0);
        let non_managed = execute_request("", String::new(), String::new(), 0);

        assert_eq!(
            validate_execute_task_contract(&unsupported),
            Err("unsupported task runtime")
        );
        assert_eq!(validate_execute_task_contract(&non_managed), Ok(()));
    }

    #[tokio::test]
    async fn execute_task_rejects_managed_execution_without_consensus_request() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        let task_id = "managed-without-consensus";
        let mut request =
            execute_request("managed-function-v0", "return 1;".into(), "{}".into(), 1);
        request.task_id = task_id.into();
        request.token = bound_token(test_private_key_pem(), ASSIGNED_OWNER, task_id);

        let error = service
            .execute_task(Request::new(request))
            .await
            .expect_err("managed execution without consensus must fail closed");

        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            error.message(),
            "managed execution requires a consensus request"
        );
        assert!(service.report_for_task(task_id).unwrap().is_none());
    }

    #[tokio::test]
    async fn execute_task_rejects_runtime_bypass_before_recording_assignment() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        let task_id = "worker-runtime-bypass";
        let mut request = execute_request("native-v1", "return 1;".into(), "{}".into(), 1);
        request.task_id = task_id.into();
        request.token = bound_token(test_private_key_pem(), ASSIGNED_OWNER, task_id);

        let error = service
            .execute_task(Request::new(request))
            .await
            .expect_err("unsupported runtime must fail admission");

        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert_eq!(error.message(), "unsupported task runtime");
        assert!(service.report_for_task(task_id).unwrap().is_none());
    }

    #[tokio::test]
    async fn stop_task_execution_reports_not_running_for_unknown_task() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "missing-task", ASSIGNED_OWNER, None);

        let response = service
            .stop_task_execution(Request::new(StopTaskExecutionRequest {
                task_id: "missing-task".into(),
                token: bound_token(test_private_key_pem(), ASSIGNED_OWNER, "missing-task"),
                attempt_id: String::new(),
                idempotency_key: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!response.success);
        assert_eq!(response.status_message, "Task not running");
    }

    #[tokio::test]
    async fn task_output_rpc_requires_valid_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());

        let response = service
            .task_output(Request::new(TaskOutputRequest {
                task_id: "task-with-output".into(),
                token: "not-a-token".into(),
            }))
            .await;

        assert!(response.is_err());
        assert_eq!(response.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn execute_task_accepts_identity_updated_after_custom_registration() {
        let tmp = TempDir::new().unwrap();
        let service = test_service_with_successful_runner(tmp.path());
        let custom_worker_id = "e2e-custom-worker";
        let task_id = "custom-worker-execution";

        // Registration is accepted by Nodepool before the control API updates
        // the shared identity used by the gRPC admission path.
        service.state.set_worker_id(custom_worker_id);
        let mut request = execute_request("", "return 42;".into(), "{}".into(), 0);
        request.task_id = task_id.into();
        request.token = bound_token_for_worker(
            test_private_key_pem(),
            ASSIGNED_OWNER,
            task_id,
            custom_worker_id,
        );

        let response = service
            .execute_task(Request::new(request))
            .await
            .expect("custom registered worker identity should be admitted")
            .into_inner();

        assert!(response.success, "{}", response.status_message);
    }

    #[tokio::test]
    async fn execute_task_routes_a_valid_managed_gpu_request_to_the_typed_channel() {
        let tmp = TempDir::new().unwrap();
        let request = managed_gpu_request_for_execute_tests();
        let selected_gpu = managed_gpu_capability_for_execute_tests(&request);
        let typed_result = managed_gpu_result_for_execute_tests(&request, selected_gpu);
        let executor_result = typed_result.clone();
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = tmp.path().join("sandbox").to_string_lossy().to_string();
        config.auth.jwt_secret = CONTROL_PLANE_SECRET.into();
        config.auth.worker_execution_public_key_pem = test_key_pair().1.clone();
        let executor = Arc::new(WorkerExecutor::new_with_task_runner(
            config.clone(),
            move |task: hivemind_models::Task,
                  _cancellation: tokio::sync::watch::Receiver<bool>| {
                let result_json = executor_result.clone();
                async move {
                    Ok(TaskResult {
                        task_id: task.task_id,
                        success: true,
                        output: None,
                        error: None,
                        exit_code: 0,
                        cpu_time_ms: 0,
                        wall_time_ms: 1,
                        peak_memory_mb: 0,
                        managed_executed_ops: 0,
                        managed_output_bytes: 0,
                        managed_receipt_json: None,
                        general_compute_result_json: None,
                        managed_gpu_result_json: Some(result_json),
                    })
                }
            },
        ));
        let state = Arc::new(WorkerGrpcState {
            config,
            executor,
            worker_id: Arc::new(Mutex::new(Some(TEST_WORKER_ID.into()))),
            cas_store: None,
            reports: Mutex::new(HashMap::new()),
            completed_consensus_results: Arc::new(Mutex::new(HashMap::new())),
            transfer_lease_authority: Arc::new(Mutex::new(None)),
        });
        let service = GrpcWorkerNodeService::new(state.clone()).with_runtime_admission(
            managed_gpu_runtime_admission_for_execute_tests(
                &request,
                managed_gpu_capability_for_execute_tests(&request),
            ),
        );
        let task_id = "managed-gpu-execute-route";
        let token = bound_managed_gpu_token(ASSIGNED_OWNER, task_id, &request);
        let response = service
            .execute_task(Request::new(ExecuteTaskRequest {
                task_id: task_id.into(),
                runtime: MANAGED_GPU_RUNTIME_VERSION.into(),
                token,
                managed_gpu_manifest_json: serde_json::to_vec(&request).unwrap(),
                consensus_round_id: String::new(),
                replica_id: String::new(),
                consensus_protocol_version: 0,
                ..ExecuteTaskRequest::default()
            }))
            .await
            .expect("valid managed GPU request should execute through its dedicated route")
            .into_inner();

        assert!(response.success, "{}", response.status_message);
        assert_eq!(response.managed_gpu_result_json, typed_result);
        assert!(response.managed_receipt_json.is_empty());
        assert!(response.general_compute_result_json.is_empty());
        assert_eq!(response.managed_executed_ops, 0);
        assert_eq!(response.managed_output_bytes, 0);
        let reports = state.reports.lock().unwrap();
        let report = reports
            .get(&WorkerTaskKey::new(task_id, Some(&request.attempt_id)))
            .expect("GPU execution should record its attempt-bound request");
        assert_eq!(report.managed_gpu_request.as_ref(), Some(&request));
        assert!(report.general_compute_request.is_none());
    }

    #[tokio::test]
    async fn execute_task_requires_valid_token_before_running_code() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());

        let response = service
            .execute_task(Request::new(ExecuteTaskRequest {
                task_id: "unauthorized-task".into(),
                torrent: String::new(),
                resource_limits: None,
                runtime: String::new(),
                task_source: String::new(),
                token: "not-a-token".into(),
                managed_budget_units: 0,
                general_compute_manifest_json: Vec::new(),
                execution_id: String::new(),
                attempt_id: String::new(),
                idempotency_key: String::new(),
                request_digest: String::new(),
                managed_dsl_backend_id: String::new(),
                managed_dsl_semantics_manifest_sha256: String::new(),
                managed_gpu_manifest_json: Vec::new(),
                consensus_round_id: String::new(),
                replica_id: String::new(),
                consensus_protocol_version: 0,
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn worker_rpc_rejects_control_plane_tokens() {
        // Given: a worker configured with the platform public key and a control-plane HS256 token.
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        let worker_token =
            bound_token(test_private_key_pem(), ASSIGNED_OWNER, "task-worker-secret");
        let control_token =
            hmac_bound_token(CONTROL_PLANE_SECRET, ASSIGNED_OWNER, "task-control-secret");

        // When/Then: worker trust validates only the worker-execution public-key token.
        assert!(service
            .validate_worker_execution_token(&worker_token)
            .is_ok());
        let error = service
            .validate_worker_execution_token(&control_token)
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn execute_task_rejects_a_regular_user_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());

        let response = service
            .execute_task(Request::new(ExecuteTaskRequest {
                task_id: "user-token-task".into(),
                torrent: String::new(),
                resource_limits: None,
                runtime: String::new(),
                task_source: String::new(),
                token: test_user_token(test_private_key_pem(), "regular-user"),
                managed_budget_units: 0,
                general_compute_manifest_json: Vec::new(),
                execution_id: String::new(),
                attempt_id: String::new(),
                idempotency_key: String::new(),
                request_digest: String::new(),
                managed_dsl_backend_id: String::new(),
                managed_dsl_semantics_manifest_sha256: String::new(),
                managed_gpu_manifest_json: Vec::new(),
                consensus_round_id: String::new(),
                replica_id: String::new(),
                consensus_protocol_version: 0,
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn execute_task_rejects_a_token_bound_to_another_task_or_worker() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());

        let response = service
            .execute_task(Request::new(ExecuteTaskRequest {
                task_id: "requested-task".into(),
                torrent: String::new(),
                resource_limits: None,
                runtime: String::new(),
                task_source: String::new(),
                token: bound_token(test_private_key_pem(), ASSIGNED_OWNER, "different-task"),
                managed_budget_units: 0,
                general_compute_manifest_json: Vec::new(),
                execution_id: String::new(),
                attempt_id: String::new(),
                idempotency_key: String::new(),
                request_digest: String::new(),
                managed_dsl_backend_id: String::new(),
                managed_dsl_semantics_manifest_sha256: String::new(),
                managed_gpu_manifest_json: Vec::new(),
                consensus_round_id: String::new(),
                replica_id: String::new(),
                consensus_protocol_version: 0,
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn execute_task_rejects_unsafe_task_id_before_running_code() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());

        let response = service
            .execute_task(Request::new(ExecuteTaskRequest {
                task_id: "../escape".into(),
                torrent: String::new(),
                resource_limits: None,
                runtime: String::new(),
                task_source: String::new(),
                token: test_token(test_private_key_pem(), ASSIGNED_OWNER),
                managed_budget_units: 0,
                general_compute_manifest_json: Vec::new(),
                execution_id: String::new(),
                attempt_id: String::new(),
                idempotency_key: String::new(),
                request_digest: String::new(),
                managed_dsl_backend_id: String::new(),
                managed_dsl_semantics_manifest_sha256: String::new(),
                managed_gpu_manifest_json: Vec::new(),
                consensus_round_id: String::new(),
                replica_id: String::new(),
                consensus_protocol_version: 0,
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn task_output_rejects_oversized_assigned_task_id() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        let task_id = "a".repeat(hivemind_proto::TASK_ID_MAX_BYTES + 1);
        seed_assignment(&service, &task_id, ASSIGNED_OWNER, Some("private output"));

        let error = service
            .task_output(Request::new(TaskOutputRequest {
                task_id: task_id.clone(),
                token: bound_token(test_private_key_pem(), ASSIGNED_OWNER, &task_id),
            }))
            .await
            .expect_err("oversized task IDs must fail assignment-bound RPC admission");

        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert_eq!(error.message(), "unsafe task id");
    }

    #[tokio::test]
    async fn task_output_upload_rejects_a_regular_user_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "assigned-output", ASSIGNED_OWNER, None);

        let response = service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: "assigned-output".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                output: "stdout".into(),
                token: test_user_token(test_private_key_pem(), ASSIGNED_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn task_result_upload_rejects_a_regular_user_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "assigned-result", ASSIGNED_OWNER, None);

        let response = service
            .task_result_upload(Request::new(TaskResultUploadRequest {
                task_id: "assigned-result".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                result_torrent: "btih:result".into(),
                token: test_user_token(test_private_key_pem(), ASSIGNED_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn task_output_rejects_a_regular_user_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(
            &service,
            "assigned-output-read",
            ASSIGNED_OWNER,
            Some("private stdout"),
        );

        let response = service
            .task_output(Request::new(TaskOutputRequest {
                task_id: "assigned-output-read".into(),
                token: test_user_token(test_private_key_pem(), ASSIGNED_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn stop_task_execution_rejects_a_regular_user_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "assigned-stop", ASSIGNED_OWNER, None);

        let response = service
            .stop_task_execution(Request::new(StopTaskExecutionRequest {
                task_id: "assigned-stop".into(),
                token: test_user_token(test_private_key_pem(), ASSIGNED_OWNER),
                attempt_id: String::new(),
                idempotency_key: String::new(),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn task_usage_rejects_a_regular_user_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "assigned-usage", ASSIGNED_OWNER, None);

        let response = service
            .task_usage(Request::new(TaskUsageRequest {
                task_id: "assigned-usage".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                usage: Some(test_usage()),
                token: test_user_token(test_private_key_pem(), ASSIGNED_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn task_output_upload_rejects_a_token_for_another_assignment() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "owner-output", ASSIGNED_OWNER, None);

        let response = service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: "owner-output".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                output: "stdout".into(),
                token: test_token(test_private_key_pem(), OTHER_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn task_output_upload_rejects_the_wrong_worker_identity() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "worker-output", ASSIGNED_OWNER, None);

        let response = service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: "worker-output".into(),
                worker_id: "other-worker".into(),
                retry_count: 0,
                output: "stdout".into(),
                token: test_token(test_private_key_pem(), ASSIGNED_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn task_result_upload_rejects_a_token_for_another_assignment() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "owner-result", ASSIGNED_OWNER, None);

        let response = service
            .task_result_upload(Request::new(TaskResultUploadRequest {
                task_id: "owner-result".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                result_torrent: "btih:result".into(),
                token: test_token(test_private_key_pem(), OTHER_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn task_result_upload_rejects_the_wrong_worker_identity() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "worker-result", ASSIGNED_OWNER, None);

        let response = service
            .task_result_upload(Request::new(TaskResultUploadRequest {
                task_id: "worker-result".into(),
                worker_id: "other-worker".into(),
                retry_count: 0,
                result_torrent: "btih:result".into(),
                token: test_token(test_private_key_pem(), ASSIGNED_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn task_output_rejects_a_token_for_another_assignment() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(
            &service,
            "owner-output-read",
            ASSIGNED_OWNER,
            Some("private stdout"),
        );

        let response = service
            .task_output(Request::new(TaskOutputRequest {
                task_id: "owner-output-read".into(),
                token: test_token(test_private_key_pem(), OTHER_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn stop_task_execution_rejects_a_token_for_another_assignment() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "owner-stop", ASSIGNED_OWNER, None);

        let response = service
            .stop_task_execution(Request::new(StopTaskExecutionRequest {
                task_id: "owner-stop".into(),
                token: test_token(test_private_key_pem(), OTHER_OWNER),
                attempt_id: String::new(),
                idempotency_key: String::new(),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn task_usage_rejects_a_token_for_another_assignment() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "owner-usage", ASSIGNED_OWNER, None);

        let response = service
            .task_usage(Request::new(TaskUsageRequest {
                task_id: "owner-usage".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                usage: Some(test_usage()),
                token: test_token(test_private_key_pem(), OTHER_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn task_usage_rejects_the_wrong_worker_identity() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "worker-usage", ASSIGNED_OWNER, None);

        let response = service
            .task_usage(Request::new(TaskUsageRequest {
                task_id: "worker-usage".into(),
                worker_id: "other-worker".into(),
                retry_count: 0,
                usage: Some(test_usage()),
                token: test_token(test_private_key_pem(), ASSIGNED_OWNER),
            }))
            .await;

        assert_eq!(response.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn task_output_upload_and_retrieval_round_trip() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "task-with-output", ASSIGNED_OWNER, None);
        let token = bound_token(test_private_key_pem(), ASSIGNED_OWNER, "task-with-output");

        let uploaded = service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: "task-with-output".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                output: "stdout body".into(),
                token: token.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(uploaded.success, "{}", uploaded.status_message);

        let response = service
            .task_output(Request::new(TaskOutputRequest {
                task_id: "task-with-output".into(),
                token,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(response.success, "{}", response.status_message);
        assert_eq!(response.output, "stdout body");
    }

    #[tokio::test]
    async fn task_reports_are_isolated_by_attempt_identity() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        let task_id = "attempt-isolated-reports";
        service
            .record_task_assignment_for_attempt(task_id, ASSIGNED_OWNER, Some("attempt-a"))
            .unwrap();
        service
            .record_task_assignment_for_attempt(task_id, ASSIGNED_OWNER, Some("attempt-b"))
            .unwrap();
        service
            .report_for_update_for_key(&WorkerTaskKey::new(task_id, Some("attempt-a")), |report| {
                report.output = Some("output-a".into());
            })
            .unwrap();
        service
            .report_for_update_for_key(&WorkerTaskKey::new(task_id, Some("attempt-b")), |report| {
                report.output = Some("output-b".into());
            })
            .unwrap();

        for (attempt_id, expected_output) in [("attempt-a", "output-a"), ("attempt-b", "output-b")]
        {
            let response = service
                .task_output(Request::new(TaskOutputRequest {
                    task_id: task_id.into(),
                    token: bound_attempt_token(ASSIGNED_OWNER, task_id, attempt_id),
                }))
                .await
                .unwrap()
                .into_inner();
            assert!(response.success, "{}", response.status_message);
            assert_eq!(response.output, expected_output);
        }
    }

    #[tokio::test]
    async fn result_upload_and_usage_reporting_accept_valid_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "task-with-result", ASSIGNED_OWNER, None);
        let token = bound_token(test_private_key_pem(), ASSIGNED_OWNER, "task-with-result");

        let result = service
            .task_result_upload(Request::new(TaskResultUploadRequest {
                task_id: "task-with-result".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                result_torrent: "btih:result-ref".into(),
                token: token.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(result.success, "{}", result.status_message);

        let usage = service
            .task_usage(Request::new(TaskUsageRequest {
                task_id: "task-with-result".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                usage: Some(hivemind_proto::ResourceUsage {
                    cpu_percent: 12.5,
                    memory_percent: 34.5,
                    gpu_percent: 0.0,
                    vram_percent: 0.0,
                    storage_percent: 1.0,
                }),
                token,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(usage.success, "{}", usage.status_message);
    }

    #[tokio::test]
    async fn task_output_upload_rejects_oversized_output() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "oversized-output", ASSIGNED_OWNER, None);
        let token = bound_token(test_private_key_pem(), ASSIGNED_OWNER, "oversized-output");

        let uploaded = service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: "oversized-output".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                output: "x".repeat(MAX_TASK_OUTPUT_BYTES + 1),
                token,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!uploaded.success);
        assert!(uploaded.status_message.contains("byte limit"));
    }

    #[tokio::test]
    async fn task_usage_rejects_non_finite_values() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "bad-usage", ASSIGNED_OWNER, None);
        let token = bound_token(test_private_key_pem(), ASSIGNED_OWNER, "bad-usage");

        let usage = service
            .task_usage(Request::new(TaskUsageRequest {
                task_id: "bad-usage".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                usage: Some(hivemind_proto::ResourceUsage {
                    cpu_percent: f32::NAN,
                    memory_percent: 0.0,
                    gpu_percent: 0.0,
                    vram_percent: 0.0,
                    storage_percent: 0.0,
                }),
                token,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!usage.success);
        assert!(usage.status_message.contains("non-finite"));
    }

    #[tokio::test]
    async fn task_usage_rejects_missing_usage_payload() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "missing-usage", ASSIGNED_OWNER, None);
        let token = bound_token(test_private_key_pem(), ASSIGNED_OWNER, "missing-usage");

        let usage = service
            .task_usage(Request::new(TaskUsageRequest {
                task_id: "missing-usage".into(),
                worker_id: TEST_WORKER_ID.into(),
                retry_count: 0,
                usage: None,
                token,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!usage.success);
        assert!(usage.status_message.contains("Usage payload is required"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stop_task_execution_rpc_cancels_running_managed_function() {
        // Bounded managed limits make every real managed function finish in
        // milliseconds, so cancellation is asserted deterministically with an
        // injected runner that only returns once cancellation is observed.
        let tmp = TempDir::new().unwrap();
        let service = Arc::new(test_service_with_cancellable_runner(tmp.path()));
        let task_id = "grpc-stop-managed-function".to_string();
        let execution_id = "execution-stop-managed";
        let attempt_id = "attempt-stop-managed";
        let idempotency_key = "idempotency-stop-managed";
        let request_digest = "sha256:stop-managed";
        let consensus_round_id = "round-stop-managed";
        let replica_id = "replica-stop-managed";
        let token = bound_consensus_token(
            ASSIGNED_OWNER,
            &task_id,
            execution_id,
            attempt_id,
            idempotency_key,
            request_digest,
            consensus_round_id,
            replica_id,
        );
        let execute_service = service.clone();
        let execute_token = token.clone();
        let execute_task_id = task_id.clone();
        let execute = tokio::spawn(async move {
            execute_service
                .execute_task(Request::new(ExecuteTaskRequest {
                    task_id: execute_task_id.clone(),
                    torrent: "null".into(),
                    resource_limits: Some(ResourceSpec {
                        cpu_cores: 1,
                        memory_mb: 1024,
                        gpu_count: 0,
                        gpu_name: String::new(),
                        vram_mb: 0,
                        cpu_score: 1,
                        gpu_score: 0,
                        storage_total_gb: 1,
                        storage_available_gb: 1,
                    }),
                    runtime: "managed-function-v0".into(),
                    task_source: "return 1;".into(),
                    token: execute_token,
                    managed_budget_units: hivemind_proto::MANAGED_BUDGET_MAX_USAGE_UNITS,
                    general_compute_manifest_json: Vec::new(),
                    execution_id: execution_id.into(),
                    attempt_id: attempt_id.into(),
                    idempotency_key: idempotency_key.into(),
                    request_digest: request_digest.into(),
                    managed_dsl_backend_id: String::new(),
                    managed_dsl_semantics_manifest_sha256: String::new(),
                    managed_gpu_manifest_json: Vec::new(),
                    consensus_round_id: consensus_round_id.into(),
                    replica_id: replica_id.into(),
                    consensus_protocol_version: 1,
                }))
                .await
                .unwrap()
                .into_inner()
        });

        // Poll instead of sleeping a fixed interval so the stop request never
        // races task registration.
        let mut stop = None;
        for _ in 0..600 {
            match service
                .stop_task_execution(Request::new(StopTaskExecutionRequest {
                    task_id: task_id.clone(),
                    token: token.clone(),
                    attempt_id: attempt_id.into(),
                    idempotency_key: idempotency_key.into(),
                }))
                .await
            {
                Ok(response) => {
                    let attempt = response.into_inner();
                    if attempt.success {
                        stop = Some(attempt);
                        break;
                    }
                }
                // `execute_task` records the task assignment as part of the
                // request, so a stop that arrives first is rejected as an
                // unauthorized assignment rather than an unknown task. Treat
                // that exactly like a not-yet-running task and keep polling.
                Err(status) if status.code() == tonic::Code::PermissionDenied => {}
                Err(status) => panic!("stop_task_execution should not fail: {status:?}"),
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let stop = stop.expect("stop_task_execution should observe the running task");

        assert!(stop.success);
        assert_eq!(stop.status_message, "Stop requested");
        let execute_response = tokio::time::timeout(Duration::from_secs(10), execute)
            .await
            .expect("execute_task should return after stop")
            .expect("execute_task join should succeed");
        assert!(!execute_response.success);
        assert!(execute_response
            .status_message
            .contains("Task execution stopped"));
    }

    #[tokio::test]
    async fn chunk_service_accepts_only_an_assigned_verified_attempt_and_replays_idempotently() {
        let tmp = TempDir::new().unwrap();
        let cas_root = TempDir::new().unwrap();
        let store = Arc::new(CasChunkStore::new(cas_root.path()).unwrap());
        let (worker, chunk_service) = chunk_test_components(tmp.path(), Some(store.clone()));
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);
        admit_general_compute_request(&worker, &request, &token, "chunk-task").await;
        let upload = general_compute_upload(&request, &token, b"print(42)");

        let first = chunk_service
            .upload_chunk(Request::new(upload.clone()))
            .await
            .expect("assigned chunk should be accepted")
            .into_inner();
        assert!(first.success);
        let replay = chunk_service
            .upload_chunk(Request::new(upload))
            .await
            .expect("identical chunk replay should be accepted")
            .into_inner();
        assert!(replay.success);
        assert_eq!(
            std::fs::read(store.chunk_path(&sha256_digest(b"print(42)")).unwrap()).unwrap(),
            b"print(42)"
        );

        let resume = GeneralComputeChunkResumeRequest {
            token: token.clone(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            artifact_id: "source".into(),
            completed_sha256: vec![sha256_digest(b"print(42)")],
            transfer_generation: 1,
        };
        let resumed = chunk_service
            .resume_chunks(Request::new(resume))
            .await
            .expect("resume should inspect the operator CAS")
            .into_inner();
        assert!(resumed.success);
        assert!(resumed.missing_chunks.is_empty());
    }

    #[tokio::test]
    async fn chunk_service_rejects_a_token_for_another_assignment() {
        let tmp = TempDir::new().unwrap();
        let cas_root = TempDir::new().unwrap();
        let store = Arc::new(CasChunkStore::new(cas_root.path()).unwrap());
        let (worker, chunk_service) = chunk_test_components(tmp.path(), Some(store));
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);
        admit_general_compute_request(&worker, &request, &token, "chunk-task").await;

        let wrong_token = bound_general_compute_token(ASSIGNED_OWNER, "other-task", &request);
        let status = chunk_service
            .upload_chunk(Request::new(general_compute_upload(
                &request,
                &wrong_token,
                b"print(42)",
            )))
            .await
            .expect_err("a token for another assignment must be rejected");
        assert_eq!(status.code(), Code::PermissionDenied);
    }

    #[tokio::test]
    async fn chunk_service_rejects_an_attempt_or_digest_not_bound_to_the_assignment() {
        let tmp = TempDir::new().unwrap();
        let cas_root = TempDir::new().unwrap();
        let store = Arc::new(CasChunkStore::new(cas_root.path()).unwrap());
        let (worker, chunk_service) = chunk_test_components(tmp.path(), Some(store));
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);
        admit_general_compute_request(&worker, &request, &token, "chunk-task").await;

        let mut stale = general_compute_upload(&request, &token, b"print(42)");
        stale.attempt_id = "attempt-stale".into();
        let status = chunk_service
            .upload_chunk(Request::new(stale))
            .await
            .expect_err("a stale attempt must be rejected");
        assert_eq!(status.code(), Code::PermissionDenied);

        let mut wrong_digest = general_compute_upload(&request, &token, b"print(42)");
        wrong_digest.request_digest = format!("sha256:{}", "b".repeat(64));
        let status = chunk_service
            .upload_chunk(Request::new(wrong_digest))
            .await
            .expect_err("a stale request digest must be rejected");
        assert_eq!(status.code(), Code::PermissionDenied);
    }

    #[tokio::test]
    async fn chunk_service_requires_an_admitted_general_compute_request() {
        let tmp = TempDir::new().unwrap();
        let cas_root = TempDir::new().unwrap();
        let store = Arc::new(CasChunkStore::new(cas_root.path()).unwrap());
        let (worker, chunk_service) = chunk_test_components(tmp.path(), Some(store));
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);
        worker
            .record_task_assignment("chunk-task", ASSIGNED_OWNER)
            .expect("assignment seed should succeed");

        let status = chunk_service
            .upload_chunk(Request::new(general_compute_upload(
                &request,
                &token,
                b"print(42)",
            )))
            .await
            .expect_err("chunk upload must require an admitted request");
        assert_eq!(status.code(), Code::PermissionDenied);
    }

    #[tokio::test]
    async fn chunk_service_fails_closed_when_operator_cas_is_unavailable() {
        let tmp = TempDir::new().unwrap();
        let (worker, chunk_service) = chunk_test_components(tmp.path(), None);
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);
        admit_general_compute_request(&worker, &request, &token, "chunk-task").await;

        let status = chunk_service
            .upload_chunk(Request::new(general_compute_upload(
                &request,
                &token,
                b"print(42)",
            )))
            .await
            .expect_err("chunk upload must fail closed without an operator CAS");
        assert_eq!(status.code(), Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn prepare_general_compute_records_admission_before_chunk_transfer() {
        let tmp = TempDir::new().unwrap();
        let cas_root = TempDir::new().unwrap();
        let store = Arc::new(CasChunkStore::new(cas_root.path()).unwrap());
        let (_worker, chunk_service) = chunk_test_components(tmp.path(), Some(store));
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);

        let prepared = chunk_service
            .prepare_general_compute(Request::new(general_compute_prepare_request(
                &request,
                &token,
                "chunk-task",
            )))
            .await
            .expect("prepare should admit the request")
            .into_inner();
        assert!(prepared.success);

        let uploaded = chunk_service
            .upload_chunk(Request::new(general_compute_upload(
                &request,
                &token,
                b"print(42)",
            )))
            .await
            .expect("prepared assignment should accept chunks")
            .into_inner();
        assert!(uploaded.success);
    }

    #[tokio::test]
    async fn execute_task_cannot_replace_a_prepared_general_compute_request() {
        let tmp = TempDir::new().unwrap();
        let (worker, chunk_service) = chunk_test_components(tmp.path(), None);
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);
        chunk_service
            .prepare_general_compute(Request::new(general_compute_prepare_request(
                &request,
                &token,
                "chunk-task",
            )))
            .await
            .expect("prepare should admit the request");

        let mut replacement = request.clone();
        replacement.attempt_id = "attempt-replacement".into();
        replacement.request_digest = replacement.canonical_request_digest();
        let status = worker
            .execute_task(Request::new(general_compute_execute_request(
                &replacement,
                &token,
                "chunk-task",
            )))
            .await
            .expect_err("prepared request identity must not be replaced");
        assert_eq!(status.code(), Code::PermissionDenied);
    }

    #[tokio::test]
    async fn execute_task_rejects_a_stale_transfer_generation_for_a_prepared_request() {
        let tmp = TempDir::new().unwrap();
        let (worker, chunk_service) = chunk_test_components(tmp.path(), None);
        let request = general_compute_request_for_chunk_tests();
        let current_token =
            bound_general_compute_token_with_generation(ASSIGNED_OWNER, "chunk-task", &request, 2);
        let mut prepare = general_compute_prepare_request(&request, &current_token, "chunk-task");
        prepare.transfer_generation = 2;
        chunk_service
            .prepare_general_compute(Request::new(prepare))
            .await
            .expect("current transfer generation should be prepared");

        let stale_token =
            bound_general_compute_token_with_generation(ASSIGNED_OWNER, "chunk-task", &request, 1);
        let status = worker
            .execute_task(Request::new(general_compute_execute_request(
                &request,
                &stale_token,
                "chunk-task",
            )))
            .await
            .expect_err("a stale execution token must not replace the prepared lease generation");
        assert_eq!(status.code(), Code::PermissionDenied);

        let reports = worker.state.reports.lock().unwrap();
        assert_eq!(
            reports
                .get(&WorkerTaskKey::new("chunk-task", Some(&request.attempt_id),))
                .and_then(|report| report.transfer_generation),
            Some(2)
        );
    }

    #[tokio::test]
    async fn reassignment_revokes_old_worker_before_chunk_replay_and_allows_new_worker() {
        let tmp_a = TempDir::new().unwrap();
        let tmp_b = TempDir::new().unwrap();
        let cas_root_a = TempDir::new().unwrap();
        let cas_root_b = TempDir::new().unwrap();
        let authority = Arc::new(MockTransferLeaseAuthority::new(
            "chunk-task",
            &general_compute_request_for_chunk_tests(),
            "worker-a",
            1,
        ));
        let request_a = general_compute_request_for_chunk_tests();
        let mut request_b = request_a.clone();
        request_b.attempt_id = "attempt-worker-b".into();
        request_b.request_digest = request_b.canonical_request_digest();
        let (worker_a, service_a) = chunk_test_components_for(
            tmp_a.path(),
            Some(Arc::new(CasChunkStore::new(cas_root_a.path()).unwrap())),
            "worker-a",
            authority.clone(),
        );
        let token_a = bound_general_compute_token_for_worker(
            ASSIGNED_OWNER,
            "chunk-task",
            &request_a,
            "worker-a",
            1,
        );
        service_a
            .prepare_general_compute(Request::new(general_compute_prepare_request_for(
                &request_a,
                &token_a,
                "chunk-task",
                1,
            )))
            .await
            .expect("worker A should accept generation 1 while assigned");

        authority.reassign(&request_b, "worker-b", 2);
        let stale = service_a
            .upload_chunk(Request::new(general_compute_upload_for(
                &request_a, &token_a, 1,
            )))
            .await
            .expect_err("worker A must fail closed after Nodepool reassignment");
        assert_eq!(stale.code(), Code::PermissionDenied);

        let (_worker_b, service_b) = chunk_test_components_for(
            tmp_b.path(),
            Some(Arc::new(CasChunkStore::new(cas_root_b.path()).unwrap())),
            "worker-b",
            authority,
        );
        let token_b = bound_general_compute_token_for_worker(
            ASSIGNED_OWNER,
            "chunk-task",
            &request_b,
            "worker-b",
            2,
        );
        let prepared = service_b
            .prepare_general_compute(Request::new(general_compute_prepare_request_for(
                &request_b,
                &token_b,
                "chunk-task",
                2,
            )))
            .await
            .expect("worker B should accept the replacement generation")
            .into_inner();
        assert!(prepared.success);
        let uploaded = service_b
            .upload_chunk(Request::new(general_compute_upload_for(
                &request_b, &token_b, 2,
            )))
            .await
            .expect("worker B should accept the active generation")
            .into_inner();
        assert!(uploaded.success);
        drop(worker_a);
    }

    #[tokio::test]
    async fn execute_task_requires_the_token_to_match_the_general_compute_attempt() {
        let tmp = TempDir::new().unwrap();
        let (worker, _chunk_service) = chunk_test_components(tmp.path(), None);
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);
        let mut replacement = request.clone();
        replacement.attempt_id = "attempt-replacement".into();
        replacement.request_digest = replacement.canonical_request_digest();

        let status = worker
            .execute_task(Request::new(general_compute_execute_request(
                &replacement,
                &token,
                "chunk-task",
            )))
            .await
            .expect_err("ExecuteTask must enforce the token-bound attempt identity");
        assert_eq!(status.code(), Code::PermissionDenied);
    }

    #[tokio::test]
    async fn prepare_general_compute_rejects_a_token_bound_to_another_attempt() {
        let tmp = TempDir::new().unwrap();
        let (_worker, chunk_service) = chunk_test_components(tmp.path(), None);
        let request = general_compute_request_for_chunk_tests();
        let token = bound_general_compute_token(ASSIGNED_OWNER, "chunk-task", &request);
        let mut replacement = request.clone();
        replacement.attempt_id = "attempt-replacement".into();
        replacement.request_digest = replacement.canonical_request_digest();

        let status = chunk_service
            .prepare_general_compute(Request::new(general_compute_prepare_request(
                &replacement,
                &token,
                "chunk-task",
            )))
            .await
            .expect_err("prepare must require token-bound attempt identity");
        assert_eq!(status.code(), Code::PermissionDenied);
    }

    #[tokio::test]
    async fn stop_task_execution_requires_attempt_bound_idempotency_key() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        let task_id = "attempt-bound-stop";
        let request = general_compute_request_for_chunk_tests();
        service
            .record_task_assignment_for_attempt(task_id, ASSIGNED_OWNER, Some(&request.attempt_id))
            .unwrap();
        let token = bound_general_compute_token(ASSIGNED_OWNER, task_id, &request);

        let mismatched = service
            .stop_task_execution(Request::new(StopTaskExecutionRequest {
                task_id: task_id.into(),
                token: token.clone(),
                attempt_id: request.attempt_id.clone(),
                idempotency_key: "wrong-idempotency".into(),
            }))
            .await
            .expect_err("stop must reject an idempotency key not bound to the token");
        assert_eq!(mismatched.code(), Code::PermissionDenied);

        let accepted = service
            .stop_task_execution(Request::new(StopTaskExecutionRequest {
                task_id: task_id.into(),
                token,
                attempt_id: request.attempt_id,
                idempotency_key: request.idempotency_key,
            }))
            .await
            .expect("matching attempt identity should pass authorization")
            .into_inner();
        assert!(!accepted.success, "the test has no active executor");
        assert_eq!(accepted.status_message, "Task not running");
    }

    fn seed_assignment(
        service: &GrpcWorkerNodeService,
        task_id: &str,
        owner: &str,
        output: Option<&str>,
    ) {
        service.record_task_assignment(task_id, owner).unwrap();
        service
            .report_for_update(task_id, |report| {
                report.output = output.map(str::to_owned);
            })
            .unwrap();
    }

    fn test_usage() -> hivemind_proto::ResourceUsage {
        hivemind_proto::ResourceUsage {
            cpu_percent: 12.5,
            memory_percent: 34.5,
            gpu_percent: 0.0,
            vram_percent: 0.0,
            storage_percent: 1.0,
        }
    }

    fn successful_task_result() -> TaskResult {
        TaskResult {
            task_id: "worker-result".into(),
            success: true,
            output: Some("42".into()),
            error: None,
            exit_code: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            managed_executed_ops: 17,
            managed_output_bytes: 2,
            managed_receipt_json: Some("{}".into()),
            general_compute_result_json: None,
            managed_gpu_result_json: None,
        }
    }

    fn test_service_with_successful_runner(base: &std::path::Path) -> GrpcWorkerNodeService {
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = base.join("sandbox").to_string_lossy().to_string();
        config.auth.jwt_secret = CONTROL_PLANE_SECRET.into();
        config.auth.worker_execution_public_key_pem = test_key_pair().1.clone();
        let executor = Arc::new(WorkerExecutor::new_with_task_runner(
            config.clone(),
            |task: hivemind_models::Task, _cancellation: tokio::sync::watch::Receiver<bool>| async move {
                let mut result = successful_task_result();
                result.task_id = task.task_id;
                Ok(result)
            },
        ));
        GrpcWorkerNodeService::new(Arc::new(WorkerGrpcState {
            config,
            executor,
            worker_id: Arc::new(Mutex::new(Some(TEST_WORKER_ID.into()))),
            cas_store: None,
            reports: Mutex::new(HashMap::new()),
            completed_consensus_results: Arc::new(Mutex::new(HashMap::new())),
            transfer_lease_authority: Arc::new(Mutex::new(None)),
        }))
    }

    fn test_service_with_cancellable_runner(base: &std::path::Path) -> GrpcWorkerNodeService {
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = base.join("sandbox").to_string_lossy().to_string();
        config.auth.jwt_secret = CONTROL_PLANE_SECRET.into();
        config.auth.worker_execution_public_key_pem = test_key_pair().1.clone();
        let executor = Arc::new(WorkerExecutor::new_with_task_runner(
            config.clone(),
            |task: hivemind_models::Task, mut cancellation: tokio::sync::watch::Receiver<bool>| async move {
                while !*cancellation.borrow() {
                    if cancellation.changed().await.is_err() {
                        break;
                    }
                }
                Ok(crate::TaskResult {
                    task_id: task.task_id.clone(),
                    success: false,
                    output: None,
                    error: Some("Task execution stopped".into()),
                    exit_code: 1,
                    cpu_time_ms: 0,
                    wall_time_ms: 0,
                    peak_memory_mb: 0,
                    managed_executed_ops: 0,
                    managed_output_bytes: 0,
                    managed_receipt_json: None,
                    general_compute_result_json: None,
                    managed_gpu_result_json: None,
                })
            },
        ));
        GrpcWorkerNodeService::new(Arc::new(WorkerGrpcState {
            config,
            executor,
            worker_id: Arc::new(Mutex::new(Some(TEST_WORKER_ID.into()))),
            cas_store: None,
            reports: Mutex::new(HashMap::new()),
            completed_consensus_results: Arc::new(Mutex::new(HashMap::new())),
            transfer_lease_authority: Arc::new(Mutex::new(None)),
        }))
    }

    fn test_service(base: &std::path::Path) -> GrpcWorkerNodeService {
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = base.join("sandbox").to_string_lossy().to_string();
        config.auth.jwt_secret = CONTROL_PLANE_SECRET.into();
        config.auth.worker_execution_public_key_pem = test_key_pair().1.clone();
        let executor = Arc::new(WorkerExecutor::new(config.clone()));
        GrpcWorkerNodeService::new(Arc::new(WorkerGrpcState {
            config,
            executor,
            worker_id: Arc::new(Mutex::new(Some(TEST_WORKER_ID.into()))),
            cas_store: None,
            reports: Mutex::new(HashMap::new()),
            completed_consensus_results: Arc::new(Mutex::new(HashMap::new())),
            transfer_lease_authority: Arc::new(Mutex::new(None)),
        }))
    }

    fn test_token(_private_key_pem: &str, subject: &str) -> String {
        test_token_with_role(subject, Some("worker-execution"))
    }

    fn bound_token(_private_key_pem: &str, subject: &str, task_id: &str) -> String {
        bound_token_for_worker(_private_key_pem, subject, task_id, TEST_WORKER_ID)
    }

    fn bound_token_for_worker(
        _private_key_pem: &str,
        subject: &str,
        task_id: &str,
        worker_id: &str,
    ) -> String {
        WorkerExecutionSigner::from_pem(test_private_key_pem())
            .unwrap()
            .encode_claims(&Claims {
                sub: subject.into(),
                user_id: subject.into(),
                role: Some("worker-execution".into()),
                task_id: Some(task_id.into()),
                worker_id: Some(worker_id.into()),
                exp: (Utc::now().timestamp() + 3600) as usize,
                iat: Utc::now().timestamp() as usize,
            })
            .unwrap()
    }

    fn bound_attempt_token(subject: &str, task_id: &str, attempt_id: &str) -> String {
        let now = Utc::now().timestamp();
        WorkerExecutionSigner::from_pem(test_private_key_pem())
            .unwrap()
            .encode_attempt_claims(
                &Claims {
                    sub: subject.into(),
                    user_id: subject.into(),
                    role: Some("worker-execution".into()),
                    task_id: Some(task_id.into()),
                    worker_id: Some(TEST_WORKER_ID.into()),
                    exp: (now + 3600) as usize,
                    iat: now as usize,
                },
                attempt_id,
            )
            .unwrap()
    }

    fn bound_consensus_token(
        subject: &str,
        task_id: &str,
        execution_id: &str,
        attempt_id: &str,
        idempotency_key: &str,
        request_digest: &str,
        consensus_round_id: &str,
        replica_id: &str,
    ) -> String {
        let now = Utc::now().timestamp();
        WorkerExecutionSigner::from_pem(test_private_key_pem())
            .unwrap()
            .encode_consensus_claims(
                &Claims {
                    sub: subject.into(),
                    user_id: subject.into(),
                    role: Some("worker-execution".into()),
                    task_id: Some(task_id.into()),
                    worker_id: Some(TEST_WORKER_ID.into()),
                    exp: (now + 3600) as usize,
                    iat: now as usize,
                },
                &WorkerExecutionIdentity {
                    execution_id: execution_id.into(),
                    attempt_id: attempt_id.into(),
                    idempotency_key: idempotency_key.into(),
                    request_digest: request_digest.into(),
                    transfer_generation: 1,
                },
                consensus_round_id,
                replica_id,
                1,
            )
            .unwrap()
    }

    fn bound_managed_gpu_token(
        subject: &str,
        task_id: &str,
        request: &ManagedGpuRequest,
    ) -> String {
        let now = Utc::now().timestamp();
        WorkerExecutionSigner::from_pem(test_private_key_pem())
            .unwrap()
            .encode_execution_claims(
                &Claims {
                    sub: subject.into(),
                    user_id: subject.into(),
                    role: Some("worker-execution".into()),
                    task_id: Some(task_id.into()),
                    worker_id: Some(TEST_WORKER_ID.into()),
                    exp: (now + 3600) as usize,
                    iat: now as usize,
                },
                &WorkerExecutionIdentity {
                    execution_id: request.execution_id.clone(),
                    attempt_id: request.attempt_id.clone(),
                    idempotency_key: request.idempotency_key.clone(),
                    request_digest: request.request_digest.clone(),
                    transfer_generation: 1,
                },
            )
            .unwrap()
    }
    fn bound_general_compute_token(
        subject: &str,
        task_id: &str,
        request: &GeneralComputeRequest,
    ) -> String {
        bound_general_compute_token_with_generation(subject, task_id, request, 1)
    }

    fn bound_general_compute_token_with_generation(
        subject: &str,
        task_id: &str,
        request: &GeneralComputeRequest,
        transfer_generation: i64,
    ) -> String {
        bound_general_compute_token_for_worker(
            subject,
            task_id,
            request,
            TEST_WORKER_ID,
            transfer_generation,
        )
    }

    fn bound_general_compute_token_for_worker(
        subject: &str,
        task_id: &str,
        request: &GeneralComputeRequest,
        worker_id: &str,
        transfer_generation: i64,
    ) -> String {
        let now = Utc::now().timestamp();
        WorkerExecutionSigner::from_pem(test_private_key_pem())
            .unwrap()
            .encode_execution_claims(
                &Claims {
                    sub: subject.into(),
                    user_id: subject.into(),
                    role: Some("worker-execution".into()),
                    task_id: Some(task_id.into()),
                    worker_id: Some(worker_id.into()),
                    exp: (now + 3600) as usize,
                    iat: now as usize,
                },
                &WorkerExecutionIdentity {
                    execution_id: request.execution_id.clone(),
                    attempt_id: request.attempt_id.clone(),
                    idempotency_key: request.idempotency_key.clone(),
                    request_digest: request.request_digest.clone(),
                    transfer_generation,
                },
            )
            .unwrap()
    }

    fn general_compute_upload_for(
        request: &GeneralComputeRequest,
        token: &str,
        transfer_generation: i64,
    ) -> GeneralComputeChunkUpload {
        GeneralComputeChunkUpload {
            token: token.into(),
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
            artifact_id: "source".into(),
            offset: 0,
            size_bytes: b"print(42)".len() as i64,
            sha256: sha256_digest(b"print(42)"),
            bytes: b"print(42)".to_vec(),
            transfer_generation,
        }
    }

    struct AllowLocalTransferLeaseAuthority;

    #[tonic::async_trait]
    impl TransferLeaseAuthority for AllowLocalTransferLeaseAuthority {
        async fn validate(
            &self,
            _token: &str,
            _worker_id: &str,
            _task_id: &str,
            _execution_id: &str,
            _attempt_id: &str,
            _transfer_generation: i64,
            _idempotency_key: &str,
            _request_digest: &str,
        ) -> Result<(), TransferLeaseAuthorityError> {
            Ok(())
        }
    }

    struct MockTransferLeaseAuthority {
        current: Mutex<MockTransferLease>,
    }

    struct MockTransferLease {
        task_id: String,
        execution_id: String,
        attempt_id: String,
        worker_id: String,
        generation: i64,
    }

    impl MockTransferLeaseAuthority {
        fn new(
            task_id: &str,
            request: &GeneralComputeRequest,
            worker_id: &str,
            generation: i64,
        ) -> Self {
            Self {
                current: Mutex::new(MockTransferLease {
                    task_id: task_id.into(),
                    execution_id: request.execution_id.clone(),
                    attempt_id: request.attempt_id.clone(),
                    worker_id: worker_id.into(),
                    generation,
                }),
            }
        }

        fn reassign(&self, request: &GeneralComputeRequest, worker_id: &str, generation: i64) {
            let mut current = self.current.lock().unwrap();
            current.execution_id = request.execution_id.clone();
            current.attempt_id = request.attempt_id.clone();
            current.worker_id = worker_id.into();
            current.generation = generation;
        }
    }

    #[tonic::async_trait]
    impl TransferLeaseAuthority for MockTransferLeaseAuthority {
        async fn validate(
            &self,
            _token: &str,
            worker_id: &str,
            task_id: &str,
            execution_id: &str,
            attempt_id: &str,
            transfer_generation: i64,
            _idempotency_key: &str,
            _request_digest: &str,
        ) -> Result<(), TransferLeaseAuthorityError> {
            let current = self.current.lock().unwrap();
            if current.task_id == task_id
                && current.execution_id == execution_id
                && current.attempt_id == attempt_id
                && current.worker_id == worker_id
                && current.generation == transfer_generation
            {
                Ok(())
            } else {
                Err(TransferLeaseAuthorityError::Denied(
                    "transfer lease is no longer active".into(),
                ))
            }
        }
    }

    fn hmac_bound_token(secret: &str, subject: &str, task_id: &str) -> String {
        jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &Claims {
                sub: subject.into(),
                user_id: subject.into(),
                role: Some("worker-execution".into()),
                task_id: Some(task_id.into()),
                worker_id: Some(TEST_WORKER_ID.into()),
                exp: (Utc::now().timestamp() + 3600) as usize,
                iat: Utc::now().timestamp() as usize,
            },
            &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    fn test_user_token(_private_key_pem: &str, subject: &str) -> String {
        // Regular user tokens remain HS256 control-plane credentials and must not authorize RPCs.
        jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &Claims {
                sub: subject.into(),
                user_id: uuid::Uuid::new_v4().to_string(),
                role: None,
                task_id: None,
                worker_id: None,
                exp: (Utc::now().timestamp() + 3600) as usize,
                iat: Utc::now().timestamp() as usize,
            },
            &jsonwebtoken::EncodingKey::from_secret(CONTROL_PLANE_SECRET.as_bytes()),
        )
        .unwrap()
    }

    fn test_token_with_role(subject: &str, role: Option<&str>) -> String {
        WorkerExecutionSigner::from_pem(test_private_key_pem())
            .unwrap()
            .encode_claims(&Claims {
                sub: subject.into(),
                user_id: uuid::Uuid::new_v4().to_string(),
                role: role.map(str::to_owned),
                task_id: None,
                worker_id: None,
                exp: (Utc::now().timestamp() + 3600) as usize,
                iat: Utc::now().timestamp() as usize,
            })
            .unwrap()
    }
}

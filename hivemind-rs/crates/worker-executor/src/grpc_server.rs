use hivemind_auth::worker_execution::WorkerExecutionVerifier;
use hivemind_models::Claims;
use hivemind_proto::{
    general_compute_chunk_service_server::GeneralComputeChunkService,
    node_manager_service_client::NodeManagerServiceClient,
    worker_node_service_server::WorkerNodeService, ExecuteTaskRequest, ExecuteTaskResponse,
    GeneralComputeChunkDescriptor, GeneralComputeChunkResumeRequest,
    GeneralComputeChunkResumeResponse, GeneralComputeChunkUpload,
    GeneralComputeChunkUploadResponse, GeneralComputePrepareRequest, GeneralComputePrepareResponse,
    StopTaskExecutionRequest, StopTaskExecutionResponse, TaskOutputRequest, TaskOutputResponse,
    TaskOutputUploadRequest, TaskOutputUploadResponse, TaskResultUploadRequest,
    TaskResultUploadResponse, TaskUsageRequest, TaskUsageResponse,
    GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES, GENERAL_COMPUTE_RESULT_MAX_BYTES,
    LEGACY_MANAGED_RECEIPT_MAX_BYTES, MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES,
    WORKER_RPC_MESSAGE_MAX_BYTES, WORKER_STATUS_MESSAGE_MAX_BYTES,
};
use prost::Message;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tonic::{Request, Response, Status};

use crate::{
    managed_prover::ManagedProverError, runtime_admission::WorkerRuntimeAdmission, StopTaskOutcome,
    TaskResult, WorkerExecutor,
};
use general_compute_runtime::artifact::CasChunkStore;
use general_compute_runtime::GeneralComputeRequest;
use hivemind_config::HivemindConfig;
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
    endpoint: String,
}

impl NodepoolTransferLeaseAuthority {
    #[must_use]
    pub fn new(endpoint: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            endpoint: crate::nodepool_client::nodepool_endpoint(&endpoint.into()),
        })
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
        let mut client = NodeManagerServiceClient::connect(self.endpoint.clone())
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
pub struct WorkerGrpcState {
    pub config: HivemindConfig,
    pub executor: Arc<WorkerExecutor>,
    worker_id: Option<String>,
    cas_store: Option<Arc<CasChunkStore>>,
    reports: Mutex<HashMap<String, WorkerTaskReport>>,
    transfer_lease_authority: Arc<Mutex<Option<Arc<dyn TransferLeaseAuthority>>>>,
}

#[derive(Clone)]
struct WorkerTaskReport {
    owner: String,
    worker_id: Option<String>,
    output: Option<String>,
    result_torrent: Option<String>,
    usage: Option<hivemind_proto::ResourceUsage>,
    general_compute_request: Option<GeneralComputeRequest>,
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
            worker_id: Some(worker_id),
            cas_store: crate::executor::cas_store_from_environment(),
            reports: Mutex::new(HashMap::new()),
            transfer_lease_authority: Arc::new(Mutex::new(None)),
        }
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
            .worker_id
            .as_deref()
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
                worker_id,
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
}

const MAX_TASK_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_RESULT_REFERENCE_BYTES: usize = 4096;

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
        if self.state.worker_id.as_deref() != Some(worker_id) {
            return Err(Status::permission_denied(
                "Token is not bound to this worker",
            ));
        }
        let reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Status::internal("task report store poisoned"))?;
        let report = reports.get(task_id).ok_or_else(|| {
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
            || claims.worker_id.as_deref() != self.state.worker_id.as_deref()
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
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Status::internal("task report store poisoned"))?;
        if let Some(report) = reports.get_mut(&request.task_id) {
            if report.owner != claims.sub {
                return Err(*task_assignment_denied());
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
                report.general_compute_request = Some(admitted_request.clone());
                report.transfer_generation = Some(request.transfer_generation);
            } else if report.transfer_generation != Some(request.transfer_generation) {
                return Err(Status::permission_denied(
                    "transfer lease generation does not match the admitted attempt",
                ));
            }
        } else {
            reports.insert(
                request.task_id.clone(),
                WorkerTaskReport {
                    owner: claims.sub,
                    worker_id: self.state.worker_id.clone(),
                    output: None,
                    result_torrent: None,
                    usage: None,
                    general_compute_request: Some(admitted_request.clone()),
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

    fn record_task_assignment(&self, task_id: &str, owner: &str) -> Result<(), Box<Status>> {
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        if let Some(report) = reports.get(task_id) {
            if report.owner != owner {
                return Err(task_assignment_denied());
            }
            return Ok(());
        }
        reports.insert(
            task_id.to_string(),
            WorkerTaskReport {
                owner: owner.to_string(),
                worker_id: self.state.worker_id.clone(),
                output: None,
                result_torrent: None,
                usage: None,
                general_compute_request: None,
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
    ) -> Result<(), Box<Status>> {
        let claims = self.validate_worker_execution_token(token)?;
        if !crate::sandbox::is_safe_task_id(task_id) {
            return Err(Box::new(Status::invalid_argument("unsafe task id")));
        }
        let reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        let report = reports.get(task_id).ok_or_else(task_assignment_denied)?;
        if report.owner != claims.sub {
            return Err(task_assignment_denied());
        }
        if claims.task_id.as_deref() != Some(task_id) {
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
        Ok(())
    }

    fn report_for_update<F>(&self, task_id: &str, update: F) -> Result<(), Box<Status>>
    where
        F: FnOnce(&mut WorkerTaskReport),
    {
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        let report = reports
            .get_mut(task_id)
            .ok_or_else(task_assignment_denied)?;
        update(report);
        Ok(())
    }

    fn report_for_task(&self, task_id: &str) -> Result<Option<WorkerTaskReport>, Box<Status>> {
        self.state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))
            .map(|reports| reports.get(task_id).cloned())
    }

    fn record_general_compute_request(
        &self,
        task_id: &str,
        request: GeneralComputeRequest,
        transfer_generation: i64,
    ) -> Result<(), Box<Status>> {
        let mut reports = self
            .state
            .reports
            .lock()
            .map_err(|_| Box::new(Status::internal("task report store poisoned")))?;
        let report = reports
            .get_mut(task_id)
            .ok_or_else(task_assignment_denied)?;
        if report
            .transfer_generation
            .is_some_and(|generation| generation != transfer_generation)
        {
            return Err(Box::new(Status::permission_denied(
                "stale transfer lease generation cannot replace the admitted attempt",
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
        "general-compute-v1alpha1" => {
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
        let request_identity = ExecuteTaskIdentity::from_request(&req);
        let claims = self
            .validate_worker_execution_token(&req.token)
            .map_err(|status| *status)?;
        if !crate::sandbox::is_safe_task_id(&req.task_id) {
            return Err(Status::invalid_argument("unsafe task id"));
        }
        if claims.task_id.as_deref() != Some(req.task_id.as_str())
            || claims.worker_id.as_deref() != self.state.worker_id.as_deref()
        {
            return Err(*task_assignment_denied());
        }
        let admitted = self
            .runtime_admission
            .admit(&req.runtime, &req.general_compute_manifest_json)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        validate_execute_task_contract(&req).map_err(Status::invalid_argument)?;
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
            if let Some(existing) = reports
                .get(&req.task_id)
                .and_then(|report| report.general_compute_request.as_ref())
            {
                if existing != request {
                    return Err(Status::permission_denied(
                        "ExecuteTask request does not match the prepared general-compute attempt",
                    ));
                }
            }
        }
        if let crate::runtime_admission::RuntimeRoute::GeneralComputeV1Alpha1(request) = admitted {
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
            self.record_task_assignment(&req.task_id, &claims.sub)
                .map_err(|status| *status)?;
            self.record_general_compute_request(&req.task_id, request, transfer_generation)
                .map_err(|status| *status)?;
        } else {
            self.record_task_assignment(&req.task_id, &claims.sub)
                .map_err(|status| *status)?;
        }
        let limits = req.resource_limits.unwrap_or_default();
        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: req.task_id.clone(),
            owner: claims.sub,
            worker_id: self.state.worker_id.clone(),
            worker_ip: None,
            status: TaskStatus::Running,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some(req.torrent),
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
            max_cpt: req.managed_budget_units,
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
        match self.state.executor.execute_task(&task).await {
            Ok(result) => Ok(Response::new(execute_response_from_result(
                result,
                matches!(
                    task.runtime.as_deref(),
                    Some("managed-function-v0") | Some("production_sandboxed_dsl")
                ),
                &request_identity,
            ))),
            Err(error) => {
                if let Some(status) = worker_execution_error_status(&error) {
                    Err(status)
                } else {
                    Ok(Response::new(failed_execute_response(
                        "Task execution failed",
                        &request_identity,
                    )))
                }
            }
        }
    }

    async fn task_output_upload(
        &self,
        request: Request<TaskOutputUploadRequest>,
    ) -> Result<Response<TaskOutputUploadResponse>, Status> {
        let req = request.into_inner();
        self.validate_task_assignment(&req.token, &req.task_id, Some(&req.worker_id))
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
        self.report_for_update(&req.task_id, |report| {
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
        self.validate_task_assignment(&req.token, &req.task_id, Some(&req.worker_id))
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
        self.report_for_update(&req.task_id, |report| {
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
        self.validate_task_assignment(&req.token, &req.task_id, None)
            .map_err(|status| *status)?;
        let Some(report) = self
            .report_for_task(&req.task_id)
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
        self.validate_task_assignment(&req.token, &req.task_id, None)
            .map_err(|status| *status)?;
        if !crate::sandbox::is_safe_task_id(&req.task_id) {
            return Err(Status::invalid_argument("unsafe task id"));
        }
        tracing::info!("Stop task {}", req.task_id);
        let (success, status_message) = match self.state.executor.stop_task_execution(&req.task_id)
        {
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
        self.validate_task_assignment(&req.token, &req.task_id, Some(&req.worker_id))
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
        self.report_for_update(&req.task_id, |report| {
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

fn execute_response_from_result(
    result: TaskResult,
    managed_proof_required: bool,
    identity: &ExecuteTaskIdentity,
) -> ExecuteTaskResponse {
    let TaskResult {
        success,
        output,
        error,
        managed_executed_ops,
        managed_output_bytes,
        managed_receipt_json,
        managed_proof,
        general_compute_result_json,
        ..
    } = result;
    let has_typed_general_compute_result = general_compute_result_json.is_some();
    let response = ExecuteTaskResponse {
        success,
        status_message: if success && has_typed_general_compute_result {
            "general-compute result attached".into()
        } else if success {
            output.unwrap_or_default()
        } else {
            error.unwrap_or_else(|| "Task execution failed".into())
        },
        managed_executed_ops,
        managed_output_bytes,
        managed_receipt_json: managed_receipt_json.unwrap_or_default(),
        managed_proof,
        general_compute_result_json: general_compute_result_json.unwrap_or_default(),
        execution_id: identity.execution_id.clone(),
        attempt_id: identity.attempt_id.clone(),
        idempotency_key: identity.idempotency_key.clone(),
        request_digest: identity.request_digest.clone(),
    };

    if managed_proof_required && response.success && response.managed_proof.is_none() {
        return failed_execute_response("Managed proof is required", identity);
    }
    if !response_fits_worker_rpc_limits(&response) {
        return failed_execute_response("Task result exceeds supported response limits", identity);
    }

    response
}

fn response_fits_worker_rpc_limits(response: &ExecuteTaskResponse) -> bool {
    response.status_message.len() <= WORKER_STATUS_MESSAGE_MAX_BYTES
        && response.managed_receipt_json.len() <= LEGACY_MANAGED_RECEIPT_MAX_BYTES
        && response.general_compute_result_json.len() <= GENERAL_COMPUTE_RESULT_MAX_BYTES
        && response
            .managed_proof
            .as_ref()
            .is_none_or(|proof| proof.encoded_len() <= MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES)
        && response.encoded_len() <= WORKER_RPC_MESSAGE_MAX_BYTES
}

fn failed_execute_response(message: &str, identity: &ExecuteTaskIdentity) -> ExecuteTaskResponse {
    ExecuteTaskResponse {
        success: false,
        status_message: message.into(),
        managed_executed_ops: 0,
        managed_output_bytes: 0,
        managed_receipt_json: String::new(),
        managed_proof: None,
        general_compute_result_json: Vec::new(),
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
        if request.runtime.trim() != "general-compute-v1alpha1" {
            return Self::default();
        }
        Self {
            execution_id: request.execution_id.clone(),
            attempt_id: request.attempt_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest: request.request_digest.clone(),
        }
    }
}

fn worker_execution_error_status(error: &anyhow::Error) -> Option<Status> {
    (error.downcast_ref::<ManagedProverError>() == Some(&ManagedProverError::QueueFull))
        .then(|| Status::resource_exhausted("Managed prover is busy"))
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
        }
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
            |_task, _cancellation| async move { Ok(successful_task_result(None)) },
        ));
        let state = Arc::new(WorkerGrpcState {
            config,
            executor,
            worker_id: Some(worker_id.into()),
            cas_store,
            reports: Mutex::new(HashMap::new()),
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

    #[test]
    fn managed_success_response_forwards_the_proof_envelope() {
        let proof = hivemind_proto::ManagedProofEnvelope {
            proof_scheme: "risc0-zkvm-3.0.6".into(),
            image_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
            journal: vec![9, 10],
            receipt_json: br#"{"receipt":true}"#.to_vec(),
        };

        let response = execute_response_from_result(
            successful_task_result(Some(proof.clone())),
            true,
            &ExecuteTaskIdentity::from_request(&execute_request(
                "managed-function-v0",
                "return 42;".into(),
                "{}".into(),
                10,
            )),
        );

        assert!(response.success);
        assert_eq!(response.status_message, "42");
        assert_eq!(response.managed_proof, Some(proof));
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
            successful_task_result(None),
            false,
            &ExecuteTaskIdentity::from_request(&request),
        );
        assert!(success.success);
        assert_eq!(success.execution_id, request.execution_id);
        assert_eq!(success.attempt_id, request.attempt_id);
        assert_eq!(success.idempotency_key, request.idempotency_key);
        assert_eq!(success.request_digest, request.request_digest);

        let failure = execute_response_from_result(
            successful_task_result(None),
            true,
            &ExecuteTaskIdentity::from_request(&request),
        );
        assert!(!failure.success);
        assert_eq!(failure.execution_id, request.execution_id);
        assert_eq!(failure.attempt_id, request.attempt_id);
        assert_eq!(failure.idempotency_key, request.idempotency_key);
        assert_eq!(failure.request_digest, request.request_digest);
    }

    #[test]
    fn worker_legacy_execute_response_keeps_attempt_identity_empty() {
        let mut request =
            execute_request("managed-function-v0", "return 42;".into(), "{}".into(), 10);
        request.execution_id = "execution-legacy-should-not-echo".into();
        request.attempt_id = "attempt-legacy-should-not-echo".into();
        request.idempotency_key = "idempotency-legacy-should-not-echo".into();
        request.request_digest = "sha256:legacy-should-not-echo".into();

        let response = execute_response_from_result(
            successful_task_result(None),
            false,
            &ExecuteTaskIdentity::from_request(&request),
        );

        assert!(response.execution_id.is_empty());
        assert!(response.attempt_id.is_empty());
        assert!(response.idempotency_key.is_empty());
        assert!(response.request_digest.is_empty());
    }

    #[test]
    fn managed_success_without_a_proof_fails_closed_before_the_rpc_boundary() {
        let response = execute_response_from_result(
            successful_task_result(None),
            true,
            &ExecuteTaskIdentity::from_request(&execute_request(
                "managed-function-v0",
                "return 42;".into(),
                "{}".into(),
                10,
            )),
        );

        assert!(!response.success);
        assert_eq!(response.status_message, "Managed proof is required");
        assert!(response.managed_proof.is_none());
        assert_eq!(response.managed_executed_ops, 0);
        assert_eq!(response.managed_output_bytes, 0);
    }

    #[test]
    fn worker_response_over_the_shared_output_cap_fails_closed() {
        let mut result = successful_task_result(None);
        result.output = Some("x".repeat(hivemind_proto::WORKER_STATUS_MESSAGE_MAX_BYTES + 1));

        let response = execute_response_from_result(
            result,
            false,
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
        assert!(response.managed_proof.is_none());
    }

    #[test]
    fn worker_execute_response_forwards_typed_general_compute_result() {
        let payload = br#"{"status":"completed"}"#.to_vec();
        let mut result = successful_task_result(None);
        result.general_compute_result_json = Some(payload.clone());

        let response = execute_response_from_result(
            result,
            false,
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
    fn full_prover_queue_maps_to_redispatchable_resource_exhaustion() {
        let error = anyhow::Error::new(ManagedProverError::QueueFull);
        let status = worker_execution_error_status(&error)
            .expect("a full local prover queue is an RPC resource exhaustion");

        assert_eq!(status.code(), tonic::Code::ResourceExhausted);
        assert_eq!(status.message(), "Managed prover is busy");
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
    async fn result_upload_and_usage_reporting_accept_valid_token() {
        let tmp = TempDir::new().unwrap();
        let service = test_service(tmp.path());
        seed_assignment(&service, "task-with-result", ASSIGNED_OWNER, None);
        let token = bound_token(test_private_key_pem(), ASSIGNED_OWNER, "task-with-result");

        let result = service
            .task_result_upload(Request::new(TaskResultUploadRequest {
                task_id: "task-with-result".into(),
                worker_id: TEST_WORKER_ID.into(),
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
        let execute_service = service.clone();
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
                    token: bound_token(test_private_key_pem(), ASSIGNED_OWNER, &execute_task_id),
                    managed_budget_units: hivemind_proto::MANAGED_BUDGET_MAX_USAGE_UNITS,
                    general_compute_manifest_json: Vec::new(),
                    execution_id: String::new(),
                    attempt_id: String::new(),
                    idempotency_key: String::new(),
                    request_digest: String::new(),
                    managed_dsl_backend_id: String::new(),
                    managed_dsl_semantics_manifest_sha256: String::new(),
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
                    token: bound_token(test_private_key_pem(), ASSIGNED_OWNER, &task_id),
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
        assert_eq!(status.code(), Code::FailedPrecondition);
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
                .get("chunk-task")
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

    fn successful_task_result(
        managed_proof: Option<hivemind_proto::ManagedProofEnvelope>,
    ) -> TaskResult {
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
            managed_proof,
            general_compute_result_json: None,
        }
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
                    managed_proof: None,
                    general_compute_result_json: None,
                })
            },
        ));
        GrpcWorkerNodeService::new(Arc::new(WorkerGrpcState {
            config,
            executor,
            worker_id: Some(TEST_WORKER_ID.into()),
            cas_store: None,
            reports: Mutex::new(HashMap::new()),
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
            worker_id: Some(TEST_WORKER_ID.into()),
            cas_store: None,
            reports: Mutex::new(HashMap::new()),
            transfer_lease_authority: Arc::new(Mutex::new(None)),
        }))
    }

    fn test_token(_private_key_pem: &str, subject: &str) -> String {
        test_token_with_role(subject, Some("worker-execution"))
    }

    fn bound_token(_private_key_pem: &str, subject: &str, task_id: &str) -> String {
        WorkerExecutionSigner::from_pem(test_private_key_pem())
            .unwrap()
            .encode_claims(&Claims {
                sub: subject.into(),
                user_id: subject.into(),
                role: Some("worker-execution".into()),
                task_id: Some(task_id.into()),
                worker_id: Some(TEST_WORKER_ID.into()),
                exp: (Utc::now().timestamp() + 3600) as usize,
                iat: Utc::now().timestamp() as usize,
            })
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

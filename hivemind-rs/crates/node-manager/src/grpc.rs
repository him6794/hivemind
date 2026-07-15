use hivemind_config::HivemindConfig;
use hivemind_proto::{
    batch_runtime_service_server::BatchRuntimeService,
    master_node_service_server::MasterNodeService,
    node_manager_service_server::NodeManagerService,
    user_service_server::UserService,
    AdminAuditLogEntry,
    AdminWorkerTrustEntry,
    CacheAnomalyEntry,
    CleanupAdminArtifactsRequest,
    CleanupAdminArtifactsResponse,
    CompleteBatchRequest,
    CompleteBatchResponse,
    DownloadTaskArtifactRequest,
    DownloadTaskArtifactResponse,
    ExecutionPackage,
    GetAdminArtifactOverviewRequest,
    GetAdminArtifactOverviewResponse,
    // Admin RPC types
    GetAdminBillingOverviewRequest,
    GetAdminBillingOverviewResponse,
    GetAdminSchedulingCacheAlertRequest,
    GetAdminSchedulingCacheAlertResponse,
    GetAdminSchedulingCacheMetricsRequest,
    GetAdminSchedulingCacheMetricsResponse,
    GetAllUserTasksRequest,
    GetAllUserTasksResponse,
    GetBalanceRequest,
    GetBalanceResponse,
    GetProviderEarningsRequest,
    GetProviderEarningsResponse,
    GetProviderWorkerSettingsRequest,
    GetProviderWorkerSettingsResponse,
    GetTaskResultRequest,
    GetTaskResultResponse,
    GetWorkerTrustProfileRequest,
    GetWorkerTrustProfileResponse,
    HeartbeatRequest,
    HeartbeatResponse,
    ListAdminAuditLogsRequest,
    ListAdminAuditLogsResponse,
    ListAdminSchedulingCacheAnomaliesRequest,
    ListAdminSchedulingCacheAnomaliesResponse,
    ListAdminWorkerTrustRequest,
    ListAdminWorkerTrustResponse,
    ListWorkersRequest,
    ListWorkersResponse,
    LoginRequest,
    LoginResponse,
    PricingBreakdown,
    ProviderEarningsEntry,
    ProviderWorkerSettings,
    PullBatchRequest,
    PullBatchResponse,
    QuoteTaskRequest,
    QuoteTaskResponse,
    RefreshTokenRequest,
    RefreshTokenResponse,
    RegisterUserRequest,
    RegisterUserResponse,
    RegisterWorkerNodeRequest,
    RemoveWorkerRequest,
    ResourceSpec as ProtoResourceSpec,
    RunningStatusRequest,
    RunningStatusResponse,
    StatusResponse,
    StopTaskExecutionRequest,
    StopTaskRequest,
    StopTaskResponse,
    TaskInfo,
    TaskLease,
    TaskOutputUploadRequest,
    TaskOutputUploadResponse,
    TaskResultUploadRequest,
    TaskResultUploadResponse,
    TaskUsageRequest,
    TaskUsageResponse,
    TasklogRequest,
    TasklogResponse,
    UpdateProviderWorkerSettingsRequest,
    UpdateProviderWorkerSettingsResponse,
    UpdateWorkerTrustControlRequest,
    UpdateWorkerTrustControlResponse,
    UploadTaskRequest,
    UploadTaskResponse,
    WorkerCacheAffinityMetric,
    WorkerInfo,
    WorkerNodeServiceClient,
    WorkerTrustProfile,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::service::{NodeManagerService as NmSvc, WorkerRegistration};
use crate::NodeManager;
use hivemind_auth::{jwt_service::JwtService, AuthManager};
use hivemind_database::postgres;
use hivemind_models::{Claims, Task, TaskStatus, WorkerNode};
use hivemind_task_scheduler::{dispatcher::worker_endpoint, BatchTaskReport, TaskScheduler};
use hivemind_torrent_service::DistributionRuntime;

const MAX_TASK_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_RESULT_REFERENCE_BYTES: usize = 4096;
const MAX_DOWNLOAD_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

pub struct NodepoolState {
    pub auth: AuthManager,
    pub worker_execution_secret: String,
    pub node_manager: Arc<NodeManager>,
    pub scheduler: TaskScheduler,
    pub artifact_root: PathBuf,
    /// Trusted task-package distribution runtime (tracker + seeder).
    /// Optional in pure unit tests that do not exercise package upload.
    pub distribution: Option<DistributionRuntime>,
}

fn safe_package_filename(task_id: &str, package_filename: &str) -> Result<String, &'static str> {
    let raw = package_filename.trim();
    let candidate = if raw.is_empty() {
        format!("{task_id}.zip")
    } else {
        Path::new(raw)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(task_id)
            .to_string()
    };
    if candidate.is_empty()
        || candidate == "."
        || candidate == ".."
        || candidate.contains("..")
        || candidate.contains('/')
        || candidate.contains('\\')
    {
        return Err("package_filename is invalid");
    }
    Ok(candidate)
}

fn max_package_upload_bytes() -> usize {
    std::env::var("HIVEMIND_MAX_TASK_UPLOAD_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100 * 1024 * 1024)
}

pub fn artifact_root_for_config(config: &HivemindConfig) -> PathBuf {
    std::env::var("HIVEMIND_ARTIFACT_ROOT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&config.torrent.api_dir).join("artifacts"))
}

// UserService
pub struct GrpcUserService {
    state: Arc<NodepoolState>,
}
impl GrpcUserService {
    pub fn new(state: Arc<NodepoolState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl UserService for GrpcUserService {
    async fn register_user(
        &self,
        request: Request<RegisterUserRequest>,
    ) -> Result<Response<RegisterUserResponse>, Status> {
        let req = request.into_inner();
        let username = req.username.trim();
        let password = req.password;
        if username.len() < 3 {
            return Ok(Response::new(RegisterUserResponse {
                success: false,
                status_message: "Username must be at least 3 characters".into(),
            }));
        }
        if is_admin(username) {
            return Ok(Response::new(RegisterUserResponse {
                success: false,
                status_message: "Username is unavailable".into(),
            }));
        }
        if password.len() < 8 {
            return Ok(Response::new(RegisterUserResponse {
                success: false,
                status_message: "Password must be at least 8 characters".into(),
            }));
        }

        match postgres::create_user(
            &self.state.scheduler.database().pool,
            username,
            &password,
            1000,
        )
        .await
        {
            Ok(()) => Ok(Response::new(RegisterUserResponse {
                success: true,
                status_message: "Account created".into(),
            })),
            Err(e) => Ok(Response::new(RegisterUserResponse {
                success: false,
                status_message: if e.to_string().contains("already exists") {
                    "Username already exists".into()
                } else {
                    e.to_string()
                },
            })),
        }
    }

    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        let req = request.into_inner();
        match self
            .state
            .auth
            .authenticate(&req.username, &req.password)
            .await
        {
            Ok(Some(token)) => Ok(Response::new(LoginResponse {
                success: true,
                status_message: "OK".into(),
                token,
            })),
            Ok(None) => Ok(Response::new(LoginResponse {
                success: false,
                status_message: "Invalid credentials".into(),
                token: String::new(),
            })),
            Err(e) => Ok(Response::new(LoginResponse {
                success: false,
                status_message: e.to_string(),
                token: String::new(),
            })),
        }
    }
    async fn get_balance(
        &self,
        request: Request<GetBalanceRequest>,
    ) -> Result<Response<GetBalanceResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !req.username.is_empty() && req.username != claims.sub {
            return Err(Status::permission_denied("Forbidden"));
        }
        let balance: Option<i64> = sqlx::query_scalar(
            "SELECT balance FROM users WHERE username = $1 AND is_active = true",
        )
        .bind(&claims.sub)
        .fetch_optional(&self.state.scheduler.database().pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        let Some(balance) = balance else {
            return Err(Status::not_found("User not found"));
        };
        Ok(Response::new(GetBalanceResponse {
            success: true,
            status_message: "OK".into(),
            balance,
        }))
    }
    async fn refresh_token(
        &self,
        request: Request<RefreshTokenRequest>,
    ) -> Result<Response<RefreshTokenResponse>, Status> {
        let req = request.into_inner();
        match self.state.auth.validate_token(&req.old_token) {
            Ok(claims) => {
                let token = self
                    .state
                    .auth
                    .jwt_service()
                    .encode_claims(&claims)
                    .map_err(|e| Status::internal(format!("Token: {}", e)))?;
                Ok(Response::new(RefreshTokenResponse {
                    success: true,
                    status_message: "OK".into(),
                    new_token: token,
                }))
            }
            Err(_) => Ok(Response::new(RefreshTokenResponse {
                success: false,
                status_message: "Invalid token".into(),
                new_token: String::new(),
            })),
        }
    }
}

// NodeManagerService
pub struct GrpcNodeManagerService {
    state: Arc<NodepoolState>,
}
impl GrpcNodeManagerService {
    pub fn new(state: Arc<NodepoolState>) -> Self {
        Self { state }
    }

    async fn authorize_worker_report(
        &self,
        token: &str,
        worker_id: &str,
    ) -> Result<ReportAuthorization, Status> {
        authorize_worker_identity(&self.state, token, worker_id).await
    }

    async fn register_reported_artifact_ref(
        &self,
        task_id: &str,
        artifact_ref: &str,
    ) -> Result<(), Status> {
        register_reported_artifact_ref(
            &self.state.scheduler.database().pool,
            &self.state.artifact_root,
            task_id,
            artifact_ref,
        )
        .await
    }
}

enum ReportAuthorization {
    Authorized,
    Denied(String),
}

async fn authorize_worker_identity(
    state: &NodepoolState,
    token: &str,
    worker_id: &str,
) -> Result<ReportAuthorization, Status> {
    if worker_id.trim().is_empty() {
        return Ok(ReportAuthorization::Denied("worker_id is required".into()));
    }
    let claims = state
        .auth
        .validate_token(token)
        .map_err(|_| Status::unauthenticated("Invalid token"))?;
    let worker = state
        .node_manager
        .get_worker(worker_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    let Some(worker) = worker else {
        return Ok(ReportAuthorization::Denied("Worker not found".into()));
    };
    if claims.sub != worker.worker_id && claims.sub != worker.username && !is_admin(&claims.sub) {
        return Ok(ReportAuthorization::Denied("Not authorized".into()));
    }
    Ok(ReportAuthorization::Authorized)
}

fn is_safe_task_id(task_id: &str) -> bool {
    if task_id.len() == 1 && task_id.as_bytes()[0] == b'.' {
        return false;
    }
    !task_id.trim().is_empty()
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !task_id.contains("..")
}

fn is_safe_worker_id(worker_id: &str) -> bool {
    is_safe_task_id(worker_id)
}

fn worker_authorization_response(status_message: String) -> Status {
    match status_message.as_str() {
        "worker_id is required" => Status::invalid_argument(status_message),
        "Worker not found" => Status::not_found(status_message),
        _ => Status::permission_denied(status_message),
    }
}

async fn register_reported_artifact_ref(
    pool: &sqlx::PgPool,
    artifact_root: &Path,
    task_id: &str,
    artifact_ref: &str,
) -> Result<(), Status> {
    let artifact_ref = artifact_ref.trim();
    if artifact_ref.is_empty() {
        return Ok(());
    }
    let path = match resolve_reported_artifact_ref(artifact_root, artifact_ref).await {
        Ok(Some(path)) => path,
        Ok(None) => return Ok(()),
        Err(status) => {
            tracing::warn!(
                task_id,
                artifact_ref,
                reason = status.message(),
                "Reported artifact reference was not registered for download"
            );
            return Ok(());
        }
    };
    let metadata = match artifact_file_metadata(task_id, artifact_ref, &path).await {
        Ok(metadata) => metadata,
        Err(status) => {
            tracing::warn!(
                task_id,
                artifact_ref,
                reason = status.message(),
                "Reported artifact file was not registered for download"
            );
            return Ok(());
        }
    };
    let artifact_key = artifact_key_for_ref(task_id, artifact_ref);
    sqlx::query(
        "INSERT INTO artifacts (task_id, artifact_key, checksum_sha1, size_bytes, storage_path, status)
         VALUES ($1, $2, $3, $4, $5, 'ready')
         ON CONFLICT (artifact_key) DO UPDATE SET
             checksum_sha1 = EXCLUDED.checksum_sha1,
             size_bytes = EXCLUDED.size_bytes,
             storage_path = EXCLUDED.storage_path,
             status = 'ready',
             created_at = NOW()",
    )
    .bind(task_id)
    .bind(artifact_key)
    .bind(metadata.sha1)
    .bind(metadata.size_bytes)
    .bind(path.to_string_lossy().as_ref())
    .execute(pool)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    Ok(())
}

struct ArtifactFileMetadata {
    sha1: String,
    size_bytes: i64,
}

async fn artifact_file_metadata(
    task_id: &str,
    artifact_ref: &str,
    path: &Path,
) -> Result<ArtifactFileMetadata, Status> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|e| Status::internal(format!("Failed to read artifact metadata: {e}")))?;
    if !metadata.is_file() {
        return Err(Status::invalid_argument("Artifact path is not a file"));
    }
    if metadata.len() > MAX_DOWNLOAD_ARTIFACT_BYTES as u64 {
        return Err(Status::resource_exhausted(format!(
            "Artifact is too large: {} bytes > {} bytes",
            metadata.len(),
            MAX_DOWNLOAD_ARTIFACT_BYTES
        )));
    }
    Ok(ArtifactFileMetadata {
        sha1: artifact_metadata_checksum(task_id, artifact_ref, path, metadata.len()),
        size_bytes: metadata.len() as i64,
    })
}

async fn resolve_reported_artifact_ref(
    artifact_root: &Path,
    artifact_ref: &str,
) -> Result<Option<PathBuf>, Status> {
    let candidate = if let Some(relative) = artifact_ref.strip_prefix("artifact://") {
        if relative.trim().is_empty() {
            return Ok(None);
        }
        artifact_root.join(safe_artifact_relative_path(relative).map_err(Status::invalid_argument)?)
    } else {
        let path = PathBuf::from(artifact_ref);
        if !path.is_absolute() {
            return Ok(None);
        }
        path
    };
    Ok(Some(
        canonical_artifact_path(artifact_root, &candidate).await?,
    ))
}

fn safe_artifact_relative_path(value: &str) -> Result<PathBuf, &'static str> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Err("Artifact reference must be relative");
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return Err("Artifact reference contains unsafe path components"),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("Artifact reference is empty");
    }
    Ok(clean)
}

async fn canonical_artifact_path(artifact_root: &Path, path: &Path) -> Result<PathBuf, Status> {
    let root = ensure_artifact_root(artifact_root).await?;
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|e| Status::not_found(format!("Artifact file not found: {e}")))?;
    if !canonical.starts_with(&root) {
        return Err(Status::permission_denied(
            "Artifact storage path is outside the configured artifact root",
        ));
    }
    Ok(canonical)
}

async fn ensure_artifact_root(artifact_root: &Path) -> Result<PathBuf, Status> {
    tokio::fs::create_dir_all(artifact_root)
        .await
        .map_err(|e| Status::internal(format!("Failed to create artifact root: {e}")))?;
    tokio::fs::canonicalize(artifact_root)
        .await
        .map_err(|e| Status::internal(format!("Failed to resolve artifact root: {e}")))
}

fn artifact_key_for_ref(task_id: &str, artifact_ref: &str) -> String {
    format!(
        "task-artifact:{:016x}",
        artifact_ref_hash(task_id, artifact_ref)
    )
}

fn artifact_metadata_checksum(
    task_id: &str,
    artifact_ref: &str,
    path: &Path,
    size_bytes: u64,
) -> String {
    let mut hasher = DefaultHasher::new();
    task_id.hash(&mut hasher);
    artifact_ref.hash(&mut hasher);
    path.hash(&mut hasher);
    size_bytes.hash(&mut hasher);
    format!("metadata:{:016x}", hasher.finish())
}

fn artifact_ref_hash(task_id: &str, artifact_ref: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    task_id.hash(&mut hasher);
    artifact_ref.hash(&mut hasher);
    hasher.finish()
}

#[tonic::async_trait]
impl NodeManagerService for GrpcNodeManagerService {
    async fn register_worker_node(
        &self,
        request: Request<RegisterWorkerNodeRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();
        let r = req.resources.unwrap_or_default();
        let username = req.username.trim().to_string();
        let worker_id = if req.worker_id.trim().is_empty() {
            username.clone()
        } else {
            req.worker_id.trim().to_string()
        };
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_safe_worker_id(&worker_id) {
            return Ok(Response::new(StatusResponse {
                success: false,
                status_message: "Invalid worker_id".into(),
            }));
        }
        if claims.sub != worker_id && claims.sub != username && !is_admin(&claims.sub) {
            return Ok(Response::new(StatusResponse {
                success: false,
                status_message: "Not authorized".into(),
            }));
        }
        if let Err(message) = validate_worker_registration_resources(&r) {
            return Ok(Response::new(StatusResponse {
                success: false,
                status_message: message.into(),
            }));
        }
        let svc = NmSvc::new((*self.state.node_manager).clone());
        let reg = WorkerRegistration {
            worker_id,
            username,
            ip: req.ip,
            resources: proto_resource_spec_to_model(r),
            location: req.location,
        };
        match svc.register_worker(&reg).await {
            Ok(w) => {
                sqlx::query(
                    "INSERT INTO worker_reputation (worker_id) VALUES ($1) ON CONFLICT (worker_id) DO NOTHING",
                )
                .bind(&w.worker_id)
                .execute(&self.state.scheduler.database().pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
                tracing::info!("Worker registered: {}", w.worker_id);
                Ok(Response::new(StatusResponse {
                    success: true,
                    status_message: "OK".into(),
                }))
            }
            Err(e) => Ok(Response::new(StatusResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }
    async fn report_status(
        &self,
        request: Request<RunningStatusRequest>,
    ) -> Result<Response<RunningStatusResponse>, Status> {
        let req = request.into_inner();
        let u = req.usage.unwrap_or_default();
        let worker_id = if req.worker_id.trim().is_empty() {
            req.username.trim().to_string()
        } else {
            req.worker_id.trim().to_string()
        };
        match self.authorize_worker_report(&req.token, &worker_id).await? {
            ReportAuthorization::Authorized => {}
            ReportAuthorization::Denied(status_message) => {
                return Ok(Response::new(RunningStatusResponse {
                    success: false,
                    status_message,
                }));
            }
        }
        match self
            .state
            .node_manager
            .update_heartbeat(
                &worker_id,
                &req.status,
                u.cpu_percent as f64,
                u.memory_percent as f64,
                u.gpu_percent as f64,
                u.vram_percent as f64,
            )
            .await
        {
            Ok(_) => Ok(Response::new(RunningStatusResponse {
                success: true,
                status_message: "OK".into(),
            })),
            Err(e) => Ok(Response::new(RunningStatusResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }

    async fn task_output_upload(
        &self,
        request: Request<TaskOutputUploadRequest>,
    ) -> Result<Response<TaskOutputUploadResponse>, Status> {
        let req = request.into_inner();
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
        match self
            .authorize_worker_report(&req.token, &req.worker_id)
            .await?
        {
            ReportAuthorization::Authorized => {}
            ReportAuthorization::Denied(status_message) => {
                return Ok(Response::new(TaskOutputUploadResponse {
                    success: false,
                    status_message,
                }));
            }
        }

        match self
            .state
            .scheduler
            .record_task_output_for_worker(&req.task_id, &req.worker_id, &req.output)
            .await
        {
            Ok(_) => Ok(Response::new(TaskOutputUploadResponse {
                success: true,
                status_message: "OK".into(),
            })),
            Err(e) => Ok(Response::new(TaskOutputUploadResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }

    async fn task_result_upload(
        &self,
        request: Request<TaskResultUploadRequest>,
    ) -> Result<Response<TaskResultUploadResponse>, Status> {
        let req = request.into_inner();
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
        match self
            .authorize_worker_report(&req.token, &req.worker_id)
            .await?
        {
            ReportAuthorization::Authorized => {}
            ReportAuthorization::Denied(status_message) => {
                return Ok(Response::new(TaskResultUploadResponse {
                    success: false,
                    status_message,
                }));
            }
        }

        match self
            .state
            .scheduler
            .complete_task_result_for_worker(&req.task_id, &req.worker_id, &req.result_torrent)
            .await
        {
            Ok(_) => {
                if let Err(status) = self
                    .register_reported_artifact_ref(&req.task_id, &req.result_torrent)
                    .await
                {
                    return Ok(Response::new(TaskResultUploadResponse {
                        success: false,
                        status_message: status.to_string(),
                    }));
                }
                Ok(Response::new(TaskResultUploadResponse {
                    success: true,
                    status_message: "OK".into(),
                }))
            }
            Err(e) => Ok(Response::new(TaskResultUploadResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }

    async fn task_usage(
        &self,
        request: Request<TaskUsageRequest>,
    ) -> Result<Response<TaskUsageResponse>, Status> {
        let req = request.into_inner();
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
        match self
            .authorize_worker_report(&req.token, &req.worker_id)
            .await?
        {
            ReportAuthorization::Authorized => {}
            ReportAuthorization::Denied(status_message) => {
                return Ok(Response::new(TaskUsageResponse {
                    success: false,
                    status_message,
                }));
            }
        }

        match self
            .state
            .scheduler
            .update_task_resource_usage_for_worker(
                &req.task_id,
                &req.worker_id,
                usage.cpu_percent as f64,
                usage.memory_percent as f64,
                usage.gpu_percent as f64,
                usage.vram_percent as f64,
            )
            .await
        {
            Ok(_) => Ok(Response::new(TaskUsageResponse {
                success: true,
                status_message: "OK".into(),
            })),
            Err(e) => Ok(Response::new(TaskUsageResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }

    async fn list_workers(
        &self,
        request: Request<ListWorkersRequest>,
    ) -> Result<Response<ListWorkersResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        let workers = self
            .state
            .node_manager
            .list_workers(req.include_offline)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let workers = if is_admin(&claims.sub) {
            workers
        } else {
            workers
                .into_iter()
                .filter(|worker| worker.username == claims.sub)
                .collect()
        };
        Ok(Response::new(ListWorkersResponse {
            success: true,
            status_message: "OK".into(),
            workers: workers.into_iter().map(worker_info_from_model).collect(),
        }))
    }

    async fn remove_worker(
        &self,
        request: Request<RemoveWorkerRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();
        let worker_id = req.worker_id.trim();
        if !is_safe_worker_id(worker_id) {
            return Ok(Response::new(StatusResponse {
                success: false,
                status_message: "Invalid worker_id".into(),
            }));
        }
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        let worker = self
            .state
            .node_manager
            .get_worker(worker_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let Some(worker) = worker else {
            return Ok(Response::new(StatusResponse {
                success: false,
                status_message: "Worker not found".into(),
            }));
        };
        if worker.username != claims.sub && !is_admin(&claims.sub) {
            return Ok(Response::new(StatusResponse {
                success: false,
                status_message: "Not authorized".into(),
            }));
        }
        let removed = self
            .state
            .node_manager
            .remove_worker(worker_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(StatusResponse {
            success: removed,
            status_message: if removed { "OK" } else { "Worker not found" }.into(),
        }))
    }
}

// MasterNodeService
pub struct GrpcMasterNodeService {
    state: Arc<NodepoolState>,
}
impl GrpcMasterNodeService {
    pub fn new(state: Arc<NodepoolState>) -> Self {
        Self { state }
    }
}

pub struct GrpcBatchRuntimeService {
    state: Arc<NodepoolState>,
}
impl GrpcBatchRuntimeService {
    pub fn new(state: Arc<NodepoolState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl MasterNodeService for GrpcMasterNodeService {
    async fn upload_task(
        &self,
        request: Request<UploadTaskRequest>,
    ) -> Result<Response<UploadTaskResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_safe_task_id(&req.task_id) {
            return Ok(Response::new(UploadTaskResponse {
                success: false,
                status_message: "task_id is required and must be a safe file name".into(),
            }));
        }
        let r = req.requirements.unwrap_or_default();
        if let Err(message) = validate_task_submission_resources(&r, req.host_count, req.max_cpt) {
            return Ok(Response::new(UploadTaskResponse {
                success: false,
                status_message: message.into(),
            }));
        }

        // Prefer package bytes from master: nodepool is the only trusted seeder.
        let mut torrent_source = req.torrent.clone();
        let mut expected_btih = None;
        if !req.package_data.is_empty() {
            if req.package_data.len() > max_package_upload_bytes() {
                return Ok(Response::new(UploadTaskResponse {
                    success: false,
                    status_message: format!(
                        "uploaded task package is too large: {} bytes > {} bytes",
                        req.package_data.len(),
                        max_package_upload_bytes()
                    ),
                }));
            }
            let Some(distribution) = self.state.distribution.as_ref() else {
                return Ok(Response::new(UploadTaskResponse {
                    success: false,
                    status_message: "nodepool package distribution runtime is not configured"
                        .into(),
                }));
            };
            let filename = match safe_package_filename(&req.task_id, &req.package_filename) {
                Ok(name) => name,
                Err(status) => {
                    return Ok(Response::new(UploadTaskResponse {
                        success: false,
                        status_message: status.to_string(),
                    }));
                }
            };
            let package_path = PathBuf::from(&filename);
            let torrent_service = distribution.torrent_service();
            let seeded = match torrent_service
                .package_bytes_to_torrent(
                    &req.package_data,
                    &package_path,
                    &distribution.announce_url,
                )
                .await
            {
                Ok(info) => info,
                Err(err) => {
                    return Ok(Response::new(UploadTaskResponse {
                        success: false,
                        status_message: format!("failed to seed task package: {err}"),
                    }));
                }
            };
            if let Err(err) = distribution
                .register_local_seeder(&seeded.info_hash, seeded.data_size)
                .await
            {
                return Ok(Response::new(UploadTaskResponse {
                    success: false,
                    status_message: format!("failed to register nodepool seeder: {err}"),
                }));
            }
            torrent_source = seeded.magnet_uri;
            expected_btih = Some(seeded.info_hash);
        } else if torrent_source.trim().is_empty() {
            return Ok(Response::new(UploadTaskResponse {
                success: false,
                status_message: "torrent or package_data is required".into(),
            }));
        }

        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: req.task_id,
            owner: claims.sub,
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some(torrent_source),
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
            expected_btih,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: r.cpu_score,
            req_gpu_score: r.gpu_score,
            req_memory_gb: (r.memory_mb / 1024) as i32,
            req_gpu_memory_gb: (r.vram_mb / 1024) as i32,
            req_storage_gb: r.storage_total_gb,
            host_count: req.host_count,
            max_cpt: req.max_cpt,
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
        match self.state.scheduler.create_task(&task).await {
            Ok(t) => {
                tracing::info!("Task {} created via gRPC", t.task_id);
                Ok(Response::new(UploadTaskResponse {
                    success: true,
                    status_message: format!("Task {} created", t.task_id),
                }))
            }
            Err(e) => Ok(Response::new(UploadTaskResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }

    async fn get_all_user_tasks(
        &self,
        request: Request<GetAllUserTasksRequest>,
    ) -> Result<Response<GetAllUserTasksResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        match self.state.scheduler.list_user_tasks(&claims.sub).await {
            Ok(tasks) => Ok(Response::new(GetAllUserTasksResponse {
                tasks: tasks.into_iter().map(task_info_from_task).collect(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_task_result(
        &self,
        request: Request<GetTaskResultRequest>,
    ) -> Result<Response<GetTaskResultResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        match self.state.scheduler.get_task(&req.task_id).await {
            Ok(Some(t)) => {
                if t.owner != claims.sub {
                    return Ok(Response::new(GetTaskResultResponse {
                        success: false,
                        status_message: "Not authorized".into(),
                        result_torrent: String::new(),
                    }));
                }
                Ok(Response::new(GetTaskResultResponse {
                    success: true,
                    status_message: "OK".into(),
                    result_torrent: t.result_torrent.unwrap_or_default(),
                }))
            }
            Ok(None) => Ok(Response::new(GetTaskResultResponse {
                success: false,
                status_message: "Not found".into(),
                result_torrent: String::new(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn stop_task(
        &self,
        request: Request<StopTaskRequest>,
    ) -> Result<Response<StopTaskResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        match self.state.scheduler.get_task(&req.task_id).await {
            Ok(Some(t)) if t.owner != claims.sub => {
                return Ok(Response::new(StopTaskResponse {
                    success: false,
                    status_message: "Not authorized".into(),
                }));
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                return Ok(Response::new(StopTaskResponse {
                    success: false,
                    status_message: "Not found".into(),
                }));
            }
            Err(e) => return Err(Status::internal(e.to_string())),
        }
        match self.state.scheduler.cancel_task(&req.task_id).await {
            Ok(task) => {
                let now = chrono::Utc::now().timestamp() as usize;
                let worker_token = encode_worker_execution_claims(
                    &self.state.worker_execution_secret,
                    &Claims {
                        sub: task.owner.clone(),
                        user_id: task.owner.clone(),
                        role: Some("worker-execution".into()),
                        task_id: Some(task.task_id.clone()),
                        worker_id: task.worker_id.clone(),
                        exp: now + 300,
                        iat: now,
                    },
                )
                .map_err(|error| Status::internal(format!("Worker token: {error}")))?;
                let status_message = match request_worker_stop(&task, &worker_token).await {
                    WorkerStopDispatch::NotAssigned => "Task cancellation recorded".to_string(),
                    WorkerStopDispatch::Requested => {
                        "Task cancellation recorded; worker stop requested".to_string()
                    }
                    WorkerStopDispatch::NotConfirmed(message) => {
                        format!("Task cancellation recorded; worker stop not confirmed: {message}")
                    }
                };
                Ok(Response::new(StopTaskResponse {
                    success: true,
                    status_message,
                }))
            }
            Err(e) => Ok(Response::new(StopTaskResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }

    async fn get_tasklog(
        &self,
        request: Request<TasklogRequest>,
    ) -> Result<Response<TasklogResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        match self.state.scheduler.get_task(&req.task_id).await {
            Ok(Some(t)) => {
                if t.owner != claims.sub {
                    return Ok(Response::new(TasklogResponse {
                        success: false,
                        log: "Not authorized".into(),
                    }));
                }
                Ok(Response::new(TasklogResponse {
                    success: true,
                    log: t.output.unwrap_or_default(),
                }))
            }
            Ok(None) => Ok(Response::new(TasklogResponse {
                success: false,
                log: "Not found".into(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn download_task_artifact(
        &self,
        request: Request<DownloadTaskArtifactRequest>,
    ) -> Result<Response<DownloadTaskArtifactResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        let Some(task) = self
            .state
            .scheduler
            .get_task(&req.task_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        else {
            return Ok(Response::new(DownloadTaskArtifactResponse {
                success: false,
                status_message: "Task not found".into(),
                filename: String::new(),
                content_type: String::new(),
                data: vec![],
            }));
        };
        if task.owner != claims.sub && !is_admin(&claims.sub) {
            return Ok(Response::new(DownloadTaskArtifactResponse {
                success: false,
                status_message: "Not authorized".into(),
                filename: String::new(),
                content_type: String::new(),
                data: vec![],
            }));
        }

        #[derive(sqlx::FromRow)]
        struct ArtifactRow {
            artifact_key: String,
            storage_path: String,
        }
        let requested_artifact_key = req.artifact_key.trim();
        if (!req.artifact_key.is_empty() && requested_artifact_key.is_empty())
            || requested_artifact_key.len() > 255
        {
            return Ok(Response::new(DownloadTaskArtifactResponse {
                success: false,
                status_message: "Invalid artifact key".into(),
                filename: String::new(),
                content_type: String::new(),
                data: vec![],
            }));
        }
        let artifact: Option<ArtifactRow> = if requested_artifact_key.is_empty() {
            sqlx::query_as(
                "SELECT artifact_key, storage_path
                 FROM artifacts
                 WHERE task_id = $1 AND status = 'ready'
                 ORDER BY created_at DESC
                 LIMIT 1",
            )
            .bind(&req.task_id)
            .fetch_optional(&self.state.scheduler.database().pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        } else {
            sqlx::query_as(
                "SELECT artifact_key, storage_path
                 FROM artifacts
                 WHERE task_id = $1 AND artifact_key = $2 AND status = 'ready'
                 LIMIT 1",
            )
            .bind(&req.task_id)
            .bind(requested_artifact_key)
            .fetch_optional(&self.state.scheduler.database().pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        };
        let Some(artifact) = artifact else {
            return Ok(Response::new(DownloadTaskArtifactResponse {
                success: false,
                status_message: "Artifact not found".into(),
                filename: String::new(),
                content_type: String::new(),
                data: vec![],
            }));
        };
        let path = match canonical_artifact_path(
            &self.state.artifact_root,
            &PathBuf::from(&artifact.storage_path),
        )
        .await
        {
            Ok(path) => path,
            Err(status) => {
                return Ok(Response::new(DownloadTaskArtifactResponse {
                    success: false,
                    status_message: status.message().to_string(),
                    filename: String::new(),
                    content_type: String::new(),
                    data: vec![],
                }));
            }
        };
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(e) => {
                return Ok(Response::new(DownloadTaskArtifactResponse {
                    success: false,
                    status_message: format!("Failed to read artifact metadata: {e}"),
                    filename: String::new(),
                    content_type: String::new(),
                    data: vec![],
                }));
            }
        };
        if !metadata.is_file() {
            return Ok(Response::new(DownloadTaskArtifactResponse {
                success: false,
                status_message: "artifact storage path is not a file".into(),
                filename: String::new(),
                content_type: String::new(),
                data: vec![],
            }));
        }
        if metadata.len() > MAX_DOWNLOAD_ARTIFACT_BYTES as u64 {
            return Ok(Response::new(DownloadTaskArtifactResponse {
                success: false,
                status_message: format!(
                    "artifact is too large: {} bytes > {} bytes",
                    metadata.len(),
                    MAX_DOWNLOAD_ARTIFACT_BYTES
                ),
                filename: String::new(),
                content_type: String::new(),
                data: vec![],
            }));
        }
        let data = tokio::fs::read(&path)
            .await
            .map_err(|e| Status::internal(format!("Failed to read artifact: {}", e)))?;
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(&artifact.artifact_key)
            .to_string();
        Ok(Response::new(DownloadTaskArtifactResponse {
            success: true,
            status_message: "OK".into(),
            filename,
            content_type: "application/octet-stream".into(),
            data,
        }))
    }

    // ============================================================
    // Admin RPCs
    // ============================================================

    async fn get_admin_billing_overview(
        &self,
        request: Request<GetAdminBillingOverviewRequest>,
    ) -> Result<Response<GetAdminBillingOverviewResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(GetAdminBillingOverviewResponse {
                success: false,
                status_message: "Forbidden".into(),
                total_payer_debit_cpt: 0,
                total_provider_credit_cpt: 0,
                total_platform_fee_cpt: 0,
                pending_billing_tasks: 0,
                currency: "CPT".into(),
            }));
        }
        let r: (i64, i64, i64) = sqlx::query_as(
            "SELECT COALESCE(SUM(CASE WHEN kind='payer_debit' THEN amount_cpt ELSE 0 END),0)::BIGINT, COALESCE(SUM(CASE WHEN kind='provider_credit' THEN amount_cpt ELSE 0 END),0)::BIGINT, COALESCE(SUM(CASE WHEN kind='platform_fee' THEN amount_cpt ELSE 0 END),0)::BIGINT FROM ledger_entries WHERE status='settled'"
        ).fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        let pending: (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint FROM tasks WHERE billing_settled=false AND status IN ('COMPLETED','FAILED')"
        ).fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetAdminBillingOverviewResponse {
            success: true,
            status_message: "OK".into(),
            total_payer_debit_cpt: r.0,
            total_provider_credit_cpt: r.1,
            total_platform_fee_cpt: r.2,
            pending_billing_tasks: pending.0,
            currency: "CPT".into(),
        }))
    }

    async fn get_admin_artifact_overview(
        &self,
        request: Request<GetAdminArtifactOverviewRequest>,
    ) -> Result<Response<GetAdminArtifactOverviewResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(GetAdminArtifactOverviewResponse {
                success: false,
                status_message: "Forbidden".into(),
                total_artifacts: 0,
                total_size_bytes: 0,
                dedup_hits: 0,
                resumable_artifacts: 0,
                expiring_in_24h: 0,
            }));
        }
        let r: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*)::bigint, COALESCE(SUM(size_bytes),0)::bigint, COALESCE(SUM(CASE WHEN dedup_hit THEN 1 ELSE 0 END),0)::bigint, COALESCE(SUM(CASE WHEN resume_supported THEN 1 ELSE 0 END),0)::bigint, COALESCE(SUM(CASE WHEN expires_at IS NOT NULL AND expires_at <= NOW() + INTERVAL '24 hours' THEN 1 ELSE 0 END),0)::bigint FROM artifacts WHERE status='ready'"
        ).fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetAdminArtifactOverviewResponse {
            success: true,
            status_message: "OK".into(),
            total_artifacts: r.0,
            total_size_bytes: r.1,
            dedup_hits: r.2,
            resumable_artifacts: r.3,
            expiring_in_24h: r.4,
        }))
    }

    async fn cleanup_admin_artifacts(
        &self,
        request: Request<CleanupAdminArtifactsRequest>,
    ) -> Result<Response<CleanupAdminArtifactsResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(CleanupAdminArtifactsResponse {
                success: false,
                status_message: "Forbidden".into(),
                dry_run: req.dry_run,
                expired_candidates: 0,
                deleted_rows: 0,
                deleted_files: 0,
                file_delete_errors: 0,
            }));
        }
        let expired: (i64,) = sqlx::query_as("SELECT COUNT(*)::bigint FROM artifacts WHERE expires_at IS NOT NULL AND expires_at <= NOW()").fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        if req.dry_run {
            return Ok(Response::new(CleanupAdminArtifactsResponse {
                success: true,
                status_message: "Dry run".into(),
                dry_run: true,
                expired_candidates: expired.0,
                deleted_rows: 0,
                deleted_files: 0,
                file_delete_errors: 0,
            }));
        }
        let deleted: u64 = sqlx::query(
            "DELETE FROM artifacts WHERE expires_at IS NOT NULL AND expires_at <= NOW()",
        )
        .execute(&self.state.scheduler.database().pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .rows_affected();
        Ok(Response::new(CleanupAdminArtifactsResponse {
            success: true,
            status_message: "OK".into(),
            dry_run: false,
            expired_candidates: expired.0,
            deleted_rows: deleted as i64,
            deleted_files: 0,
            file_delete_errors: 0,
        }))
    }

    async fn get_admin_scheduling_cache_metrics(
        &self,
        request: Request<GetAdminSchedulingCacheMetricsRequest>,
    ) -> Result<Response<GetAdminSchedulingCacheMetricsResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(GetAdminSchedulingCacheMetricsResponse {
                success: false,
                status_message: "Forbidden".into(),
                total_completed_tasks: 0,
                total_cache_hits: 0,
                cache_hit_rate: 0.0,
                top_workers: vec![],
            }));
        }
        let total: (i64, i64) = sqlx::query_as("SELECT COUNT(*)::bigint, COALESCE(SUM(cache_hits),0)::bigint FROM tasks WHERE status='COMPLETED'").fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        let hit_rate = if total.0 > 0 {
            total.1 as f64 / total.0 as f64 * 100.0
        } else {
            0.0
        };
        #[derive(sqlx::FromRow)]
        struct CacheW {
            worker_id: String,
            completed_tasks: i64,
            cache_hits: i64,
            recent_completed_tasks_7d: i64,
        }
        let top: Vec<CacheW> = sqlx::query_as("SELECT w.worker_id, COUNT(*) FILTER(WHERE t.status='COMPLETED')::bigint as completed_tasks, COALESCE(SUM(t.cache_hits),0)::bigint as cache_hits, COUNT(*) FILTER(WHERE t.status='COMPLETED' AND t.completed_at >= NOW() - INTERVAL '7 days')::bigint as recent_completed_tasks_7d FROM worker_nodes w INNER JOIN tasks t ON t.worker_id = w.worker_id GROUP BY w.worker_id ORDER BY completed_tasks DESC LIMIT 10").fetch_all(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        let workers = top
            .into_iter()
            .map(|w| WorkerCacheAffinityMetric {
                worker_id: w.worker_id,
                completed_tasks: w.completed_tasks,
                cache_hits: w.cache_hits,
                recent_completed_tasks_7d: w.recent_completed_tasks_7d,
            })
            .collect();
        Ok(Response::new(GetAdminSchedulingCacheMetricsResponse {
            success: true,
            status_message: "OK".into(),
            total_completed_tasks: total.0,
            total_cache_hits: total.1,
            cache_hit_rate: hit_rate,
            top_workers: workers,
        }))
    }

    async fn get_admin_scheduling_cache_alert(
        &self,
        request: Request<GetAdminSchedulingCacheAlertRequest>,
    ) -> Result<Response<GetAdminSchedulingCacheAlertResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(GetAdminSchedulingCacheAlertResponse {
                success: false,
                status_message: "Forbidden".into(),
                low_threshold: 0.0,
                high_threshold: 0.0,
                cache_hit_rate: 0.0,
                severity: "unknown".into(),
                message: "Forbidden".into(),
            }));
        }
        let total: (i64, i64) = sqlx::query_as("SELECT COUNT(*)::bigint, COALESCE(SUM(cache_hits),0)::bigint FROM tasks WHERE status='COMPLETED'").fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        let hit_rate = if total.0 > 0 {
            total.1 as f64 / total.0 as f64 * 100.0
        } else {
            0.0
        };
        let low = if req.low_threshold > 0.0 {
            req.low_threshold
        } else {
            5.0
        };
        let high = if req.high_threshold > 0.0 {
            req.high_threshold
        } else {
            80.0
        };
        let (severity, msg) = if hit_rate < low {
            (
                "critical",
                format!(
                    "Cache hit rate {:.1}% below low threshold {:.1}%",
                    hit_rate, low
                ),
            )
        } else if hit_rate > high {
            (
                "warning",
                format!(
                    "Cache hit rate {:.1}% above high threshold {:.1}%",
                    hit_rate, high
                ),
            )
        } else {
            (
                "normal",
                format!("Cache hit rate {:.1}% within range", hit_rate),
            )
        };
        Ok(Response::new(GetAdminSchedulingCacheAlertResponse {
            success: true,
            status_message: "OK".into(),
            low_threshold: low,
            high_threshold: high,
            cache_hit_rate: hit_rate,
            severity: severity.into(),
            message: msg,
        }))
    }

    async fn list_admin_scheduling_cache_anomalies(
        &self,
        request: Request<ListAdminSchedulingCacheAnomaliesRequest>,
    ) -> Result<Response<ListAdminSchedulingCacheAnomaliesResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(ListAdminSchedulingCacheAnomaliesResponse {
                success: false,
                status_message: "Forbidden".into(),
                entries: vec![],
            }));
        }
        let limit = req.limit.clamp(1, 100);
        #[derive(sqlx::FromRow)]
        struct A {
            severity: String,
            cache_hit_rate: f64,
            low_threshold: f64,
            high_threshold: f64,
            message: String,
            created_at: chrono::DateTime<chrono::Utc>,
        }
        let rows: Vec<A> = sqlx::query_as("SELECT severity, cache_hit_rate, low_threshold, high_threshold, message, created_at FROM cache_alert_anomalies ORDER BY created_at DESC LIMIT $1").bind(limit).fetch_all(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListAdminSchedulingCacheAnomaliesResponse {
            success: true,
            status_message: "OK".into(),
            entries: rows
                .into_iter()
                .map(|r| CacheAnomalyEntry {
                    severity: r.severity,
                    cache_hit_rate: r.cache_hit_rate,
                    low_threshold: r.low_threshold,
                    high_threshold: r.high_threshold,
                    message: r.message,
                    created_at: r.created_at.to_rfc3339(),
                })
                .collect(),
        }))
    }

    async fn get_worker_trust_profile(
        &self,
        request: Request<GetWorkerTrustProfileRequest>,
    ) -> Result<Response<GetWorkerTrustProfileResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        let worker = self
            .state
            .node_manager
            .get_worker(&req.worker_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let Some(worker) = worker else {
            return Ok(Response::new(GetWorkerTrustProfileResponse {
                success: false,
                status_message: "Worker not found".into(),
                trust: None,
            }));
        };
        if worker.username != claims.sub && !is_admin(&claims.sub) {
            return Ok(Response::new(GetWorkerTrustProfileResponse {
                success: false,
                status_message: "Not authorized".into(),
                trust: None,
            }));
        }
        let pool = &self.state.scheduler.database().pool;
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id) VALUES ($1) ON CONFLICT (worker_id) DO NOTHING",
        )
        .bind(&req.worker_id)
        .execute(pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        #[derive(sqlx::FromRow)]
        struct TrustR {
            worker_id: String,
            successful_tasks: i64,
            failed_tasks: i64,
            score: i32,
            banned: bool,
            last_attested_at: Option<chrono::DateTime<chrono::Utc>>,
        }
        let row: TrustR = sqlx::query_as("SELECT worker_id, successful_tasks, failed_tasks, score, banned, last_attested_at FROM worker_reputation WHERE worker_id = $1").bind(&req.worker_id).fetch_one(pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetWorkerTrustProfileResponse {
            success: true,
            status_message: "OK".into(),
            trust: Some(WorkerTrustProfile {
                worker_id: row.worker_id,
                successful_tasks: row.successful_tasks,
                failed_tasks: row.failed_tasks,
                score: row.score,
                banned: row.banned,
                last_attested_at: row
                    .last_attested_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
            }),
        }))
    }

    async fn update_worker_trust_control(
        &self,
        request: Request<UpdateWorkerTrustControlRequest>,
    ) -> Result<Response<UpdateWorkerTrustControlResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(UpdateWorkerTrustControlResponse {
                success: false,
                status_message: "Forbidden".into(),
                worker_id: req.worker_id.clone(),
                banned: false,
                score: 0,
            }));
        }
        let worker_id = req.worker_id.trim().to_string();
        if !is_safe_worker_id(&worker_id) {
            return Ok(Response::new(UpdateWorkerTrustControlResponse {
                success: false,
                status_message: "Invalid worker_id".into(),
                worker_id: String::new(),
                banned: false,
                score: 0,
            }));
        }
        let exists: (bool,) =
            sqlx::query_as("SELECT EXISTS(SELECT 1 FROM worker_reputation WHERE worker_id=$1)")
                .bind(&worker_id)
                .fetch_one(&self.state.scheduler.database().pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        if exists.0 {
            sqlx::query("UPDATE worker_reputation SET banned=$1, score=$2, updated_at=NOW() WHERE worker_id=$3").bind(req.banned).bind(req.score).bind(&worker_id).execute(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        } else {
            sqlx::query("INSERT INTO worker_reputation (worker_id, banned, score, successful_tasks, failed_tasks) VALUES ($1, $2, $3, 0, 0)").bind(&worker_id).bind(req.banned).bind(req.score).execute(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        }
        Ok(Response::new(UpdateWorkerTrustControlResponse {
            success: true,
            status_message: "OK".into(),
            worker_id,
            banned: req.banned,
            score: req.score,
        }))
    }

    async fn list_admin_worker_trust(
        &self,
        request: Request<ListAdminWorkerTrustRequest>,
    ) -> Result<Response<ListAdminWorkerTrustResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(ListAdminWorkerTrustResponse {
                success: false,
                status_message: "Forbidden".into(),
                entries: vec![],
            }));
        }
        #[derive(sqlx::FromRow)]
        struct TrustLR {
            worker_id: String,
            username: String,
            worker_status: String,
            score: i32,
            banned: bool,
            successful_tasks: i64,
            failed_tasks: i64,
            last_attested_at: Option<chrono::DateTime<chrono::Utc>>,
        }
        let pool = &self.state.scheduler.database().pool;
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id)
             SELECT worker_id FROM worker_nodes
             ON CONFLICT (worker_id) DO NOTHING",
        )
        .execute(pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        let rows: Vec<TrustLR> = sqlx::query_as("SELECT w.worker_id, w.username, w.status as worker_status, r.score, r.banned, r.successful_tasks, r.failed_tasks, r.last_attested_at FROM worker_nodes w INNER JOIN worker_reputation r ON r.worker_id = w.worker_id ORDER BY r.score DESC").fetch_all(pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListAdminWorkerTrustResponse {
            success: true,
            status_message: "OK".into(),
            entries: rows
                .into_iter()
                .map(|r| AdminWorkerTrustEntry {
                    worker_id: r.worker_id,
                    username: r.username,
                    worker_status: r.worker_status,
                    score: r.score,
                    banned: r.banned,
                    successful_tasks: r.successful_tasks,
                    failed_tasks: r.failed_tasks,
                    last_attested_at: r
                        .last_attested_at
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_default(),
                })
                .collect(),
        }))
    }

    async fn list_admin_audit_logs(
        &self,
        request: Request<ListAdminAuditLogsRequest>,
    ) -> Result<Response<ListAdminAuditLogsResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) {
            return Ok(Response::new(ListAdminAuditLogsResponse {
                success: false,
                status_message: "Forbidden".into(),
                entries: vec![],
            }));
        }
        let limit = req.limit.clamp(1, 500);
        #[derive(sqlx::FromRow)]
        struct AuditR {
            id: uuid::Uuid,
            admin_user: String,
            action: String,
            target_type: String,
            target_id: String,
            detail: sqlx::types::Json<serde_json::Value>,
            created_at: chrono::DateTime<chrono::Utc>,
        }
        let rows: Vec<AuditR> = sqlx::query_as("SELECT id, admin_user, action, target_type, target_id, detail, created_at FROM admin_audit_logs ORDER BY created_at DESC LIMIT $1").bind(limit).fetch_all(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListAdminAuditLogsResponse {
            success: true,
            status_message: "OK".into(),
            entries: rows
                .into_iter()
                .map(|r| AdminAuditLogEntry {
                    id: r.id.to_string(),
                    username: r.admin_user,
                    action: r.action,
                    resource: format!("{}:{}", r.target_type, r.target_id),
                    details: r.detail.to_string(),
                    created_at: r.created_at.to_rfc3339(),
                })
                .collect(),
        }))
    }
    async fn quote_task(
        &self,
        request: Request<QuoteTaskRequest>,
    ) -> Result<Response<QuoteTaskResponse>, Status> {
        let req = request.into_inner();
        self.state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        if validate_quote_resources(
            req.cpu_score,
            req.gpu_score,
            req.memory_gb,
            req.gpu_memory_gb,
            req.storage_gb,
            req.host_count,
        )
        .is_err()
        {
            return Ok(Response::new(QuoteTaskResponse {
                success: false,
                quoted_cpt: 0,
                currency: "CPT".into(),
                breakdown: None,
            }));
        }
        let breakdown = quote_breakdown(
            req.cpu_score,
            req.gpu_score,
            req.memory_gb,
            req.gpu_memory_gb,
            req.storage_gb,
            req.host_count,
        );
        Ok(Response::new(QuoteTaskResponse {
            success: true,
            quoted_cpt: breakdown.total,
            currency: "CPT".into(),
            breakdown: Some(breakdown),
        }))
    }

    async fn get_provider_earnings(
        &self,
        request: Request<GetProviderEarningsRequest>,
    ) -> Result<Response<GetProviderEarningsResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        let pool = &self.state.scheduler.database().pool;
        let total: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount_cpt), 0)::BIGINT
             FROM ledger_entries
             WHERE kind = 'provider_credit'
               AND (provider_user = $1 OR provider_worker_id IN (
                    SELECT worker_id FROM worker_nodes WHERE username = $1
               ))",
        )
        .bind(&claims.sub)
        .fetch_one(pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        let limit = req.limit.clamp(1, 100);
        #[derive(sqlx::FromRow)]
        struct EarningsRow {
            task_id: String,
            payer_user: String,
            provider_worker_id: Option<String>,
            amount_cpt: i64,
            status: String,
            created_at: chrono::DateTime<chrono::Utc>,
        }
        let rows: Vec<EarningsRow> = sqlx::query_as(
            "SELECT task_id, payer_user, provider_worker_id, amount_cpt, status, created_at
             FROM ledger_entries
             WHERE kind = 'provider_credit'
               AND (provider_user = $1 OR provider_worker_id IN (
                    SELECT worker_id FROM worker_nodes WHERE username = $1
               ))
             ORDER BY created_at DESC
             LIMIT $2",
        )
        .bind(&claims.sub)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetProviderEarningsResponse {
            success: true,
            status_message: "OK".into(),
            total_earned_cpt: total,
            currency: "CPT".into(),
            entries: rows
                .into_iter()
                .map(|row| ProviderEarningsEntry {
                    task_id: row.task_id,
                    payer_user: row.payer_user,
                    provider_worker_id: row.provider_worker_id.unwrap_or_default(),
                    amount_cpt: row.amount_cpt,
                    status: row.status,
                    created_at: row.created_at.to_rfc3339(),
                })
                .collect(),
        }))
    }

    async fn get_provider_worker_settings(
        &self,
        request: Request<GetProviderWorkerSettingsRequest>,
    ) -> Result<Response<GetProviderWorkerSettingsResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        let Some(worker) = self
            .state
            .node_manager
            .get_worker(&req.worker_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        else {
            return Ok(Response::new(GetProviderWorkerSettingsResponse {
                success: false,
                message: "Worker not found".into(),
                worker_id: req.worker_id,
                settings: None,
            }));
        };
        if worker.username != claims.sub && !is_admin(&claims.sub) {
            return Ok(Response::new(GetProviderWorkerSettingsResponse {
                success: false,
                message: "Not authorized".into(),
                worker_id: req.worker_id,
                settings: None,
            }));
        }
        Ok(Response::new(GetProviderWorkerSettingsResponse {
            success: true,
            message: "OK".into(),
            worker_id: worker.worker_id.clone(),
            settings: Some(provider_settings_from_worker(&worker)),
        }))
    }

    async fn update_provider_worker_settings(
        &self,
        request: Request<UpdateProviderWorkerSettingsRequest>,
    ) -> Result<Response<UpdateProviderWorkerSettingsResponse>, Status> {
        let req = request.into_inner();
        let claims = self
            .state
            .auth
            .validate_token(&req.token)
            .map_err(|_| Status::unauthenticated("Invalid token"))?;
        let Some(current) = self
            .state
            .node_manager
            .get_worker(&req.worker_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        else {
            return Ok(Response::new(UpdateProviderWorkerSettingsResponse {
                success: false,
                message: "Worker not found".into(),
                worker_id: req.worker_id,
                settings: None,
            }));
        };
        if current.username != claims.sub && !is_admin(&claims.sub) {
            return Ok(Response::new(UpdateProviderWorkerSettingsResponse {
                success: false,
                message: "Not authorized".into(),
                worker_id: req.worker_id,
                settings: None,
            }));
        }
        let Some(settings) = req.settings else {
            return Ok(Response::new(UpdateProviderWorkerSettingsResponse {
                success: false,
                message: "settings is required".into(),
                worker_id: req.worker_id,
                settings: None,
            }));
        };
        let updated: WorkerNode = sqlx::query_as(
            "UPDATE worker_nodes
             SET provider_enabled = $1,
                 cpu_cores_limit = $2,
                 memory_gb_limit = $3,
                 gpu_memory_gb_limit = $4,
                 storage_gb_limit = $5,
                 min_cpt_per_hour = $6,
                 updated_at = NOW()
             WHERE worker_id = $7
             RETURNING *",
        )
        .bind(settings.enabled)
        .bind(settings.cpu_cores_limit.max(0))
        .bind(settings.memory_gb_limit.max(0))
        .bind(settings.gpu_memory_gb_limit.max(0))
        .bind(settings.storage_gb_limit.max(0))
        .bind(settings.min_cpt_per_hour.max(0))
        .bind(&req.worker_id)
        .fetch_one(&self.state.scheduler.database().pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(UpdateProviderWorkerSettingsResponse {
            success: true,
            message: "OK".into(),
            worker_id: updated.worker_id.clone(),
            settings: Some(provider_settings_from_worker(&updated)),
        }))
    }
}

#[tonic::async_trait]
impl BatchRuntimeService for GrpcBatchRuntimeService {
    async fn pull_batch(
        &self,
        request: Request<PullBatchRequest>,
    ) -> Result<Response<PullBatchResponse>, Status> {
        let req = request.into_inner();
        match authorize_worker_identity(&self.state, &req.token, &req.worker_id).await? {
            ReportAuthorization::Authorized => {}
            ReportAuthorization::Denied(status_message) => {
                return Err(worker_authorization_response(status_message));
            }
        }

        let worker = match self.state.node_manager.get_worker(&req.worker_id).await {
            Ok(Some(worker)) => worker,
            Ok(None) => return Err(Status::not_found("Worker not found")),
            Err(e) => return Err(Status::internal(e.to_string())),
        };

        let limit = req.max_inflight_batches.max(1) as i64;
        let claimed = self
            .state
            .scheduler
            .claim_pending_for_worker(&worker.worker_id, &worker.ip, limit)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let leases = claimed.iter().map(task_lease_from_task).collect();

        Ok(Response::new(PullBatchResponse {
            success: true,
            status_message: "OK".into(),
            batch_id: uuid::Uuid::new_v4().to_string(),
            tasks: leases,
        }))
    }

    async fn complete_batch(
        &self,
        request: Request<CompleteBatchRequest>,
    ) -> Result<Response<CompleteBatchResponse>, Status> {
        let req = request.into_inner();
        match authorize_worker_identity(&self.state, &req.token, &req.worker_id).await? {
            ReportAuthorization::Authorized => {}
            ReportAuthorization::Denied(status_message) => {
                return Err(worker_authorization_response(status_message));
            }
        }
        if req.tasks.iter().any(|task| !is_safe_task_id(&task.task_id)) {
            return Err(Status::invalid_argument("Invalid task_id"));
        }
        for task in req.tasks {
            let artifact_refs: Vec<String> = task
                .result_artifact_refs
                .iter()
                .chain(std::iter::once(&task.stdout_artifact_ref))
                .chain(std::iter::once(&task.stderr_artifact_ref))
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect();
            let report_output = batch_task_output_refs(&task);
            let metrics = task.metrics.as_ref();
            if task.status.eq_ignore_ascii_case("COMPLETED") {
                self.state
                    .scheduler
                    .complete_task_for_worker(
                        &task.task_id,
                        &req.worker_id,
                        task.result_artifact_refs.first().map(String::as_str),
                        None,
                    )
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
            } else {
                self.state
                    .scheduler
                    .fail_task_for_worker(
                        &task.task_id,
                        &req.worker_id,
                        &format!("Batch task ended with status {}", task.status),
                    )
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
            }
            self.state
                .scheduler
                .record_batch_task_report_for_worker(
                    &task.task_id,
                    &req.worker_id,
                    BatchTaskReport {
                        output: report_output.as_deref(),
                        cpu_time_ms: metrics.map(|m| m.cpu_time_ms).unwrap_or(0),
                        wall_time_ms: metrics.map(|m| m.wall_time_ms).unwrap_or(0),
                        peak_memory_mb: metrics.map(|m| m.peak_memory_mb).unwrap_or(0),
                        download_bytes: metrics.map(|m| m.download_bytes).unwrap_or(0),
                        cache_hits: metrics.map(|m| m.cache_hits).unwrap_or(0),
                    },
                )
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            for artifact_ref in artifact_refs {
                register_reported_artifact_ref(
                    &self.state.scheduler.database().pool,
                    &self.state.artifact_root,
                    &task.task_id,
                    &artifact_ref,
                )
                .await?;
            }
        }

        Ok(Response::new(CompleteBatchResponse {
            success: true,
            status_message: "OK".into(),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let req = request.into_inner();
        match authorize_worker_identity(&self.state, &req.token, &req.worker_id).await? {
            ReportAuthorization::Authorized => {}
            ReportAuthorization::Denied(status_message) => {
                return Err(worker_authorization_response(status_message));
            }
        }
        self.state
            .node_manager
            .update_heartbeat(&req.worker_id, &req.status, 0.0, 0.0, 0.0, 0.0)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(HeartbeatResponse {
            success: true,
            status_message: "OK".into(),
        }))
    }
}

fn batch_task_output_refs(task: &hivemind_proto::CompletedTask) -> Option<String> {
    let mut refs = Vec::new();
    if !task.stdout_artifact_ref.trim().is_empty() {
        refs.push(format!(
            "stdout_artifact_ref={}",
            task.stdout_artifact_ref.trim()
        ));
    }
    if !task.stderr_artifact_ref.trim().is_empty() {
        refs.push(format!(
            "stderr_artifact_ref={}",
            task.stderr_artifact_ref.trim()
        ));
    }
    if refs.is_empty() {
        None
    } else {
        Some(refs.join("\n"))
    }
}

fn task_lease_from_task(task: &Task) -> TaskLease {
    TaskLease {
        task_id: task.task_id.clone(),
        execution_package: Some(ExecutionPackage {
            runtime_version: "hivemind-rs".into(),
            task_code_ref: task.torrent_source.clone().unwrap_or_default(),
            artifact_refs: vec![],
            constraints: Default::default(),
        }),
        artifacts: vec![],
        resource_limits: Some(ProtoResourceSpec {
            cpu_cores: 0,
            memory_mb: task.req_memory_gb as i64 * 1024,
            gpu_count: 0,
            gpu_name: String::new(),
            vram_mb: task.req_gpu_memory_gb as i64 * 1024,
            cpu_score: task.req_cpu_score,
            gpu_score: task.req_gpu_score,
            storage_total_gb: task.req_storage_gb,
            storage_available_gb: task.req_storage_gb,
        }),
        deadline_unix: task
            .deadline
            .map(|deadline| deadline.timestamp())
            .unwrap_or_default(),
        priority: task.priority,
    }
}

fn task_info_from_task(task: Task) -> TaskInfo {
    TaskInfo {
        task_id: task.task_id,
        owner: task.owner,
        status: task.status.as_str().into(),
        status_message: task.status_message.unwrap_or_default(),
        worker_ip: task.worker_ip.unwrap_or_default(),
        output: task.output.unwrap_or_default(),
        result_torrent: task.result_torrent.unwrap_or_default(),
        billed_amount: task.billed_amount,
        billing_settled: task.billing_settled,
        retry_count: task.retry_count,
        wall_time_ms: task.wall_time_ms,
        peak_memory_mb: task.peak_memory_mb,
        cpu_usage: task.cpu_usage,
        memory_usage: task.memory_usage,
        gpu_usage: task.gpu_usage,
        gpu_memory_usage: task.gpu_memory_usage,
        deterministic: task.deterministic,
    }
}

fn proto_resource_spec_to_model(r: hivemind_proto::ResourceSpec) -> hivemind_models::ResourceSpec {
    hivemind_models::ResourceSpec {
        cpu_cores: r.cpu_cores,
        memory_mb: r.memory_mb,
        gpu_count: r.gpu_count,
        gpu_name: r.gpu_name,
        vram_mb: r.vram_mb,
        cpu_score: r.cpu_score,
        gpu_score: r.gpu_score,
        storage_total_gb: r.storage_total_gb,
        storage_available_gb: r.storage_available_gb,
    }
}

fn validate_worker_registration_resources(
    resources: &ProtoResourceSpec,
) -> Result<(), &'static str> {
    if resources.cpu_cores < 0
        || resources.memory_mb < 0
        || resources.gpu_count < 0
        || resources.vram_mb < 0
        || resources.cpu_score < 0
        || resources.gpu_score < 0
        || resources.storage_total_gb < 0
        || resources.storage_available_gb < 0
    {
        return Err("worker capacity values must be non-negative");
    }
    if resources.storage_available_gb > resources.storage_total_gb {
        return Err("worker storage_available_gb cannot exceed storage_total_gb");
    }
    Ok(())
}

fn worker_info_from_model(worker: WorkerNode) -> WorkerInfo {
    WorkerInfo {
        worker_id: worker.worker_id,
        username: worker.username,
        ip: worker.ip,
        status: worker.status.as_str().into(),
        cpu_cores: worker.cpu_cores,
        memory_gb: worker.memory_gb,
        cpu_score: worker.cpu_score,
        gpu_score: worker.gpu_score,
        gpu_memory_gb: worker.gpu_memory_gb,
        gpu_name: worker.gpu_name.unwrap_or_default(),
        vram_mb: worker.vram_mb,
        storage_total_gb: worker.storage_total_gb,
        storage_available_gb: worker.storage_available_gb,
        provider_enabled: worker.provider_enabled,
        cpu_cores_limit: worker.cpu_cores_limit,
        memory_gb_limit: worker.memory_gb_limit,
        gpu_memory_gb_limit: worker.gpu_memory_gb_limit,
        storage_gb_limit: worker.storage_gb_limit,
        min_cpt_per_hour: worker.min_cpt_per_hour,
        location: worker.location,
        cpu_usage: worker.cpu_usage,
        memory_usage: worker.memory_usage,
        gpu_usage: worker.gpu_usage,
        gpu_memory_usage: worker.gpu_memory_usage,
    }
}

fn provider_settings_from_worker(worker: &WorkerNode) -> ProviderWorkerSettings {
    ProviderWorkerSettings {
        enabled: worker.provider_enabled,
        cpu_cores_limit: worker.cpu_cores_limit,
        memory_gb_limit: worker.memory_gb_limit,
        gpu_memory_gb_limit: worker.gpu_memory_gb_limit,
        storage_gb_limit: worker.storage_gb_limit,
        min_cpt_per_hour: worker.min_cpt_per_hour,
    }
}

fn validate_quote_resources(
    cpu_score: i32,
    gpu_score: i32,
    memory_gb: i32,
    gpu_memory_gb: i32,
    storage_gb: i64,
    host_count: i32,
) -> Result<(), &'static str> {
    if cpu_score < 0 || gpu_score < 0 || memory_gb < 0 || gpu_memory_gb < 0 || storage_gb < 0 {
        return Err("task resource values must be non-negative");
    }
    if host_count < 1 {
        return Err("host_count must be at least 1");
    }
    Ok(())
}

fn validate_task_submission_resources(
    resources: &ProtoResourceSpec,
    host_count: i32,
    max_cpt: i64,
) -> Result<(), &'static str> {
    if resources.cpu_score < 0
        || resources.gpu_score < 0
        || resources.memory_mb < 0
        || resources.vram_mb < 0
        || resources.storage_total_gb < 0
        || resources.storage_available_gb < 0
    {
        return Err("task resource values must be non-negative");
    }
    if host_count < 1 {
        return Err("host_count must be at least 1");
    }
    if max_cpt < 0 {
        return Err("max_cpt must be non-negative");
    }
    Ok(())
}

fn quote_breakdown(
    cpu_score: i32,
    gpu_score: i32,
    memory_gb: i32,
    gpu_memory_gb: i32,
    storage_gb: i64,
    host_count: i32,
) -> PricingBreakdown {
    let base = 10;
    let cpu = i64::from(cpu_score.max(0)) / 10;
    let gpu = i64::from(gpu_score.max(0)) / 5;
    let memory = i64::from(memory_gb.max(0)) * 2;
    let gpu_memory = i64::from(gpu_memory_gb.max(0)) * 3;
    let storage = storage_gb.max(0);
    let hosts = i64::from(host_count.max(1));
    let per_host_total = base + cpu + gpu + memory + gpu_memory + storage;
    PricingBreakdown {
        base,
        cpu,
        gpu,
        memory,
        gpu_memory,
        storage,
        host_count: hosts,
        per_host_total,
        total: per_host_total * hosts,
    }
}

// Admin helpers
fn is_admin(sub: &str) -> bool {
    std::env::var("HIVEMIND_ADMIN_USERS")
        .ok()
        .map(|users| {
            users
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .any(|s| s == sub)
        })
        .unwrap_or(false)
}

fn resource_usage_is_finite(usage: &hivemind_proto::ResourceUsage) -> bool {
    usage.cpu_percent.is_finite()
        && usage.memory_percent.is_finite()
        && usage.gpu_percent.is_finite()
        && usage.vram_percent.is_finite()
        && usage.storage_percent.is_finite()
}

enum WorkerStopDispatch {
    NotAssigned,
    Requested,
    NotConfirmed(String),
}

fn encode_worker_execution_claims(secret: &str, claims: &Claims) -> anyhow::Result<String> {
    JwtService::new(secret, 1).encode_claims(claims)
}

async fn request_worker_stop(task: &Task, token: &str) -> WorkerStopDispatch {
    let Some(worker_ip) = task
        .worker_ip
        .as_deref()
        .map(str::trim)
        .filter(|ip| !ip.is_empty())
    else {
        return WorkerStopDispatch::NotAssigned;
    };
    let endpoint = match worker_endpoint(worker_ip) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            tracing::warn!(
                "Task {} cancellation recorded but worker stop endpoint is invalid for {}: {}",
                task.task_id,
                worker_ip,
                error
            );
            return WorkerStopDispatch::NotConfirmed(format!("invalid worker endpoint: {error}"));
        }
    };
    let mut client = match WorkerNodeServiceClient::connect(endpoint.clone()).await {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(
                "Task {} cancellation recorded but worker stop connect failed at {}: {}",
                task.task_id,
                endpoint,
                error
            );
            return WorkerStopDispatch::NotConfirmed(format!("connect failed at {endpoint}"));
        }
    };
    match client
        .stop_task_execution(StopTaskExecutionRequest {
            task_id: task.task_id.clone(),
            token: token.to_string(),
        })
        .await
    {
        Ok(response) => {
            let response = response.into_inner();
            if response.success {
                WorkerStopDispatch::Requested
            } else {
                let message = if response.status_message.trim().is_empty() {
                    "worker returned an unsuccessful stop response".to_string()
                } else {
                    response.status_message
                };
                tracing::warn!(
                    "Task {} cancellation recorded but worker stop was not confirmed: {}",
                    task.task_id,
                    message
                );
                WorkerStopDispatch::NotConfirmed(message)
            }
        }
        Err(error) => {
            tracing::warn!(
                "Task {} cancellation recorded but worker stop RPC failed: {}",
                task.task_id,
                error
            );
            WorkerStopDispatch::NotConfirmed("RPC failed".into())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hivemind_config::HivemindConfig;
    use hivemind_models::Claims;
    use hivemind_proto::{
        worker_node_service_server::{WorkerNodeService, WorkerNodeServiceServer},
        ExecuteTaskRequest, ExecuteTaskResponse, StopTaskExecutionRequest,
        StopTaskExecutionResponse, TaskOutputRequest, TaskOutputResponse, TaskOutputUploadRequest,
        TaskOutputUploadResponse, TaskResultUploadRequest, TaskResultUploadResponse,
        TaskUsageRequest, TaskUsageResponse,
    };
    use std::net::SocketAddr;
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;

    fn grpc_db_lock() -> Arc<tokio::sync::Mutex<()>> {
        static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
        LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    struct AdminUsersEnvGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl AdminUsersEnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var_os("HIVEMIND_ADMIN_USERS");
            std::env::set_var("HIVEMIND_ADMIN_USERS", value);
            Self { previous }
        }
    }

    impl Drop for AdminUsersEnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("HIVEMIND_ADMIN_USERS", value),
                None => std::env::remove_var("HIVEMIND_ADMIN_USERS"),
            }
        }
    }

    #[test]
    fn cancellation_token_uses_worker_execution_secret() {
        // Given: cancellation claims and separate trust-domain secrets.
        let now = Utc::now().timestamp() as usize;
        let claims = Claims {
            sub: "task-owner".into(),
            user_id: "task-owner".into(),
            role: Some("worker-execution".into()),
            task_id: Some("cancel-task".into()),
            worker_id: Some("worker-7".into()),
            exp: now + 300,
            iat: now,
        };
        let worker_secret = "unit-test-worker-execution-secret-at-least-32-bytes";
        let control_secret = "unit-test-control-plane-secret-at-least-32-bytes";

        // When: nodepool creates the cancellation token.
        let token = encode_worker_execution_claims(worker_secret, &claims).unwrap();

        // Then: only the worker-execution trust domain can decode it.
        assert!(
            hivemind_auth::jwt_service::JwtService::new(worker_secret, 1)
                .decode(&token)
                .is_ok()
        );
        assert!(
            hivemind_auth::jwt_service::JwtService::new(control_secret, 1)
                .decode(&token)
                .is_err()
        );
    }

    async fn test_service() -> Option<(GrpcMasterNodeService, String, String, String)> {
        let config = HivemindConfig::for_test();
        let fixture = hivemind_database::postgres::create_isolated_test_pool_with_config(
            &config,
            "node_manager_grpc",
        )
        .await
        .ok()?;
        if hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .is_err()
        {
            fixture.cleanup().await.ok();
            return None;
        }
        let db = hivemind_database::DatabaseManager {
            pool: fixture.pool.clone(),
        };
        let auth = AuthManager::new(&db, "grpc-owner-test-secret", 24);
        let scheduler = TaskScheduler::new(db.clone(), auth.clone());
        let node_manager = Arc::new(NodeManager::new(&config, db.clone()));
        let state = Arc::new(NodepoolState {
            auth: auth.clone(),
            worker_execution_secret: config.auth.worker_execution_secret.clone(),
            node_manager,
            scheduler: scheduler.clone(),
            artifact_root: artifact_root_for_config(&config),
            distribution: None,
        });
        let service = GrpcMasterNodeService::new(state);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("grpc-owner-task-{unique}");
        let owner = format!("grpc-owner-{unique}");
        let other = format!("grpc-other-{unique}");
        let task = make_task(&task_id, &owner);
        if scheduler.create_task(&task).await.is_err() {
            fixture.cleanup().await.ok();
            return None;
        }
        let other_token = token_for(&auth, &other);
        Some((service, task_id, other_token, owner))
    }

    #[tokio::test]
    async fn test_service_uses_isolated_schema() {
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };

        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&service.state.scheduler.database().pool)
            .await
            .unwrap();

        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(
            schema.starts_with("hm_test_"),
            "node-manager gRPC tests must use an isolated schema, got {schema}"
        );
    }

    #[tokio::test]
    async fn register_user_rejects_configured_admin_username() {
        // Given: a configured admin username and a real isolated registration database.
        let lock = grpc_db_lock();
        let _lock_guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = test_service()
            .await
            .expect("registration security tests require PostgreSQL");
        let admin_username = format!("reserved-admin-{}", uuid::Uuid::new_v4());
        let _env_guard = AdminUsersEnvGuard::set(&format!("other-admin, {admin_username}"));
        let user_service = GrpcUserService::new(service.state.clone());

        // When: the public gRPC registration surface receives that username.
        let response = user_service
            .register_user(Request::new(RegisterUserRequest {
                username: admin_username.clone(),
                password: "valid-test-password".into(),
            }))
            .await
            .unwrap()
            .into_inner();

        // Then: registration is denied and no account is persisted.
        let persisted: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
                .bind(&admin_username)
                .fetch_one(&service.state.scheduler.database().pool)
                .await
                .unwrap();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&admin_username)
            .execute(&service.state.scheduler.database().pool)
            .await
            .unwrap();
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(!persisted);
    }

    #[tokio::test]
    async fn register_user_allows_non_admin_username_when_admin_is_configured() {
        // Given: a configured admin username and a distinct public username.
        let lock = grpc_db_lock();
        let _lock_guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = test_service()
            .await
            .expect("registration security tests require PostgreSQL");
        let configured_admin = format!("reserved-admin-{}", uuid::Uuid::new_v4());
        let username = format!("public-user-{}", uuid::Uuid::new_v4());
        let _env_guard = AdminUsersEnvGuard::set(&configured_admin);
        let user_service = GrpcUserService::new(service.state.clone());

        // When: the public gRPC registration surface receives the non-admin username.
        let response = user_service
            .register_user(Request::new(RegisterUserRequest {
                username: username.clone(),
                password: "valid-test-password".into(),
            }))
            .await
            .unwrap()
            .into_inner();

        // Then: the account is created in the real registration store.
        let persisted: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
                .bind(&username)
                .fetch_one(&service.state.scheduler.database().pool)
                .await
                .unwrap();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&service.state.scheduler.database().pool)
            .await
            .unwrap();
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(response.success);
        assert!(persisted);
    }

    #[tokio::test]
    async fn test_upload_task_rejects_single_dot_task_id() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);

        let response = service
            .upload_task(Request::new(UploadTaskRequest {
                task_id: ".".into(),
                torrent: "btih:test-input".into(),
                requirements: Some(ProtoResourceSpec {
                    cpu_cores: 1,
                    memory_mb: 1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: 100,
                    gpu_score: 0,
                    storage_total_gb: 1,
                    storage_available_gb: 1,
                }),
                location: String::new(),
                host_count: 1,
                token: owner_token,
                max_cpt: 10,
                runtime: String::new(),
                task_source: String::new(),
                package_data: Default::default(),
                package_filename: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        let dot_task = service.state.scheduler.get_task(".").await.unwrap();
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response.status_message.contains("task_id"));
        assert!(dot_task.is_none());
    }

    #[tokio::test]
    async fn test_quote_task_rejects_invalid_resource_values() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);

        let response = service
            .quote_task(Request::new(QuoteTaskRequest {
                token: owner_token,
                cpu_score: -10,
                gpu_score: 0,
                memory_gb: -1,
                gpu_memory_gb: 0,
                storage_gb: 1,
                host_count: 1,
            }))
            .await
            .unwrap()
            .into_inner();

        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert_eq!(response.quoted_cpt, 0);
    }

    #[tokio::test]
    async fn test_upload_task_rejects_invalid_resource_values_before_insert() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);
        let invalid_task_id = format!("grpc-invalid-resources-{task_id}");

        let response = service
            .upload_task(Request::new(UploadTaskRequest {
                task_id: invalid_task_id.clone(),
                torrent: "btih:test-input".into(),
                requirements: Some(ProtoResourceSpec {
                    cpu_cores: 1,
                    memory_mb: -1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: -100,
                    gpu_score: 0,
                    storage_total_gb: 1,
                    storage_available_gb: 1,
                }),
                location: String::new(),
                host_count: 0,
                token: owner_token,
                max_cpt: -1,
                runtime: String::new(),
                task_source: String::new(),
                package_data: Default::default(),
                package_filename: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = service
            .state
            .scheduler
            .get_task(&invalid_task_id)
            .await
            .unwrap();
        cleanup(&service.state.scheduler, &invalid_task_id, &owner).await;
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response.status_message.contains("resource"));
        assert!(stored.is_none());
    }

    #[tokio::test]
    async fn test_register_worker_node_rejects_single_dot_worker_id() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let owner_token = token_for(&master_service.state.auth, &owner);

        let response = node_service
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: owner.clone(),
                worker_id: ".".into(),
                ip: "10.77.9.10:50053".into(),
                resources: Some(ProtoResourceSpec {
                    cpu_cores: 4,
                    memory_mb: 16 * 1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: 400,
                    gpu_score: 0,
                    storage_total_gb: 500,
                    storage_available_gb: 250,
                }),
                location: "local".into(),
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let dot_worker = master_service
            .state
            .node_manager
            .get_worker(".")
            .await
            .unwrap();
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response.status_message.contains("worker_id"));
        assert!(dot_worker.is_none());
    }

    #[tokio::test]
    async fn test_register_worker_node_rejects_invalid_capacity_before_insert() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("grpc-invalid-capacity-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);

        let response = node_service
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: owner.clone(),
                worker_id: worker_id.clone(),
                ip: "10.77.9.10:50053".into(),
                resources: Some(ProtoResourceSpec {
                    cpu_cores: -1,
                    memory_mb: -1024,
                    gpu_count: -1,
                    gpu_name: String::new(),
                    vram_mb: -1024,
                    cpu_score: -10,
                    gpu_score: -10,
                    storage_total_gb: 100,
                    storage_available_gb: 200,
                }),
                location: "local".into(),
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .node_manager
            .get_worker(&worker_id)
            .await
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response.status_message.contains("capacity"));
        assert!(stored.is_none());
    }

    #[tokio::test]
    async fn test_register_worker_node_rejects_mismatched_token_subject() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("grpc-register-spoof-{task_id}");

        let response = node_service
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: owner.clone(),
                worker_id: worker_id.clone(),
                ip: "10.77.9.10:50053".into(),
                resources: Some(ProtoResourceSpec {
                    cpu_cores: 4,
                    memory_mb: 16 * 1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: 400,
                    gpu_score: 0,
                    storage_total_gb: 500,
                    storage_available_gb: 250,
                }),
                location: "local".into(),
                token: other_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .node_manager
            .get_worker(&worker_id)
            .await
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert_eq!(response.status_message, "Not authorized");
        assert!(stored.is_none());
    }

    #[tokio::test]
    async fn test_remove_worker_rejects_single_dot_worker_id() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(service.state.clone());
        let owner_token = token_for(&service.state.auth, &owner);

        let response = node_service
            .remove_worker(Request::new(RemoveWorkerRequest {
                worker_id: ".".into(),
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();

        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response.status_message.contains("worker_id"));
    }

    #[tokio::test]
    async fn test_get_task_result_rejects_non_owner() {
        let (service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };

        let response = service
            .get_task_result(Request::new(GetTaskResultRequest {
                token: other_token,
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!response.success);
        assert_eq!(response.status_message, "Not authorized");
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_get_tasklog_rejects_non_owner() {
        let (service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };

        let response = service
            .get_tasklog(Request::new(TasklogRequest {
                token: other_token,
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!response.success);
        assert_eq!(response.log, "Not authorized");
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_stop_task_rejects_non_owner() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };

        let response = service
            .stop_task(Request::new(StopTaskRequest {
                token: other_token,
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!response.success);
        assert_eq!(response.status_message, "Not authorized");
        let stored = service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Pending);
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_stop_task_reports_cancellation_recorded() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);

        let response = service
            .stop_task(Request::new(StopTaskRequest {
                token: owner_token,
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(response.success);
        assert_eq!(response.status_message, "Task cancellation recorded");
        let stored = service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Cancelled);
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_stop_task_sends_stop_execution_to_assigned_worker() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);
        let (worker_addr, mut stop_rx) = match fake_worker_stop_server().await {
            Some(parts) => parts,
            None => return,
        };
        let worker_id = format!("grpc-stop-worker-{task_id}");
        service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();

        let response = service
            .stop_task(Request::new(StopTaskRequest {
                token: owner_token,
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(response.success, "{}", response.status_message);
        let stopped_task_id = tokio::time::timeout(Duration::from_secs(2), stop_rx.recv())
            .await
            .expect("worker should receive StopTaskExecution")
            .expect("fake worker stop channel should stay open");
        assert_eq!(stopped_task_id, task_id);
        let stored = service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Cancelled);
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_stop_task_keeps_cancellation_when_worker_stop_fails() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);
        let unused_worker_addr = match reserve_loopback_addr() {
            Some(addr) => addr,
            None => return,
        };
        let worker_id = format!("grpc-stop-offline-worker-{task_id}");
        service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, &unused_worker_addr.to_string())
            .await
            .unwrap();

        let response = service
            .stop_task(Request::new(StopTaskRequest {
                token: owner_token,
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(response.success, "{}", response.status_message);
        assert!(
            response
                .status_message
                .contains("worker stop not confirmed"),
            "{}",
            response.status_message
        );
        let stored = service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Cancelled);
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_download_task_artifact_rejects_storage_path_outside_artifact_root() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);
        let outside_path = std::env::temp_dir().join(format!(
            "hivemind-outside-artifact-{}.txt",
            uuid::Uuid::new_v4()
        ));
        tokio::fs::write(&outside_path, b"private outside artifact")
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO artifacts (task_id, artifact_key, checksum_sha1, size_bytes, storage_path, status)
             VALUES ($1, $2, $3, $4, $5, 'ready')",
        )
        .bind(&task_id)
        .bind(format!("outside-artifact-{task_id}"))
        .bind("not-checked")
        .bind(24i64)
        .bind(outside_path.to_string_lossy().as_ref())
        .execute(&service.state.scheduler.database().pool)
        .await
        .unwrap();

        let response = service
            .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                task_id: task_id.clone(),
                token: owner_token,
                artifact_key: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        sqlx::query("DELETE FROM artifacts WHERE task_id = $1")
            .bind(&task_id)
            .execute(&service.state.scheduler.database().pool)
            .await
            .ok();
        tokio::fs::remove_file(&outside_path).await.ok();
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response
            .status_message
            .to_ascii_lowercase()
            .contains("artifact storage"));
        assert!(response.data.is_empty());
    }

    #[tokio::test]
    async fn test_download_task_artifact_rejects_oversized_file_before_reading() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);
        let max_download_bytes = 16 * 1024 * 1024usize;
        let artifact_root = service.state.artifact_root.clone();
        tokio::fs::create_dir_all(&artifact_root).await.unwrap();
        let big_path = artifact_root.join(format!("big-{}.bin", uuid::Uuid::new_v4()));
        std::fs::File::create(&big_path)
            .unwrap()
            .set_len((max_download_bytes + 1) as u64)
            .unwrap();

        sqlx::query(
            "INSERT INTO artifacts (task_id, artifact_key, checksum_sha1, size_bytes, storage_path, status)
             VALUES ($1, $2, $3, $4, $5, 'ready')",
        )
        .bind(&task_id)
        .bind(format!("big-artifact-{task_id}"))
        .bind("not-checked")
        .bind((max_download_bytes + 1) as i64)
        .bind(big_path.to_string_lossy().as_ref())
        .execute(&service.state.scheduler.database().pool)
        .await
        .unwrap();

        let response = service
            .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                task_id: task_id.clone(),
                token: owner_token,
                artifact_key: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        sqlx::query("DELETE FROM artifacts WHERE task_id = $1")
            .bind(&task_id)
            .execute(&service.state.scheduler.database().pool)
            .await
            .ok();
        tokio::fs::remove_file(&big_path).await.ok();
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response.status_message.contains("too large"));
        assert!(response.data.is_empty());
    }

    #[tokio::test]
    async fn test_worker_report_rpc_persists_output_result_and_usage_for_provider_worker() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("grpc-report-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);

        node_service
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: owner.clone(),
                worker_id: worker_id.clone(),
                ip: "10.77.1.10:50053".into(),
                resources: Some(ProtoResourceSpec {
                    cpu_cores: 4,
                    memory_mb: 16 * 1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: 400,
                    gpu_score: 0,
                    storage_total_gb: 500,
                    storage_available_gb: 250,
                }),
                location: "local".into(),
                token: owner_token.clone(),
            }))
            .await
            .unwrap();
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.77.1.10:50053")
            .await
            .unwrap();

        let output_response = node_service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                output: "persisted stdout".into(),
                token: owner_token.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(
            output_response.success,
            "{}",
            output_response.status_message
        );

        let usage_response = node_service
            .task_usage(Request::new(TaskUsageRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                usage: Some(hivemind_proto::ResourceUsage {
                    cpu_percent: 12.5,
                    memory_percent: 23.5,
                    gpu_percent: 34.5,
                    vram_percent: 45.5,
                    storage_percent: 56.5,
                }),
                token: owner_token.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(usage_response.success, "{}", usage_response.status_message);

        let result_response = node_service
            .task_result_upload(Request::new(TaskResultUploadRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                result_torrent: "btih:reported-result".into(),
                token: owner_token.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(
            result_response.success,
            "{}",
            result_response.status_message
        );

        let task_log = master_service
            .get_tasklog(Request::new(TasklogRequest {
                token: owner_token.clone(),
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        let task_result = master_service
            .get_task_result(Request::new(GetTaskResultRequest {
                token: owner_token,
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();

        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(task_log.success);
        assert_eq!(task_log.log, "persisted stdout");
        assert!(task_result.success);
        assert_eq!(task_result.result_torrent, "btih:reported-result");
        assert_eq!(stored.status, TaskStatus::Completed);
        assert_eq!(stored.cpu_usage, 12.5);
        assert_eq!(stored.memory_usage, 23.5);
        assert_eq!(stored.gpu_usage, 34.5);
        assert_eq!(stored.gpu_memory_usage, 45.5);
    }

    #[tokio::test]
    async fn test_worker_report_rpc_rejects_non_owner_for_assigned_worker() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("grpc-report-deny-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);

        node_service
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: owner.clone(),
                worker_id: worker_id.clone(),
                ip: "10.77.2.10:50053".into(),
                resources: Some(ProtoResourceSpec {
                    cpu_cores: 4,
                    memory_mb: 16 * 1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: 400,
                    gpu_score: 0,
                    storage_total_gb: 500,
                    storage_available_gb: 250,
                }),
                location: "local".into(),
                token: owner_token,
            }))
            .await
            .unwrap();
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.77.2.10:50053")
            .await
            .unwrap();

        let response = node_service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                output: "unauthorized output".into(),
                token: other_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert_eq!(response.status_message, "Not authorized");
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.output, None);
    }

    #[tokio::test]
    async fn test_worker_report_rpc_accepts_worker_self_token() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("grpc-report-self-worker-{task_id}");
        let worker_token = token_for(&master_service.state.auth, &worker_id);
        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.77.5.10:50053")
            .await
            .unwrap();

        let response = node_service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                output: "self token output".into(),
                token: worker_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(response.success, "{}", response.status_message);
        assert_eq!(stored.output.as_deref(), Some("self token output"));
    }

    #[tokio::test]
    async fn test_worker_report_rpc_accepts_output_and_usage_after_result_completion() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("grpc-report-result-first-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.77.7.10:50053")
            .await
            .unwrap();

        let result_response = node_service
            .task_result_upload(Request::new(TaskResultUploadRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                result_torrent: "btih:result-first".into(),
                token: owner_token.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(
            result_response.success,
            "{}",
            result_response.status_message
        );

        let output_response = node_service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                output: "stdout after result".into(),
                token: owner_token.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        let usage_response = node_service
            .task_usage(Request::new(TaskUsageRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                usage: Some(hivemind_proto::ResourceUsage {
                    cpu_percent: 21.0,
                    memory_percent: 22.0,
                    gpu_percent: 23.0,
                    vram_percent: 24.0,
                    storage_percent: 25.0,
                }),
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(
            output_response.success,
            "{}",
            output_response.status_message
        );
        assert!(usage_response.success, "{}", usage_response.status_message);
        assert_eq!(stored.status, TaskStatus::Completed);
        assert_eq!(stored.result_torrent.as_deref(), Some("btih:result-first"));
        assert_eq!(stored.output.as_deref(), Some("stdout after result"));
        assert_eq!(stored.cpu_usage, 21.0);
        assert_eq!(stored.gpu_memory_usage, 24.0);
    }

    #[tokio::test]
    async fn test_worker_report_rpc_rejects_same_provider_wrong_assigned_worker() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let assigned_worker = format!("grpc-report-assigned-worker-{task_id}");
        let wrong_worker = format!("grpc-report-wrong-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        register_report_worker(&node_service, &owner, &assigned_worker).await;
        register_report_worker(&node_service, &owner, &wrong_worker).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &assigned_worker, "10.77.6.10:50053")
            .await
            .unwrap();

        let response = node_service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: task_id.clone(),
                worker_id: wrong_worker.clone(),
                output: "wrong worker output".into(),
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &assigned_worker).await;
        cleanup_report_worker(&master_service, &wrong_worker).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.worker_id.as_deref(), Some(assigned_worker.as_str()));
        assert_eq!(stored.output, None);
    }

    #[tokio::test]
    async fn test_worker_report_rpc_rejects_oversized_output_before_persisting() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("grpc-report-big-output-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.77.3.10:50053")
            .await
            .unwrap();

        let response = node_service
            .task_output_upload(Request::new(TaskOutputUploadRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                output: "x".repeat(1024 * 1024 + 1),
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response.status_message.contains("byte limit"));
        assert_eq!(stored.output, None);
    }

    #[tokio::test]
    async fn test_worker_report_rpc_rejects_oversized_result_before_completing() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("grpc-report-big-result-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.77.4.10:50053")
            .await
            .unwrap();

        let response = node_service
            .task_result_upload(Request::new(TaskResultUploadRequest {
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                result_torrent: "x".repeat(4097),
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert!(response.status_message.contains("byte limit"));
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.result_torrent, None);
    }

    #[tokio::test]
    async fn test_get_balance_reads_database_value() {
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let user_service = GrpcUserService::new(service.state.clone());
        let username = format!("grpc-balance-{}", uuid::Uuid::new_v4());
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', $2)")
            .bind(&username)
            .bind(4321i64)
            .execute(&service.state.scheduler.database().pool)
            .await
            .unwrap();
        let token = token_for(&service.state.auth, &username);
        let response = user_service
            .get_balance(Request::new(GetBalanceRequest {
                username: username.clone(),
                token,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(response.success);
        assert_eq!(response.balance, 4321);
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&service.state.scheduler.database().pool)
            .await
            .ok();
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_get_worker_trust_profile_missing_worker_returns_not_found() {
        let (service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };

        let response = service
            .get_worker_trust_profile(Request::new(GetWorkerTrustProfileRequest {
                token: other_token,
                worker_id: "missing-worker".into(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!response.success);
        assert_eq!(response.status_message, "Worker not found");
        assert!(response.trust.is_none());
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_get_worker_trust_profile_rejects_non_owner() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(service.state.clone());
        let worker_id = format!("grpc-trust-profile-worker-{task_id}");
        let owner_token = token_for(&service.state.auth, &owner);

        register_report_worker(&node_service, &owner, &worker_id).await;

        let denied = service
            .get_worker_trust_profile(Request::new(GetWorkerTrustProfileRequest {
                token: other_token,
                worker_id: worker_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        let allowed = service
            .get_worker_trust_profile(Request::new(GetWorkerTrustProfileRequest {
                token: owner_token,
                worker_id: worker_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        cleanup_report_worker(&service, &worker_id).await;
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!denied.success);
        assert_eq!(denied.status_message, "Not authorized");
        assert!(denied.trust.is_none());
        assert!(allowed.success);
        assert_eq!(
            allowed.trust.as_ref().map(|trust| trust.worker_id.as_str()),
            Some(worker_id.as_str())
        );
    }

    #[tokio::test]
    async fn test_provider_can_manage_worker_registered_with_distinct_worker_id() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let worker_id = format!("provider-owned-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);

        let registered = node_service
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: owner.clone(),
                worker_id: worker_id.clone(),
                ip: "10.77.0.10:50053".into(),
                resources: Some(ProtoResourceSpec {
                    cpu_cores: 4,
                    memory_mb: 16 * 1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: 400,
                    gpu_score: 0,
                    storage_total_gb: 500,
                    storage_available_gb: 250,
                }),
                location: "local".into(),
                token: owner_token.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(registered.success, "{}", registered.status_message);

        let response = master_service
            .get_provider_worker_settings(Request::new(GetProviderWorkerSettingsRequest {
                token: owner_token,
                worker_id: worker_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();

        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(response.success, "{}", response.message);
        assert_eq!(response.worker_id, worker_id);
        assert!(response.settings.is_some());
    }

    #[tokio::test]
    async fn test_list_workers_scopes_to_owner_and_admin() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(service.state.clone());
        let owner_worker_id = format!("grpc-list-workers-owner-{task_id}");
        let other_user = format!("grpc-list-workers-other-{task_id}");
        let other_worker_id = format!("grpc-list-workers-other-worker-{task_id}");
        let other_token = token_for(&service.state.auth, &other_user);
        let admin = format!("grpc-admin-{}", uuid::Uuid::new_v4());
        let previous_admins = std::env::var("HIVEMIND_ADMIN_USERS").ok();
        std::env::set_var("HIVEMIND_ADMIN_USERS", &admin);
        let admin_token = token_for(&service.state.auth, &admin);

        register_report_worker(&node_service, &owner, &owner_worker_id).await;
        node_service
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: other_user.clone(),
                worker_id: other_worker_id.clone(),
                ip: "10.77.0.11:50053".into(),
                resources: Some(ProtoResourceSpec {
                    cpu_cores: 4,
                    memory_mb: 16 * 1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: 400,
                    gpu_score: 0,
                    storage_total_gb: 500,
                    storage_available_gb: 250,
                }),
                location: "local".into(),
                token: other_token,
            }))
            .await
            .unwrap()
            .into_inner();

        let owner_view = node_service
            .list_workers(Request::new(ListWorkersRequest {
                include_offline: false,
                token: token_for(&service.state.auth, &owner),
            }))
            .await
            .unwrap()
            .into_inner();
        let allowed = node_service
            .list_workers(Request::new(ListWorkersRequest {
                include_offline: false,
                token: admin_token,
            }))
            .await
            .unwrap()
            .into_inner();

        match previous_admins {
            Some(value) => std::env::set_var("HIVEMIND_ADMIN_USERS", value),
            None => std::env::remove_var("HIVEMIND_ADMIN_USERS"),
        }
        cleanup_report_worker(&service, &owner_worker_id).await;
        cleanup_report_worker(&service, &other_worker_id).await;
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(owner_view.success);
        assert_eq!(owner_view.workers.len(), 1);
        assert_eq!(owner_view.workers[0].worker_id, owner_worker_id);
        assert_eq!(owner_view.workers[0].username, owner);
        assert!(allowed.success);
        assert_eq!(allowed.workers.len(), 2);
        assert!(allowed
            .workers
            .iter()
            .any(|worker| worker.worker_id == owner_worker_id));
        assert!(allowed
            .workers
            .iter()
            .any(|worker| worker.worker_id == other_worker_id));
    }

    #[tokio::test]
    async fn test_get_all_user_tasks_preserves_persisted_task_fields() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);

        sqlx::query(
            "UPDATE tasks SET
                worker_ip = '10.42.0.9',
                status = 'COMPLETED',
                status_message = 'finished',
                output = 'stdout body',
                result_torrent = 'btih:result',
                billed_amount = 37,
                billing_settled = true,
                retry_count = 2,
                wall_time_ms = 1234,
                peak_memory_mb = 256,
                cpu_usage = 12.5,
                memory_usage = 34.5,
                gpu_usage = 56.5,
                gpu_memory_usage = 78.5,
                deterministic = true
             WHERE task_id = $1",
        )
        .bind(&task_id)
        .execute(&service.state.scheduler.database().pool)
        .await
        .unwrap();

        let response = service
            .get_all_user_tasks(Request::new(GetAllUserTasksRequest { token: owner_token }))
            .await
            .unwrap()
            .into_inner();
        let task = response
            .tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .expect("expected owner task in task list");
        let actual = task.clone();

        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert_eq!(actual.owner, owner);
        assert_eq!(actual.status, "COMPLETED");
        assert_eq!(actual.status_message, "finished");
        assert_eq!(actual.worker_ip, "10.42.0.9");
        assert_eq!(actual.output, "stdout body");
        assert_eq!(actual.result_torrent, "btih:result");
        assert_eq!(actual.billed_amount, 37);
        assert!(actual.billing_settled);
        assert_eq!(actual.retry_count, 2);
        assert_eq!(actual.wall_time_ms, 1234);
        assert_eq!(actual.peak_memory_mb, 256);
        assert_eq!(actual.cpu_usage, 12.5);
        assert_eq!(actual.memory_usage, 34.5);
        assert_eq!(actual.gpu_usage, 56.5);
        assert_eq!(actual.gpu_memory_usage, 78.5);
        assert!(actual.deterministic);
    }

    #[tokio::test]
    async fn test_admin_billing_overview_rejects_non_admin() {
        let (service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };

        let response = service
            .get_admin_billing_overview(Request::new(GetAdminBillingOverviewRequest {
                token: other_token,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!response.success);
        assert_eq!(response.status_message, "Forbidden");
        cleanup(&service.state.scheduler, &task_id, &owner).await;
    }

    #[tokio::test]
    async fn test_admin_billing_overview_uses_payer_debit_and_failed_pending_tasks() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let admin = format!("grpc-admin-{}", uuid::Uuid::new_v4());
        let previous_admins = std::env::var("HIVEMIND_ADMIN_USERS").ok();
        std::env::set_var("HIVEMIND_ADMIN_USERS", &admin);
        let admin_token = token_for(&service.state.auth, &admin);
        let ledger_task_id = format!("grpc-ledger-{task_id}");
        let pending_failed_task_id = format!("grpc-failed-{task_id}");
        let pool = &service.state.scheduler.database().pool;

        sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
            .bind(&ledger_task_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&pending_failed_task_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query(
            "INSERT INTO ledger_entries (
                task_id, payer_user, provider_worker_id, provider_user,
                kind, amount_cpt, currency, status, idempotency_key
             ) VALUES
                ($1, $2, NULL, NULL, 'payer_debit', 70, 'CPT', 'settled', $3),
                ($1, $2, NULL, NULL, 'provider_credit', 50, 'CPT', 'settled', $4),
                ($1, $2, NULL, NULL, 'platform_fee', 20, 'CPT', 'settled', $5),
                ($1, $2, NULL, NULL, 'task_debit', 999, 'CPT', 'settled', $6)",
        )
        .bind(&ledger_task_id)
        .bind(&owner)
        .bind(format!("{ledger_task_id}:payer_debit"))
        .bind(format!("{ledger_task_id}:provider_credit"))
        .bind(format!("{ledger_task_id}:platform_fee"))
        .bind(format!("{ledger_task_id}:task_debit"))
        .execute(pool)
        .await
        .unwrap();

        let mut failed_task = make_task(&pending_failed_task_id, &owner);
        failed_task.status = TaskStatus::Failed;
        failed_task.billing_settled = false;
        service
            .state
            .scheduler
            .create_task(&failed_task)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE tasks SET status = 'FAILED', billing_settled = false WHERE task_id = $1",
        )
        .bind(&pending_failed_task_id)
        .execute(pool)
        .await
        .unwrap();

        let expected_pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE billing_settled=false AND status IN ('COMPLETED','FAILED')",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let expected_totals: (i64, i64, i64) = sqlx::query_as(
            "SELECT
                COALESCE(SUM(CASE WHEN kind='payer_debit' THEN amount_cpt ELSE 0 END),0)::BIGINT,
                COALESCE(SUM(CASE WHEN kind='provider_credit' THEN amount_cpt ELSE 0 END),0)::BIGINT,
                COALESCE(SUM(CASE WHEN kind='platform_fee' THEN amount_cpt ELSE 0 END),0)::BIGINT
             FROM ledger_entries WHERE status='settled'",
        )
        .fetch_one(pool)
        .await
        .unwrap();

        let response = service
            .get_admin_billing_overview(Request::new(GetAdminBillingOverviewRequest {
                token: admin_token,
            }))
            .await
            .unwrap()
            .into_inner();

        match previous_admins {
            Some(value) => std::env::set_var("HIVEMIND_ADMIN_USERS", value),
            None => std::env::remove_var("HIVEMIND_ADMIN_USERS"),
        }
        sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
            .bind(&ledger_task_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&pending_failed_task_id)
            .execute(pool)
            .await
            .ok();
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(response.success);
        assert_eq!(response.total_payer_debit_cpt, expected_totals.0);
        assert_eq!(response.total_provider_credit_cpt, expected_totals.1);
        assert_eq!(response.total_platform_fee_cpt, expected_totals.2);
        assert_eq!(response.pending_billing_tasks, expected_pending);
    }

    #[tokio::test]
    async fn test_update_worker_trust_control_rejects_unsafe_worker_id_before_insert() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let admin = format!("grpc-admin-{}", uuid::Uuid::new_v4());
        let previous_admins = std::env::var("HIVEMIND_ADMIN_USERS").ok();
        std::env::set_var("HIVEMIND_ADMIN_USERS", &admin);
        let admin_token = token_for(&service.state.auth, &admin);
        let unsafe_worker_id = ".";
        let pool = &service.state.scheduler.database().pool;
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(unsafe_worker_id)
            .execute(pool)
            .await
            .ok();

        let response = service
            .update_worker_trust_control(Request::new(UpdateWorkerTrustControlRequest {
                token: admin_token,
                worker_id: unsafe_worker_id.into(),
                banned: true,
                score: 1,
            }))
            .await
            .unwrap()
            .into_inner();
        let inserted_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM worker_reputation WHERE worker_id = $1")
                .bind(unsafe_worker_id)
                .fetch_one(pool)
                .await
                .unwrap();

        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(unsafe_worker_id)
            .execute(pool)
            .await
            .ok();
        match previous_admins {
            Some(value) => std::env::set_var("HIVEMIND_ADMIN_USERS", value),
            None => std::env::remove_var("HIVEMIND_ADMIN_USERS"),
        }
        cleanup(&service.state.scheduler, &task_id, &owner).await;

        assert!(!response.success);
        assert_eq!(response.status_message, "Invalid worker_id");
        assert_eq!(inserted_count, 0);
    }

    #[tokio::test]
    async fn test_batch_runtime_pull_batch_rejects_missing_token() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-pull-worker-{task_id}");
        register_report_worker(&node_service, &owner, &worker_id).await;

        let response = batch_service
            .pull_batch(Request::new(PullBatchRequest {
                worker_id: worker_id.clone(),
                max_inflight_batches: 1,
                available_memory_gb: 8,
                queue_capacity: 1,
                cache_summary: None,
                ..Default::default()
            }))
            .await;

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(matches!(response, Err(status) if status.code() == tonic::Code::Unauthenticated));
        assert_eq!(stored.status, TaskStatus::Pending);
        assert_eq!(stored.worker_id, None);
    }

    #[tokio::test]
    async fn test_batch_runtime_complete_batch_rejects_missing_token() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-complete-worker-{task_id}");
        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.88.1.10:50053")
            .await
            .unwrap();

        let response = batch_service
            .complete_batch(Request::new(CompleteBatchRequest {
                worker_id: worker_id.clone(),
                batch_id: "batch-without-token".into(),
                tasks: vec![hivemind_proto::CompletedTask {
                    task_id: task_id.clone(),
                    status: "COMPLETED".into(),
                    stdout_artifact_ref: String::new(),
                    stderr_artifact_ref: String::new(),
                    result_artifact_refs: vec!["btih:batch-result".into()],
                    metrics: None,
                }],
                ..Default::default()
            }))
            .await;

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(matches!(response, Err(status) if status.code() == tonic::Code::Unauthenticated));
        assert_eq!(stored.status, TaskStatus::Assigned);
        assert_eq!(stored.result_torrent, None);
    }

    #[tokio::test]
    async fn test_batch_runtime_heartbeat_rejects_missing_token() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-heartbeat-worker-{task_id}");
        register_report_worker(&node_service, &owner, &worker_id).await;

        let response = batch_service
            .heartbeat(Request::new(HeartbeatRequest {
                worker_id: worker_id.clone(),
                status: "BUSY".into(),
                available_memory_gb: 7,
                queue_capacity: 2,
                ..Default::default()
            }))
            .await;

        let worker = master_service
            .state
            .node_manager
            .get_worker(&worker_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(matches!(response, Err(status) if status.code() == tonic::Code::Unauthenticated));
        assert_eq!(worker.status, hivemind_models::WorkerStatus::Active);
    }

    #[tokio::test]
    async fn test_batch_runtime_pull_batch_authorizes_registered_worker_owner_only() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-pull-owner-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        register_report_worker(&node_service, &owner, &worker_id).await;

        let denied = batch_service
            .pull_batch(Request::new(PullBatchRequest {
                worker_id: worker_id.clone(),
                max_inflight_batches: 1,
                available_memory_gb: 8,
                queue_capacity: 1,
                cache_summary: None,
                token: other_token,
            }))
            .await;
        let stored_after_denied = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();

        let allowed = batch_service
            .pull_batch(Request::new(PullBatchRequest {
                worker_id: worker_id.clone(),
                max_inflight_batches: 1,
                available_memory_gb: 8,
                queue_capacity: 1,
                cache_summary: None,
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();
        let stored_after_allowed = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(matches!(denied, Err(status) if status.code() == tonic::Code::PermissionDenied));
        assert_eq!(stored_after_denied.status, TaskStatus::Pending);
        assert_eq!(stored_after_denied.worker_id, None);
        assert!(allowed.success, "{}", allowed.status_message);
        assert_eq!(allowed.tasks.len(), 1);
        assert_eq!(allowed.tasks[0].task_id, task_id);
        assert_eq!(stored_after_allowed.status, TaskStatus::Assigned);
        assert_eq!(
            stored_after_allowed.worker_id.as_deref(),
            Some(worker_id.as_str())
        );
    }

    #[tokio::test]
    async fn test_batch_runtime_complete_batch_authorizes_registered_worker_owner_only() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-complete-owner-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.88.2.10:50053")
            .await
            .unwrap();

        let denied = batch_service
            .complete_batch(Request::new(batch_complete_request(
                &worker_id,
                &task_id,
                other_token,
            )))
            .await;
        let stored_after_denied = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();

        let allowed = batch_service
            .complete_batch(Request::new(batch_complete_request(
                &worker_id,
                &task_id,
                owner_token,
            )))
            .await
            .unwrap()
            .into_inner();
        let stored_after_allowed = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(matches!(denied, Err(status) if status.code() == tonic::Code::PermissionDenied));
        assert_eq!(stored_after_denied.status, TaskStatus::Assigned);
        assert_eq!(stored_after_denied.result_torrent, None);
        assert!(allowed.success, "{}", allowed.status_message);
        assert_eq!(stored_after_allowed.status, TaskStatus::Completed);
        assert_eq!(
            stored_after_allowed.result_torrent.as_deref(),
            Some("btih:batch-result")
        );
    }

    #[tokio::test]
    async fn test_batch_runtime_complete_batch_rejects_bad_task_id_before_mutating_any_task() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-prevalidate-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        let second_task_id = format!("grpc-batch-prevalidate-second-{task_id}");
        let second_task = make_task(&second_task_id, &owner);
        master_service
            .state
            .scheduler
            .create_task(&second_task)
            .await
            .unwrap();
        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.88.7.10:50053")
            .await
            .unwrap();
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&second_task_id, &worker_id, "10.88.7.10:50053")
            .await
            .unwrap();

        let mut request = batch_complete_request(&worker_id, &task_id, owner_token);
        request.tasks.push(hivemind_proto::CompletedTask {
            task_id: ".".into(),
            status: "COMPLETED".into(),
            stdout_artifact_ref: String::new(),
            stderr_artifact_ref: String::new(),
            result_artifact_refs: vec!["btih:bad-task-id-result".into()],
            metrics: None,
        });
        let response = batch_service.complete_batch(Request::new(request)).await;
        let first_after = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        let second_after = master_service
            .state
            .scheduler
            .get_task(&second_task_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &second_task_id, &owner).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(matches!(response, Err(status) if status.code() == tonic::Code::InvalidArgument));
        assert_eq!(first_after.status, TaskStatus::Assigned);
        assert_eq!(second_after.status, TaskStatus::Assigned);
    }

    #[tokio::test]
    async fn test_batch_runtime_complete_batch_persists_artifact_refs_and_metrics() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-metrics-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.88.3.10:50053")
            .await
            .unwrap();

        let response = batch_service
            .complete_batch(Request::new(batch_complete_request_with_report(
                &worker_id,
                &task_id,
                owner_token,
                "artifact://stdout.log",
                "artifact://stderr.log",
                Some(hivemind_proto::ExecutionMetrics {
                    cpu_time_ms: 101,
                    wall_time_ms: 202,
                    peak_memory_mb: 303,
                    download_bytes: 404,
                    cache_hits: 5,
                }),
            )))
            .await
            .unwrap()
            .into_inner();

        let stored = master_service
            .state
            .scheduler
            .get_task(&task_id)
            .await
            .unwrap()
            .unwrap();
        let task_log = master_service
            .get_tasklog(Request::new(TasklogRequest {
                token: token_for(&master_service.state.auth, &owner),
                task_id: task_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(response.success, "{}", response.status_message);
        assert_eq!(stored.status, TaskStatus::Completed);
        assert_eq!(stored.result_torrent.as_deref(), Some("btih:batch-result"));
        assert_eq!(stored.cpu_time_ms, 101);
        assert_eq!(stored.wall_time_ms, 202);
        assert_eq!(stored.peak_memory_mb, 303);
        assert_eq!(stored.download_bytes, 404);
        assert_eq!(stored.cache_hits, 5);
        assert!(task_log.success);
        assert_eq!(
            task_log.log,
            "stdout_artifact_ref=artifact://stdout.log\nstderr_artifact_ref=artifact://stderr.log"
        );
    }

    #[tokio::test]
    async fn test_batch_runtime_complete_batch_registers_local_stdout_artifact_for_download() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-artifact-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        let artifact_root = master_service.state.artifact_root.clone();
        tokio::fs::create_dir_all(&artifact_root).await.unwrap();
        let stdout_path = artifact_root.join("stdout.log");
        tokio::fs::write(&stdout_path, b"batch stdout bytes")
            .await
            .unwrap();

        register_report_worker(&node_service, &owner, &worker_id).await;
        master_service
            .state
            .scheduler
            .assign_task_to_worker(&task_id, &worker_id, "10.88.4.10:50053")
            .await
            .unwrap();

        let response = batch_service
            .complete_batch(Request::new(batch_complete_request_with_report(
                &worker_id,
                &task_id,
                owner_token.clone(),
                "artifact://stdout.log",
                "",
                None,
            )))
            .await
            .unwrap()
            .into_inner();
        let artifact_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM artifacts WHERE task_id = $1")
                .bind(&task_id)
                .fetch_one(&master_service.state.scheduler.database().pool)
                .await
                .unwrap();
        let download = master_service
            .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                task_id: task_id.clone(),
                token: owner_token,
                artifact_key: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        sqlx::query("DELETE FROM artifacts WHERE task_id = $1")
            .bind(&task_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;
        tokio::fs::remove_file(&stdout_path).await.ok();

        assert!(response.success, "{}", response.status_message);
        assert_eq!(artifact_count, 1);
        assert!(download.success, "{}", download.status_message);
        assert_eq!(download.data, b"batch stdout bytes");
    }

    #[tokio::test]
    async fn test_download_task_artifact_can_select_specific_artifact_key() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (service, task_id, _other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let owner_token = token_for(&service.state.auth, &owner);
        let artifact_root = service.state.artifact_root.clone();
        tokio::fs::create_dir_all(&artifact_root).await.unwrap();
        let first_path = artifact_root.join(format!("first-{}.log", uuid::Uuid::new_v4()));
        let second_path = artifact_root.join(format!("second-{}.log", uuid::Uuid::new_v4()));
        tokio::fs::write(&first_path, b"first artifact bytes")
            .await
            .unwrap();
        tokio::fs::write(&second_path, b"second artifact bytes")
            .await
            .unwrap();
        let first_key = format!("stdout-{task_id}");
        let second_key = format!("stderr-{task_id}");

        sqlx::query(
            "INSERT INTO artifacts (task_id, artifact_key, checksum_sha1, size_bytes, storage_path, status, created_at)
             VALUES ($1, $2, $3, $4, $5, 'ready', NOW() - INTERVAL '1 minute'),
                    ($1, $6, $7, $8, $9, 'ready', NOW())",
        )
        .bind(&task_id)
        .bind(&first_key)
        .bind("first-checksum")
        .bind(20i64)
        .bind(first_path.to_string_lossy().as_ref())
        .bind(&second_key)
        .bind("second-checksum")
        .bind(21i64)
        .bind(second_path.to_string_lossy().as_ref())
        .execute(&service.state.scheduler.database().pool)
        .await
        .unwrap();

        let selected = service
            .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                task_id: task_id.clone(),
                token: owner_token.clone(),
                artifact_key: first_key.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        let missing = service
            .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                task_id: task_id.clone(),
                token: owner_token.clone(),
                artifact_key: format!("missing-{task_id}"),
            }))
            .await
            .unwrap()
            .into_inner();
        let latest = service
            .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                task_id: task_id.clone(),
                token: owner_token.clone(),
                artifact_key: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        let whitespace_key = service
            .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                task_id: task_id.clone(),
                token: owner_token,
                artifact_key: "   ".into(),
            }))
            .await
            .unwrap()
            .into_inner();

        sqlx::query("DELETE FROM artifacts WHERE task_id = $1")
            .bind(&task_id)
            .execute(&service.state.scheduler.database().pool)
            .await
            .ok();
        cleanup(&service.state.scheduler, &task_id, &owner).await;
        tokio::fs::remove_file(&first_path).await.ok();
        tokio::fs::remove_file(&second_path).await.ok();

        assert!(selected.success, "{}", selected.status_message);
        assert_eq!(
            selected.filename,
            first_path.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(selected.data, b"first artifact bytes");
        assert!(!missing.success);
        assert_eq!(missing.status_message, "Artifact not found");
        assert!(missing.data.is_empty());
        assert!(latest.success, "{}", latest.status_message);
        assert_eq!(latest.data, b"second artifact bytes");
        assert!(!whitespace_key.success);
        assert_eq!(whitespace_key.status_message, "Invalid artifact key");
        assert!(whitespace_key.data.is_empty());
    }

    #[tokio::test]
    async fn test_batch_runtime_heartbeat_authorizes_registered_worker_owner_only() {
        let lock = grpc_db_lock();
        let _guard = lock.lock().await;
        let (master_service, task_id, other_token, owner) = match test_service().await {
            Some(parts) => parts,
            None => return,
        };
        let node_service = GrpcNodeManagerService::new(master_service.state.clone());
        let batch_service = GrpcBatchRuntimeService::new(master_service.state.clone());
        let worker_id = format!("grpc-batch-heartbeat-owner-worker-{task_id}");
        let owner_token = token_for(&master_service.state.auth, &owner);
        register_report_worker(&node_service, &owner, &worker_id).await;

        let denied = batch_service
            .heartbeat(Request::new(HeartbeatRequest {
                worker_id: worker_id.clone(),
                status: "BUSY".into(),
                available_memory_gb: 7,
                queue_capacity: 2,
                token: other_token,
            }))
            .await;
        let worker_after_denied = master_service
            .state
            .node_manager
            .get_worker(&worker_id)
            .await
            .unwrap()
            .unwrap();

        let allowed = batch_service
            .heartbeat(Request::new(HeartbeatRequest {
                worker_id: worker_id.clone(),
                status: "BUSY".into(),
                available_memory_gb: 7,
                queue_capacity: 2,
                token: owner_token,
            }))
            .await
            .unwrap()
            .into_inner();
        let worker_after_allowed = master_service
            .state
            .node_manager
            .get_worker(&worker_id)
            .await
            .unwrap()
            .unwrap();
        cleanup_report_worker(&master_service, &worker_id).await;
        cleanup(&master_service.state.scheduler, &task_id, &owner).await;

        assert!(matches!(denied, Err(status) if status.code() == tonic::Code::PermissionDenied));
        assert_eq!(
            worker_after_denied.status,
            hivemind_models::WorkerStatus::Active
        );
        assert!(allowed.success, "{}", allowed.status_message);
        assert_eq!(
            worker_after_allowed.status,
            hivemind_models::WorkerStatus::Busy
        );
    }

    async fn fake_worker_stop_server() -> Option<(SocketAddr, tokio::sync::mpsc::Receiver<String>)>
    {
        let addr = reserve_loopback_addr()?;
        let (stop_tx, stop_rx) = tokio::sync::mpsc::channel(1);
        let service = WorkerNodeServiceServer::new(FakeWorkerStopService { stop_tx });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve(addr)
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::worker_node_service_client::WorkerNodeServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, stop_rx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    async fn register_report_worker(
        node_service: &GrpcNodeManagerService,
        owner: &str,
        worker_id: &str,
    ) {
        let registered = node_service
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: owner.to_string(),
                worker_id: worker_id.to_string(),
                ip: "10.77.99.10:50053".into(),
                resources: Some(ProtoResourceSpec {
                    cpu_cores: 4,
                    memory_mb: 16 * 1024,
                    gpu_count: 0,
                    gpu_name: String::new(),
                    vram_mb: 0,
                    cpu_score: 400,
                    gpu_score: 0,
                    storage_total_gb: 500,
                    storage_available_gb: 250,
                }),
                location: "local".into(),
                token: token_for(&node_service.state.auth, owner),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(registered.success, "{}", registered.status_message);
    }

    async fn cleanup_report_worker(master_service: &GrpcMasterNodeService, worker_id: &str) {
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&master_service.state.scheduler.database().pool)
            .await
            .ok();
    }

    fn batch_complete_request(
        worker_id: &str,
        task_id: &str,
        token: String,
    ) -> CompleteBatchRequest {
        batch_complete_request_with_report(worker_id, task_id, token, "", "", None)
    }

    fn batch_complete_request_with_report(
        worker_id: &str,
        task_id: &str,
        token: String,
        stdout_artifact_ref: &str,
        stderr_artifact_ref: &str,
        metrics: Option<hivemind_proto::ExecutionMetrics>,
    ) -> CompleteBatchRequest {
        CompleteBatchRequest {
            worker_id: worker_id.to_string(),
            batch_id: "batch-auth-test".into(),
            tasks: vec![hivemind_proto::CompletedTask {
                task_id: task_id.to_string(),
                status: "COMPLETED".into(),
                stdout_artifact_ref: stdout_artifact_ref.to_string(),
                stderr_artifact_ref: stderr_artifact_ref.to_string(),
                result_artifact_refs: vec!["btih:batch-result".into()],
                metrics,
            }],
            token,
        }
    }

    fn reserve_loopback_addr() -> Option<SocketAddr> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
        let addr = listener.local_addr().ok()?;
        drop(listener);
        Some(addr)
    }

    struct FakeWorkerStopService {
        stop_tx: tokio::sync::mpsc::Sender<String>,
    }

    #[tonic::async_trait]
    impl WorkerNodeService for FakeWorkerStopService {
        async fn execute_task(
            &self,
            _request: Request<ExecuteTaskRequest>,
        ) -> Result<Response<ExecuteTaskResponse>, Status> {
            Err(Status::unimplemented("fake worker does not execute tasks"))
        }

        async fn task_output_upload(
            &self,
            _request: Request<TaskOutputUploadRequest>,
        ) -> Result<Response<TaskOutputUploadResponse>, Status> {
            Err(Status::unimplemented("fake worker does not upload output"))
        }

        async fn task_result_upload(
            &self,
            _request: Request<TaskResultUploadRequest>,
        ) -> Result<Response<TaskResultUploadResponse>, Status> {
            Err(Status::unimplemented("fake worker does not upload results"))
        }

        async fn task_output(
            &self,
            _request: Request<TaskOutputRequest>,
        ) -> Result<Response<TaskOutputResponse>, Status> {
            Err(Status::unimplemented("fake worker has no output"))
        }

        async fn stop_task_execution(
            &self,
            request: Request<StopTaskExecutionRequest>,
        ) -> Result<Response<StopTaskExecutionResponse>, Status> {
            let task_id = request.into_inner().task_id;
            let _ = self.stop_tx.send(task_id).await;
            Ok(Response::new(StopTaskExecutionResponse {
                success: true,
                status_message: "Stop requested".into(),
            }))
        }

        async fn task_usage(
            &self,
            _request: Request<TaskUsageRequest>,
        ) -> Result<Response<TaskUsageResponse>, Status> {
            Err(Status::unimplemented("fake worker does not report usage"))
        }
    }

    fn token_for(auth: &AuthManager, username: &str) -> String {
        auth.jwt_service()
            .encode_claims(&Claims {
                sub: username.into(),
                user_id: uuid::Uuid::new_v4().to_string(),
                role: None,
                task_id: None,
                worker_id: None,
                exp: (Utc::now().timestamp() + 3600) as usize,
                iat: Utc::now().timestamp() as usize,
            })
            .unwrap()
    }

    async fn cleanup(scheduler: &TaskScheduler, task_id: &str, owner: &str) {
        let schema: Option<String> = sqlx::query_scalar("SELECT current_schema()")
            .fetch_optional(&scheduler.database().pool)
            .await
            .ok()
            .flatten();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .execute(&scheduler.database().pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(owner)
            .execute(&scheduler.database().pool)
            .await
            .ok();
        if let Some(schema) = schema.filter(|name| name.starts_with("hm_test_")) {
            scheduler.database().pool.close().await;
            let config = HivemindConfig::for_test();
            if let Ok(admin_pool) = hivemind_database::postgres::create_pool(&config).await {
                let sql = format!("DROP SCHEMA IF EXISTS {schema} CASCADE");
                sqlx::query(sqlx::AssertSqlSafe(sql))
                    .execute(&admin_pool)
                    .await
                    .ok();
                admin_pool.close().await;
            }
        }
    }

    fn make_task(task_id: &str, owner: &str) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            task_id: task_id.into(),
            owner: owner.into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: Some("queued".into()),
            output: Some("private log".into()),
            result_torrent: Some("private-result".into()),
            torrent_source: Some("input".into()),
            runtime: None,
            task_source: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 100,
            req_gpu_score: 0,
            req_memory_gb: 1,
            req_gpu_memory_gb: 0,
            req_storage_gb: 1,
            host_count: 1,
            max_cpt: 10,
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
            created_at: Utc::now(),
            last_update: Utc::now(),
            completed_at: None,
        }
    }
}

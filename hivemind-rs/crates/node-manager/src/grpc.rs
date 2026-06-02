use hivemind_proto::{
    batch_runtime_service_server::BatchRuntimeService,
    master_node_service_server::MasterNodeService, node_manager_service_server::NodeManagerService,
    user_service_server::UserService, CompleteBatchRequest, CompleteBatchResponse,
    ExecutionPackage, GetAllUserTasksRequest, GetAllUserTasksResponse, GetBalanceRequest,
    GetBalanceResponse, GetTaskResultRequest, GetTaskResultResponse, HeartbeatRequest,
    HeartbeatResponse, LoginRequest, LoginResponse, PullBatchRequest, PullBatchResponse,
    RefreshTokenRequest, RefreshTokenResponse, RegisterWorkerNodeRequest,
    ResourceSpec as ProtoResourceSpec, ResourceUsage as ProtoResourceUsage, RunningStatusRequest,
    RunningStatusResponse, StatusResponse, StopTaskRequest, StopTaskResponse, TaskInfo, TaskLease,
    TasklogRequest, TasklogResponse, UploadTaskRequest, UploadTaskResponse,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::service::{NodeManagerService as NmSvc, WorkerRegistration};
use crate::NodeManager;
use hivemind_auth::AuthManager;
use hivemind_models::{Task, TaskStatus};
use hivemind_task_scheduler::TaskScheduler;

pub struct NodepoolState {
    pub auth: AuthManager,
    pub node_manager: Arc<NodeManager>,
    pub scheduler: TaskScheduler,
}

// ── UserService ──
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
        _: Request<GetBalanceRequest>,
    ) -> Result<Response<GetBalanceResponse>, Status> {
        Ok(Response::new(GetBalanceResponse {
            success: true,
            status_message: "OK".into(),
            balance: 1000,
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

// ── NodeManagerService ──
pub struct GrpcNodeManagerService {
    state: Arc<NodepoolState>,
}
impl GrpcNodeManagerService {
    pub fn new(state: Arc<NodepoolState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl NodeManagerService for GrpcNodeManagerService {
    async fn register_worker_node(
        &self,
        request: Request<RegisterWorkerNodeRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();
        let r = req.resources.unwrap_or_default();
        let svc = NmSvc::new((*self.state.node_manager).clone());
        let reg = WorkerRegistration {
            worker_id: req.username.clone(),
            username: req.username,
            ip: req.ip,
            resources: proto_resource_spec_to_model(r),
            location: req.location,
        };
        match svc.register_worker(&reg).await {
            Ok(w) => {
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
        match self
            .state
            .node_manager
            .update_heartbeat(
                &req.username,
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
}

// ── MasterNodeService ──
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
        let r = req.requirements.unwrap_or_default();
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
            torrent_source: Some(req.torrent),
            expected_btih: None,
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
                tasks: tasks
                    .into_iter()
                    .map(|t| TaskInfo {
                        task_id: t.task_id,
                        status: t.status.as_str().into(),
                        status_message: t.status_message.unwrap_or_default(),
                        usage: Some(ProtoResourceUsage {
                            cpu_percent: t.cpu_usage as f32,
                            memory_percent: t.memory_usage as f32,
                            gpu_percent: t.gpu_usage as f32,
                            vram_percent: t.gpu_memory_usage as f32,
                            storage_percent: 0.0,
                        }),
                        worker_ip: t.worker_ip.unwrap_or_default(),
                    })
                    .collect(),
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
            Ok(_) => Ok(Response::new(StopTaskResponse {
                success: true,
                status_message: "Task stopped".into(),
            })),
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
}

#[tonic::async_trait]
impl BatchRuntimeService for GrpcBatchRuntimeService {
    async fn pull_batch(
        &self,
        request: Request<PullBatchRequest>,
    ) -> Result<Response<PullBatchResponse>, Status> {
        let req = request.into_inner();
        if req.worker_id.trim().is_empty() {
            return Ok(Response::new(PullBatchResponse {
                success: false,
                status_message: "worker_id is required".into(),
                batch_id: String::new(),
                tasks: vec![],
            }));
        }

        let worker = match self.state.node_manager.get_worker(&req.worker_id).await {
            Ok(Some(worker)) => worker,
            Ok(None) => {
                return Ok(Response::new(PullBatchResponse {
                    success: false,
                    status_message: "worker not registered".into(),
                    batch_id: String::new(),
                    tasks: vec![],
                }));
            }
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
        for task in req.tasks {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hivemind_config::HivemindConfig;
    use hivemind_models::Claims;

    async fn test_service() -> Option<(GrpcMasterNodeService, String, String, String)> {
        let config = HivemindConfig::default();
        let db = hivemind_database::DatabaseManager::new(&config)
            .await
            .ok()?;
        db.run_migrations().await.ok()?;
        let auth = AuthManager::new(&db, "grpc-owner-test-secret", 24);
        let scheduler = TaskScheduler::new(db.clone(), auth.clone());
        let node_manager = Arc::new(NodeManager::new(&config, db.clone()));
        let state = Arc::new(NodepoolState {
            auth: auth.clone(),
            node_manager,
            scheduler: scheduler.clone(),
        });
        let service = GrpcMasterNodeService::new(state);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("grpc-owner-task-{unique}");
        let owner = format!("grpc-owner-{unique}");
        let other = format!("grpc-other-{unique}");
        let task = make_task(&task_id, &owner);
        scheduler.create_task(&task).await.ok()?;
        let other_token = token_for(&auth, &other);
        Some((service, task_id, other_token, owner))
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

    fn token_for(auth: &AuthManager, username: &str) -> String {
        auth.jwt_service()
            .encode_claims(&Claims {
                sub: username.into(),
                user_id: uuid::Uuid::new_v4().to_string(),
                exp: (Utc::now().timestamp() + 3600) as usize,
                iat: Utc::now().timestamp() as usize,
            })
            .unwrap()
    }

    async fn cleanup(scheduler: &TaskScheduler, task_id: &str, owner: &str) {
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

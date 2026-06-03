use hivemind_proto::{
    batch_runtime_service_server::BatchRuntimeService,
    master_node_service_server::MasterNodeService, node_manager_service_server::NodeManagerService,
    user_service_server::UserService, CompleteBatchRequest, CompleteBatchResponse,
    ExecutionPackage, GetAllUserTasksRequest, GetAllUserTasksResponse, GetBalanceRequest,
    GetBalanceResponse, GetTaskResultRequest, GetTaskResultResponse, HeartbeatRequest,
    HeartbeatResponse, LoginRequest, LoginResponse, PullBatchRequest, PullBatchResponse,
    RefreshTokenRequest, RefreshTokenResponse, RegisterWorkerNodeRequest,
    ResourceSpec as ProtoResourceSpec, RunningStatusRequest,
    RunningStatusResponse, StatusResponse, StopTaskRequest,
    ListWorkersRequest, ListWorkersResponse, RemoveWorkerRequest,
    QuoteTaskRequest, QuoteTaskResponse, GetProviderEarningsRequest, GetProviderEarningsResponse,
    GetProviderWorkerSettingsRequest, GetProviderWorkerSettingsResponse,
    UpdateProviderWorkerSettingsRequest, UpdateProviderWorkerSettingsResponse, StopTaskResponse, TaskInfo, TaskLease,
    TasklogRequest, TasklogResponse, UploadTaskRequest, UploadTaskResponse,
    // Admin RPC types
    GetAdminBillingOverviewRequest, GetAdminBillingOverviewResponse,
    GetAdminArtifactOverviewRequest, GetAdminArtifactOverviewResponse,
    CleanupAdminArtifactsRequest, CleanupAdminArtifactsResponse,
    GetAdminSchedulingCacheMetricsRequest, GetAdminSchedulingCacheMetricsResponse,
    WorkerCacheAffinityMetric,
    GetAdminSchedulingCacheAlertRequest, GetAdminSchedulingCacheAlertResponse,
    ListAdminSchedulingCacheAnomaliesRequest, ListAdminSchedulingCacheAnomaliesResponse,
    CacheAnomalyEntry,
    GetWorkerTrustProfileRequest, GetWorkerTrustProfileResponse, WorkerTrustProfile,
    UpdateWorkerTrustControlRequest, UpdateWorkerTrustControlResponse,
    ListAdminWorkerTrustRequest, ListAdminWorkerTrustResponse, AdminWorkerTrustEntry,
    ListAdminAuditLogsRequest, ListAdminAuditLogsResponse, AdminAuditLogEntry,
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

    async fn list_workers(
        &self,
        _request: Request<ListWorkersRequest>,
    ) -> Result<Response<ListWorkersResponse>, Status> {
        Err(Status::unimplemented("list_workers not implemented"))
    }

    async fn remove_worker(
        &self,
        _request: Request<RemoveWorkerRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        Err(Status::unimplemented("remove_worker not implemented"))
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
                        // usage: removed
                        owner: String::new(),
                        output: String::new(),
                        result_torrent: String::new(),
                        billed_amount: 0,
                        billing_settled: false,
                        retry_count: 0,
                        wall_time_ms: 0,
                        peak_memory_mb: 0,
                        cpu_usage: 0.0,
                        memory_usage: 0.0,
                        gpu_usage: 0.0,
                        gpu_memory_usage: 0.0,
                        deterministic: false,
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



    // ============================================================
    // Admin RPCs
    // ============================================================

    async fn get_admin_billing_overview(
        &self,
        request: Request<GetAdminBillingOverviewRequest>,
    ) -> Result<Response<GetAdminBillingOverviewResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(admin_billing_default(false, "Forbidden")); }
        let r: (i64, i64, i64) = sqlx::query_as(
            "SELECT COALESCE(SUM(CASE WHEN kind='task_debit' THEN amount_cpt ELSE 0 END),0), COALESCE(SUM(CASE WHEN kind='provider_credit' THEN amount_cpt ELSE 0 END),0), COALESCE(SUM(CASE WHEN kind='platform_fee' THEN amount_cpt ELSE 0 END),0) FROM ledger_entries WHERE status='settled'"
        ).fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        let pending: (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint FROM tasks WHERE billing_settled=false AND status IN ('COMPLETED','FAILING')"
        ).fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetAdminBillingOverviewResponse { success: true, status_message: "OK".into(), total_payer_debit_cpt: r.0, total_provider_credit_cpt: r.1, total_platform_fee_cpt: r.2, pending_billing_tasks: pending.0, currency: "CPT".into() }))
    }

    async fn get_admin_artifact_overview(
        &self,
        request: Request<GetAdminArtifactOverviewRequest>,
    ) -> Result<Response<GetAdminArtifactOverviewResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(artifact_default(false)); }
        let r: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*)::bigint, COALESCE(SUM(size_bytes),0)::bigint, COALESCE(SUM(CASE WHEN dedup_hit THEN 1 ELSE 0 END),0)::bigint, COALESCE(SUM(CASE WHEN resume_supported THEN 1 ELSE 0 END),0)::bigint, COALESCE(SUM(CASE WHEN expires_at IS NOT NULL AND expires_at <= NOW() + INTERVAL '24 hours' THEN 1 ELSE 0 END),0)::bigint FROM artifacts WHERE status='ready'"
        ).fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetAdminArtifactOverviewResponse { success: true, status_message: "OK".into(), total_artifacts: r.0, total_size_bytes: r.1, dedup_hits: r.2, resumable_artifacts: r.3, expiring_in_24h: r.4 }))
    }

    async fn cleanup_admin_artifacts(
        &self,
        request: Request<CleanupAdminArtifactsRequest>,
    ) -> Result<Response<CleanupAdminArtifactsResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(Response::new(CleanupAdminArtifactsResponse { success: false, status_message: "Forbidden".into(), dry_run: req.dry_run, expired_candidates:0, deleted_rows:0, deleted_files:0, file_delete_errors:0 })); }
        let expired: (i64,) = sqlx::query_as("SELECT COUNT(*)::bigint FROM artifacts WHERE expires_at IS NOT NULL AND expires_at <= NOW()").fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        if req.dry_run { return Ok(Response::new(CleanupAdminArtifactsResponse { success: true, status_message: "Dry run".into(), dry_run: true, expired_candidates: expired.0, deleted_rows: 0, deleted_files: 0, file_delete_errors: 0 })); }
        let deleted: u64 = sqlx::query("DELETE FROM artifacts WHERE expires_at IS NOT NULL AND expires_at <= NOW()").execute(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?.rows_affected();
        Ok(Response::new(CleanupAdminArtifactsResponse { success: true, status_message: "OK".into(), dry_run: false, expired_candidates: expired.0, deleted_rows: deleted as i64, deleted_files: 0, file_delete_errors: 0 }))
    }

    async fn get_admin_scheduling_cache_metrics(
        &self,
        request: Request<GetAdminSchedulingCacheMetricsRequest>,
    ) -> Result<Response<GetAdminSchedulingCacheMetricsResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(cache_metrics_default()); }
        let total: (i64, i64) = sqlx::query_as("SELECT COUNT(*)::bigint, COALESCE(SUM(cache_hits),0)::bigint FROM tasks WHERE status='COMPLETED'").fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        let hit_rate = if total.0 > 0 { total.1 as f64 / total.0 as f64 * 100.0 } else { 0.0 };
        #[derive(sqlx::FromRow)] struct CacheW { worker_id: String, completed_tasks: i64, cache_hits: i64, recent_completed_tasks_7d: i64 }
        let top: Vec<CacheW> = sqlx::query_as("SELECT w.worker_id, COUNT(*) FILTER(WHERE t.status='COMPLETED')::bigint as completed_tasks, COALESCE(SUM(t.cache_hits),0)::bigint as cache_hits, COUNT(*) FILTER(WHERE t.status='COMPLETED' AND t.completed_at >= NOW() - INTERVAL '7 days')::bigint as recent_completed_tasks_7d FROM worker_nodes w INNER JOIN tasks t ON t.worker_id = w.worker_id GROUP BY w.worker_id ORDER BY completed_tasks DESC LIMIT 10").fetch_all(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        let workers = top.into_iter().map(|w| WorkerCacheAffinityMetric { worker_id: w.worker_id, completed_tasks: w.completed_tasks, cache_hits: w.cache_hits, recent_completed_tasks_7d: w.recent_completed_tasks_7d }).collect();
        Ok(Response::new(GetAdminSchedulingCacheMetricsResponse { success: true, status_message: "OK".into(), total_completed_tasks: total.0, total_cache_hits: total.1, cache_hit_rate: hit_rate, top_workers: workers }))
    }

    async fn get_admin_scheduling_cache_alert(
        &self,
        request: Request<GetAdminSchedulingCacheAlertRequest>,
    ) -> Result<Response<GetAdminSchedulingCacheAlertResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(Response::new(GetAdminSchedulingCacheAlertResponse { success: false, status_message: "Forbidden".into(), low_threshold: 0.0, high_threshold: 0.0, cache_hit_rate: 0.0, severity: "unknown".into(), message: "Forbidden".into() })); }
        let total: (i64, i64) = sqlx::query_as("SELECT COUNT(*)::bigint, COALESCE(SUM(cache_hits),0)::bigint FROM tasks WHERE status='COMPLETED'").fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        let hit_rate = if total.0 > 0 { total.1 as f64 / total.0 as f64 * 100.0 } else { 0.0 };
        let low = if req.low_threshold > 0.0 { req.low_threshold } else { 5.0 };
        let high = if req.high_threshold > 0.0 { req.high_threshold } else { 80.0 };
        let (severity, msg) = if hit_rate < low { ("critical", format!("Cache hit rate {:.1}% below low threshold {:.1}%", hit_rate, low)) } else if hit_rate > high { ("warning", format!("Cache hit rate {:.1}% above high threshold {:.1}%", hit_rate, high)) } else { ("normal", format!("Cache hit rate {:.1}% within range", hit_rate)) };
        Ok(Response::new(GetAdminSchedulingCacheAlertResponse { success: true, status_message: "OK".into(), low_threshold: low, high_threshold: high, cache_hit_rate: hit_rate, severity: severity.into(), message: msg }))
    }

    async fn list_admin_scheduling_cache_anomalies(
        &self,
        request: Request<ListAdminSchedulingCacheAnomaliesRequest>,
    ) -> Result<Response<ListAdminSchedulingCacheAnomaliesResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(Response::new(ListAdminSchedulingCacheAnomaliesResponse { success: false, status_message: "Forbidden".into(), entries: vec![] })); }
        let limit = req.limit.max(1).min(100);
        #[derive(sqlx::FromRow)] struct A { severity: String, cache_hit_rate: f64, low_threshold: f64, high_threshold: f64, message: String, created_at: chrono::DateTime<chrono::Utc> }
        let rows: Vec<A> = sqlx::query_as("SELECT severity, cache_hit_rate, low_threshold, high_threshold, message, created_at FROM cache_alert_anomalies ORDER BY created_at DESC LIMIT $1").bind(limit).fetch_all(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListAdminSchedulingCacheAnomaliesResponse { success: true, status_message: "OK".into(), entries: rows.into_iter().map(|r| CacheAnomalyEntry { severity: r.severity, cache_hit_rate: r.cache_hit_rate, low_threshold: r.low_threshold, high_threshold: r.high_threshold, message: r.message, created_at: r.created_at.to_rfc3339() }).collect() }))
    }

    async fn get_worker_trust_profile(
        &self,
        request: Request<GetWorkerTrustProfileRequest>,
    ) -> Result<Response<GetWorkerTrustProfileResponse>, Status> {
        let req = request.into_inner();
        let _claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        #[derive(sqlx::FromRow)] struct TrustR { worker_id: String, successful_tasks: i64, failed_tasks: i64, score: i32, banned: bool, last_attested_at: Option<chrono::DateTime<chrono::Utc>> }
        let row: TrustR = sqlx::query_as("SELECT worker_id, successful_tasks, failed_tasks, score, banned, last_attested_at FROM worker_reputation WHERE worker_id = $1").bind(&req.worker_id).fetch_optional(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?.unwrap_or(TrustR { worker_id: req.worker_id.clone(), successful_tasks: 0, failed_tasks: 0, score: 100, banned: false, last_attested_at: None });
        Ok(Response::new(GetWorkerTrustProfileResponse { success: true, status_message: "OK".into(), trust: Some(WorkerTrustProfile { worker_id: row.worker_id, successful_tasks: row.successful_tasks, failed_tasks: row.failed_tasks, score: row.score, banned: row.banned, last_attested_at: row.last_attested_at.map(|t| t.to_rfc3339()).unwrap_or_default() }) }))
    }

    async fn update_worker_trust_control(
        &self,
        request: Request<UpdateWorkerTrustControlRequest>,
    ) -> Result<Response<UpdateWorkerTrustControlResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(Response::new(UpdateWorkerTrustControlResponse { success: false, status_message: "Forbidden".into(), worker_id: req.worker_id.clone(), banned: false, score: 0 })); }
        let exists: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM worker_reputation WHERE worker_id=$1)").bind(&req.worker_id).fetch_one(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        if exists.0 {
            sqlx::query("UPDATE worker_reputation SET banned=$1, score=$2, updated_at=NOW() WHERE worker_id=$3").bind(req.banned).bind(req.score).bind(&req.worker_id).execute(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        } else {
            sqlx::query("INSERT INTO worker_reputation (worker_id, banned, score, successful_tasks, failed_tasks) VALUES ($1, $2, $3, 0, 0)").bind(&req.worker_id).bind(req.banned).bind(req.score).execute(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        }
        Ok(Response::new(UpdateWorkerTrustControlResponse { success: true, status_message: "OK".into(), worker_id: req.worker_id, banned: req.banned, score: req.score }))
    }

    async fn list_admin_worker_trust(
        &self,
        request: Request<ListAdminWorkerTrustRequest>,
    ) -> Result<Response<ListAdminWorkerTrustResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(Response::new(ListAdminWorkerTrustResponse { success: false, status_message: "Forbidden".into(), entries: vec![] })); }
        #[derive(sqlx::FromRow)] struct TrustLR { worker_id: String, username: String, worker_status: String, score: i32, banned: bool, successful_tasks: i64, failed_tasks: i64, last_attested_at: Option<chrono::DateTime<chrono::Utc>> }
        let rows: Vec<TrustLR> = sqlx::query_as("SELECT w.worker_id, w.username, w.status as worker_status, COALESCE(r.score, 100) as score, COALESCE(r.banned, false) as banned, COALESCE(r.successful_tasks, 0) as successful_tasks, COALESCE(r.failed_tasks, 0) as failed_tasks, r.last_attested_at FROM worker_nodes w LEFT JOIN worker_reputation r ON r.worker_id = w.worker_id ORDER BY r.score DESC").fetch_all(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListAdminWorkerTrustResponse { success: true, status_message: "OK".into(), entries: rows.into_iter().map(|r| AdminWorkerTrustEntry { worker_id: r.worker_id, username: r.username, worker_status: r.worker_status, score: r.score, banned: r.banned, successful_tasks: r.successful_tasks, failed_tasks: r.failed_tasks, last_attested_at: r.last_attested_at.map(|t| t.to_rfc3339()).unwrap_or_default() }).collect() }))
    }

    async fn list_admin_audit_logs(
        &self,
        request: Request<ListAdminAuditLogsRequest>,
    ) -> Result<Response<ListAdminAuditLogsResponse>, Status> {
        let req = request.into_inner();
        let claims = self.state.auth.validate_token(&req.token).map_err(|_| Status::unauthenticated("Invalid token"))?;
        if !is_admin(&claims.sub) { return Ok(Response::new(ListAdminAuditLogsResponse { success: false, status_message: "Forbidden".into(), entries: vec![] })); }
        let limit = req.limit.max(1).min(500);
        #[derive(sqlx::FromRow)] struct AuditR { id: uuid::Uuid, admin_user: String, action: String, target_type: String, target_id: String, detail: sqlx::types::Json<serde_json::Value>, created_at: chrono::DateTime<chrono::Utc> }
        let rows: Vec<AuditR> = sqlx::query_as("SELECT id, admin_user, action, target_type, target_id, detail, created_at FROM admin_audit_logs ORDER BY created_at DESC LIMIT $1").bind(limit).fetch_all(&self.state.scheduler.database().pool).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListAdminAuditLogsResponse { success: true, status_message: "OK".into(), entries: rows.into_iter().map(|r| AdminAuditLogEntry { id: r.id.to_string(), username: r.admin_user, action: r.action, resource: format!("{}:{}", r.target_type, r.target_id), details: r.detail.to_string(), created_at: r.created_at.to_rfc3339() }).collect() }))
    }
    async fn quote_task(
        &self,
        _request: Request<QuoteTaskRequest>,
    ) -> Result<Response<QuoteTaskResponse>, Status> {
        Err(Status::unimplemented("quote_task not implemented"))
    }

    async fn get_provider_earnings(
        &self,
        _request: Request<GetProviderEarningsRequest>,
    ) -> Result<Response<GetProviderEarningsResponse>, Status> {
        Err(Status::unimplemented("get_provider_earnings not implemented"))
    }

    async fn get_provider_worker_settings(
        &self,
        _request: Request<GetProviderWorkerSettingsRequest>,
    ) -> Result<Response<GetProviderWorkerSettingsResponse>, Status> {
        Err(Status::unimplemented("get_provider_worker_settings not implemented"))
    }

    async fn update_provider_worker_settings(
        &self,
        _request: Request<UpdateProviderWorkerSettingsRequest>,
    ) -> Result<Response<UpdateProviderWorkerSettingsResponse>, Status> {
        Err(Status::unimplemented("update_provider_worker_settings not implemented"))
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


// Admin helpers
fn is_admin(sub: &str) -> bool {
    std::env::var("HIVEMIND_ADMIN_USERS")
        .unwrap_or_else(|_| "testuser".into())
        .split(',')
        .any(|s| s.trim() == sub)
}

fn admin_billing_default(success: bool, status_message: &str) -> tonic::Response<hivemind_proto::GetAdminBillingOverviewResponse> {
    tonic::Response::new(hivemind_proto::GetAdminBillingOverviewResponse {
        success,
        status_message: status_message.into(),
        total_payer_debit_cpt: 0,
        total_provider_credit_cpt: 0,
        total_platform_fee_cpt: 0,
        pending_billing_tasks: 0,
        currency: "CPT".into(),
    })
}

fn artifact_default(success: bool) -> tonic::Response<hivemind_proto::GetAdminArtifactOverviewResponse> {
    tonic::Response::new(hivemind_proto::GetAdminArtifactOverviewResponse {
        success,
        status_message: if success { "OK".into() } else { "Forbidden".into() },
        total_artifacts: 0,
        total_size_bytes: 0,
        dedup_hits: 0,
        resumable_artifacts: 0,
        expiring_in_24h: 0,
    })
}

fn cache_metrics_default() -> tonic::Response<hivemind_proto::GetAdminSchedulingCacheMetricsResponse> {
    tonic::Response::new(hivemind_proto::GetAdminSchedulingCacheMetricsResponse {
        success: false,
        status_message: "Forbidden".into(),
        total_completed_tasks: 0,
        total_cache_hits: 0,
        cache_hit_rate: 0.0,
        top_workers: vec![],
    })
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

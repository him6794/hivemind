use hivemind_proto::{
    master_node_service_client::MasterNodeServiceClient,
    node_manager_service_client::NodeManagerServiceClient,
    user_service_client::UserServiceClient,
    CleanupAdminArtifactsRequest,
    CleanupAdminArtifactsResponse,
    DownloadTaskArtifactRequest,
    DownloadTaskArtifactResponse,
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
    QuoteTaskRequest,
    QuoteTaskResponse,
    RegisterUserRequest,
    RegisterUserResponse,
    RegisterWorkerNodeRequest,
    RemoveWorkerRequest,
    ResourceSpec as ProtoResourceSpec,
    StatusResponse,
    StopTaskRequest,
    StopTaskResponse,
    TasklogRequest,
    TasklogResponse,
    UpdateProviderWorkerSettingsRequest,
    UpdateProviderWorkerSettingsResponse,
    UpdateWorkerTrustControlRequest,
    UpdateWorkerTrustControlResponse,
    UploadTaskRequest,
    UploadTaskResponse,
};
use tokio::time::{sleep, Duration};
use tonic::transport::{Channel, Endpoint};
use tonic::Request;

#[derive(Clone)]
pub struct GrpcClient {
    user: UserServiceClient<Channel>,
    master: MasterNodeServiceClient<Channel>,
    node_mgr: NodeManagerServiceClient<Channel>,
}

impl GrpcClient {
    pub async fn connect(addr: &str) -> Result<Self, tonic::transport::Error> {
        let endpoint = Endpoint::from_shared(format!("http://{}", addr))?;
        let channel = endpoint.connect().await?;
        Ok(Self {
            user: UserServiceClient::new(channel.clone()),
            master: MasterNodeServiceClient::new(channel.clone()),
            node_mgr: NodeManagerServiceClient::new(channel),
        })
    }

    pub async fn connect_with_retry(
        addr: &str,
        attempts: usize,
        delay: Duration,
    ) -> Result<Self, tonic::transport::Error> {
        let mut last_err = None;
        for _ in 0..attempts.max(1) {
            match Self::connect(addr).await {
                Ok(client) => return Ok(client),
                Err(err) => {
                    last_err = Some(err);
                    sleep(delay).await;
                }
            }
        }
        Err(last_err.expect("attempts.max(1) ensures at least one error"))
    }

    // ---- UserService ----
    pub async fn register_user(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<RegisterUserResponse, tonic::Status> {
        self.user
            .register_user(Request::new(RegisterUserRequest {
                username: username.to_string(),
                password: password.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }

    pub async fn login(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<LoginResponse, tonic::Status> {
        self.user
            .login(Request::new(LoginRequest {
                username: username.to_string(),
                password: password.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_balance(
        &mut self,
        username: &str,
        token: &str,
    ) -> Result<GetBalanceResponse, tonic::Status> {
        self.user
            .get_balance(Request::new(GetBalanceRequest {
                username: username.to_string(),
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }

    // ---- MasterNodeService ----
    #[allow(clippy::too_many_arguments)]
    pub async fn upload_task(
        &mut self,
        task_id: &str,
        torrent: &str,
        requirements: ProtoResourceSpec,
        location: &str,
        host_count: i32,
        token: &str,
        max_cpt: i64,
    ) -> Result<UploadTaskResponse, tonic::Status> {
        self.master
            .upload_task(Request::new(UploadTaskRequest {
                task_id: task_id.to_string(),
                torrent: torrent.to_string(),
                requirements: Some(requirements),
                location: location.to_string(),
                host_count,
                token: token.to_string(),
                max_cpt,
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_all_user_tasks(
        &mut self,
        token: &str,
    ) -> Result<GetAllUserTasksResponse, tonic::Status> {
        self.master
            .get_all_user_tasks(Request::new(GetAllUserTasksRequest {
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_task_result(
        &mut self,
        task_id: &str,
        token: &str,
    ) -> Result<GetTaskResultResponse, tonic::Status> {
        self.master
            .get_task_result(Request::new(GetTaskResultRequest {
                task_id: task_id.to_string(),
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn stop_task(
        &mut self,
        task_id: &str,
        token: &str,
    ) -> Result<StopTaskResponse, tonic::Status> {
        self.master
            .stop_task(Request::new(StopTaskRequest {
                task_id: task_id.to_string(),
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_tasklog(
        &mut self,
        task_id: &str,
        token: &str,
    ) -> Result<TasklogResponse, tonic::Status> {
        self.master
            .get_tasklog(Request::new(TasklogRequest {
                task_id: task_id.to_string(),
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn download_task_artifact(
        &mut self,
        task_id: &str,
        token: &str,
        artifact_key: Option<&str>,
    ) -> Result<DownloadTaskArtifactResponse, tonic::Status> {
        self.master
            .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                task_id: task_id.to_string(),
                token: token.to_string(),
                artifact_key: artifact_key.unwrap_or_default().to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn quote_task(
        &mut self,
        token: &str,
        cpu_score: i32,
        gpu_score: i32,
        memory_gb: i32,
        gpu_memory_gb: i32,
        storage_gb: i64,
        host_count: i32,
    ) -> Result<QuoteTaskResponse, tonic::Status> {
        self.master
            .quote_task(Request::new(QuoteTaskRequest {
                token: token.to_string(),
                cpu_score,
                gpu_score,
                memory_gb,
                gpu_memory_gb,
                storage_gb,
                host_count,
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_provider_earnings(
        &mut self,
        token: &str,
        limit: i64,
    ) -> Result<GetProviderEarningsResponse, tonic::Status> {
        self.master
            .get_provider_earnings(Request::new(GetProviderEarningsRequest {
                token: token.to_string(),
                limit,
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_provider_worker_settings(
        &mut self,
        token: &str,
        worker_id: &str,
    ) -> Result<GetProviderWorkerSettingsResponse, tonic::Status> {
        self.master
            .get_provider_worker_settings(Request::new(GetProviderWorkerSettingsRequest {
                token: token.to_string(),
                worker_id: worker_id.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn update_provider_worker_settings(
        &mut self,
        token: &str,
        worker_id: &str,
        enabled: bool,
        cpu_cores_limit: i32,
        memory_gb_limit: i32,
        gpu_memory_gb_limit: i32,
        storage_gb_limit: i64,
        min_cpt_per_hour: i64,
    ) -> Result<UpdateProviderWorkerSettingsResponse, tonic::Status> {
        self.master
            .update_provider_worker_settings(Request::new(UpdateProviderWorkerSettingsRequest {
                token: token.to_string(),
                worker_id: worker_id.to_string(),
                settings: Some(hivemind_proto::ProviderWorkerSettings {
                    enabled,
                    cpu_cores_limit,
                    memory_gb_limit,
                    gpu_memory_gb_limit,
                    storage_gb_limit,
                    min_cpt_per_hour,
                }),
            }))
            .await
            .map(|r| r.into_inner())
    }

    // ---- Admin RPCs ----
    pub async fn get_admin_billing_overview(
        &mut self,
        token: &str,
    ) -> Result<GetAdminBillingOverviewResponse, tonic::Status> {
        self.master
            .get_admin_billing_overview(Request::new(GetAdminBillingOverviewRequest {
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_admin_artifact_overview(
        &mut self,
        token: &str,
    ) -> Result<GetAdminArtifactOverviewResponse, tonic::Status> {
        self.master
            .get_admin_artifact_overview(Request::new(GetAdminArtifactOverviewRequest {
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn cleanup_admin_artifacts(
        &mut self,
        token: &str,
        dry_run: bool,
    ) -> Result<CleanupAdminArtifactsResponse, tonic::Status> {
        self.master
            .cleanup_admin_artifacts(Request::new(CleanupAdminArtifactsRequest {
                token: token.to_string(),
                dry_run,
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_admin_scheduling_cache_metrics(
        &mut self,
        token: &str,
    ) -> Result<GetAdminSchedulingCacheMetricsResponse, tonic::Status> {
        self.master
            .get_admin_scheduling_cache_metrics(Request::new(
                GetAdminSchedulingCacheMetricsRequest {
                    token: token.to_string(),
                },
            ))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_admin_scheduling_cache_alert(
        &mut self,
        token: &str,
        low_threshold: f64,
        high_threshold: f64,
    ) -> Result<GetAdminSchedulingCacheAlertResponse, tonic::Status> {
        self.master
            .get_admin_scheduling_cache_alert(Request::new(GetAdminSchedulingCacheAlertRequest {
                token: token.to_string(),
                low_threshold,
                high_threshold,
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn list_admin_scheduling_cache_anomalies(
        &mut self,
        token: &str,
        limit: i64,
    ) -> Result<ListAdminSchedulingCacheAnomaliesResponse, tonic::Status> {
        self.master
            .list_admin_scheduling_cache_anomalies(Request::new(
                ListAdminSchedulingCacheAnomaliesRequest {
                    token: token.to_string(),
                    limit,
                },
            ))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn get_worker_trust_profile(
        &mut self,
        token: &str,
        worker_id: &str,
    ) -> Result<GetWorkerTrustProfileResponse, tonic::Status> {
        self.master
            .get_worker_trust_profile(Request::new(GetWorkerTrustProfileRequest {
                token: token.to_string(),
                worker_id: worker_id.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn update_worker_trust_control(
        &mut self,
        token: &str,
        worker_id: &str,
        banned: bool,
        score: i32,
    ) -> Result<UpdateWorkerTrustControlResponse, tonic::Status> {
        self.master
            .update_worker_trust_control(Request::new(UpdateWorkerTrustControlRequest {
                token: token.to_string(),
                worker_id: worker_id.to_string(),
                banned,
                score,
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn list_admin_worker_trust(
        &mut self,
        token: &str,
    ) -> Result<ListAdminWorkerTrustResponse, tonic::Status> {
        self.master
            .list_admin_worker_trust(Request::new(ListAdminWorkerTrustRequest {
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn list_admin_audit_logs(
        &mut self,
        token: &str,
        limit: i64,
    ) -> Result<ListAdminAuditLogsResponse, tonic::Status> {
        self.master
            .list_admin_audit_logs(Request::new(ListAdminAuditLogsRequest {
                token: token.to_string(),
                limit,
            }))
            .await
            .map(|r| r.into_inner())
    }

    // ---- NodeManagerService ----
    pub async fn list_workers(
        &mut self,
        include_offline: bool,
        token: &str,
    ) -> Result<ListWorkersResponse, tonic::Status> {
        self.node_mgr
            .list_workers(Request::new(ListWorkersRequest {
                include_offline,
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn register_worker_node(
        &mut self,
        username: &str,
        worker_id: &str,
        ip: &str,
        resources: ProtoResourceSpec,
        location: &str,
        token: &str,
    ) -> Result<StatusResponse, tonic::Status> {
        self.node_mgr
            .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                username: username.to_string(),
                worker_id: worker_id.to_string(),
                ip: ip.to_string(),
                resources: Some(resources),
                location: location.to_string(),
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
    pub async fn remove_worker(
        &mut self,
        worker_id: &str,
        token: &str,
    ) -> Result<StatusResponse, tonic::Status> {
        self.node_mgr
            .remove_worker(Request::new(RemoveWorkerRequest {
                worker_id: worker_id.to_string(),
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
}

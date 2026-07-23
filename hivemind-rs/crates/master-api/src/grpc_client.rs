use hivemind_proto::{
    master_node_service_client::MasterNodeServiceClient,
    node_manager_service_client::NodeManagerServiceClient, user_service_client::UserServiceClient,
    CleanupAdminArtifactsRequest, CleanupAdminArtifactsResponse, DownloadTaskArtifactRequest,
    DownloadTaskArtifactResponse, GetAdminArtifactOverviewRequest,
    GetAdminArtifactOverviewResponse, GetAdminBillingOverviewRequest,
    GetAdminBillingOverviewResponse, GetAdminSchedulingCacheAlertRequest,
    GetAdminSchedulingCacheAlertResponse, GetAdminSchedulingCacheMetricsRequest,
    GetAdminSchedulingCacheMetricsResponse, GetAllUserTasksRequest, GetAllUserTasksResponse,
    GetBalanceRequest, GetBalanceResponse, GetProviderEarningsRequest, GetProviderEarningsResponse,
    GetProviderWorkerSettingsRequest, GetProviderWorkerSettingsResponse, GetTaskResultRequest,
    GetTaskResultResponse, GetWorkerTrustProfileRequest, GetWorkerTrustProfileResponse,
    ListAdminAuditLogsRequest, ListAdminAuditLogsResponse,
    ListAdminSchedulingCacheAnomaliesRequest, ListAdminSchedulingCacheAnomaliesResponse,
    ListAdminWorkerTrustRequest, ListAdminWorkerTrustResponse, ListWorkersRequest,
    ListWorkersResponse, LoginRequest, LoginResponse, QuoteTaskRequest, QuoteTaskResponse,
    RegisterUserRequest, RegisterUserResponse, RegisterWorkerNodeRequest, RemoveWorkerRequest,
    ResourceSpec as ProtoResourceSpec, StatusResponse, StopTaskRequest, StopTaskResponse,
    TasklogRequest, TasklogResponse, UpdateProviderWorkerSettingsRequest,
    UpdateProviderWorkerSettingsResponse, UpdateWorkerTrustControlRequest,
    UpdateWorkerTrustControlResponse, UploadTaskRequest, UploadTaskResponse,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tonic::transport::{Channel, Endpoint};
use tonic::{Code, Request, Status};

#[derive(Clone)]
struct ConnectedClients {
    user: UserServiceClient<Channel>,
    master: MasterNodeServiceClient<Channel>,
    node_mgr: NodeManagerServiceClient<Channel>,
}

/// Lazy nodepool gRPC client for user-deployed masters.
///
/// Master UI can start before the VPN overlay is up. Handlers call any RPC helper,
/// which connects on demand via [`GrpcClient::ensure_connected`].
#[derive(Clone)]
pub struct GrpcClient {
    endpoint: Arc<Mutex<String>>,
    inner: Arc<Mutex<Option<ConnectedClients>>>,
}

impl GrpcClient {
    /// Create a client that will connect lazily to `addr`.
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            endpoint: Arc::new(Mutex::new(addr.into())),
            inner: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn connect(addr: &str) -> Result<Self, tonic::transport::Error> {
        let client = Self::new(addr);
        client.ensure_connected().await?;
        Ok(client)
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

    /// Connect if needed. Safe to call repeatedly after VPN bootstrap.
    pub async fn ensure_connected(&self) -> Result<(), tonic::transport::Error> {
        let mut guard = self.inner.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        let endpoint = self.endpoint.lock().await.clone();
        let endpoint =
            Endpoint::from_shared(format!("http://{}", endpoint))?.http2_adaptive_window(true);
        let channel = endpoint.connect().await?;
        // Task package uploads can be large; raise client decode/encode limits.
        let max_msg = 128 * 1024 * 1024;
        *guard = Some(ConnectedClients {
            user: UserServiceClient::new(channel.clone()),
            master: MasterNodeServiceClient::new(channel.clone())
                .max_decoding_message_size(max_msg)
                .max_encoding_message_size(max_msg),
            node_mgr: NodeManagerServiceClient::new(channel),
        });
        Ok(())
    }

    /// Drop a stale connection so the next RPC reconnects (e.g. after VPN join).
    pub async fn reset(&self) {
        let mut guard = self.inner.lock().await;
        *guard = None;
    }

    /// Update the nodepool endpoint discovered after VPN join and drop any stale channel.
    pub async fn set_endpoint(&self, endpoint: impl Into<String>) {
        let endpoint = endpoint.into();
        {
            let mut guard = self.endpoint.lock().await;
            if *guard == endpoint {
                // Still reset below so a previous failed channel is not reused.
            } else {
                *guard = endpoint;
            }
        }
        self.reset().await;
    }

    pub async fn endpoint(&self) -> String {
        self.endpoint.lock().await.clone()
    }

    async fn with_clients<T, F, Fut>(&self, f: F) -> Result<T, Status>
    where
        F: FnOnce(ConnectedClients) -> Fut,
        Fut: std::future::Future<Output = Result<T, Status>>,
    {
        if let Err(err) = self.ensure_connected().await {
            let endpoint = self.endpoint.lock().await.clone();
            return Err(Status::new(
                Code::Unavailable,
                format!("nodepool gRPC unavailable at {}: {}", endpoint, err),
            ));
        }
        let clients = {
            let guard = self.inner.lock().await;
            guard
                .clone()
                .expect("ensure_connected populates the client")
        };
        f(clients).await
    }

    // ---- UserService ----
    pub async fn register_user(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<RegisterUserResponse, tonic::Status> {
        let username = username.to_string();
        let password = password.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .user
                .register_user(Request::new(RegisterUserRequest { username, password }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn login(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<LoginResponse, tonic::Status> {
        let username = username.to_string();
        let password = password.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .user
                .login(Request::new(LoginRequest { username, password }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_balance(
        &mut self,
        username: &str,
        token: &str,
    ) -> Result<GetBalanceResponse, tonic::Status> {
        let username = username.to_string();
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .user
                .get_balance(Request::new(GetBalanceRequest { username, token }))
                .await
                .map(|r| r.into_inner())
        })
        .await
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
        runtime: &str,
        task_source: &str,
        package_data: Vec<u8>,
        package_filename: &str,
    ) -> Result<UploadTaskResponse, tonic::Status> {
        let task_id = task_id.to_string();
        let torrent = torrent.to_string();
        let location = location.to_string();
        let token = token.to_string();
        let runtime = runtime.to_string();
        let task_source = task_source.to_string();
        let package_filename = package_filename.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .upload_task(Request::new(UploadTaskRequest {
                    task_id,
                    torrent,
                    requirements: Some(requirements),
                    location,
                    host_count,
                    token,
                    max_cpt,
                    runtime,
                    task_source,
                    package_data,
                    package_filename,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_all_user_tasks(
        &mut self,
        token: &str,
    ) -> Result<GetAllUserTasksResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_all_user_tasks(Request::new(GetAllUserTasksRequest { token }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_task_result(
        &mut self,
        task_id: &str,
        token: &str,
    ) -> Result<GetTaskResultResponse, tonic::Status> {
        let task_id = task_id.to_string();
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_task_result(Request::new(GetTaskResultRequest { task_id, token }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn stop_task(
        &mut self,
        task_id: &str,
        token: &str,
    ) -> Result<StopTaskResponse, tonic::Status> {
        let task_id = task_id.to_string();
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .stop_task(Request::new(StopTaskRequest { task_id, token }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_tasklog(
        &mut self,
        task_id: &str,
        token: &str,
    ) -> Result<TasklogResponse, tonic::Status> {
        let task_id = task_id.to_string();
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_tasklog(Request::new(TasklogRequest { task_id, token }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn download_task_artifact(
        &mut self,
        task_id: &str,
        token: &str,
        artifact_key: Option<&str>,
    ) -> Result<DownloadTaskArtifactResponse, tonic::Status> {
        let task_id = task_id.to_string();
        let token = token.to_string();
        let artifact_key = artifact_key.unwrap_or_default().to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .download_task_artifact(Request::new(DownloadTaskArtifactRequest {
                    task_id,
                    token,
                    artifact_key,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
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
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .quote_task(Request::new(QuoteTaskRequest {
                    token,
                    cpu_score,
                    gpu_score,
                    memory_gb,
                    gpu_memory_gb,
                    storage_gb,
                    host_count,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_provider_earnings(
        &mut self,
        token: &str,
        limit: i64,
    ) -> Result<GetProviderEarningsResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_provider_earnings(Request::new(GetProviderEarningsRequest { token, limit }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_provider_worker_settings(
        &mut self,
        token: &str,
        worker_id: &str,
    ) -> Result<GetProviderWorkerSettingsResponse, tonic::Status> {
        let token = token.to_string();
        let worker_id = worker_id.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_provider_worker_settings(Request::new(GetProviderWorkerSettingsRequest {
                    token,
                    worker_id,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
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
        let token = token.to_string();
        let worker_id = worker_id.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .update_provider_worker_settings(Request::new(
                    UpdateProviderWorkerSettingsRequest {
                        token,
                        worker_id,
                        settings: Some(hivemind_proto::ProviderWorkerSettings {
                            enabled,
                            cpu_cores_limit,
                            memory_gb_limit,
                            gpu_memory_gb_limit,
                            storage_gb_limit,
                            min_cpt_per_hour,
                        }),
                    },
                ))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    // ---- Admin RPCs ----
    pub async fn get_admin_billing_overview(
        &mut self,
        token: &str,
    ) -> Result<GetAdminBillingOverviewResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_admin_billing_overview(Request::new(GetAdminBillingOverviewRequest { token }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_admin_artifact_overview(
        &mut self,
        token: &str,
    ) -> Result<GetAdminArtifactOverviewResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_admin_artifact_overview(Request::new(GetAdminArtifactOverviewRequest {
                    token,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn cleanup_admin_artifacts(
        &mut self,
        token: &str,
        dry_run: bool,
    ) -> Result<CleanupAdminArtifactsResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .cleanup_admin_artifacts(Request::new(CleanupAdminArtifactsRequest {
                    token,
                    dry_run,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_admin_scheduling_cache_metrics(
        &mut self,
        token: &str,
    ) -> Result<GetAdminSchedulingCacheMetricsResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_admin_scheduling_cache_metrics(Request::new(
                    GetAdminSchedulingCacheMetricsRequest { token },
                ))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_admin_scheduling_cache_alert(
        &mut self,
        token: &str,
        low_threshold: f64,
        high_threshold: f64,
    ) -> Result<GetAdminSchedulingCacheAlertResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_admin_scheduling_cache_alert(Request::new(
                    GetAdminSchedulingCacheAlertRequest {
                        token,
                        low_threshold,
                        high_threshold,
                    },
                ))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn list_admin_scheduling_cache_anomalies(
        &mut self,
        token: &str,
        limit: i64,
    ) -> Result<ListAdminSchedulingCacheAnomaliesResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .list_admin_scheduling_cache_anomalies(Request::new(
                    ListAdminSchedulingCacheAnomaliesRequest { token, limit },
                ))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn get_worker_trust_profile(
        &mut self,
        token: &str,
        worker_id: &str,
    ) -> Result<GetWorkerTrustProfileResponse, tonic::Status> {
        let token = token.to_string();
        let worker_id = worker_id.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .get_worker_trust_profile(Request::new(GetWorkerTrustProfileRequest {
                    token,
                    worker_id,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn update_worker_trust_control(
        &mut self,
        token: &str,
        worker_id: &str,
        banned: bool,
        score: i32,
    ) -> Result<UpdateWorkerTrustControlResponse, tonic::Status> {
        let token = token.to_string();
        let worker_id = worker_id.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .update_worker_trust_control(Request::new(UpdateWorkerTrustControlRequest {
                    token,
                    worker_id,
                    banned,
                    score,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn list_admin_worker_trust(
        &mut self,
        token: &str,
    ) -> Result<ListAdminWorkerTrustResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .list_admin_worker_trust(Request::new(ListAdminWorkerTrustRequest { token }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn list_admin_audit_logs(
        &mut self,
        token: &str,
        limit: i64,
    ) -> Result<ListAdminAuditLogsResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .master
                .list_admin_audit_logs(Request::new(ListAdminAuditLogsRequest { token, limit }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    // ---- NodeManagerService ----
    pub async fn list_workers(
        &mut self,
        include_offline: bool,
        token: &str,
    ) -> Result<ListWorkersResponse, tonic::Status> {
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .node_mgr
                .list_workers(Request::new(ListWorkersRequest {
                    include_offline,
                    token,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
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
        let username = username.to_string();
        let worker_id = worker_id.to_string();
        let ip = ip.to_string();
        let location = location.to_string();
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .node_mgr
                .register_worker_node(Request::new(RegisterWorkerNodeRequest {
                    username,
                    worker_id,
                    ip,
                    resources: Some(resources),
                    location,
                    token,
                }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }

    pub async fn remove_worker(
        &mut self,
        worker_id: &str,
        token: &str,
    ) -> Result<StatusResponse, tonic::Status> {
        let worker_id = worker_id.to_string();
        let token = token.to_string();
        self.with_clients(|mut clients| async move {
            clients
                .node_mgr
                .remove_worker(Request::new(RemoveWorkerRequest { worker_id, token }))
                .await
                .map(|r| r.into_inner())
        })
        .await
    }
}

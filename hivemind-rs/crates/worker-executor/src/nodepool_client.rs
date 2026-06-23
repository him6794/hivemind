use std::sync::Arc;
use std::time::Duration;

use hivemind_models::{ResourceSpec, ResourceUsage};
use hivemind_proto::{
    node_manager_service_client::NodeManagerServiceClient, RegisterWorkerNodeRequest,
    ResourceSpec as ProtoResourceSpec, ResourceUsage as ProtoResourceUsage, RunningStatusRequest,
    TaskOutputUploadRequest, TaskResultUploadRequest, TaskUsageRequest,
};
use tokio::sync::watch;
use tonic::transport::Channel;
use tracing::{info, warn};

use crate::WorkerExecutor;

pub fn nodepool_endpoint(addr: &str) -> String {
    let addr = addr.trim();
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        format!("http://{}", replace_unspecified_host_for_local_client(addr))
    }
}

pub fn advertise_addr(listen_addr: &str, configured: Option<String>) -> anyhow::Result<String> {
    if let Some(addr) = configured.filter(|addr| !addr.trim().is_empty()) {
        return Ok(addr);
    }

    if has_unspecified_host(listen_addr) {
        anyhow::bail!(
            "WORKER_ADVERTISE_ADDR must be set when WORKER_GRPC_ADDR listens on an unspecified host ({listen_addr}); use an address reachable by the nodepool"
        );
    }

    Ok(listen_addr.to_string())
}

pub fn build_register_request(
    worker_id: &str,
    worker_addr: &str,
    resources: ResourceSpec,
    location: &str,
    token: &str,
) -> RegisterWorkerNodeRequest {
    RegisterWorkerNodeRequest {
        username: worker_id.to_string(),
        worker_id: worker_id.to_string(),
        ip: worker_addr.to_string(),
        resources: Some(resource_spec_to_proto(resources)),
        location: location.to_string(),
        token: token.to_string(),
    }
}

pub fn build_status_request(
    worker_id: &str,
    status: &str,
    usage: ResourceUsage,
    token: &str,
) -> RunningStatusRequest {
    RunningStatusRequest {
        username: worker_id.to_string(),
        worker_id: worker_id.to_string(),
        status: status.to_string(),
        usage: Some(resource_usage_to_proto(usage)),
        token: token.to_string(),
    }
}

pub async fn register_once(
    endpoint: &str,
    worker_id: &str,
    worker_addr: &str,
    resources: ResourceSpec,
    location: &str,
    token: &str,
) -> anyhow::Result<()> {
    let mut client = NodeManagerServiceClient::connect(nodepool_endpoint(endpoint)).await?;
    let response = client
        .register_worker_node(build_register_request(
            worker_id,
            worker_addr,
            resources,
            location,
            token,
        ))
        .await?
        .into_inner();
    if !response.success {
        anyhow::bail!(response.status_message);
    }
    Ok(())
}

pub async fn report_task_output_once(
    endpoint: &str,
    worker_id: &str,
    token: &str,
    task_id: &str,
    output: &str,
) -> anyhow::Result<()> {
    let mut client = NodeManagerServiceClient::connect(nodepool_endpoint(endpoint)).await?;
    let response = client
        .task_output_upload(TaskOutputUploadRequest {
            task_id: task_id.to_string(),
            output: output.to_string(),
            token: token.to_string(),
            worker_id: worker_id.to_string(),
        })
        .await?
        .into_inner();
    if !response.success {
        anyhow::bail!(response.status_message);
    }
    Ok(())
}

pub async fn report_task_result_once(
    endpoint: &str,
    worker_id: &str,
    token: &str,
    task_id: &str,
    result_torrent: &str,
) -> anyhow::Result<()> {
    let mut client = NodeManagerServiceClient::connect(nodepool_endpoint(endpoint)).await?;
    let response = client
        .task_result_upload(TaskResultUploadRequest {
            task_id: task_id.to_string(),
            result_torrent: result_torrent.to_string(),
            token: token.to_string(),
            worker_id: worker_id.to_string(),
        })
        .await?
        .into_inner();
    if !response.success {
        anyhow::bail!(response.status_message);
    }
    Ok(())
}

pub async fn report_task_usage_once(
    endpoint: &str,
    worker_id: &str,
    token: &str,
    task_id: &str,
    usage: ResourceUsage,
) -> anyhow::Result<()> {
    let mut client = NodeManagerServiceClient::connect(nodepool_endpoint(endpoint)).await?;
    let response = client
        .task_usage(TaskUsageRequest {
            task_id: task_id.to_string(),
            usage: Some(resource_usage_to_proto(usage)),
            token: token.to_string(),
            worker_id: worker_id.to_string(),
        })
        .await?
        .into_inner();
    if !response.success {
        anyhow::bail!(response.status_message);
    }
    Ok(())
}

pub fn start_registration_loop(
    executor: Arc<WorkerExecutor>,
    nodepool_addr: String,
    worker_id: String,
    worker_addr: String,
    location: String,
    token: String,
    interval: Duration,
) -> watch::Sender<bool> {
    let (tx, mut rx) = watch::channel(false);
    tokio::spawn(async move {
        let endpoint = nodepool_endpoint(&nodepool_addr);
        let mut client: Option<NodeManagerServiceClient<Channel>> = None;
        let mut tick = tokio::time::interval(interval);

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if client.is_none() {
                        match NodeManagerServiceClient::connect(endpoint.clone()).await {
                            Ok(mut connected) => {
                                let request = build_register_request(
                                    &worker_id,
                                    &worker_addr,
                                    executor.get_resource_spec(),
                                    &location,
                                    &token,
                                );
                                match connected.register_worker_node(request).await {
                                    Ok(response) if response.get_ref().success => {
                                        info!("Worker {} registered with nodepool {}", worker_id, endpoint);
                                        client = Some(connected);
                                    }
                                    Ok(response) => warn!("Worker registration rejected: {}", response.get_ref().status_message),
                                    Err(e) => warn!("Worker registration failed: {}", e),
                                }
                            }
                            Err(e) => warn!("Nodepool connection failed: {}", e),
                        }
                    }

                    if let Some(connected) = client.as_mut() {
                        let request = build_status_request(&worker_id, "IDLE", executor.get_resource_usage(), &token);
                        match connected.report_status(request).await {
                            Ok(response) if response.get_ref().success => {}
                            Ok(response) => {
                                warn!("Worker heartbeat rejected: {}", response.get_ref().status_message);
                                client = None;
                            }
                            Err(e) => {
                                warn!("Worker heartbeat failed: {}", e);
                                client = None;
                            }
                        }
                    }
                }
                _ = rx.changed() => {
                    if *rx.borrow() {
                        info!("Worker registration loop shutting down");
                        break;
                    }
                }
            }
        }
    });
    tx
}

fn replace_unspecified_host_for_local_client(addr: &str) -> String {
    addr.strip_prefix("0.0.0.0:")
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or_else(|| addr.to_string())
}

fn has_unspecified_host(addr: &str) -> bool {
    addr.strip_prefix("0.0.0.0:").is_some() || addr.strip_prefix("[::]:").is_some()
}

fn resource_spec_to_proto(spec: ResourceSpec) -> ProtoResourceSpec {
    ProtoResourceSpec {
        cpu_cores: spec.cpu_cores,
        memory_mb: spec.memory_mb,
        gpu_count: spec.gpu_count,
        gpu_name: spec.gpu_name,
        vram_mb: spec.vram_mb,
        cpu_score: spec.cpu_score,
        gpu_score: spec.gpu_score,
        storage_total_gb: spec.storage_total_gb,
        storage_available_gb: spec.storage_available_gb,
    }
}

fn resource_usage_to_proto(usage: ResourceUsage) -> ProtoResourceUsage {
    ProtoResourceUsage {
        cpu_percent: usage.cpu_percent as f32,
        memory_percent: usage.memory_percent as f32,
        gpu_percent: usage.gpu_percent as f32,
        vram_percent: usage.vram_percent as f32,
        storage_percent: usage.storage_percent as f32,
    }
}

#[cfg(test)]
mod tests {
    use hivemind_models::{ResourceSpec, ResourceUsage};
    use hivemind_proto::{
        node_manager_service_server::{NodeManagerService, NodeManagerServiceServer},
        ListWorkersRequest, ListWorkersResponse, RemoveWorkerRequest, RunningStatusResponse,
        StatusResponse, TaskOutputUploadRequest, TaskOutputUploadResponse, TaskResultUploadRequest,
        TaskResultUploadResponse, TaskUsageRequest, TaskUsageResponse,
    };
    use std::net::SocketAddr;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tonic::{Request, Response, Status};

    #[test]
    fn nodepool_endpoint_adds_http_scheme_and_replaces_unspecified_host() {
        assert_eq!(
            super::nodepool_endpoint("0.0.0.0:50051"),
            "http://127.0.0.1:50051"
        );
        assert_eq!(
            super::nodepool_endpoint("127.0.0.1:50051"),
            "http://127.0.0.1:50051"
        );
        assert_eq!(
            super::nodepool_endpoint("http://nodepool:50051"),
            "http://nodepool:50051"
        );
    }

    #[test]
    fn advertise_addr_requires_reachable_address_for_unspecified_listener() {
        let error = super::advertise_addr("0.0.0.0:50053", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("WORKER_ADVERTISE_ADDR"));
        assert_eq!(
            super::advertise_addr("192.0.2.10:50053", None).unwrap(),
            "192.0.2.10:50053"
        );
        assert_eq!(
            super::advertise_addr("0.0.0.0:50053", Some("worker.local:50053".to_string())).unwrap(),
            "worker.local:50053"
        );
    }

    #[test]
    fn build_register_request_carries_resources() {
        let spec = ResourceSpec {
            cpu_cores: 8,
            memory_mb: 32768,
            gpu_count: 1,
            gpu_name: "RTX".into(),
            vram_mb: 12288,
            cpu_score: 800,
            gpu_score: 1200,
            storage_total_gb: 1000,
            storage_available_gb: 500,
        };

        let request =
            super::build_register_request("worker-1", "127.0.0.1:50053", spec, "local", "token");
        let resources = request.resources.unwrap();

        assert_eq!(request.worker_id, "worker-1");
        assert_eq!(request.username, "worker-1");
        assert_eq!(request.ip, "127.0.0.1:50053");
        assert_eq!(request.token, "token");
        assert_eq!(resources.cpu_cores, 8);
        assert_eq!(resources.memory_mb, 32768);
        assert_eq!(resources.vram_mb, 12288);
    }

    #[test]
    fn build_status_request_carries_usage() {
        let usage = ResourceUsage {
            cpu_percent: 10.5,
            memory_percent: 30.0,
            gpu_percent: 40.0,
            vram_percent: 50.0,
            storage_percent: 60.0,
        };

        let request = super::build_status_request("worker-1", "IDLE", usage, "token");
        let usage = request.usage.unwrap();

        assert_eq!(request.username, "worker-1");
        assert_eq!(request.worker_id, "worker-1");
        assert_eq!(request.status, "IDLE");
        assert_eq!(request.token, "token");
        assert_eq!(usage.cpu_percent, 10.5);
        assert_eq!(usage.storage_percent, 60.0);
    }

    #[tokio::test]
    async fn report_task_output_once_sends_worker_scoped_rpc() {
        let (addr, mut reports) = match fake_node_manager_report_server().await {
            Some(parts) => parts,
            None => return,
        };

        super::report_task_output_once(
            &addr.to_string(),
            "worker-report-1",
            "worker-token-1",
            "task-report-1",
            "stdout payload",
        )
        .await
        .unwrap();

        let request = tokio::time::timeout(Duration::from_secs(2), reports.output_rx.recv())
            .await
            .expect("node manager should receive output report")
            .expect("report channel should stay open");
        assert_eq!(request.worker_id, "worker-report-1");
        assert_eq!(request.task_id, "task-report-1");
        assert_eq!(request.token, "worker-token-1");
        assert_eq!(request.output, "stdout payload");
    }

    #[tokio::test]
    async fn report_task_result_once_sends_worker_scoped_rpc() {
        let (addr, mut reports) = match fake_node_manager_report_server().await {
            Some(parts) => parts,
            None => return,
        };

        super::report_task_result_once(
            &addr.to_string(),
            "worker-report-2",
            "worker-token-2",
            "task-report-2",
            "btih:result-ref",
        )
        .await
        .unwrap();

        let request = tokio::time::timeout(Duration::from_secs(2), reports.result_rx.recv())
            .await
            .expect("node manager should receive result report")
            .expect("report channel should stay open");
        assert_eq!(request.worker_id, "worker-report-2");
        assert_eq!(request.task_id, "task-report-2");
        assert_eq!(request.token, "worker-token-2");
        assert_eq!(request.result_torrent, "btih:result-ref");
    }

    #[tokio::test]
    async fn report_task_usage_once_sends_worker_scoped_rpc() {
        let (addr, mut reports) = match fake_node_manager_report_server().await {
            Some(parts) => parts,
            None => return,
        };

        super::report_task_usage_once(
            &addr.to_string(),
            "worker-report-3",
            "worker-token-3",
            "task-report-3",
            ResourceUsage {
                cpu_percent: 11.0,
                memory_percent: 22.0,
                gpu_percent: 33.0,
                vram_percent: 44.0,
                storage_percent: 55.0,
            },
        )
        .await
        .unwrap();

        let request = tokio::time::timeout(Duration::from_secs(2), reports.usage_rx.recv())
            .await
            .expect("node manager should receive usage report")
            .expect("report channel should stay open");
        let usage = request.usage.unwrap();
        assert_eq!(request.worker_id, "worker-report-3");
        assert_eq!(request.task_id, "task-report-3");
        assert_eq!(request.token, "worker-token-3");
        assert_eq!(usage.cpu_percent, 11.0);
        assert_eq!(usage.vram_percent, 44.0);
    }

    async fn fake_node_manager_report_server() -> Option<(SocketAddr, CapturedReports)> {
        let addr = reserve_loopback_addr()?;
        let (output_tx, output_rx) = mpsc::channel(1);
        let (result_tx, result_rx) = mpsc::channel(1);
        let (usage_tx, usage_rx) = mpsc::channel(1);
        let service = NodeManagerServiceServer::new(FakeNodeManagerReportService {
            output_tx,
            result_tx,
            usage_tx,
        });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve(addr)
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::node_manager_service_client::NodeManagerServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((
                    addr,
                    CapturedReports {
                        output_rx,
                        result_rx,
                        usage_rx,
                    },
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    fn reserve_loopback_addr() -> Option<SocketAddr> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
        let addr = listener.local_addr().ok()?;
        drop(listener);
        Some(addr)
    }

    struct CapturedReports {
        output_rx: mpsc::Receiver<TaskOutputUploadRequest>,
        result_rx: mpsc::Receiver<TaskResultUploadRequest>,
        usage_rx: mpsc::Receiver<TaskUsageRequest>,
    }

    struct FakeNodeManagerReportService {
        output_tx: mpsc::Sender<TaskOutputUploadRequest>,
        result_tx: mpsc::Sender<TaskResultUploadRequest>,
        usage_tx: mpsc::Sender<TaskUsageRequest>,
    }

    #[tonic::async_trait]
    impl NodeManagerService for FakeNodeManagerReportService {
        async fn register_worker_node(
            &self,
            _request: Request<hivemind_proto::RegisterWorkerNodeRequest>,
        ) -> Result<Response<StatusResponse>, Status> {
            Ok(Response::new(StatusResponse {
                success: true,
                status_message: "OK".into(),
            }))
        }

        async fn report_status(
            &self,
            _request: Request<hivemind_proto::RunningStatusRequest>,
        ) -> Result<Response<RunningStatusResponse>, Status> {
            Ok(Response::new(RunningStatusResponse {
                success: true,
                status_message: "OK".into(),
            }))
        }

        async fn task_output_upload(
            &self,
            request: Request<TaskOutputUploadRequest>,
        ) -> Result<Response<TaskOutputUploadResponse>, Status> {
            self.output_tx
                .send(request.into_inner())
                .await
                .map_err(|_| Status::internal("report receiver dropped"))?;
            Ok(Response::new(TaskOutputUploadResponse {
                success: true,
                status_message: "OK".into(),
            }))
        }

        async fn task_result_upload(
            &self,
            request: Request<TaskResultUploadRequest>,
        ) -> Result<Response<TaskResultUploadResponse>, Status> {
            self.result_tx
                .send(request.into_inner())
                .await
                .map_err(|_| Status::internal("report receiver dropped"))?;
            Ok(Response::new(TaskResultUploadResponse {
                success: true,
                status_message: "OK".into(),
            }))
        }

        async fn task_usage(
            &self,
            request: Request<TaskUsageRequest>,
        ) -> Result<Response<TaskUsageResponse>, Status> {
            self.usage_tx
                .send(request.into_inner())
                .await
                .map_err(|_| Status::internal("report receiver dropped"))?;
            Ok(Response::new(TaskUsageResponse {
                success: true,
                status_message: "OK".into(),
            }))
        }

        async fn list_workers(
            &self,
            _request: Request<ListWorkersRequest>,
        ) -> Result<Response<ListWorkersResponse>, Status> {
            Ok(Response::new(ListWorkersResponse {
                success: true,
                status_message: "OK".into(),
                workers: vec![],
            }))
        }

        async fn remove_worker(
            &self,
            _request: Request<RemoveWorkerRequest>,
        ) -> Result<Response<StatusResponse>, Status> {
            Ok(Response::new(StatusResponse {
                success: true,
                status_message: "OK".into(),
            }))
        }
    }
}

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use hivemind_models::{
    ResourceSpec, ResourceUsage, WorkerCapabilityReport as ModelWorkerCapabilityReport,
};
use hivemind_proto::{
    node_manager_service_client::NodeManagerServiceClient, user_service_client::UserServiceClient,
    worker_node_service_server::WorkerNodeService, worker_session_client_frame,
    worker_session_server_frame, worker_session_service_client::WorkerSessionServiceClient,
    ExecuteTaskRequest, ExecuteTaskResponse, LoginRequest, RegisterWorkerNodeRequest,
    ResourceSpec as ProtoResourceSpec, ResourceUsage as ProtoResourceUsage, RunningStatusRequest,
    TaskOutputUploadRequest, TaskResultUploadRequest, TaskUsageRequest,
    ValidateGeneralComputeTransferLeaseRequest,
    WorkerCapabilityReport as ProtoWorkerCapabilityReport, WorkerSessionAck,
    WorkerSessionCancelAck, WorkerSessionClientFrame, WorkerSessionClose, WorkerSessionHeartbeat,
    WorkerSessionHello, WorkerSessionResult,
};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Endpoint};
use tracing::{info, warn};

use crate::grpc_server::GrpcWorkerNodeService;
use crate::WorkerExecutor;

pub fn nodepool_endpoint(addr: &str) -> String {
    let addr = addr.trim();
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        format!("http://{}", replace_unspecified_host_for_local_client(addr))
    }
}

const TRANSFER_LEASE_AUTHORITY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransferLeaseAuthorityError {
    #[error("Nodepool denied the general-compute transfer lease: {0}")]
    Denied(String),
    #[error("Nodepool transfer-lease authority is unavailable: {0}")]
    Unavailable(String),
}

#[tonic::async_trait]
pub trait TransferLeaseAuthority: Send + Sync {
    async fn validate(
        &self,
        request: ValidateGeneralComputeTransferLeaseRequest,
    ) -> Result<(), TransferLeaseAuthorityError>;
}

#[derive(Debug, Clone)]
pub struct NodepoolTransferLeaseAuthority {
    endpoint: String,
    timeout: Duration,
}

impl NodepoolTransferLeaseAuthority {
    #[must_use]
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            timeout: TRANSFER_LEASE_AUTHORITY_TIMEOUT,
        }
    }
}

#[tonic::async_trait]
impl TransferLeaseAuthority for NodepoolTransferLeaseAuthority {
    async fn validate(
        &self,
        request: ValidateGeneralComputeTransferLeaseRequest,
    ) -> Result<(), TransferLeaseAuthorityError> {
        hivemind_proto::validate_general_compute_transfer_lease_request(&request)
            .map_err(|message| TransferLeaseAuthorityError::Denied(message.into()))?;
        let endpoint = Endpoint::from_shared(nodepool_endpoint(&self.endpoint))
            .map_err(|error| TransferLeaseAuthorityError::Unavailable(error.to_string()))?
            .connect_timeout(self.timeout);
        let response = tokio::time::timeout(self.timeout, async move {
            let channel = endpoint
                .connect()
                .await
                .map_err(|error| error.to_string())?;
            NodeManagerServiceClient::new(channel)
                .validate_general_compute_transfer_lease(request)
                .await
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| {
            TransferLeaseAuthorityError::Unavailable(
                "validation timed out before Nodepool responded".into(),
            )
        })?
        .map_err(TransferLeaseAuthorityError::Unavailable)?
        .into_inner();
        if response.success {
            Ok(())
        } else {
            Err(TransferLeaseAuthorityError::Denied(
                if response.status_message.trim().is_empty() {
                    "transfer lease is inactive".into()
                } else {
                    response.status_message
                },
            ))
        }
    }
}

pub fn advertise_addr(listen_addr: &str, configured: Option<String>) -> anyhow::Result<String> {
    advertise_addr_for_vpn(listen_addr, configured, None)
}

pub fn validate_advertise_addr(addr: &str) -> anyhow::Result<String> {
    let addr = addr.trim();
    if addr.is_empty() {
        anyhow::bail!("Worker advertise address must not be blank");
    }
    if addr.contains("://") {
        anyhow::bail!("Worker advertise address must be host:port, not a URL");
    }

    let (host, port) = if let Some(rest) = addr.strip_prefix('[') {
        let (host, port) = rest
            .split_once(']')
            .ok_or_else(|| anyhow::anyhow!("invalid Worker advertise address: {addr}"))?;
        let port = port
            .strip_prefix(':')
            .ok_or_else(|| anyhow::anyhow!("invalid Worker advertise address: {addr}"))?;
        (host, port)
    } else {
        addr.rsplit_once(':').ok_or_else(|| {
            anyhow::anyhow!("Worker advertise address must include a port: {addr}")
        })?
    };
    if host.trim().is_empty() {
        anyhow::bail!("Worker advertise address must include a host");
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("invalid Worker advertise port in {addr}"))?;
    if port == 0 {
        anyhow::bail!("Worker advertise port must be non-zero");
    }
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        anyhow::bail!("Worker advertise address must not use localhost");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip.is_loopback() || ip.is_unspecified() {
            anyhow::bail!("Worker advertise address must be reachable from Nodepool, not {host}");
        }
    }
    Ok(addr.to_string())
}

/// Resolve the address the Nodepool should use to call the Worker.
///
/// A keyed Headscale worker commonly binds `0.0.0.0` locally. In that mode the
/// overlay address is the only safe implicit advertisement; never register the
/// process-local worker id as if it were network-reachable.
pub fn advertise_addr_for_vpn(
    listen_addr: &str,
    configured: Option<String>,
    overlay_ip: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(addr) = configured.filter(|addr| !addr.trim().is_empty()) {
        return validate_advertise_addr(&addr);
    }

    if has_unspecified_host(listen_addr) {
        if let Some(ip) = overlay_ip.map(str::trim).filter(|ip| !ip.is_empty()) {
            let port = listen_addr
                .rsplit_once(':')
                .map(|(_, port)| port)
                .filter(|port| !port.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "WORKER_GRPC_ADDR must include a port when deriving the Headscale advertise address ({listen_addr})"
                    )
                })?;
            let host = if ip.contains(':') && !ip.starts_with('[') {
                format!("[{ip}]")
            } else {
                ip.to_string()
            };
            return Ok(format!("{host}:{port}"));
        }
        anyhow::bail!(
            "WORKER_ADVERTISE_ADDR or a connected Headscale overlay IP is required when WORKER_GRPC_ADDR listens on an unspecified host ({listen_addr})"
        );
    }

    Ok(listen_addr.to_string())
}

pub async fn login_to_nodepool(
    nodepool_addr: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<String> {
    let username = username.trim();
    if username.is_empty() {
        anyhow::bail!("nodepool login username is required");
    }
    if password.is_empty() {
        anyhow::bail!("nodepool login password is required");
    }
    let endpoint = Endpoint::from_shared(nodepool_endpoint(nodepool_addr))?
        .connect_timeout(Duration::from_secs(5));
    let channel = endpoint.connect().await?;
    let mut client = UserServiceClient::new(channel);
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        client.login(LoginRequest {
            username: username.to_string(),
            password: password.to_string(),
        }),
    )
    .await
    .map_err(|_| anyhow::anyhow!("nodepool login timed out after 10 seconds"))??
    .into_inner();
    if !response.success || response.token.trim().is_empty() {
        anyhow::bail!(
            "nodepool login failed: {}",
            if response.status_message.is_empty() {
                "invalid credentials"
            } else {
                response.status_message.as_str()
            }
        );
    }
    Ok(response.token)
}

pub async fn resolve_nodepool_token(
    nodepool_addr: &str,
    worker_id: &str,
    configured_token: Option<&str>,
    login_username: Option<&str>,
    login_password: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(token) = configured_token.map(str::trim).filter(|t| !t.is_empty()) {
        return Ok(token.to_string());
    }
    let username = login_username
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            let id = worker_id.trim();
            if id.is_empty() {
                None
            } else {
                Some(id.to_string())
            }
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "set WORKER_NODEPOOL_TOKEN or WORKER_NODEPOOL_USERNAME/WORKER_NODEPOOL_PASSWORD (or WORKER_USERNAME/WORKER_PASSWORD)"
            )
        })?;
    let password = login_password
        .ok_or_else(|| anyhow::anyhow!("WORKER_NODEPOOL_PASSWORD (or WORKER_PASSWORD) is required when WORKER_NODEPOOL_TOKEN is not set"))?;
    login_to_nodepool(nodepool_addr, &username, password).await
}

pub fn capability_report_to_proto(
    report: &ModelWorkerCapabilityReport,
) -> anyhow::Result<ProtoWorkerCapabilityReport> {
    Ok(ProtoWorkerCapabilityReport {
        protocol_version: report.protocol_version,
        capabilities_json: report
            .capabilities_json()
            .map_err(|error| anyhow::anyhow!(error))?,
        ready: report.ready,
        readiness_reason: report.readiness_reason.clone(),
    })
}

pub fn build_register_request_with_capability_report(
    worker_id: &str,
    username: &str,
    worker_addr: &str,
    resources: ResourceSpec,
    location: &str,
    token: &str,
    capability_report: Option<ProtoWorkerCapabilityReport>,
) -> RegisterWorkerNodeRequest {
    RegisterWorkerNodeRequest {
        username: username.to_string(),
        worker_id: worker_id.to_string(),
        ip: worker_addr.to_string(),
        resources: Some(resource_spec_to_proto(resources)),
        location: location.to_string(),
        token: token.to_string(),
        capability_report,
    }
}

pub fn build_register_request(
    worker_id: &str,
    username: &str,
    worker_addr: &str,
    resources: ResourceSpec,
    location: &str,
    token: &str,
) -> RegisterWorkerNodeRequest {
    build_register_request_with_capability_report(
        worker_id,
        username,
        worker_addr,
        resources,
        location,
        token,
        None,
    )
}

pub fn build_status_request_with_capability_report(
    worker_id: &str,
    status: &str,
    usage: ResourceUsage,
    token: &str,
    capability_report: Option<ProtoWorkerCapabilityReport>,
) -> RunningStatusRequest {
    RunningStatusRequest {
        username: worker_id.to_string(),
        worker_id: worker_id.to_string(),
        status: status.to_string(),
        usage: Some(resource_usage_to_proto(usage)),
        token: token.to_string(),
        capability_report,
    }
}

pub fn build_status_request(
    worker_id: &str,
    status: &str,
    usage: ResourceUsage,
    token: &str,
) -> RunningStatusRequest {
    build_status_request_with_capability_report(worker_id, status, usage, token, None)
}

pub async fn register_once(
    endpoint: &str,
    worker_id: &str,
    username: &str,
    worker_addr: &str,
    resources: ResourceSpec,
    location: &str,
    token: &str,
) -> anyhow::Result<()> {
    register_once_with_capability_report(
        endpoint,
        worker_id,
        username,
        worker_addr,
        resources,
        location,
        token,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn register_once_with_capability_report(
    endpoint: &str,
    worker_id: &str,
    username: &str,
    worker_addr: &str,
    resources: ResourceSpec,
    location: &str,
    token: &str,
    capability_report: Option<ProtoWorkerCapabilityReport>,
) -> anyhow::Result<()> {
    let channel = Endpoint::from_shared(nodepool_endpoint(endpoint))?
        .connect_timeout(Duration::from_secs(5))
        .connect()
        .await?;
    let mut client = NodeManagerServiceClient::new(channel);
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        client.register_worker_node(build_register_request_with_capability_report(
            worker_id,
            username,
            worker_addr,
            resources,
            location,
            token,
            capability_report,
        )),
    )
    .await
    .map_err(|_| anyhow::anyhow!("worker registration timed out after 10 seconds"))??
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

pub struct RegistrationLoopConfig {
    pub nodepool_addr: Arc<std::sync::Mutex<String>>,
    pub worker_id: String,
    pub username: String,
    pub worker_addr: Arc<std::sync::Mutex<String>>,
    pub location: String,
    pub token: String,
    pub interval: Duration,
}

pub fn start_registration_loop(
    executor: Arc<WorkerExecutor>,
    registration: RegistrationLoopConfig,
) -> watch::Sender<bool> {
    let (tx, mut rx) = watch::channel(false);
    tokio::spawn(async move {
        let mut endpoint = String::new();
        let mut client: Option<NodeManagerServiceClient<Channel>> = None;
        let mut registered_worker_addr: Option<String> = None;
        let mut tick = tokio::time::interval(registration.interval);

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Some(session) = hivemind_client_runtime::current_vpn_session(
                        hivemind_client_runtime::ClientRole::Worker,
                    ).await {
                        if let Some(bridge) = session.bridge_endpoint() {
                            let mut guard = registration
                                .nodepool_addr
                                .lock()
                                .unwrap_or_else(|err| err.into_inner());
                            if *guard != bridge {
                                *guard = bridge;
                                client = None;
                            }
                        }
                    }
                    let configured_addr = registration
                        .nodepool_addr
                        .lock()
                        .unwrap_or_else(|err| err.into_inner())
                        .clone();
                    let current_endpoint = nodepool_endpoint(&configured_addr);
                    if endpoint != current_endpoint {
                        endpoint = current_endpoint.clone();
                        client = None;
                    }

                    let worker_addr = registration
                        .worker_addr
                        .lock()
                        .unwrap_or_else(|err| err.into_inner())
                        .clone();
                    let capability_report = match capability_report_to_proto(
                        &executor.dynamic_capability_report(),
                    ) {
                        Ok(report) => report,
                        Err(error) => {
                            warn!("Worker capability report is invalid: {error}");
                            continue;
                        }
                    };
                    if registered_worker_addr.as_deref() != Some(worker_addr.as_str()) {
                        // A changed callback address must be persisted through a fresh
                        // registration; a heartbeat alone leaves Nodepool's old address.
                        client = None;
                    }

                    if client.is_none() {
                        match tokio::time::timeout(
                            Duration::from_secs(10),
                            NodeManagerServiceClient::connect(current_endpoint.clone()),
                        )
                        .await
                        {
                            Ok(Ok(mut connected)) => {
                                let request = build_register_request_with_capability_report(
                                    &registration.worker_id,
                                    &registration.username,
                                    &worker_addr,
                                    executor.get_resource_spec(),
                                    &registration.location,
                                    &registration.token,
                                    Some(capability_report.clone()),
                                );
                                match tokio::time::timeout(
                                    Duration::from_secs(10),
                                    connected.register_worker_node(request),
                                )
                                .await
                                {
                                    Ok(Ok(response)) if response.get_ref().success => {
                                        info!("Worker {} registered with nodepool {}", registration.worker_id, current_endpoint);
                                        registered_worker_addr = Some(worker_addr.clone());
                                        client = Some(connected);
                                    }
                                    Ok(Ok(response)) => warn!("Worker registration rejected: {}", response.get_ref().status_message),
                                    Ok(Err(e)) => warn!("Worker registration failed: {}", e),
                                    Err(_) => warn!("Worker registration timed out after 10 seconds"),
                                }
                            }
                            Ok(Err(e)) => warn!("Nodepool connection failed: {}", e),
                            Err(_) => warn!("Nodepool connection timed out after 10 seconds"),
                        }
                    }

                    if let Some(connected) = client.as_mut() {
                        let request = build_status_request_with_capability_report(
                            &registration.worker_id,
                            "IDLE",
                            executor.get_resource_usage(),
                            &registration.token,
                            Some(capability_report),
                        );
                        match tokio::time::timeout(
                            Duration::from_secs(10),
                            connected.report_status(request),
                        )
                        .await
                        {
                            Ok(Ok(response)) if response.get_ref().success => {}
                            Ok(Ok(response)) => {
                                warn!("Worker heartbeat rejected: {}", response.get_ref().status_message);
                                client = None;
                            }
                            Ok(Err(e)) => {
                                warn!("Worker heartbeat failed: {}", e);
                                client = None;
                            }
                            Err(_) => {
                                warn!("Worker heartbeat timed out after 10 seconds");
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

#[derive(Clone)]
pub struct SessionLoopConfig {
    pub nodepool_addr: Arc<std::sync::Mutex<String>>,
    pub worker_id: String,
    pub username: String,
    pub client_instance_id: String,
    pub token: String,
    pub interval: Duration,
    pub service: Arc<GrpcWorkerNodeService>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionRunOutcome {
    Reconnect,
    Terminal,
    Shutdown,
}

pub fn start_session_loop(
    executor: Arc<WorkerExecutor>,
    config: SessionLoopConfig,
) -> watch::Sender<bool> {
    let (tx, mut shutdown) = watch::channel(false);
    tokio::spawn(async move {
        let mut resume_token = None;
        let mut last_received_sequence = 0;
        let mut backoff = Duration::from_secs(1);
        loop {
            let result = run_worker_session(
                executor.clone(),
                &config,
                &mut resume_token,
                &mut last_received_sequence,
                &mut shutdown,
            )
            .await;
            match result {
                Ok(SessionRunOutcome::Shutdown) | Ok(SessionRunOutcome::Terminal) => break,
                Ok(SessionRunOutcome::Reconnect) | Err(_) => {
                    backoff = if matches!(result, Ok(SessionRunOutcome::Reconnect)) {
                        Duration::from_secs(1)
                    } else {
                        if let Err(error) = result.as_ref() {
                            warn!(
                                worker_id = %config.worker_id,
                                error = %error,
                                "Worker outbound session attempt failed; retrying with backoff"
                            );
                        }
                        (backoff + backoff).min(Duration::from_secs(30))
                    };
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {}
                        changed = shutdown.changed() => {
                            if changed.is_ok() && *shutdown.borrow() {
                                break;
                            }
                        }
                    }
                }
            }
        }
        info!(worker_id = %config.worker_id, "Worker outbound session loop stopped");
    });
    tx
}

async fn run_worker_session(
    executor: Arc<WorkerExecutor>,
    config: &SessionLoopConfig,
    resume_token: &mut Option<String>,
    last_received_sequence: &mut u64,
    shutdown: &mut watch::Receiver<bool>,
) -> anyhow::Result<SessionRunOutcome> {
    refresh_nodepool_bridge(&config.nodepool_addr).await;
    let configured_addr = config
        .nodepool_addr
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let endpoint = nodepool_endpoint(&configured_addr);
    let endpoint_builder =
        Endpoint::from_shared(endpoint.clone())?.connect_timeout(Duration::from_secs(10));
    let channel =
        match tokio::time::timeout(Duration::from_secs(10), endpoint_builder.connect()).await {
            Ok(Ok(channel)) => channel,
            Ok(Err(error)) => anyhow::bail!("Worker session connection failed: {error}"),
            Err(_) => anyhow::bail!("Worker session connection timed out"),
        };
    let mut client = WorkerSessionServiceClient::new(channel)
        .max_decoding_message_size(hivemind_proto::WORKER_SESSION_FRAME_MAX_BYTES)
        .max_encoding_message_size(hivemind_proto::WORKER_SESSION_FRAME_MAX_BYTES);
    let (sender, receiver) = mpsc::channel(64);
    let capability_report = capability_report_to_proto(&executor.dynamic_capability_report())?;
    send_client_frame(
        &sender,
        WorkerSessionClientFrame {
            frame: Some(worker_session_client_frame::Frame::Hello(
                WorkerSessionHello {
                    protocol_version: hivemind_proto::WORKER_SESSION_PROTOCOL_VERSION,
                    token: config.token.clone(),
                    worker_id: config.worker_id.clone(),
                    owner: config.username.clone(),
                    client_instance_id: config.client_instance_id.clone(),
                    capability_report: Some(capability_report),
                    resume_token: resume_token.clone().unwrap_or_default(),
                    last_received_sequence: *last_received_sequence,
                },
            )),
        },
    )
    .await?;
    let response = match tokio::time::timeout(
        Duration::from_secs(10),
        client.open_session(tonic::Request::new(ReceiverStream::new(receiver))),
    )
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(status)) if is_terminal_session_status(&status) => {
            warn!(worker_id = %config.worker_id, "Worker session authentication was rejected");
            return Ok(SessionRunOutcome::Terminal);
        }
        Ok(Err(status)) => anyhow::bail!("Worker session open failed: {status}"),
        Err(_) => anyhow::bail!("Worker session open timed out"),
    };
    let mut inbound = response.into_inner();
    let welcome = tokio::time::timeout(Duration::from_secs(10), inbound.message())
        .await
        .map_err(|_| anyhow::anyhow!("Worker session welcome timed out"))??
        .ok_or_else(|| anyhow::anyhow!("Worker session closed before welcome"))?;
    hivemind_proto::validate_worker_session_server_frame(&welcome)
        .map_err(|message| anyhow::anyhow!(message))?;
    let Some(worker_session_server_frame::Frame::Welcome(welcome)) = welcome.frame else {
        anyhow::bail!("Worker session did not return a welcome frame");
    };
    if !welcome.success
        || welcome.worker_id != config.worker_id
        || welcome.owner != config.username
        || welcome.client_instance_id != config.client_instance_id
    {
        anyhow::bail!("Worker session welcome identity was rejected");
    }
    if welcome.resume_token.trim().is_empty() {
        anyhow::bail!("Worker session welcome did not include resume state");
    }
    *resume_token = Some(welcome.resume_token);

    let mut heartbeat = tokio::time::interval(config.interval.max(Duration::from_secs(1)));
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() {
                    let _ = send_client_frame(
                        &sender,
                        WorkerSessionClientFrame {
                            frame: Some(worker_session_client_frame::Frame::Close(
                                WorkerSessionClose { reason: "shutdown".into() },
                            )),
                        },
                    ).await;
                    return Ok(SessionRunOutcome::Shutdown);
                }
            }
            message = inbound.message() => {
                let Some(frame) = message? else {
                    return Ok(SessionRunOutcome::Reconnect);
                };
                hivemind_proto::validate_worker_session_server_frame(&frame)
                    .map_err(|message| anyhow::anyhow!(message))?;
                match frame.frame {
                    Some(worker_session_server_frame::Frame::Task(task)) => {
                        let Some(request) = task.request else {
                            anyhow::bail!("Worker session task is missing its request");
                        };
                        *last_received_sequence = (*last_received_sequence).max(task.delivery_sequence);
                        let task_sender = sender.clone();
                        let service = config.service.clone();
                        tokio::spawn(async move {
                            process_session_task(service, task_sender, task.delivery_sequence, request).await;
                        });
                    }
                    Some(worker_session_server_frame::Frame::Cancel(cancel)) => {
                        let Some(request) = cancel.request else {
                            anyhow::bail!("Worker session cancellation is missing its request");
                        };
                        *last_received_sequence =
                            (*last_received_sequence).max(cancel.delivery_sequence);
                        let task_id = request.task_id.clone();
                        let attempt_id = request.attempt_id.clone();
                        let idempotency_key = request.idempotency_key.clone();
                        let task_sender = sender.clone();
                        let service = config.service.clone();
                        tokio::spawn(async move {
                            match WorkerNodeService::stop_task_execution(
                                service.as_ref(),
                                tonic::Request::new(request),
                            )
                            .await
                            {
                                Ok(_) => {
                                    let _ = send_client_frame(
                                        &task_sender,
                                        WorkerSessionClientFrame {
                                            frame: Some(
                                                worker_session_client_frame::Frame::CancelAck(
                                                    WorkerSessionCancelAck {
                                                        delivery_sequence: cancel.delivery_sequence,
                                                        task_id,
                                                        attempt_id,
                                                        idempotency_key,
                                                    },
                                                ),
                                            ),
                                        },
                                    )
                                    .await;
                                }
                                Err(status) => {
                                    warn!(
                                        task_id = %task_id,
                                        code = ?status.code(),
                                        "Worker session cancellation could not be applied"
                                    );
                                    let _ = send_client_frame(
                                        &task_sender,
                                        WorkerSessionClientFrame {
                                            frame: Some(worker_session_client_frame::Frame::Close(
                                                WorkerSessionClose {
                                                    reason: "cancellation request failed".into(),
                                                },
                                            )),
                                        },
                                    )
                                    .await;
                                }
                            }
                        });
                    }
                    Some(worker_session_server_frame::Frame::Heartbeat(heartbeat)) => {
                        *last_received_sequence = (*last_received_sequence).max(heartbeat.last_received_sequence);
                    }
                    Some(worker_session_server_frame::Frame::Error(error)) => {
                        if error.terminal {
                            return Ok(SessionRunOutcome::Terminal);
                        }
                    }
                    Some(worker_session_server_frame::Frame::Close(_)) => {
                        return Ok(SessionRunOutcome::Reconnect);
                    }
                    Some(worker_session_server_frame::Frame::Welcome(_)) => {
                        anyhow::bail!("Worker session returned a duplicate welcome frame");
                    }
                    None => anyhow::bail!("Worker session returned an empty frame"),
                }
            }
            _ = heartbeat.tick() => {
                refresh_nodepool_bridge(&config.nodepool_addr).await;
                let current_addr = config
                    .nodepool_addr
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone();
                if nodepool_endpoint(&current_addr) != endpoint {
                    return Ok(SessionRunOutcome::Reconnect);
                }
                send_client_frame(
                    &sender,
                    WorkerSessionClientFrame {
                        frame: Some(worker_session_client_frame::Frame::Heartbeat(
                            WorkerSessionHeartbeat {
                                last_received_sequence: *last_received_sequence,
                            },
                        )),
                    },
                )
                .await?;
            }
        }
    }
}

async fn process_session_task(
    service: Arc<GrpcWorkerNodeService>,
    sender: mpsc::Sender<WorkerSessionClientFrame>,
    delivery_sequence: u64,
    request: ExecuteTaskRequest,
) {
    if request.task_id.trim().is_empty()
        || request.execution_id.trim().is_empty()
        || request.attempt_id.trim().is_empty()
        || request.idempotency_key.trim().is_empty()
        || request.request_digest.trim().is_empty()
    {
        tracing::warn!(
            task_id = %request.task_id,
            "Worker session task has invalid execution identity"
        );
        if !request.task_id.trim().is_empty() {
            let _ = send_client_frame(
                &sender,
                WorkerSessionClientFrame {
                    frame: Some(worker_session_client_frame::Frame::Result(
                        WorkerSessionResult {
                            delivery_sequence,
                            task_id: request.task_id.clone(),
                            response: Some(failed_session_response(&request)),
                        },
                    )),
                },
            )
            .await;
        }
        return;
    }
    if send_client_frame(
        &sender,
        WorkerSessionClientFrame {
            frame: Some(worker_session_client_frame::Frame::Ack(WorkerSessionAck {
                delivery_sequence,
                task_id: request.task_id.clone(),
                attempt_id: request.attempt_id.clone(),
                idempotency_key: request.idempotency_key.clone(),
            })),
        },
    )
    .await
    .is_err()
    {
        return;
    }
    let response = match WorkerNodeService::execute_task(
        service.as_ref(),
        tonic::Request::new(request.clone()),
    )
    .await
    {
        Ok(response) => response.into_inner(),
        Err(status) => {
            tracing::warn!(
                task_id = %request.task_id,
                code = ?status.code(),
                "Worker session task execution was rejected"
            );
            failed_session_response(&request)
        }
    };
    let _ = send_client_frame(
        &sender,
        WorkerSessionClientFrame {
            frame: Some(worker_session_client_frame::Frame::Result(
                WorkerSessionResult {
                    delivery_sequence,
                    task_id: request.task_id,
                    response: Some(response),
                },
            )),
        },
    )
    .await;
}

fn failed_session_response(request: &ExecuteTaskRequest) -> ExecuteTaskResponse {
    ExecuteTaskResponse {
        success: false,
        status_message: "Worker execution failed".into(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        ..ExecuteTaskResponse::default()
    }
}

async fn send_client_frame(
    sender: &mpsc::Sender<WorkerSessionClientFrame>,
    frame: WorkerSessionClientFrame,
) -> anyhow::Result<()> {
    hivemind_proto::validate_worker_session_client_frame(&frame)
        .map_err(|message| anyhow::anyhow!(message))?;
    sender
        .send(frame)
        .await
        .map_err(|_| anyhow::anyhow!("Worker session stream is closed"))
}

async fn refresh_nodepool_bridge(nodepool_addr: &Arc<std::sync::Mutex<String>>) {
    if let Some(session) =
        hivemind_client_runtime::current_vpn_session(hivemind_client_runtime::ClientRole::Worker)
            .await
    {
        if let Some(bridge) = session.bridge_endpoint() {
            let mut guard = nodepool_addr
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            *guard = bridge;
        }
    }
}

fn is_terminal_session_status(status: &tonic::Status) -> bool {
    matches!(
        status.code(),
        tonic::Code::Unauthenticated
            | tonic::Code::PermissionDenied
            | tonic::Code::InvalidArgument
            | tonic::Code::Unimplemented
    )
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
        ValidateGeneralComputeTransferLeaseRequest,
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

        let request = super::build_register_request(
            "worker-1",
            "worker-1",
            "127.0.0.1:50053",
            spec,
            "local",
            "token",
        );
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

    fn transfer_authority_request() -> ValidateGeneralComputeTransferLeaseRequest {
        ValidateGeneralComputeTransferLeaseRequest {
            token: "signed-worker-execution-token".into(),
            task_id: "task-transfer-1".into(),
            worker_id: "worker-transfer-1".into(),
            execution_id: "execution-transfer-1".into(),
            attempt_id: "attempt-transfer-1".into(),
            transfer_generation: 17,
            idempotency_key: "idempotency-transfer-1".into(),
            request_digest: format!("sha256:{}", "a".repeat(64)),
        }
    }

    #[tokio::test]
    async fn nodepool_transfer_lease_authority_forwards_the_complete_request() {
        let (addr, mut reports) = match fake_node_manager_server(AuthorityBehavior::Active).await {
            Some(parts) => parts,
            None => return,
        };
        let authority = super::NodepoolTransferLeaseAuthority::new(addr.to_string());
        let expected = transfer_authority_request();

        super::TransferLeaseAuthority::validate(&authority, expected.clone())
            .await
            .expect("active Nodepool lease should authorize transfer");

        let captured = tokio::time::timeout(Duration::from_secs(2), reports.authority_rx.recv())
            .await
            .expect("Nodepool should receive the authority request")
            .expect("authority request channel should remain open");
        assert_eq!(captured, expected);
    }

    #[tokio::test]
    async fn nodepool_transfer_lease_authority_maps_rejection_to_denied() {
        let (addr, _reports) = match fake_node_manager_server(AuthorityBehavior::Denied).await {
            Some(parts) => parts,
            None => return,
        };
        let authority = super::NodepoolTransferLeaseAuthority::new(addr.to_string());

        let error =
            super::TransferLeaseAuthority::validate(&authority, transfer_authority_request())
                .await
                .expect_err("inactive Nodepool lease must be denied");

        assert_eq!(
            error,
            super::TransferLeaseAuthorityError::Denied("lease revoked".into())
        );
    }

    #[tokio::test]
    async fn nodepool_transfer_lease_authority_maps_connect_rpc_and_timeout_to_unavailable() {
        let unused_addr = reserve_loopback_addr().expect("loopback address should be available");
        let connect_authority = super::NodepoolTransferLeaseAuthority {
            endpoint: unused_addr.to_string(),
            timeout: Duration::from_millis(100),
        };
        let connect_error = super::TransferLeaseAuthority::validate(
            &connect_authority,
            transfer_authority_request(),
        )
        .await
        .expect_err("connection failure must fail closed");
        assert!(matches!(
            connect_error,
            super::TransferLeaseAuthorityError::Unavailable(_)
        ));

        let (rpc_addr, _reports) =
            match fake_node_manager_server(AuthorityBehavior::RpcFailure).await {
                Some(parts) => parts,
                None => return,
            };
        let rpc_authority = super::NodepoolTransferLeaseAuthority {
            endpoint: rpc_addr.to_string(),
            timeout: Duration::from_millis(100),
        };
        let rpc_error =
            super::TransferLeaseAuthority::validate(&rpc_authority, transfer_authority_request())
                .await
                .expect_err("RPC failure must fail closed");
        assert!(matches!(
            rpc_error,
            super::TransferLeaseAuthorityError::Unavailable(_)
        ));

        let (slow_addr, _reports) = match fake_node_manager_server(AuthorityBehavior::Delayed).await
        {
            Some(parts) => parts,
            None => return,
        };
        let slow_authority = super::NodepoolTransferLeaseAuthority {
            endpoint: slow_addr.to_string(),
            timeout: Duration::from_millis(20),
        };
        let timeout_error =
            super::TransferLeaseAuthority::validate(&slow_authority, transfer_authority_request())
                .await
                .expect_err("authority timeout must fail closed");
        assert!(matches!(
            timeout_error,
            super::TransferLeaseAuthorityError::Unavailable(_)
        ));
    }

    async fn fake_node_manager_report_server() -> Option<(SocketAddr, CapturedReports)> {
        fake_node_manager_server(AuthorityBehavior::Active).await
    }

    async fn fake_node_manager_server(
        authority_behavior: AuthorityBehavior,
    ) -> Option<(SocketAddr, CapturedReports)> {
        let addr = reserve_loopback_addr()?;
        let (output_tx, output_rx) = mpsc::channel(1);
        let (result_tx, result_rx) = mpsc::channel(1);
        let (usage_tx, usage_rx) = mpsc::channel(1);
        let (authority_tx, authority_rx) = mpsc::channel(1);
        let service = NodeManagerServiceServer::new(FakeNodeManagerReportService {
            output_tx,
            result_tx,
            usage_tx,
            authority_tx,
            authority_behavior,
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
                        authority_rx,
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
        authority_rx: mpsc::Receiver<ValidateGeneralComputeTransferLeaseRequest>,
    }

    #[derive(Clone, Copy)]
    enum AuthorityBehavior {
        Active,
        Denied,
        RpcFailure,
        Delayed,
    }

    struct FakeNodeManagerReportService {
        output_tx: mpsc::Sender<TaskOutputUploadRequest>,
        result_tx: mpsc::Sender<TaskResultUploadRequest>,
        usage_tx: mpsc::Sender<TaskUsageRequest>,
        authority_tx: mpsc::Sender<ValidateGeneralComputeTransferLeaseRequest>,
        authority_behavior: AuthorityBehavior,
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

        async fn validate_general_compute_transfer_lease(
            &self,
            request: Request<hivemind_proto::ValidateGeneralComputeTransferLeaseRequest>,
        ) -> Result<Response<hivemind_proto::ValidateGeneralComputeTransferLeaseResponse>, Status>
        {
            self.authority_tx
                .send(request.into_inner())
                .await
                .map_err(|_| Status::internal("authority receiver dropped"))?;
            match self.authority_behavior {
                AuthorityBehavior::Active => Ok(Response::new(
                    hivemind_proto::ValidateGeneralComputeTransferLeaseResponse {
                        success: true,
                        status_message: "active".into(),
                    },
                )),
                AuthorityBehavior::Denied => Ok(Response::new(
                    hivemind_proto::ValidateGeneralComputeTransferLeaseResponse {
                        success: false,
                        status_message: "lease revoked".into(),
                    },
                )),
                AuthorityBehavior::RpcFailure => Err(Status::internal("authority backend failed")),
                AuthorityBehavior::Delayed => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    Ok(Response::new(
                        hivemind_proto::ValidateGeneralComputeTransferLeaseResponse {
                            success: true,
                            status_message: "active".into(),
                        },
                    ))
                }
            }
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

    #[test]
    fn advertise_addr_uses_overlay_ip_before_unspecified_listener_fallback() {
        assert_eq!(
            super::advertise_addr_for_vpn("0.0.0.0:50053", None, Some("100.64.0.42")).unwrap(),
            "100.64.0.42:50053"
        );
        assert_eq!(
            super::advertise_addr_for_vpn(
                "0.0.0.0:50053",
                Some("worker.example:50053".to_string()),
                Some("100.64.0.42"),
            )
            .unwrap(),
            "worker.example:50053"
        );
    }
}

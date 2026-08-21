use anyhow::{Context, Result};
#[cfg(any(feature = "master", feature = "worker"))]
use hivemind_client_runtime as client_runtime;
use hivemind_config::HivemindConfig;
#[cfg(feature = "nodepool")]
use prost::Message;
#[cfg(feature = "nodepool")]
use std::io::{Read, Write};
use tokio::sync::watch;
#[cfg(feature = "worker")]
use tokio_stream::wrappers::TcpListenerStream;
use tracing::info;

#[cfg(feature = "cli")]
mod cli;

#[cfg(feature = "nodepool")]
use hivemind_auth::AuthManager;
#[cfg(feature = "nodepool")]
use hivemind_database::DatabaseManager;
#[cfg(feature = "master")]
use hivemind_master_api::MasterApiServer;
#[cfg(feature = "nodepool")]
use hivemind_node_manager::grpc::{
    GrpcBatchRuntimeService, GrpcGeneralComputeArtifactService, GrpcMasterNodeService,
    GrpcNodeManagerService, GrpcUserService, NodepoolState,
};
#[cfg(feature = "nodepool")]
use hivemind_node_manager::{heartbeat::HeartbeatHandler, NodeManager};
use hivemind_proto::GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES;
#[cfg(feature = "nodepool")]
use hivemind_proto::{
    batch_runtime_service_server::BatchRuntimeServiceServer,
    general_compute_artifact_service_server::GeneralComputeArtifactServiceServer,
    master_node_service_server::MasterNodeServiceServer,
    node_manager_service_server::NodeManagerServiceServer, user_service_server::UserServiceServer,
    vpn_service_server::VpnServiceServer,
};
#[cfg(feature = "worker")]
use hivemind_proto::{
    general_compute_chunk_service_server::GeneralComputeChunkServiceServer,
    worker_node_service_server::WorkerNodeServiceServer, WORKER_RPC_MESSAGE_MAX_BYTES,
};
#[cfg(feature = "nodepool")]
use hivemind_task_scheduler::{dispatcher::Dispatcher, TaskScheduler};
#[cfg(feature = "nodepool")]
use hivemind_vpn_service::grpc_server::GrpcVpnService;
#[cfg(feature = "nodepool")]
use hivemind_vpn_service::VpnService;
#[cfg(feature = "website")]
use hivemind_website_api::WebsiteApiServer;
#[cfg(feature = "worker")]
use hivemind_worker_executor::control_api::WorkerProfile;
#[cfg(feature = "worker")]
use hivemind_worker_executor::grpc_server::{
    GrpcGeneralComputeChunkService, GrpcWorkerNodeService, WorkerGrpcState,
};
#[cfg(feature = "worker")]
use hivemind_worker_executor::nodepool_client;
#[cfg(feature = "worker")]
use hivemind_worker_executor::runtime_admission::WorkerRuntimeAdmission;
#[cfg(feature = "worker")]
use hivemind_worker_executor::WorkerExecutor;
#[cfg(any(feature = "nodepool", feature = "worker"))]
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRole {
    Master,
    Nodepool,
    Worker,
    Website,
    All,
}

const VERIFY_MANAGED_PROOF_ARG: &str = "--verify-managed-proof";
#[cfg(feature = "nodepool")]
pub const MANAGED_PROOF_STDIN_MAX_BYTES: usize =
    hivemind_proto::MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES;

pub fn is_managed_proof_verifier_mode(args: &[String]) -> bool {
    matches!(args, [_, argument] if argument == VERIFY_MANAGED_PROOF_ARG)
}

#[cfg(feature = "nodepool")]
pub fn verify_managed_proof_bytes(input: &[u8]) -> Result<Vec<u8>> {
    if input.len() > MANAGED_PROOF_STDIN_MAX_BYTES {
        anyhow::bail!("managed proof input exceeds the verifier limit");
    }

    let envelope = hivemind_proto::ManagedProofEnvelope::decode(input)
        .map_err(|_| anyhow::anyhow!("managed proof input is malformed"))?;
    if envelope.encode_to_vec() != input {
        anyhow::bail!("managed proof input is not canonical");
    }

    let claim = hivemind_managed_proof::verify_risc0_proof_envelope(&envelope)
        .map_err(|_| anyhow::anyhow!("managed proof verification failed"))?;
    serde_json::to_vec(&claim).map_err(Into::into)
}

#[cfg(feature = "nodepool")]
fn read_managed_proof_input(reader: impl Read) -> Result<Vec<u8>> {
    let mut input = Vec::new();
    reader
        .take((MANAGED_PROOF_STDIN_MAX_BYTES + 1) as u64)
        .read_to_end(&mut input)?;
    if input.len() > MANAGED_PROOF_STDIN_MAX_BYTES {
        anyhow::bail!("managed proof input exceeds the verifier limit");
    }
    Ok(input)
}

#[cfg(feature = "nodepool")]
fn run_managed_proof_verifier_inner(reader: impl Read, mut writer: impl Write) -> Result<()> {
    let input = read_managed_proof_input(reader)?;
    let output = verify_managed_proof_bytes(&input)?;
    writer.write_all(&output)?;
    writer.flush()?;
    Ok(())
}

#[cfg(feature = "nodepool")]
fn run_managed_proof_verifier_io(reader: impl Read, writer: impl Write) -> bool {
    run_managed_proof_verifier_inner(reader, writer).is_ok()
}

#[cfg(feature = "nodepool")]
pub fn run_managed_proof_verifier() -> ! {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    if run_managed_proof_verifier_io(stdin.lock(), stdout.lock()) {
        std::process::exit(0);
    }

    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    let _ = stderr.write_all(b"managed proof verification failed\n");
    let _ = stderr.flush();
    std::process::exit(1);
}

impl ServiceRole {
    pub fn includes_master(self) -> bool {
        matches!(self, ServiceRole::Master | ServiceRole::All)
    }

    pub fn includes_website(self) -> bool {
        matches!(self, ServiceRole::Website | ServiceRole::All)
    }

    pub fn includes_nodepool(self) -> bool {
        matches!(self, ServiceRole::Nodepool | ServiceRole::All)
    }

    pub fn includes_worker(self) -> bool {
        matches!(self, ServiceRole::Worker | ServiceRole::All)
    }
}

pub async fn run_service(role: ServiceRole) -> Result<()> {
    hivemind_common::init_tracing("hivemind");
    ensure_role_supported(role)?;
    run_service_inner(role).await
}

#[cfg(feature = "cli")]
pub async fn run_from_cli(args: Vec<String>) -> Result<()> {
    #[cfg(feature = "nodepool")]
    if is_managed_proof_verifier_mode(&args) {
        run_managed_proof_verifier();
    }

    hivemind_common::init_tracing("hivemind");
    let command = cli::parse_cli_args(&args)?;
    if let cli::CliCommand::Submit(submit) = command {
        return cli::run_submit(submit).await;
    }
    if let cli::CliCommand::Status(status) = command {
        return cli::run_status(status).await;
    }
    if let cli::CliCommand::Result(result) = command {
        return cli::run_result(result).await;
    }
    let service = match command {
        cli::CliCommand::Service(service) => service,
        _ => unreachable!(),
    };
    let role = match service.as_str() {
        "master" => ServiceRole::Master,
        "nodepool" => ServiceRole::Nodepool,
        "worker" => ServiceRole::Worker,
        "website" => ServiceRole::Website,
        "all" => ServiceRole::All,
        _ => {
            eprintln!("Usage: hivemind-bin [master|nodepool|worker|website|all]");
            std::process::exit(1);
        }
    };
    ensure_role_supported(role)?;
    run_service_inner(role).await
}

fn ensure_role_supported(role: ServiceRole) -> Result<()> {
    if role.includes_master() && !cfg!(feature = "master") {
        anyhow::bail!("this binary was built without master support");
    }
    if role.includes_website() && !cfg!(feature = "website") {
        anyhow::bail!("this binary was built without website support");
    }
    if role.includes_nodepool() && !cfg!(feature = "nodepool") {
        anyhow::bail!("this binary was built without nodepool support");
    }
    if role.includes_worker() && !cfg!(feature = "worker") {
        anyhow::bail!("this binary was built without worker support");
    }
    Ok(())
}

#[cfg(any(feature = "nodepool", test))]
fn should_seed_default_user(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some(value) if value.eq_ignore_ascii_case("true")
            || value == "1"
            || value.eq_ignore_ascii_case("yes")
    )
}

fn validate_service_config(config: &HivemindConfig, role: ServiceRole) -> Result<()> {
    // User-deployed masters must not require the platform JWT signing secret.
    // Nodepool remains the signing authority; website-api is platform-side.
    if role.includes_nodepool() || role.includes_website() {
        config.auth.validate_jwt_secret()?;
    }
    if role.includes_nodepool() {
        config.auth.validate_worker_execution_private_key()?;
    }
    if role.includes_worker() {
        config.auth.validate_worker_execution_public_key()?;
    }
    Ok(())
}

#[cfg(any(feature = "master", feature = "worker", feature = "website"))]
fn nodepool_client_addr(config: &HivemindConfig, run_nodepool: bool) -> Result<String> {
    if let Some(endpoint) = config
        .server
        .nodepool_grpc_endpoint
        .as_ref()
        .filter(|endpoint| !endpoint.trim().is_empty())
    {
        return Ok(endpoint.clone());
    }

    let addr = config.server.nodepool_grpc_addr.trim();
    if !addr.is_empty() && !addr.starts_with("0.0.0.0:") && !addr.starts_with("[::]:") {
        return Ok(addr.to_string());
    }

    // Downloaded master/worker clients default to the platform nodepool hostname
    // on the VPN overlay. Website-api still requires an explicit endpoint when it
    // is not colocated with nodepool.
    #[cfg(any(feature = "master", feature = "worker"))]
    if !run_nodepool {
        return Ok(client_runtime::resolve_nodepool_grpc_endpoint(config));
    }

    if run_nodepool {
        if let Some(port) = addr.strip_prefix("0.0.0.0:") {
            return Ok(format!("127.0.0.1:{port}"));
        }
        if let Some(port) = addr.strip_prefix("[::]:") {
            return Ok(format!("[::1]:{port}"));
        }
    }

    if !run_nodepool && (addr.starts_with("0.0.0.0:") || addr.starts_with("[::]:")) {
        anyhow::bail!(
            "NODEPOOL_GRPC_ENDPOINT must be set when this process does not run nodepool and NODEPOOL_GRPC_ADDR is a bind address ({})",
            config.server.nodepool_grpc_addr
        );
    }

    Ok(config.server.nodepool_grpc_addr.clone())
}

#[cfg(feature = "worker")]
async fn worker_nodepool_token(
    config: &HivemindConfig,
    nodepool_addr: &str,
    worker_id: &str,
) -> Result<String> {
    hivemind_worker_executor::nodepool_client::resolve_nodepool_token(
        nodepool_addr,
        worker_id,
        config.server.worker_nodepool_token.as_deref(),
        config.server.worker_nodepool_username.as_deref(),
        config.server.worker_nodepool_password.as_deref(),
    )
    .await
}

async fn run_service_inner(role: ServiceRole) -> Result<()> {
    #[cfg(feature = "worker")]
    if role.includes_worker() && std::env::var_os("HIVEMIND_CONFIG").is_none() {
        // Downloaded workers must be runnable with only website credentials.
        // Keep the release egress gate enabled with the platform-safe defaults;
        // explicit operator environment variables still override these values.
        set_worker_egress_default("EXECUTOR_NETWORK_EGRESS_ENABLED", "true");
        set_worker_egress_default("EXECUTOR_NETWORK_EGRESS_MODE", "allowlist");
        set_worker_egress_default(
            "EXECUTOR_NETWORK_EGRESS_TARGETS",
            "8.8.8.8,1.1.1.1,100.64.0.0/10",
        );
    }
    let config = HivemindConfig::load()?;

    let run_master = role.includes_master();
    let run_website = role.includes_website();
    let run_nodepool = role.includes_nodepool();
    let run_worker = role.includes_worker();

    #[cfg(not(feature = "master"))]
    let _ = run_master;
    #[cfg(not(feature = "website"))]
    let _ = run_website;
    #[cfg(not(feature = "worker"))]
    let _ = run_worker;

    validate_service_config(&config, role)?;

    #[cfg(any(feature = "nodepool", feature = "worker"))]
    let mut shutdown_handles: Vec<watch::Sender<bool>> = Vec::new();
    #[cfg(all(not(feature = "nodepool"), not(feature = "worker")))]
    let shutdown_handles: Vec<watch::Sender<bool>> = Vec::new();

    #[cfg(feature = "nodepool")]
    if run_nodepool {
        let db = DatabaseManager::new(&config).await?;
        db.run_migrations().await?;
        let seed_default_user = std::env::var("HIVEMIND_SEED_DEFAULT_USER").ok();
        if should_seed_default_user(seed_default_user.as_deref()) {
            hivemind_database::postgres::seed_default_user(&db.pool).await?;
        }
        let auth = AuthManager::new(&db, &config.auth.jwt_secret, config.auth.token_expiry_hours);
        let scheduler = TaskScheduler::new(db.clone(), auth.clone());

        let node_mgr = Arc::new(NodeManager::new(&config, db.clone()));
        let vpn = Arc::new(VpnService::new(config.clone(), db.clone()));

        // Background loops
        let hb_handler = Arc::new(HeartbeatHandler::new(node_mgr.clone(), 30));
        let hb_shutdown = hb_handler.start_cleanup_loop(std::time::Duration::from_secs(10));
        shutdown_handles.push(hb_shutdown);

        let task_timeout = 30u64;
        let max_redispatch = 2i32;
        let dispatcher = Arc::new(
            Dispatcher::new(db.clone(), task_timeout, max_redispatch)
                .with_worker_execution_private_key(
                    config.auth.worker_execution_private_key_pem.clone(),
                )
                .with_managed_proof_rollout_mode(config.managed_proof.rollout_mode),
        );

        let disp_shutdown = dispatcher
            .clone()
            .start_registered_dispatch_loop(std::time::Duration::from_secs(5));
        shutdown_handles.push(disp_shutdown);

        let timeout_shutdown = dispatcher.start_timeout_loop(std::time::Duration::from_secs(10));
        shutdown_handles.push(timeout_shutdown);

        // Build gRPC servers
        let np_state = Arc::new(NodepoolState {
            auth: auth.clone(),
            worker_execution_private_key_pem: config.auth.worker_execution_private_key_pem.clone(),
            worker_execution_public_key_pem: config.auth.worker_execution_public_key_pem.clone(),
            managed_proof_rollout_mode: config.managed_proof.rollout_mode,
            node_manager: node_mgr.clone(),
            scheduler: scheduler.clone(),
            artifact_root: hivemind_node_manager::grpc::artifact_root_for_config(&config),
        });

        let user_svc = UserServiceServer::new(GrpcUserService::new(np_state.clone()));
        let node_svc = NodeManagerServiceServer::new(GrpcNodeManagerService::new(np_state.clone()));
        let master_svc = MasterNodeServiceServer::new(GrpcMasterNodeService::new(np_state.clone()));
        let artifact_svc = GeneralComputeArtifactServiceServer::new(
            GrpcGeneralComputeArtifactService::new(np_state.clone()),
        )
        .max_decoding_message_size(GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES)
        .max_encoding_message_size(GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES);
        let batch_svc = BatchRuntimeServiceServer::new(GrpcBatchRuntimeService::new(np_state));
        let vpn_svc = VpnServiceServer::new(GrpcVpnService::new(vpn));

        let np_addr = config.server.nodepool_grpc_addr.clone();
        tokio::spawn(async move {
            let max_msg = 128 * 1024 * 1024;
            if let Err(e) = tonic::transport::Server::builder()
                .add_service(user_svc)
                .add_service(node_svc)
                .add_service(
                    master_svc
                        .max_decoding_message_size(max_msg)
                        .max_encoding_message_size(max_msg),
                )
                .add_service(artifact_svc)
                .add_service(batch_svc)
                .add_service(vpn_svc)
                .serve(np_addr.parse().unwrap())
                .await
            {
                tracing::error!("Nodepool gRPC server error: {}", e);
            }
        });
        info!(
            "Nodepool gRPC server started on {}",
            config.server.nodepool_grpc_addr
        );
    }

    #[cfg(feature = "master")]
    if run_master {
        let nodepool_grpc = nodepool_client_addr(&config, run_nodepool)?;
        let api = MasterApiServer::new(nodepool_grpc, config.clone()).await?;
        let addr = config.server.master_http_addr.clone();
        let master_ui_dir = config.server.master_ui_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = api.serve_with_ui(&addr, &master_ui_dir).await {
                tracing::error!("Master API error: {}", e);
            }
        });
        info!(
            "Master HTTP API started on {}",
            config.server.master_http_addr
        );
    }

    #[cfg(feature = "website")]
    if run_website {
        let nodepool_grpc = nodepool_client_addr(&config, run_nodepool)?;
        let jwt_secret = config.auth.jwt_secret.clone();
        let token_expiry = config.auth.token_expiry_hours;
        let api =
            WebsiteApiServer::new(jwt_secret, token_expiry, nodepool_grpc, config.clone()).await?;
        let addr = config.server.website_http_addr.clone();
        tokio::spawn(async move {
            if let Err(e) = api.serve(&addr).await {
                tracing::error!("Website API error: {}", e);
            }
        });
        info!(
            "Website HTTP API started on {}",
            config.server.website_http_addr
        );
    }

    #[cfg(feature = "worker")]
    if run_worker {
        let configured_nodepool_addr = nodepool_client_addr(&config, run_nodepool)?;
        // An explicit role-scoped Headscale key makes VPN and Nodepool readiness
        // part of startup. Without one, preserve deferred UI-login enrollment.
        let vpn_endpoint = client_runtime::ensure_env_vpn_for_endpoint(
            &config,
            client_runtime::ClientRole::Worker,
            &configured_nodepool_addr,
        )
        .await?;
        let nodepool_addr = vpn_endpoint
            .clone()
            .unwrap_or_else(|| configured_nodepool_addr.clone());
        let nodepool_addr_state = std::sync::Arc::new(std::sync::Mutex::new(nodepool_addr.clone()));

        let executor = Arc::new(WorkerExecutor::try_new(config.clone())?);
        let resources = executor.get_system_resources();
        info!(
            "Worker executor: {} cores, {} GB RAM",
            resources.cpu_cores, resources.total_memory_gb
        );

        let wk_addr = config.server.worker_grpc_addr.clone();
        // Bind before resolving credentials or scheduling registration. A worker
        // must never advertise/register before its control plane can accept RPCs.
        let wk_listener = tokio::net::TcpListener::bind(&wk_addr).await?;
        let worker_id = std::env::var("WORKER_ID")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| format!("worker-{}", uuid::Uuid::new_v4()));
        let overlay_ip = if vpn_endpoint.is_some() {
            client_runtime::current_vpn_session(client_runtime::ClientRole::Worker)
                .await
                .and_then(|session| session.overlay_ip.clone())
        } else {
            None
        };
        let worker_advertise_addr = match nodepool_client::advertise_addr_for_vpn(
            &wk_addr,
            config.server.worker_advertise_addr.clone(),
            overlay_ip.as_deref(),
        ) {
            Ok(addr) => addr,
            Err(err) if vpn_endpoint.is_some() => {
                return Err(err.context(
                    "keyed Worker startup cannot determine a Headscale-reachable advertise address",
                ));
            }
            Err(err) => {
                // Preserve compatibility for no-key local/UI deployments. The
                // address is replaced by the real overlay address after login.
                let port = wk_addr.rsplit(':').next().unwrap_or("50053");
                let fallback = format!("{worker_id}:{port}");
                tracing::warn!(
                    "WORKER_ADVERTISE_ADDR unset ({err}); using fallback advertise addr {fallback} until UI login"
                );
                fallback
            }
        };
        let worker_advertise_addr = if vpn_endpoint.is_some() {
            nodepool_client::validate_advertise_addr(&worker_advertise_addr).context(
                "keyed Worker startup requires a Nodepool-reachable WORKER_ADVERTISE_ADDR",
            )?
        } else {
            worker_advertise_addr
        };
        let wk_state = Arc::new(WorkerGrpcState::new_with_transfer_lease_authority(
            config.clone(),
            executor.clone(),
            worker_id.clone(),
            hivemind_worker_executor::grpc_server::NodepoolTransferLeaseAuthority::new_shared(
                nodepool_addr_state.clone(),
            ),
        ));
        let runtime_admission = WorkerRuntimeAdmission::from_environment()?;
        let wk_chunk_svc = GeneralComputeChunkServiceServer::new(
            GrpcGeneralComputeChunkService::new(wk_state.clone(), runtime_admission.clone()),
        )
        .max_decoding_message_size(GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES)
        .max_encoding_message_size(GENERAL_COMPUTE_CHUNK_RPC_MESSAGE_MAX_BYTES);
        let worker_identity = wk_state.worker_identity_handle();
        let wk_svc = WorkerNodeServiceServer::new(
            GrpcWorkerNodeService::new(wk_state).with_runtime_admission(runtime_admission),
        )
        .max_decoding_message_size(WORKER_RPC_MESSAGE_MAX_BYTES)
        .max_encoding_message_size(WORKER_RPC_MESSAGE_MAX_BYTES);

        let wk_incoming = TcpListenerStream::new(wk_listener);
        tokio::spawn(async move {
            if let Err(e) = tonic::transport::Server::builder()
                .add_service(wk_svc)
                .add_service(wk_chunk_svc)
                .serve_with_incoming(wk_incoming)
                .await
            {
                tracing::error!("Worker gRPC server error: {}", e);
            }
        });
        info!("Worker gRPC server started on {}", wk_addr);

        let worker_addr_state =
            std::sync::Arc::new(std::sync::Mutex::new(worker_advertise_addr.clone()));
        let control_profile = WorkerProfile::from_resource_spec(
            worker_id.clone(),
            worker_advertise_addr.clone(),
            std::env::var("WORKER_LOCATION").unwrap_or_else(|_| "local".into()),
            executor.get_resource_spec(),
        );
        let control_state = hivemind_worker_executor::control_api::ControlApiState {
            profile: control_profile,
            worker_addr: worker_addr_state.clone(),
            nodepool_addr: nodepool_addr_state.clone(),
            config: config.clone(),
            executor: executor.clone(),
            worker_identity,
            registration_shutdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
        };
        let control_addr = config.server.worker_control_http_addr.clone();
        let worker_control_allowed_origins =
            config.server.worker_control_cors_allowed_origins.clone();
        let worker_ui_dir = config.server.worker_ui_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = hivemind_worker_executor::control_api::serve_with_allowed_origins(
                &control_addr,
                control_state,
                &worker_control_allowed_origins,
                Some(&worker_ui_dir),
            )
            .await
            {
                tracing::error!("Worker control HTTP API error: {}", e);
            }
        });
        info!(
            "Worker control HTTP API started on {}",
            config.server.worker_control_http_addr
        );

        // Only start the automatic registration loop when credentials/token are
        // already provisioned. Downloaded workers authenticate through the local
        // UI after VPN bootstrap and register from there.
        let has_preprovisioned_auth = config
            .server
            .worker_nodepool_token
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .is_some()
            || (config
                .server
                .worker_nodepool_username
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .is_some()
                && config
                    .server
                    .worker_nodepool_password
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .is_some());

        if has_preprovisioned_auth {
            let worker_nodepool_token =
                worker_nodepool_token(&config, &nodepool_addr, &worker_id).await?;
            let worker_username = config
                .server
                .worker_nodepool_username
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| worker_id.clone());
            let reg_shutdown = nodepool_client::start_registration_loop(
                executor.clone(),
                nodepool_client::RegistrationLoopConfig {
                    nodepool_addr: nodepool_addr_state.clone(),
                    worker_id: worker_id.clone(),
                    username: worker_username,
                    worker_addr: worker_addr_state.clone(),
                    location: std::env::var("WORKER_LOCATION").unwrap_or_else(|_| "local".into()),
                    token: worker_nodepool_token,
                    interval: std::time::Duration::from_secs(10),
                },
            );
            shutdown_handles.push(reg_shutdown);
        } else {
            info!(
                "Worker registration loop deferred until UI login (no WORKER_NODEPOOL_TOKEN/USERNAME/PASSWORD)"
            );
        }
    }

    info!("Hivemind running. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;

    for handle in shutdown_handles {
        let _ = handle.send(true);
    }
    info!("Shutting down...");
    Ok(())
}

#[cfg(feature = "worker")]
fn set_worker_egress_default(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        // The process is the downloaded worker's configuration boundary; these
        // defaults are set before HivemindConfig applies environment overrides.
        unsafe { std::env::set_var(key, value) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_proof_verifier_mode_requires_the_exact_hidden_argument() {
        let nodepool = "hivemind-nodepool".to_owned();
        let hidden_flag = "--verify-managed-proof".to_owned();

        assert!(is_managed_proof_verifier_mode(&[
            nodepool.clone(),
            hidden_flag.clone()
        ]));
        assert!(!is_managed_proof_verifier_mode(std::slice::from_ref(
            &nodepool
        )));
        assert!(!is_managed_proof_verifier_mode(&[
            nodepool.clone(),
            "--verify-managed-proofs".to_owned()
        ]));
        assert!(!is_managed_proof_verifier_mode(&[
            nodepool.clone(),
            hidden_flag.clone(),
            "unexpected".to_owned()
        ]));
        assert!(!is_managed_proof_verifier_mode(&[
            nodepool,
            "all".to_owned(),
            hidden_flag
        ]));
    }

    #[cfg(feature = "nodepool")]
    mod managed_proof_verifier {
        use super::*;
        use hivemind_managed_proof::ExecutionClaim;
        use hivemind_proto::ManagedProofEnvelope;
        use prost::Message;
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct ProofFixture {
            proof_scheme: String,
            image_id: [u32; 8],
            journal: Vec<u8>,
            receipt: serde_json::Value,
        }

        fn fixture() -> ProofFixture {
            serde_json::from_slice(include_bytes!(
                "../../managed-proof/tests/fixtures/risc0-managed-proof-v1.json"
            ))
            .expect("pinned proof fixture parses")
        }

        fn encode_fixture(fixture: &ProofFixture) -> Vec<u8> {
            ManagedProofEnvelope {
                proof_scheme: fixture.proof_scheme.clone(),
                image_id: fixture.image_id.to_vec(),
                journal: fixture.journal.clone(),
                receipt_json: serde_json::to_vec(&fixture.receipt).expect("receipt serializes"),
            }
            .encode_to_vec()
        }

        #[test]
        fn verifier_returns_only_the_claim_json_for_the_pinned_real_proof() {
            let output = verify_managed_proof_bytes(&encode_fixture(&fixture()))
                .expect("pinned proof verifies");

            let claim: ExecutionClaim =
                serde_json::from_slice(&output).expect("output is an execution claim");
            assert_eq!(claim.task_id, "task-zk-golden");
            assert_eq!(claim.usage_units, 29);
            assert_eq!(output.last(), Some(&b'}'));
        }

        #[test]
        fn verifier_rejects_an_invalid_cryptographic_proof() {
            let mut fixture = fixture();
            let seal_word = &mut fixture.receipt["inner"]["Composite"]["segments"][0]["seal"][0];
            let changed = seal_word.as_u64().expect("seal word is an integer") ^ 1;
            *seal_word = serde_json::Value::from(changed);

            assert!(verify_managed_proof_bytes(&encode_fixture(&fixture)).is_err());
        }

        #[test]
        fn verifier_rejects_a_malformed_envelope() {
            assert!(verify_managed_proof_bytes(&[0x0a, 0xff]).is_err());
        }

        #[test]
        fn verifier_io_adapter_reports_failure_without_writing_stdout() {
            let mut output = Vec::new();

            let succeeded = run_managed_proof_verifier_io(&[0x0a, 0xff][..], &mut output);

            assert!(!succeeded);
            assert!(output.is_empty());
        }

        #[test]
        fn verifier_rejects_trailing_protobuf_fields() {
            let mut input = encode_fixture(&fixture());
            input.extend_from_slice(&[0x28, 0x00]);

            assert!(verify_managed_proof_bytes(&input).is_err());
        }

        #[test]
        fn verifier_rejects_input_over_the_strict_cap() {
            let input = vec![0; MANAGED_PROOF_STDIN_MAX_BYTES + 1];

            assert!(verify_managed_proof_bytes(&input).is_err());
        }
    }

    #[test]
    fn default_user_seed_requires_explicit_truthy_env_value() {
        assert!(!should_seed_default_user(None));
        assert!(!should_seed_default_user(Some("")));
        assert!(!should_seed_default_user(Some("false")));
        assert!(!should_seed_default_user(Some("0")));
        assert!(should_seed_default_user(Some("true")));
        assert!(should_seed_default_user(Some("1")));
        assert!(should_seed_default_user(Some("yes")));
    }

    #[test]
    fn master_startup_does_not_require_platform_jwt_secret() {
        // Given: a default control-plane secret (not distributed to masters).
        let mut config = HivemindConfig::default();
        config.auth.jwt_secret = "CHANGE_ME_IN_PRODUCTION".into();

        // When/Then: user-deployed master startup no longer requires JWT_SECRET.
        validate_service_config(&config, ServiceRole::Master).unwrap();
    }

    #[test]
    fn worker_startup_uses_public_key_and_not_control_plane_secret() {
        // Given: the embedded platform public key while JWT_SECRET remains default.
        let mut config = HivemindConfig::default();
        config.auth.jwt_secret = "CHANGE_ME_IN_PRODUCTION".into();

        // When/Then: worker-only startup succeeds without control-plane trust or private key.
        validate_service_config(&config, ServiceRole::Worker).unwrap();

        // Given: an invalid public key override.
        config.auth.worker_execution_public_key_pem = "not-a-key".into();

        // When: worker-only startup validates the verification key.
        let error = validate_service_config(&config, ServiceRole::Worker)
            .unwrap_err()
            .to_string();

        // Then: it reports the public-key boundary, not JWT_SECRET.
        assert!(error.contains("WORKER_EXECUTION_PUBLIC_KEY_PEM"));
        assert!(!error.contains("JWT_SECRET"));
    }

    #[test]
    fn nodepool_startup_requires_jwt_secret_and_execution_private_key() {
        // Given: valid control-plane auth but a missing worker-execution private key.
        let mut config = HivemindConfig::default();
        config.auth.jwt_secret = "unit-test-control-plane-secret-at-least-32-bytes".into();

        // When: nodepool startup validates both trust domains.
        let error = validate_service_config(&config, ServiceRole::Nodepool)
            .unwrap_err()
            .to_string();

        // Then: worker-execution signing material is independently required.
        assert!(error.contains("WORKER_EXECUTION_PRIVATE_KEY_PEM"));

        // Given: both trust domains are configured.
        let (private_key, _) = hivemind_config::generate_worker_execution_test_key_pair();
        config.auth.worker_execution_private_key_pem = private_key;

        // When/Then: nodepool startup accepts the complete trust contract.
        validate_service_config(&config, ServiceRole::Nodepool).unwrap();
    }

    #[cfg(any(feature = "master", feature = "worker"))]
    #[test]
    fn remote_master_or_worker_defaults_nodepool_endpoint_for_bind_addr() {
        let mut config = HivemindConfig::default();
        config.server.nodepool_grpc_addr = "0.0.0.0:50051".into();
        config.server.nodepool_grpc_endpoint = None;

        // Downloaded clients fall back to the platform VPN MagicDNS endpoint.
        assert_eq!(
            nodepool_client_addr(&config, false).unwrap(),
            client_runtime::DEFAULT_NODEPOOL_GRPC_ENDPOINT
        );

        config.server.nodepool_grpc_endpoint = Some("nodepool.internal:50051".into());
        assert_eq!(
            nodepool_client_addr(&config, false).unwrap(),
            "nodepool.internal:50051"
        );
    }

    #[cfg(any(feature = "master", feature = "worker", feature = "website"))]
    #[test]
    fn colocated_all_mode_normalizes_nodepool_bind_addr() {
        let mut config = HivemindConfig::default();
        config.server.nodepool_grpc_addr = "0.0.0.0:50051".into();
        config.server.nodepool_grpc_endpoint = None;

        assert_eq!(
            nodepool_client_addr(&config, true).unwrap(),
            "127.0.0.1:50051"
        );

        config.server.nodepool_grpc_addr = "[::]:50052".into();
        assert_eq!(nodepool_client_addr(&config, true).unwrap(), "[::1]:50052");
    }

    #[cfg(feature = "worker")]
    #[tokio::test]
    async fn worker_uses_configured_nodepool_token_without_login() {
        let mut config = HivemindConfig::default();
        config.server.worker_nodepool_token = Some(" worker-token ".into());
        let token = worker_nodepool_token(&config, "127.0.0.1:50051", "worker-1")
            .await
            .unwrap();
        assert_eq!(token, "worker-token");
    }
}

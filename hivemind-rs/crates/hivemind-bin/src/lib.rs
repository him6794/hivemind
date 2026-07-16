use anyhow::Result;
use hivemind_config::HivemindConfig;
use tokio::sync::watch;
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
    GrpcBatchRuntimeService, GrpcMasterNodeService, GrpcNodeManagerService, GrpcUserService,
    NodepoolState,
};
#[cfg(feature = "nodepool")]
use hivemind_node_manager::{heartbeat::HeartbeatHandler, NodeManager};
#[cfg(feature = "worker")]
use hivemind_proto::worker_node_service_server::WorkerNodeServiceServer;
#[cfg(feature = "nodepool")]
use hivemind_proto::{
    batch_runtime_service_server::BatchRuntimeServiceServer,
    master_node_service_server::MasterNodeServiceServer,
    node_manager_service_server::NodeManagerServiceServer, user_service_server::UserServiceServer,
    vpn_service_server::VpnServiceServer,
};
#[cfg(feature = "nodepool")]
use hivemind_task_scheduler::{dispatcher::Dispatcher, TaskScheduler};
#[cfg(feature = "nodepool")]
use hivemind_torrent_service::DistributionRuntime;
#[cfg(feature = "nodepool")]
use hivemind_vpn_service::grpc_server::GrpcVpnService;
#[cfg(feature = "nodepool")]
use hivemind_vpn_service::VpnService;
#[cfg(feature = "website")]
use hivemind_website_api::WebsiteApiServer;
#[cfg(feature = "worker")]
use hivemind_worker_executor::control_api::WorkerProfile;
#[cfg(feature = "worker")]
use hivemind_worker_executor::grpc_server::{GrpcWorkerNodeService, WorkerGrpcState};
#[cfg(feature = "worker")]
use hivemind_worker_executor::nodepool_client;
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
    if role.includes_master() || role.includes_nodepool() || role.includes_website() {
        config.auth.validate_jwt_secret()?;
    }
    if role.includes_nodepool() || role.includes_worker() {
        config.auth.validate_worker_execution_secret()?;
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

    if !run_nodepool && config.server.nodepool_grpc_addr.starts_with("0.0.0.0:") {
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
                .with_worker_execution_secret(config.auth.worker_execution_secret.clone()),
        );

        let disp_shutdown = dispatcher
            .clone()
            .start_registered_dispatch_loop(std::time::Duration::from_secs(5));
        shutdown_handles.push(disp_shutdown);

        let timeout_shutdown = dispatcher.start_timeout_loop(std::time::Duration::from_secs(10));
        shutdown_handles.push(timeout_shutdown);

        // Build gRPC servers
        let (distribution, distribution_handles) = DistributionRuntime::start(&config).await?;
        info!(
            "Nodepool torrent tracker on {} (announce {}), seed listener on {}",
            distribution.tracker_addr, distribution.announce_url, distribution.seed_addr
        );
        // Keep distribution tasks alive for process lifetime.
        for handle in distribution_handles {
            std::mem::forget(handle);
        }
        let np_state = Arc::new(NodepoolState {
            auth: auth.clone(),
            worker_execution_secret: config.auth.worker_execution_secret.clone(),
            node_manager: node_mgr.clone(),
            scheduler: scheduler.clone(),
            artifact_root: hivemind_node_manager::grpc::artifact_root_for_config(&config),
            distribution: Some(distribution),
        });

        let user_svc = UserServiceServer::new(GrpcUserService::new(np_state.clone()));
        let node_svc = NodeManagerServiceServer::new(GrpcNodeManagerService::new(np_state.clone()));
        let master_svc = MasterNodeServiceServer::new(GrpcMasterNodeService::new(np_state.clone()));
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
        let jwt_secret = config.auth.jwt_secret.clone();
        let token_expiry = config.auth.token_expiry_hours;
        let api =
            MasterApiServer::new(jwt_secret, token_expiry, nodepool_grpc, config.clone()).await?;
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
        let executor = Arc::new(WorkerExecutor::new(config.clone()));
        let resources = executor.get_system_resources();
        info!(
            "Worker executor: {} cores, {} GB RAM",
            resources.cpu_cores, resources.total_memory_gb
        );

        let wk_addr = config.server.worker_grpc_addr.clone();
        let worker_id = std::env::var("WORKER_ID")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| format!("worker-{}", uuid::Uuid::new_v4()));
        let wk_state = Arc::new(WorkerGrpcState::new(
            config.clone(),
            executor.clone(),
            worker_id.clone(),
        ));
        let wk_svc = WorkerNodeServiceServer::new(GrpcWorkerNodeService::new(wk_state));
        let nodepool_addr = nodepool_client_addr(&config, run_nodepool)?;
        let worker_nodepool_token =
            worker_nodepool_token(&config, &nodepool_addr, &worker_id).await?;
        let worker_advertise_addr = nodepool_client::advertise_addr(
            &config.server.worker_grpc_addr,
            config.server.worker_advertise_addr.clone(),
        )?;
        let control_profile = WorkerProfile::from_resource_spec(
            worker_id.clone(),
            worker_advertise_addr.clone(),
            std::env::var("WORKER_LOCATION").unwrap_or_else(|_| "local".into()),
            executor.get_resource_spec(),
        );
        let control_state = hivemind_worker_executor::control_api::ControlApiState {
            profile: control_profile,
            nodepool_addr: nodepool_addr.clone(),
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
                nodepool_addr: nodepool_addr.clone(),
                worker_id,
                username: worker_username,
                worker_addr: worker_advertise_addr,
                location: std::env::var("WORKER_LOCATION").unwrap_or_else(|_| "local".into()),
                token: worker_nodepool_token,
                interval: std::time::Duration::from_secs(10),
            },
        );
        shutdown_handles.push(reg_shutdown);

        tokio::spawn(async move {
            if let Err(e) = tonic::transport::Server::builder()
                .add_service(wk_svc)
                .serve(wk_addr.parse().unwrap())
                .await
            {
                tracing::error!("Worker gRPC server error: {}", e);
            }
        });
        info!(
            "Worker gRPC server started on {}",
            config.server.worker_grpc_addr
        );
    }

    info!("Hivemind running. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;

    for handle in shutdown_handles {
        let _ = handle.send(true);
    }
    info!("Shutting down...");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn master_startup_rejects_default_control_plane_secret() {
        // Given: valid worker trust but a default control-plane secret.
        let mut config = HivemindConfig::default();
        config.auth.jwt_secret = "CHANGE_ME_IN_PRODUCTION".into();
        config.auth.worker_execution_secret =
            "unit-test-worker-execution-secret-at-least-32-bytes".into();

        // When: master startup validates its role-specific requirements.
        let error = validate_service_config(&config, ServiceRole::Master)
            .unwrap_err()
            .to_string();

        // Then: only the control-plane boundary blocks startup.
        assert!(error.contains("JWT_SECRET"));
    }

    #[test]
    fn worker_startup_requires_worker_secret_but_not_control_plane_secret() {
        // Given: a worker secret is valid while JWT_SECRET remains at its default.
        let mut config = HivemindConfig::for_test();
        config.auth.worker_execution_secret =
            "unit-test-worker-execution-secret-at-least-32-bytes".into();

        // When/Then: worker-only startup succeeds without control-plane trust.
        validate_service_config(&config, ServiceRole::Worker).unwrap();

        // Given: only the control-plane secret is valid.
        config.auth.jwt_secret = "unit-test-control-plane-secret-at-least-32-bytes".into();
        config.auth.worker_execution_secret = "CHANGE_ME_WORKER_EXECUTION_SECRET".into();

        // When: worker-only startup validates the wrong trust secret.
        let error = validate_service_config(&config, ServiceRole::Worker)
            .unwrap_err()
            .to_string();

        // Then: it reports the worker-execution boundary, not JWT_SECRET.
        assert!(error.contains("WORKER_EXECUTION_SECRET"));
    }

    #[test]
    fn nodepool_startup_requires_both_trust_secrets() {
        // Given: valid control-plane auth but a default worker-execution secret.
        let mut config = HivemindConfig::for_test();
        config.auth.jwt_secret = "unit-test-control-plane-secret-at-least-32-bytes".into();

        // When: nodepool startup validates both trust domains.
        let error = validate_service_config(&config, ServiceRole::Nodepool)
            .unwrap_err()
            .to_string();

        // Then: worker-execution trust is independently required.
        assert!(error.contains("WORKER_EXECUTION_SECRET"));

        // Given: both trust domains are configured.
        config.auth.worker_execution_secret =
            "unit-test-worker-execution-secret-at-least-32-bytes".into();

        // When/Then: nodepool startup accepts the complete trust contract.
        validate_service_config(&config, ServiceRole::Nodepool).unwrap();
    }

    #[test]
    fn remote_master_or_worker_requires_nodepool_endpoint_for_bind_addr() {
        let mut config = HivemindConfig::default();
        config.server.nodepool_grpc_addr = "0.0.0.0:50051".into();
        config.server.nodepool_grpc_endpoint = None;

        let error = nodepool_client_addr(&config, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("NODEPOOL_GRPC_ENDPOINT"));

        config.server.nodepool_grpc_endpoint = Some("nodepool.internal:50051".into());
        assert_eq!(
            nodepool_client_addr(&config, false).unwrap(),
            "nodepool.internal:50051"
        );
    }

    #[test]
    fn colocated_all_mode_can_use_nodepool_bind_addr() {
        let mut config = HivemindConfig::default();
        config.server.nodepool_grpc_addr = "0.0.0.0:50051".into();
        config.server.nodepool_grpc_endpoint = None;

        assert_eq!(
            nodepool_client_addr(&config, true).unwrap(),
            "0.0.0.0:50051"
        );
    }

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

use anyhow::Result;
use hivemind_auth::AuthManager;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_master_api::MasterApiServer;
use hivemind_node_manager::grpc::{
    GrpcBatchRuntimeService, GrpcMasterNodeService, GrpcNodeManagerService, GrpcUserService,
    NodepoolState,
};
use hivemind_node_manager::{heartbeat::HeartbeatHandler, NodeManager};
use hivemind_proto::{
    batch_runtime_service_server::BatchRuntimeServiceServer,
    master_node_service_server::MasterNodeServiceServer,
    node_manager_service_server::NodeManagerServiceServer, user_service_server::UserServiceServer,
    vpn_service_server::VpnServiceServer, worker_node_service_server::WorkerNodeServiceServer,
};
use hivemind_task_scheduler::{dispatcher::Dispatcher, TaskScheduler};
use hivemind_vpn_service::grpc_server::GrpcVpnService;
use hivemind_vpn_service::VpnService;
use hivemind_worker_executor::control_api::WorkerProfile;
use hivemind_worker_executor::grpc_server::{GrpcWorkerNodeService, WorkerGrpcState};
use hivemind_worker_executor::nodepool_client;
use hivemind_worker_executor::WorkerExecutor;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::info;

mod cli;

fn should_seed_default_user(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some(value) if value.eq_ignore_ascii_case("true")
            || value == "1"
            || value.eq_ignore_ascii_case("yes")
    )
}

fn validate_auth_service_config(config: &HivemindConfig) -> Result<()> {
    config.auth.validate_jwt_secret()
}

#[tokio::main]
async fn main() -> Result<()> {
    hivemind_common::init_tracing("hivemind");

    let args: Vec<String> = std::env::args().collect();
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
        cli::CliCommand::Submit(_) => unreachable!(),
        cli::CliCommand::Status(_) => unreachable!(),
        cli::CliCommand::Result(_) => unreachable!(),
    };

    let config = HivemindConfig::load()?;

    let run_master = service == "master" || service == "all";
    let run_nodepool = service == "nodepool" || service == "all";
    let run_worker = service == "worker" || service == "all";

    if !run_master && !run_nodepool && !run_worker {
        eprintln!(
            "Usage: {} [master|nodepool|worker|all]\n       {} submit <job.zip> --username <user> --password <pass> [--task-id <id>] [--max-cpt <cpt>]\n       {} status <task-id> --username <user> --password <pass>\n       {} result <task-id> --username <user> --password <pass>",
            args[0], args[0], args[0], args[0]
        );
        std::process::exit(1);
    }

    if run_master || run_nodepool {
        validate_auth_service_config(&config)?;
    }

    let mut shutdown_handles: Vec<watch::Sender<bool>> = Vec::new();

    // ---- Nodepool (DB-backed gRPC server) ----
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
        let dispatcher = Arc::new(Dispatcher::new(db.clone(), task_timeout, max_redispatch));

        let disp_shutdown = dispatcher
            .clone()
            .start_registered_dispatch_loop(std::time::Duration::from_secs(5));
        shutdown_handles.push(disp_shutdown);

        let timeout_shutdown = dispatcher.start_timeout_loop(std::time::Duration::from_secs(10));
        shutdown_handles.push(timeout_shutdown);

        // Build gRPC servers
        let np_state = Arc::new(NodepoolState {
            auth: auth.clone(),
            node_manager: node_mgr.clone(),
            scheduler: scheduler.clone(),
            artifact_root: hivemind_node_manager::grpc::artifact_root_for_config(&config),
        });

        let user_svc = UserServiceServer::new(GrpcUserService::new(np_state.clone()));
        let node_svc = NodeManagerServiceServer::new(GrpcNodeManagerService::new(np_state.clone()));
        let master_svc = MasterNodeServiceServer::new(GrpcMasterNodeService::new(np_state.clone()));
        let batch_svc = BatchRuntimeServiceServer::new(GrpcBatchRuntimeService::new(np_state));
        let vpn_svc = VpnServiceServer::new(GrpcVpnService::new(vpn));

        let np_addr = config.server.nodepool_grpc_addr.clone();
        tokio::spawn(async move {
            if let Err(e) = tonic::transport::Server::builder()
                .add_service(user_svc)
                .add_service(node_svc)
                .add_service(master_svc)
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

    // ---- Master (HTTP-to-gRPC proxy, no DB) ----
    if run_master {
        let nodepool_grpc = config.server.nodepool_grpc_addr.clone();
        let jwt_secret = config.auth.jwt_secret.clone();
        let token_expiry = config.auth.token_expiry_hours;
        let api =
            MasterApiServer::new(jwt_secret, token_expiry, nodepool_grpc, config.clone()).await?;
        let addr = config.server.master_http_addr.clone();
        tokio::spawn(async move {
            if let Err(e) = api.serve(&addr).await {
                tracing::error!("Master API error: {}", e);
            }
        });
        info!(
            "Master HTTP API started on {}",
            config.server.master_http_addr
        );
    }

    // ---- Worker gRPC Server ----────────────────────────────
    if run_worker {
        let executor = Arc::new(WorkerExecutor::new(config.clone()));
        let resources = executor.get_system_resources();
        info!(
            "Worker executor: {} cores, {} GB RAM",
            resources.cpu_cores, resources.total_memory_gb
        );

        let wk_state = Arc::new(WorkerGrpcState::new(config.clone(), executor.clone()));
        let wk_svc = WorkerNodeServiceServer::new(GrpcWorkerNodeService::new(wk_state));

        let wk_addr = config.server.worker_grpc_addr.clone();
        let worker_id = std::env::var("WORKER_ID")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| format!("worker-{}", uuid::Uuid::new_v4()));
        let worker_advertise_addr = nodepool_client::advertise_addr(
            &config.server.worker_grpc_addr,
            config.server.worker_advertise_addr.clone(),
        );
        let control_profile = WorkerProfile::from_resource_spec(
            worker_id.clone(),
            worker_advertise_addr.clone(),
            std::env::var("WORKER_LOCATION").unwrap_or_else(|_| "local".into()),
            executor.get_resource_spec(),
        );
        let control_addr = config.server.worker_control_http_addr.clone();
        let worker_control_allowed_origins =
            config.server.worker_control_cors_allowed_origins.clone();
        tokio::spawn(async move {
            if let Err(e) = hivemind_worker_executor::control_api::serve_with_allowed_origins(
                &control_addr,
                control_profile,
                &worker_control_allowed_origins,
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

        let reg_shutdown = nodepool_client::start_registration_loop(
            executor.clone(),
            config.server.nodepool_grpc_addr.clone(),
            worker_id,
            worker_advertise_addr,
            std::env::var("WORKER_LOCATION").unwrap_or_else(|_| "local".into()),
            std::time::Duration::from_secs(10),
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
    fn auth_service_startup_rejects_default_jwt_secret() {
        let mut config = HivemindConfig::default();
        config.auth.jwt_secret = "CHANGE_ME_IN_PRODUCTION".into();

        let error = validate_auth_service_config(&config)
            .unwrap_err()
            .to_string();

        assert!(error.contains("JWT_SECRET"));
    }
}

use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_auth::AuthManager;
use hivemind_node_manager::{NodeManager, heartbeat::HeartbeatHandler};
use hivemind_task_scheduler::{TaskScheduler, dispatcher::Dispatcher};
use hivemind_master_api::MasterApiServer;
use hivemind_worker_executor::WorkerExecutor;
use hivemind_vpn_service::VpnService;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    hivemind_common::init_tracing("hivemind");

    let args: Vec<String> = std::env::args().collect();
    let service = args.get(1).map(|s| s.as_str()).unwrap_or("all");

    let config = HivemindConfig::load()?;
    let db = DatabaseManager::new(&config).await?;
    db.run_migrations().await?;
    hivemind_database::postgres::seed_default_user(&db.pool).await?;

    let auth = AuthManager::new(&db, &config.auth.jwt_secret, config.auth.token_expiry_hours);
    let scheduler = TaskScheduler::new(db.clone(), auth.clone());

    let run_master = service == "master" || service == "all";
    let run_nodepool = service == "nodepool" || service == "all";
    let run_worker = service == "worker" || service == "all";

    if !run_master && !run_nodepool && !run_worker {
        eprintln!("Usage: {} [master|nodepool|worker|all]", args[0]);
        std::process::exit(1);
    }

    let mut _shutdown_handles: Vec<tokio::sync::watch::Sender<bool>> = Vec::new();

    if run_master {
        let nodepool_grpc = config.server.nodepool_grpc_addr.clone();
        let api = MasterApiServer::new(
            db.clone(),
            auth.clone(),
            scheduler.clone(),
            nodepool_grpc,
        );

        let addr = config.server.master_http_addr.clone();
        tokio::spawn(async move {
            if let Err(e) = api.serve(&addr).await {
                tracing::error!("Master API server error: {}", e);
            }
        });
        info!("Master API started on {}", config.server.master_http_addr);
    }

    if run_nodepool {
        let node_mgr = Arc::new(NodeManager::new(&config, db.clone()));
        let _vpn = VpnService::new(config.clone(), db.clone());

        // Start heartbeat cleanup
        let heartbeat_handler = Arc::new(HeartbeatHandler::new(node_mgr.clone(), 30));
        let hb_shutdown = heartbeat_handler.start_cleanup_loop(std::time::Duration::from_secs(10));
        _shutdown_handles.push(hb_shutdown);

        // Start task dispatch loop
        let task_timeout = 30u64;
        let max_redispatch = 2i32;
        let dispatcher = Arc::new(Dispatcher::new(db.clone(), task_timeout, max_redispatch));
        let (_workers_tx, workers_rx) = tokio::sync::watch::channel(Vec::new());
        let dispatch_shutdown = dispatcher.clone().start_dispatch_loop(
            workers_rx,
            std::time::Duration::from_secs(5),
        );
        _shutdown_handles.push(dispatch_shutdown);

        // Start timeout monitor
        let timeout_shutdown = dispatcher.start_timeout_loop(std::time::Duration::from_secs(10));
        _shutdown_handles.push(timeout_shutdown);

        info!("Nodepool services started (heartbeat cleanup, dispatch loop, timeout monitor)");
    }

    if run_worker {
        let executor = WorkerExecutor::new(config.clone());
        let resources = executor.get_system_resources();
        info!(
            "Worker executor ready: {} cores, {} GB memory",
            resources.cpu_cores, resources.total_memory_gb
        );
    }

    info!("Hivemind running. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;

    // Graceful shutdown: signal all background loops to stop
    for handle in _shutdown_handles {
        let _ = handle.send(true);
    }

    info!("Shutting down...");
    Ok(())
}

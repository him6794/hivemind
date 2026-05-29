use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_auth::AuthManager;
use hivemind_node_manager::NodeManager;
use hivemind_task_scheduler::TaskScheduler;
use hivemind_master_api::MasterApiServer;
use hivemind_worker_executor::WorkerExecutor;
use hivemind_vpn_service::VpnService;
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
        let node_mgr = NodeManager::new(&config, db.clone());
        let _vpn = VpnService::new(config.clone(), db.clone());

        let node_mgr_clone = node_mgr;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                match node_mgr_clone.mark_offline_stale().await {
                    Ok(count) if count > 0 => {
                        tracing::info!("Marked {} stale workers offline", count);
                    }
                    Err(e) => tracing::error!("Stale worker cleanup error: {}", e),
                    _ => {}
                }
            }
        });

        info!("Nodepool services started");
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
    info!("Shutting down...");

    Ok(())
}

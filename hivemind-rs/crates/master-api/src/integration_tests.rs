use std::net::SocketAddr;
use std::sync::Arc;

use hivemind_auth::AuthManager;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_node_manager::grpc::{
    GrpcMasterNodeService, GrpcNodeManagerService, GrpcUserService, NodepoolState,
};
use hivemind_node_manager::NodeManager;
use hivemind_proto::{
    master_node_service_server::MasterNodeServiceServer,
    node_manager_service_server::NodeManagerServiceServer, user_service_server::UserServiceServer,
    ResourceSpec,
};
use hivemind_task_scheduler::TaskScheduler;

use crate::grpc_client::GrpcClient;

async fn nodepool_fixture() -> Option<(GrpcClient, DatabaseManager, String)> {
    let mut config = HivemindConfig::default();
    config.database.url = std::env::var("HIVEMIND_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://hivemind:hivemind@localhost:5432/hivemind_test".into());

    let db = DatabaseManager::new(&config).await.ok()?;
    db.run_migrations().await.ok()?;
    hivemind_database::postgres::seed_default_user(&db.pool)
        .await
        .ok()?;

    let auth = AuthManager::new(&db, &config.auth.jwt_secret, config.auth.token_expiry_hours);
    let scheduler = TaskScheduler::new(db.clone(), auth.clone());
    let node_manager = Arc::new(NodeManager::new(&config, db.clone()));
    let state = Arc::new(NodepoolState {
        auth,
        node_manager,
        scheduler,
    });

    let user_svc = UserServiceServer::new(GrpcUserService::new(state.clone()));
    let node_svc = NodeManagerServiceServer::new(GrpcNodeManagerService::new(state.clone()));
    let master_svc = MasterNodeServiceServer::new(GrpcMasterNodeService::new(state));

    let addr = reserve_loopback_addr()?;
    tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(user_svc)
            .add_service(node_svc)
            .add_service(master_svc)
            .serve(addr)
            .await;
    });

    let endpoint = addr.to_string();
    for _ in 0..30 {
        if let Ok(client) = GrpcClient::connect(&endpoint).await {
            return Some((client, db, endpoint));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    None
}

fn reserve_loopback_addr() -> Option<SocketAddr> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    let addr = listener.local_addr().ok()?;
    drop(listener);
    Some(addr)
}

#[tokio::test]
async fn grpc_client_talks_to_nodepool_fixture_for_provider_flow() {
    let (mut client, db, _endpoint) = match nodepool_fixture().await {
        Some(fixture) => fixture,
        None => return,
    };

    let unique = uuid::Uuid::new_v4().to_string();
    let worker_id = format!("it-worker-{unique}");

    let login = client.login("testuser", "testpass123").await.unwrap();
    assert!(login.success);
    let token = login.token;

    let quote = client
        .quote_task(&token, 200, 0, 4, 0, 10, 2)
        .await
        .unwrap();
    assert!(quote.success);
    assert!(quote.quoted_cpt > 0);
    assert_eq!(quote.currency, "CPT");

    let registered = client
        .register_worker_node(
            &worker_id,
            "127.0.0.1:50053",
            ResourceSpec {
                cpu_cores: 4,
                memory_mb: 16 * 1024,
                gpu_count: 0,
                gpu_name: String::new(),
                vram_mb: 0,
                cpu_score: 400,
                gpu_score: 0,
                storage_total_gb: 500,
                storage_available_gb: 250,
            },
            "local",
        )
        .await
        .unwrap();
    assert!(registered.success);

    let workers = client.list_workers(true).await.unwrap();
    assert!(workers
        .workers
        .iter()
        .any(|worker| worker.worker_id == worker_id));

    let updated = client
        .update_provider_worker_settings(&token, &worker_id, true, 2, 8, 0, 100, 25)
        .await
        .unwrap();
    assert!(updated.success);
    let settings = updated.settings.unwrap();
    assert_eq!(settings.cpu_cores_limit, 2);
    assert_eq!(settings.memory_gb_limit, 8);
    assert_eq!(settings.min_cpt_per_hour, 25);

    let fetched = client
        .get_provider_worker_settings(&token, &worker_id)
        .await
        .unwrap();
    assert!(fetched.success);
    assert_eq!(fetched.settings.unwrap().storage_gb_limit, 100);

    let earnings = client.get_provider_earnings(&token, 5).await.unwrap();
    assert!(earnings.success);
    assert_eq!(earnings.currency, "CPT");

    let removed = client.remove_worker(&worker_id, &token).await.unwrap();
    assert!(removed.success);

    sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
}

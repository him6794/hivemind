use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use hivemind_auth::AuthManager;
use hivemind_config::HivemindConfig;
use hivemind_database::{postgres::IsolatedTestPool, DatabaseManager};
use hivemind_node_manager::grpc::{
    artifact_root_for_config, GrpcMasterNodeService, GrpcNodeManagerService, GrpcUserService,
    NodepoolState,
};
use hivemind_node_manager::NodeManager;
use hivemind_proto::{
    master_node_service_server::MasterNodeServiceServer,
    node_manager_service_server::NodeManagerServiceServer, user_service_server::UserServiceServer,
    ResourceSpec,
};
use hivemind_task_scheduler::TaskScheduler;
use tower::ServiceExt;

use crate::grpc_client::GrpcClient;

struct NodepoolTestFixture {
    client: Option<GrpcClient>,
    db: DatabaseManager,
    endpoint: String,
    shutdown: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
    schema: IsolatedTestPool,
}

impl NodepoolTestFixture {
    fn take_client(&mut self) -> Option<GrpcClient> {
        self.client.take()
    }

    async fn cleanup(self) {
        let _ = self.shutdown.send(());
        let _ = self.server.await;
        self.schema.cleanup().await.ok();
    }
}

// Test-only nodepool fixture: spins up an in-process gRPC server against a throwaway test DB.
async fn nodepool_test_fixture() -> Option<NodepoolTestFixture> {
    let config = HivemindConfig::for_test();
    let fixture =
        hivemind_database::postgres::create_isolated_test_pool("master_api_nodepool_fixture")
            .await
            .ok()?;
    hivemind_database::postgres::run_migrations(&fixture.pool)
        .await
        .ok()?;
    let db = DatabaseManager {
        pool: fixture.pool.clone(),
    };

    let auth = AuthManager::new(&db, &config.auth.jwt_secret, config.auth.token_expiry_hours);
    let scheduler = TaskScheduler::new(db.clone(), auth.clone());
    let node_manager = Arc::new(NodeManager::new(&config, db.clone()));
    let state = Arc::new(NodepoolState {
        auth,
        node_manager,
        scheduler,
        artifact_root: artifact_root_for_config(&config),
        distribution: None,
    });

    let user_svc = UserServiceServer::new(GrpcUserService::new(state.clone()));
    let node_svc = NodeManagerServiceServer::new(GrpcNodeManagerService::new(state.clone()));
    let master_svc = MasterNodeServiceServer::new(GrpcMasterNodeService::new(state));

    let addr = reserve_loopback_addr()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(user_svc)
            .add_service(node_svc)
            .add_service(master_svc)
            .serve_with_shutdown(addr, async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    let endpoint = addr.to_string();
    for _ in 0..30 {
        if let Ok(client) = GrpcClient::connect(&endpoint).await {
            return Some(NodepoolTestFixture {
                client: Some(client),
                db,
                endpoint,
                shutdown: shutdown_tx,
                server,
                schema: fixture,
            });
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
async fn grpc_client_talks_to_nodepool_test_fixture_for_provider_flow() {
    let mut fixture = match nodepool_test_fixture().await {
        Some(fixture) => fixture,
        None => return,
    };
    let mut client = match fixture.take_client() {
        Some(client) => client,
        None => return,
    };
    let db = fixture.db.clone();
    let _endpoint = fixture.endpoint.clone();

    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(
        schema.starts_with("hm_test_"),
        "expected isolated test schema, got {schema}"
    );

    let unique = uuid::Uuid::new_v4().to_string();
    let username = format!("it-user-{unique}");
    let password = "integration-pass-example";

    let hash = bcrypt::hash(password, 12).unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, $3)")
        .bind(&username)
        .bind(&hash)
        .bind(1000i64)
        .execute(&db.pool)
        .await
        .unwrap();

    let login = client.login(&username, password).await.unwrap();
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
            &username,
            &username,
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
            &token,
        )
        .await
        .unwrap();
    assert!(registered.success);

    let workers = client.list_workers(true, &token).await.unwrap();
    assert!(workers
        .workers
        .iter()
        .any(|worker| worker.worker_id == username));

    let updated = client
        .update_provider_worker_settings(&token, &username, true, 2, 8, 0, 100, 25)
        .await
        .unwrap();
    assert!(updated.success);
    let settings = updated.settings.unwrap();
    assert_eq!(settings.cpu_cores_limit, 2);
    assert_eq!(settings.memory_gb_limit, 8);
    assert_eq!(settings.min_cpt_per_hour, 25);

    let fetched = client
        .get_provider_worker_settings(&token, &username)
        .await
        .unwrap();
    assert!(fetched.success);
    assert_eq!(fetched.settings.unwrap().storage_gb_limit, 100);

    let earnings = client.get_provider_earnings(&token, 5).await.unwrap();
    assert!(earnings.success);
    assert_eq!(earnings.currency, "CPT");

    let removed = client.remove_worker(&username, &token).await.unwrap();
    assert!(removed.success);

    sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
        .bind(&username)
        .execute(&db.pool)
        .await
        .ok();
    fixture.cleanup().await;
}

#[tokio::test]
async fn worker_path_routes_reject_unsafe_worker_ids_before_grpc() {
    let mut fixture = match nodepool_test_fixture().await {
        Some(fixture) => fixture,
        None => return,
    };
    let mut client = match fixture.take_client() {
        Some(client) => client,
        None => return,
    };
    let db = fixture.db.clone();

    let unique = uuid::Uuid::new_v4().to_string();
    let username = format!("it-worker-path-user-{unique}");
    let password = "integration-pass-example";

    let hash = bcrypt::hash(password, 12).unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, $3)")
        .bind(&username)
        .bind(&hash)
        .bind(1000i64)
        .execute(&db.pool)
        .await
        .unwrap();

    let login = client.login(&username, password).await.unwrap();
    assert!(login.success);
    let token = login.token;

    let config = HivemindConfig::for_test();
    let state = crate::handlers::AppState {
        jwt_secret: config.auth.jwt_secret.clone(),
        token_expiry_hours: config.auth.token_expiry_hours,
        grpc_client: client,
        config,
        task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
            crate::handlers::TaskSubmitRateLimiter::new(),
        )),
    };
    let app = crate::routes::create_router(state);

    let unsafe_worker_id = "worker..path";
    let cases = [
        (
            "GET",
            format!("/api/provider/workers/{unsafe_worker_id}/settings"),
            None,
        ),
        (
            "PUT",
            format!("/api/provider/workers/{unsafe_worker_id}/settings"),
            Some(
                r#"{"enabled":true,"cpu_cores_limit":1,"memory_gb_limit":1,"gpu_memory_gb_limit":0,"storage_gb_limit":1,"min_cpt_per_hour":1}"#,
            ),
        ),
        (
            "GET",
            format!("/api/provider/workers/{unsafe_worker_id}/trust"),
            None,
        ),
        (
            "PUT",
            format!("/api/admin/workers/{unsafe_worker_id}/trust-control"),
            Some(r#"{"banned":false,"score":0}"#),
        ),
    ];

    for (method, uri, body) in cases {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method}");
    }

    sqlx::query("DELETE FROM users WHERE username = $1")
        .bind(&username)
        .execute(&db.pool)
        .await
        .ok();
    fixture.cleanup().await;
}

#[tokio::test]
async fn task_path_routes_reject_unsafe_task_ids_before_grpc() {
    let mut fixture = match nodepool_test_fixture().await {
        Some(fixture) => fixture,
        None => return,
    };
    let mut client = match fixture.take_client() {
        Some(client) => client,
        None => return,
    };
    let db = fixture.db.clone();

    let unique = uuid::Uuid::new_v4().to_string();
    let username = format!("it-task-path-user-{unique}");
    let password = "integration-pass-example";

    let hash = bcrypt::hash(password, 12).unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, $3)")
        .bind(&username)
        .bind(&hash)
        .bind(1000i64)
        .execute(&db.pool)
        .await
        .unwrap();

    let login = client.login(&username, password).await.unwrap();
    assert!(login.success);
    let token = login.token;

    let config = HivemindConfig::for_test();
    let state = crate::handlers::AppState {
        jwt_secret: config.auth.jwt_secret.clone(),
        token_expiry_hours: config.auth.token_expiry_hours,
        grpc_client: client,
        config,
        task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
            crate::handlers::TaskSubmitRateLimiter::new(),
        )),
    };
    let app = crate::routes::create_router(state);

    let unsafe_task_id = "task..path";
    let cases = [
        ("GET", format!("/api/tasks/{unsafe_task_id}/log")),
        ("GET", format!("/api/tasks/{unsafe_task_id}/result")),
        (
            "GET",
            format!("/api/tasks/{unsafe_task_id}/artifact/download"),
        ),
        ("POST", format!("/api/tasks/{unsafe_task_id}/stop")),
    ];

    for (method, uri) in cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method}");
    }

    sqlx::query("DELETE FROM users WHERE username = $1")
        .bind(&username)
        .execute(&db.pool)
        .await
        .ok();
    fixture.cleanup().await;
}

#[tokio::test]
async fn task_submission_routes_reject_invalid_resource_values_before_grpc() {
    let mut fixture = match nodepool_test_fixture().await {
        Some(fixture) => fixture,
        None => return,
    };
    let mut client = match fixture.take_client() {
        Some(client) => client,
        None => return,
    };
    let db = fixture.db.clone();

    let unique = uuid::Uuid::new_v4().to_string();
    let username = format!("it-task-resource-user-{unique}");
    let password = "integration-pass-example";

    let hash = bcrypt::hash(password, 12).unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, $3)")
        .bind(&username)
        .bind(&hash)
        .bind(1000i64)
        .execute(&db.pool)
        .await
        .unwrap();

    let login = client.login(&username, password).await.unwrap();
    assert!(login.success);
    let token = login.token;

    let config = HivemindConfig::for_test();
    let state = crate::handlers::AppState {
        jwt_secret: config.auth.jwt_secret.clone(),
        token_expiry_hours: config.auth.token_expiry_hours,
        grpc_client: client,
        config,
        task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
            crate::handlers::TaskSubmitRateLimiter::new(),
        )),
    };
    let app = crate::routes::create_router(state);

    let quote_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks/quote")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"task_id":"bad-resources","memory_gb":-1,"host_count":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(quote_response.status(), StatusCode::BAD_REQUEST);

    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"task_id":"bad-resources-{unique}","torrent":"magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567","cpu_score":-10,"host_count":0,"max_cpt":1000}}"#,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::BAD_REQUEST);

    sqlx::query("DELETE FROM users WHERE username = $1")
        .bind(&username)
        .execute(&db.pool)
        .await
        .ok();
    fixture.cleanup().await;
}

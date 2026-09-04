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
    artifact_root_for_config, GrpcGeneralComputeArtifactService, GrpcMasterNodeService,
    GrpcNodeManagerService, GrpcUserService, NodepoolState,
};
use hivemind_node_manager::NodeManager;
use hivemind_proto::{
    general_compute_artifact_service_server::GeneralComputeArtifactServiceServer,
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

    fn api_config(&self) -> HivemindConfig {
        let mut config = HivemindConfig::for_test();
        config.server.nodepool_grpc_endpoint = Some(self.endpoint.clone());
        config
    }

    async fn cleanup(self) {
        let _ = self.shutdown.send(());
        let _ = self.server.await;
        self.schema.cleanup().await.ok();
    }
}

// Test-only nodepool fixture: spins up an in-process gRPC server against a throwaway test DB.
async fn nodepool_test_fixture() -> Option<NodepoolTestFixture> {
    let addr = reserve_loopback_addr()?;
    let endpoint = addr.to_string();
    let mut config = HivemindConfig::for_test();
    // The in-process nodepool is the test transport authority. Advertising its
    // reachable endpoint lets the production VPN gate validate local transport
    // without contacting an external Website API or Headscale instance.
    config.server.nodepool_grpc_endpoint = Some(endpoint.clone());
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
        worker_execution_private_key_pem: config.auth.worker_execution_private_key_pem.clone(),
        worker_execution_public_key_pem: config.auth.worker_execution_public_key_pem.clone(),
        managed_proof_rollout_mode: config.managed_proof.rollout_mode,
        node_manager,
        session_registry: hivemind_client_core::SessionRegistry::shared(Default::default()),
        dispatcher: None,
        scheduler,
        artifact_root: artifact_root_for_config(&config),
    });

    let user_svc = UserServiceServer::new(GrpcUserService::new(state.clone()));
    let node_svc = NodeManagerServiceServer::new(GrpcNodeManagerService::new(state.clone()));
    let artifact_svc = GeneralComputeArtifactServiceServer::new(
        GrpcGeneralComputeArtifactService::new(state.clone()),
    );
    let master_svc = MasterNodeServiceServer::new(GrpcMasterNodeService::new(state));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(user_svc)
            .add_service(node_svc)
            .add_service(artifact_svc)
            .add_service(master_svc)
            .serve_with_shutdown(addr, async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

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
async fn master_http_artifact_chunk_proxy_persists_a_manifest_bound_source() {
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
    let username = format!("it-artifact-user-{unique}");
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

    let bytes = b"source";
    let digest = general_compute_runtime::sha256_digest(bytes);
    let mut manifest = general_compute_runtime::GeneralComputeRequest {
        execution_id: format!("execution-{unique}"),
        attempt_id: format!("attempt-{unique}"),
        idempotency_key: format!("idempotency-{unique}"),
        request_digest: String::new(),
        runtime_version: general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        backend_id: "python-cpython-312".into(),
        entrypoint: "main".into(),
        source_artifact: general_compute_runtime::ArtifactManifest {
            artifact_id: "source".into(),
            role: general_compute_runtime::ArtifactRole::Source,
            size_bytes: bytes.len() as u64,
            mime_type: "text/plain".into(),
            sha256: digest.clone(),
            chunks: vec![general_compute_runtime::ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: digest.clone(),
            }],
            inline_bytes: None,
        },
        input_artifacts: vec![],
        execution_policy: general_compute_runtime::ExecutionPolicy::default(),
        determinism: general_compute_runtime::DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    manifest.request_digest = manifest.canonical_request_digest();
    let manifest_json = serde_json::to_vec(&manifest).unwrap();
    let task_id = format!("it-artifact-task-{unique}");
    let created = client
        .upload_task(
            &task_id,
            "",
            ResourceSpec::default(),
            "local",
            1,
            &token,
            1,
            general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION,
            "",
            &manifest_json,
            "",
            "",
            &[],
        )
        .await
        .unwrap();
    assert!(created.success, "{}", created.status_message);

    let config = fixture.api_config();
    let state = crate::handlers::AppState {
        grpc_client: client,
        config,
        task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
            crate::handlers::TaskSubmitRateLimiter::new(),
        )),
    };
    let app = crate::routes::create_router(state);
    let body = serde_json::json!({
        "artifact_id": "source",
        "offset": 0,
        "size_bytes": bytes.len(),
        "sha256": digest,
        "bytes": bytes,
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/tasks/{task_id}/general-compute/artifacts/chunk"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let persisted: (i64, String, Vec<u8>) = sqlx::query_as(
        "SELECT size_bytes, sha256, content
         FROM general_compute_artifact_chunks
         WHERE task_id = $1 AND artifact_id = 'source' AND offset_bytes = 0",
    )
    .bind(&task_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, bytes.len() as i64);
    assert_eq!(persisted.1, general_compute_runtime::sha256_digest(bytes));
    assert_eq!(persisted.2, bytes);

    sqlx::query("DELETE FROM users WHERE username = $1")
        .bind(&username)
        .execute(&db.pool)
        .await
        .ok();
    fixture.cleanup().await;
}

#[tokio::test]
async fn master_http_managed_gpu_submission_persists_manifest_without_legacy_torrent() {
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
    let username = format!("it-managed-gpu-user-{unique}");
    let password = "integration-pass-example";
    let task_id = format!("it-managed-gpu-task-{unique}");

    let hash = bcrypt::hash(password, 12).unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, $3)")
        .bind(&username)
        .bind(&hash)
        .bind(1_000i64)
        .execute(&db.pool)
        .await
        .unwrap();
    let login = client.login(&username, password).await.unwrap();
    assert!(login.success);
    let token = login.token;

    let manifest = managed_gpu_request_for_http_test(&unique);
    let manifest_value = serde_json::to_value(&manifest).unwrap();
    let response = {
        let config = fixture.api_config();
        let state = crate::handlers::AppState {
            grpc_client: client,
            config,
            task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
                crate::handlers::TaskSubmitRateLimiter::new(),
            )),
        };
        let app = crate::routes::create_router(state);
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "task_id": task_id,
                        "runtime": general_compute_runtime::managed_gpu::MANAGED_GPU_RUNTIME_VERSION,
                        "managed_gpu_manifest_json": manifest_value,
                        "host_count": 1,
                        "max_cpt": manifest.reservation_cpt,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
    };
    assert_eq!(response.status(), StatusCode::CREATED);

    type PersistedManagedGpuTask = (
        Option<String>,
        Option<Vec<u8>>,
        Option<String>,
        Option<String>,
        Option<Vec<u8>>,
        Option<String>,
        i64,
    );
    let (
        runtime,
        persisted_manifest,
        torrent_source,
        task_source,
        general_manifest,
        result_torrent,
        max_cpt,
    ): PersistedManagedGpuTask = sqlx::query_as(
        "SELECT runtime, managed_gpu_manifest_json, torrent_source, task_source,
                    general_compute_manifest_json, result_torrent, max_cpt
             FROM tasks WHERE task_id = $1",
    )
    .bind(&task_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();

    assert_eq!(
        runtime.as_deref(),
        Some(general_compute_runtime::managed_gpu::MANAGED_GPU_RUNTIME_VERSION)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            persisted_manifest
                .as_deref()
                .expect("managed GPU manifest must be persisted"),
        )
        .unwrap(),
        serde_json::to_value(&manifest).unwrap()
    );
    assert!(torrent_source.is_none());
    assert!(task_source.is_none());
    assert!(general_manifest.is_none());
    assert!(result_torrent.is_none());
    assert_eq!(max_cpt, manifest.reservation_cpt as i64);

    sqlx::query("DELETE FROM tasks WHERE task_id = $1")
        .bind(&task_id)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM users WHERE username = $1")
        .bind(&username)
        .execute(&db.pool)
        .await
        .ok();
    fixture.cleanup().await;
}

#[tokio::test]
async fn master_http_managed_gpu_result_returns_typed_json_without_legacy_torrent() {
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
    let username = format!("it-managed-gpu-result-user-{unique}");
    let password = "integration-pass-example";
    let task_id = format!("it-managed-gpu-result-task-{unique}");
    let worker_id = format!("it-managed-gpu-result-worker-{unique}");

    let hash = bcrypt::hash(password, 12).unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, $3)")
        .bind(&username)
        .bind(&hash)
        .bind(1_000i64)
        .execute(&db.pool)
        .await
        .unwrap();
    let login = client.login(&username, password).await.unwrap();
    assert!(login.success);
    let token = login.token;

    let manifest = managed_gpu_request_for_http_test(&unique);
    let manifest_json = serde_json::to_vec(&manifest).unwrap();
    let submitted = client
        .upload_task(
            &task_id,
            "",
            ResourceSpec::default(),
            "local",
            1,
            &token,
            manifest.reservation_cpt as i64,
            general_compute_runtime::managed_gpu::MANAGED_GPU_RUNTIME_VERSION,
            "",
            &[],
            "",
            "",
            &manifest_json,
        )
        .await
        .unwrap();
    assert!(submitted.success, "{}", submitted.status_message);

    let capability = managed_gpu_capability_for_http_test(&manifest);
    let registration = managed_gpu_registration_for_http_test(&manifest, &capability);
    let result = managed_gpu_result_for_http_test(&manifest, &capability);
    sqlx::query(
        "UPDATE tasks
         SET worker_id = $1,
             status = 'COMPLETED',
             billing_settled = true,
             billed_amount = $2,
             result_torrent = NULL
         WHERE task_id = $3",
    )
    .bind(&worker_id)
    .bind(manifest.reservation_cpt as i64)
    .bind(&task_id)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO managed_gpu_attempt_bindings
            (task_id, attempt_generation, worker_id,
             capability_snapshot_json, selected_gpu_json)
         VALUES ($1, 1, $2, $3, $4)",
    )
    .bind(&task_id)
    .bind(&worker_id)
    .bind(serde_json::to_vec(&registration).unwrap())
    .bind(serde_json::to_vec(&capability).unwrap())
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO managed_gpu_results
            (task_id, attempt_id, attempt_generation, worker_id, result_json)
         VALUES ($1, $2, 1, $3, $4)",
    )
    .bind(&task_id)
    .bind(&manifest.attempt_id)
    .bind(&worker_id)
    .bind(serde_json::to_vec(&result).unwrap())
    .execute(&db.pool)
    .await
    .unwrap();

    let config = fixture.api_config();
    let state = crate::handlers::AppState {
        grpc_client: client,
        config,
        task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
            crate::handlers::TaskSubmitRateLimiter::new(),
        )),
    };
    let app = crate::routes::create_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/tasks/{task_id}/result"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(
        response.into_body(),
        hivemind_proto::MANAGED_GPU_RESULT_MAX_BYTES + 1024,
    )
    .await
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["success"], true);
    assert_eq!(value["task_id"], task_id);
    assert_eq!(value["result_torrent"], "");
    assert_eq!(value["status_message"], "OK");
    assert_eq!(
        value["managed_gpu_result"]["runtime_version"],
        general_compute_runtime::managed_gpu::MANAGED_GPU_RUNTIME_VERSION
    );
    assert_eq!(value["managed_gpu_result"]["status"], "completed");
    assert_eq!(value["managed_gpu_result"]["output"], "[[4,6]]");
    assert!(value["managed_gpu_result"].is_object());

    sqlx::query("DELETE FROM tasks WHERE task_id = $1")
        .bind(&task_id)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM users WHERE username = $1")
        .bind(&username)
        .execute(&db.pool)
        .await
        .ok();
    fixture.cleanup().await;
}

fn managed_gpu_capability_for_http_test(
    request: &general_compute_runtime::managed_gpu::ManagedGpuRequest,
) -> general_compute_runtime::managed_gpu::ManagedGpuCapability {
    general_compute_runtime::managed_gpu::ManagedGpuCapability::new(
        "cuda-test-0",
        request.gpu_requirement.compute_capability.clone(),
        request.gpu_requirement.runtime_version.clone(),
        request.gpu_requirement.driver_abi.clone(),
        16 * 1024 * 1024 * 1024,
        32,
        request.guest_image_digest.clone(),
        0,
        "GPU-0123456789abcdef",
    )
    .unwrap()
}

fn managed_gpu_registration_for_http_test(
    request: &general_compute_runtime::managed_gpu::ManagedGpuRequest,
    capability: &general_compute_runtime::managed_gpu::ManagedGpuCapability,
) -> general_compute_runtime::TrustedWorkerCapabilityRegistration {
    general_compute_runtime::TrustedWorkerCapabilityRegistration {
        worker: general_compute_runtime::WorkerCapabilities {
            guest_image_digests: vec![request.guest_image_digest.clone()],
            capabilities: vec!["cuda".into()],
            max_threads: 4,
            gpu_available: true,
        },
        gpu_capabilities: vec![],
        managed_gpu_backends: vec![
            general_compute_runtime::managed_gpu::ManagedGpuBackendRegistration {
                backend_id: request.backend_id.clone(),
                runtime_version: general_compute_runtime::managed_gpu::MANAGED_GPU_RUNTIME_VERSION
                    .into(),
                semantics_manifest_sha256:
                    general_compute_runtime::managed_gpu::MANAGED_GPU_SEMANTICS_MANIFEST_SHA256
                        .into(),
                operation_registry_version:
                    general_compute_runtime::managed_gpu::MANAGED_GPU_OPERATION_REGISTRY_VERSION
                        .into(),
                guest_image_digest: request.guest_image_digest.clone(),
                billing_version: general_compute_runtime::managed_gpu::MANAGED_GPU_BILLING_VERSION
                    .into(),
                cost_model_version:
                    general_compute_runtime::managed_gpu::MANAGED_GPU_COST_MODEL_VERSION.into(),
                reservation_cpt: request.reservation_cpt,
                max_source_bytes: 256 * 1024,
                max_input_bytes: 16 * 1024 * 1024,
                max_output_bytes: 16 * 1024 * 1024,
                max_operations: 1_000_000,
                max_gpu_time_ms: 120_000,
                capabilities: vec![capability.clone()],
            },
        ],
        backends: vec![],
    }
}

fn managed_gpu_result_for_http_test(
    request: &general_compute_runtime::managed_gpu::ManagedGpuRequest,
    capability: &general_compute_runtime::managed_gpu::ManagedGpuCapability,
) -> general_compute_runtime::managed_gpu::ManagedGpuResult {
    let output = "[[4,6]]";
    general_compute_runtime::managed_gpu::ManagedGpuResult {
        protocol_version: general_compute_runtime::managed_gpu::MANAGED_GPU_RESULT_PROTOCOL_VERSION
            .into(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        runtime_version: request.runtime_version.clone(),
        semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
        operation_registry_version: request.operation_registry_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        source_sha256: request.source_sha256(),
        input_sha256: request.input_sha256(),
        reservation_cpt: request.reservation_cpt,
        status: general_compute_runtime::managed_gpu::ManagedGpuStatus::Completed,
        exit_code: Some(0),
        error_code: None,
        output: output.into(),
        output_sha256: general_compute_runtime::sha256_digest(output.as_bytes()),
        selected_gpu: capability.clone(),
        usage: general_compute_runtime::managed_gpu::ManagedGpuUsage {
            source_bytes: request.source.len() as u64,
            input_bytes: request.input_json.len() as u64,
            output_bytes: output.len() as u64,
            executed_operations: 1,
            operation_cost_units: 10,
            wall_time_ms: 1,
            gpu_time_ms: 1,
            gpu_memory_bytes: 1024,
        },
        evidence: general_compute_runtime::managed_gpu::ManagedGpuEvidence::default(),
    }
}

fn managed_gpu_request_for_http_test(
    unique: &str,
) -> general_compute_runtime::managed_gpu::ManagedGpuRequest {
    let image_digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let requirement = general_compute_runtime::managed_gpu::ManagedGpuRequirement::new(
        "8.9",
        "12.4",
        "550",
        8 * 1024 * 1024 * 1024,
        1,
        image_digest,
    )
    .unwrap();
    let mut request = general_compute_runtime::managed_gpu::ManagedGpuRequest {
        protocol_version:
            general_compute_runtime::managed_gpu::MANAGED_GPU_REQUEST_PROTOCOL_VERSION.into(),
        execution_id: format!("gpu-execution-{unique}"),
        attempt_id: format!("gpu-attempt-{unique}"),
        idempotency_key: format!("gpu-idempotency-{unique}"),
        request_digest: String::new(),
        runtime_version: general_compute_runtime::managed_gpu::MANAGED_GPU_RUNTIME_VERSION.into(),
        semantics_manifest_sha256:
            general_compute_runtime::managed_gpu::MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
        operation_registry_version:
            general_compute_runtime::managed_gpu::MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
        backend_id: "managed-cuda-test".into(),
        guest_image_digest: image_digest.into(),
        source: "gpu_add_f32([1.0], [2.0])".into(),
        input_json: "{}".into(),
        gpu_requirement: requirement,
        limits: general_compute_runtime::managed_gpu::ManagedGpuLimits::default(),
        reservation_cpt: 1_000,
        billing_version: general_compute_runtime::managed_gpu::MANAGED_GPU_BILLING_VERSION.into(),
        cost_model_version: general_compute_runtime::managed_gpu::MANAGED_GPU_COST_MODEL_VERSION
            .into(),
        settlement_basis: general_compute_runtime::managed_gpu::MANAGED_GPU_SETTLEMENT_BASIS.into(),
        proof_policy: general_compute_runtime::managed_gpu::ManagedGpuProofPolicy::None,
    };
    request.request_digest = request.canonical_request_digest();
    request
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

    let config = fixture.api_config();
    let state = crate::handlers::AppState {
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

    let config = fixture.api_config();
    let state = crate::handlers::AppState {
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

    let config = fixture.api_config();
    let state = crate::handlers::AppState {
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

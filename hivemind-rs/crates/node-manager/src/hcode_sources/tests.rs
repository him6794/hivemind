use super::*;
use hivemind_auth::jwt_service::JwtService;
use hivemind_config::HivemindConfig;
use hivemind_database::postgres::{create_isolated_test_pool, run_migrations, IsolatedTestPool};
use hivemind_database::DatabaseManager;
use hivemind_models::Claims;
use hivemind_proto::{
    hcode_artifact_source_service_client::HcodeArtifactSourceServiceClient,
    hcode_artifact_source_service_server::HcodeArtifactSourceServiceServer,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

const SECRET: &str = "hcode-source-registration-test-secret";
type Pause = (oneshot::Sender<()>, oneshot::Receiver<()>);

fn namespace() -> String {
    ["HEADSCALE_CLIENT_USER", "NODEPOOL_VPN_HEADSCALE_USER"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| "nodepool".into())
}

struct Controller {
    url: String,
    requests: Arc<AtomicUsize>,
    pause: Arc<Mutex<Option<Pause>>>,
    expiry: Arc<Mutex<Option<DateTime<Utc>>>>,
    listing: Arc<Mutex<Option<Vec<serde_json::Value>>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Controller {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let pause: Arc<Mutex<Option<Pause>>> = Arc::new(Mutex::new(None));
        let paused = pause.clone();
        let expiry: Arc<Mutex<Option<DateTime<Utc>>>> = Arc::new(Mutex::new(None));
        let node_expiry = expiry.clone();
        let listing: Arc<Mutex<Option<Vec<serde_json::Value>>>> = Arc::new(Mutex::new(None));
        let served = listing.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let len = stream.read(&mut buffer).await.unwrap();
                    assert!(len > 0 && request.len() < 8192);
                    request.extend_from_slice(&buffer[..len]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer test-control-key\r\n"));
                if request.starts_with("GET /api/v1/node ") {
                    count.fetch_add(1, Ordering::SeqCst);
                    let nodes = served.lock().unwrap().clone().unwrap_or_default();
                    let body = serde_json::to_vec(&json!({"nodes": nodes})).unwrap();
                    let mut response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    response.extend_from_slice(&body);
                    stream.write_all(&response).await.unwrap();
                    continue;
                }
                let node_id: u64 = request
                    .strip_prefix("GET /api/v1/node/")
                    .unwrap()
                    .split(' ')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                count.fetch_add(1, Ordering::SeqCst);
                let pause = paused.lock().unwrap().take();
                if let Some((arrived, resume)) = pause {
                    arrived.send(()).unwrap();
                    resume.await.unwrap();
                }
                let expiry = *node_expiry.lock().unwrap();
                let body = serde_json::to_vec(&json!({"node": {
                    "id": node_id.to_string(), "user": {"name": namespace()},
                    "preAuthKey": {"id": node_id.to_string(), "reusable": false},
                    "ipAddresses": [format!("100.64.42.{node_id}")],
                    "online": true, "expiry": expiry.map(|expiry| expiry.to_rfc3339())
                }}))
                .unwrap();
                let mut response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes();
                response.extend_from_slice(&body);
                stream.write_all(&response).await.unwrap();
            }
        });
        Self {
            url,
            requests,
            pause,
            expiry,
            listing,
            task,
        }
    }

    /// Nodes returned by `GET /api/v1/node`, the shared-namespace listing.
    fn set_listing(&self, nodes: Vec<serde_json::Value>) {
        *self.listing.lock().unwrap() = Some(nodes);
    }

    fn pause_next(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (arrived, waiting) = oneshot::channel();
        let (resume, resumed) = oneshot::channel();
        *self.pause.lock().unwrap() = Some((arrived, resumed));
        (waiting, resume)
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Fixture {
    db: IsolatedTestPool,
    controller: Controller,
    service: Arc<GrpcHcodeArtifactSourceService>,
    owner_id: Uuid,
    token: String,
    other_token: String,
    artifact: Uuid,
}

fn token(id: Uuid, name: &str, role: Option<&str>, expiry: DateTime<Utc>) -> String {
    JwtService::new(SECRET, 24)
        .encode_claims(&Claims {
            sub: name.into(),
            user_id: id.to_string(),
            role: role.map(String::from),
            task_id: None,
            worker_id: None,
            exp: expiry.timestamp() as usize,
            iat: (Utc::now().timestamp() - 60) as usize,
        })
        .unwrap()
}

impl Fixture {
    async fn new(name: &str) -> Self {
        let db = create_isolated_test_pool(name)
            .await
            .expect("isolated PostgreSQL required");
        run_migrations(&db.pool).await.unwrap();
        let controller = Controller::start().await;
        let mut config = HivemindConfig::for_test();
        config.auth.jwt_secret = SECRET.into();
        config.vpn.headscale_url = controller.url.clone();
        config.vpn.headscale_api_key = "test-control-key".into();
        let vpn = Arc::new(
            VpnService::new(
                config,
                DatabaseManager {
                    pool: db.pool.clone(),
                },
            )
            .unwrap(),
        );
        let service = Arc::new(GrpcHcodeArtifactSourceService::new(db.pool.clone(), vpn));
        let owner_id: Uuid = sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ('alice', 'test-hash') RETURNING id")
            .fetch_one(&db.pool).await.unwrap();
        let other_id: Uuid = sqlx::query_scalar(
            "INSERT INTO users (username, password_hash) VALUES ('bob', 'test-hash') RETURNING id",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let token = token(
            owner_id,
            "alice",
            None,
            Utc::now() + chrono::Duration::hours(1),
        );
        let other_token = self::token(
            other_id,
            "bob",
            Some("admin"),
            Utc::now() + chrono::Duration::hours(1),
        );
        sqlx::query("INSERT INTO tasks (task_id, owner, runtime, status, deadline) VALUES ('source-task', 'alice', 'hcode', 'PENDING', NOW() + INTERVAL '1 hour')")
            .execute(&db.pool).await.unwrap();
        let digest = general_compute_runtime::sha256_digest(b"12345678");
        let artifact = sqlx::query_scalar(
            "INSERT INTO general_compute_artifacts (task_id, artifact_id, sha256, size_bytes, expected_chunk_count, expires_at, owner_user_id)
             VALUES ('source-task', 'model', $1, 8, 1, NOW() + INTERVAL '1 hour', $2) RETURNING id"
        ).bind(&digest).bind(owner_id).fetch_one(&db.pool).await.unwrap();
        sqlx::query("INSERT INTO general_compute_artifact_manifest_chunks (task_id, artifact_id, offset_bytes, size_bytes, sha256) VALUES ('source-task', 'model', 0, 8, $1)")
            .bind(digest).execute(&db.pool).await.unwrap();
        let fixture = Self {
            db,
            controller,
            service,
            owner_id,
            token,
            other_token,
            artifact,
        };
        fixture.peer(7).await;
        fixture
    }

    async fn peer(&self, node: i64) {
        let id: Uuid = sqlx::query_scalar("INSERT INTO vpn_peers (worker_id, hostname, virtual_ip, auth_key, online) VALUES ($1, 'test', $2, 'not-a-credential', true) RETURNING id")
            .bind(format!("user:alice:device-{node}"))
            .bind(format!("127.0.0.{node}"))
            .fetch_one(&self.db.pool).await.unwrap();
        sqlx::query("INSERT INTO vpn_peer_enrollments (peer_id, user_id, controller_url, namespace, headscale_key_id) VALUES ($1, $2, $3, $4, $5)")
            .bind(id).bind(self.owner_id).bind(format!("{}/api/v1/", self.controller.url))
            .bind(namespace()).bind(node).execute(&self.db.pool).await.unwrap();
    }

    fn registration(&self, node: u64) -> RegisterHcodeSourceRequest {
        RegisterHcodeSourceRequest {
            token: self.token.clone(),
            task_id: "source-task".into(),
            artifact_id: "model".into(),
            vpn_client_id: format!("user:alice:device-{node}"),
            headscale_node_id: node,
        }
    }

    fn lookup(&self, id: &str) -> HcodeSourceRequest {
        HcodeSourceRequest {
            token: self.token.clone(),
            source_id: id.into(),
        }
    }

    async fn register(&self, node: u64) -> HcodeSourceRegistration {
        self.service
            .register_source(Request::new(self.registration(node)))
            .await
            .unwrap()
            .into_inner()
    }

    async fn finish(self) {
        self.db.cleanup().await.unwrap();
    }
}

#[test]
fn source_registration_rejects_invalid_unbounded_and_path_identities() {
    let valid = RegisterHcodeSourceRequest {
        token: "test-token".into(),
        task_id: "task-1".into(),
        artifact_id: "model..weights".into(),
        vpn_client_id: "user:alice:desktop".into(),
        headscale_node_id: 7,
    };
    assert!(validate_registration(&valid).is_ok());
    for task in [
        "",
        ".",
        "..",
        "task..1",
        "../task",
        "https://example.invalid",
        "task name",
        &"x".repeat(256),
    ] {
        let mut invalid = valid.clone();
        invalid.task_id = task.into();
        assert!(validate_registration(&invalid).is_err());
    }
    for id in ["", "../model", "model/path", &"x".repeat(256)] {
        let mut invalid = valid.clone();
        invalid.artifact_id = id.into();
        assert!(validate_registration(&invalid).is_err());
    }
    for token in ["", "with space", &"x".repeat(MAX_ACCOUNT_TOKEN_BYTES + 1)] {
        let mut invalid = valid.clone();
        invalid.token = token.into();
        assert!(validate_registration(&invalid).is_err());
    }
    for node in [0, u64::MAX] {
        let mut invalid = valid.clone();
        invalid.headscale_node_id = node;
        assert!(validate_registration(&invalid).is_err());
    }
    for id in [
        "",
        "not-uuid",
        "00000000-0000-0000-0000-000000000000",
        &Uuid::new_v4().simple().to_string(),
    ] {
        assert!(source_id(&HcodeSourceRequest {
            token: "test".into(),
            source_id: id.into()
        })
        .is_err());
    }
}

#[tokio::test]
async fn source_registration_real_rpc_renews_revokes_and_never_moves_bytes_or_dispatches() {
    let fixture = Fixture::new("hcode_source_rpc").await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let service =
        GrpcHcodeArtifactSourceService::new(fixture.db.pool.clone(), fixture.service.vpn.clone());
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                HcodeArtifactSourceServiceServer::new(service)
                    .max_decoding_message_size(HCODE_SOURCE_RPC_MAX_BYTES),
            )
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stopped.await;
                },
            )
            .await
            .unwrap();
    });
    let mut client = HcodeArtifactSourceServiceClient::connect(url)
        .await
        .unwrap();
    let first = client
        .register_source(fixture.registration(7))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.state, "registered_pending");
    assert_eq!(first.overlay_ipv4, "100.64.42.7");
    assert_eq!(first.port, u32::from(HCODE_SOURCE_PORT));
    assert_eq!(first.artifact_size_bytes, 8);
    assert_eq!(
        first.artifact_sha256,
        general_compute_runtime::sha256_digest(b"12345678")
    );
    assert!(first.expires_at_unix_ms > Utc::now().timestamp_millis());
    assert!(
        first.expires_at_unix_ms <= (Utc::now() + chrono::Duration::seconds(60)).timestamp_millis()
    );
    let second = client
        .register_source(fixture.registration(7))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.source_id, second.source_id);
    assert_ne!(first.registration_id, second.registration_id);
    assert_eq!(
        client
            .get_source(fixture.lookup(&first.source_id))
            .await
            .unwrap()
            .into_inner(),
        second
    );
    run_migrations(&fixture.db.pool).await.unwrap();
    let state: (bool, String) = sqlx::query_as(
        "SELECT complete, availability_status FROM general_compute_artifacts WHERE id = $1",
    )
    .bind(fixture.artifact)
    .fetch_one(&fixture.db.pool)
    .await
    .unwrap();
    assert_eq!(state, (false, "pending".into()));
    for query in [
        "SELECT COUNT(*) FROM general_compute_artifact_chunks",
        "SELECT COUNT(*) FROM general_compute_artifact_sources",
        "SELECT COUNT(*) FROM managed_consensus_attempts",
        "SELECT COUNT(*) FROM managed_consensus_usage_holds",
    ] {
        let count: i64 = sqlx::query_scalar(query)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "registration must not change this count: {query}");
    }
    let task_state: String =
        sqlx::query_scalar("SELECT status FROM tasks WHERE task_id = 'source-task'")
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    assert_eq!(task_state, "PENDING");
    let foreign = HcodeSourceRequest {
        token: fixture.other_token.clone(),
        source_id: first.source_id.clone(),
    };
    assert_eq!(
        client.get_source(foreign.clone()).await.unwrap_err().code(),
        tonic::Code::NotFound
    );
    assert!(
        !client
            .revoke_source(foreign)
            .await
            .unwrap()
            .into_inner()
            .revoked
    );
    assert!(
        client
            .revoke_source(fixture.lookup(&first.source_id))
            .await
            .unwrap()
            .into_inner()
            .revoked
    );
    assert!(
        !client
            .revoke_source(fixture.lookup(&first.source_id))
            .await
            .unwrap()
            .into_inner()
            .revoked
    );
    assert_eq!(
        client
            .get_source(fixture.lookup(&first.source_id))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );
    let mut oversized = fixture.registration(7);
    oversized.token = "x".repeat(HCODE_SOURCE_RPC_MAX_BYTES + 1);
    assert!(client.register_source(oversized).await.is_err());
    stop.send(()).unwrap();
    server.await.unwrap();
    fixture.finish().await;
}

#[tokio::test]
async fn source_registration_checks_owner_purpose_and_legacy_metadata_before_http() {
    let fixture = Fixture::new("hcode_source_admission").await;
    for (token, code) in [
        (fixture.other_token.clone(), tonic::Code::NotFound),
        ("forged".into(), tonic::Code::Unauthenticated),
        (
            self::token(
                fixture.owner_id,
                "alice",
                Some("worker-execution"),
                Utc::now() + chrono::Duration::hours(1),
            ),
            tonic::Code::Unauthenticated,
        ),
        (
            self::token(
                fixture.owner_id,
                "alice",
                None,
                Utc::now() - chrono::Duration::seconds(1),
            ),
            tonic::Code::Unauthenticated,
        ),
    ] {
        let mut request = fixture.registration(7);
        request.token = token;
        assert_eq!(
            fixture
                .service
                .register_source(Request::new(request))
                .await
                .unwrap_err()
                .code(),
            code
        );
    }
    sqlx::query("UPDATE general_compute_artifacts SET owner_user_id = NULL")
        .execute(&fixture.db.pool)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .service
            .register_source(Request::new(fixture.registration(7)))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );
    assert_eq!(fixture.controller.requests.load(Ordering::SeqCst), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn source_registration_rechecks_task_and_artifact_after_controller_observation() {
    for mutation in [
        "UPDATE tasks SET status = 'COMPLETED'",
        "UPDATE tasks SET owner = 'bob'",
        "UPDATE tasks SET runtime = 'general-compute-v1alpha1'",
        "UPDATE tasks SET deadline = NOW() - INTERVAL '1 second'",
        "UPDATE general_compute_artifacts SET expires_at = NOW() - INTERVAL '1 second'",
        "UPDATE general_compute_artifacts SET owner_user_id = NULL",
    ] {
        let fixture = Fixture::new("hcode_source_race").await;
        let (arrived, resume) = fixture.controller.pause_next();
        let service = fixture.service.clone();
        let request = fixture.registration(7);
        let registration =
            tokio::spawn(async move { service.register_source(Request::new(request)).await });
        tokio::time::timeout(Duration::from_secs(3), arrived)
            .await
            .unwrap()
            .unwrap();
        sqlx::query(mutation)
            .execute(&fixture.db.pool)
            .await
            .unwrap();
        resume.send(()).unwrap();
        assert_eq!(
            registration.await.unwrap().unwrap_err().code(),
            tonic::Code::NotFound,
            "{mutation}"
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hcode_artifact_sources")
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        fixture.finish().await;
    }
}

#[tokio::test]
async fn source_registration_live_reads_reject_expired_revoked_rotated_and_deleted_parents() {
    for mutation in [
        "UPDATE hcode_artifact_sources SET observed_at = NOW() - INTERVAL '2 minutes', expires_at = NOW() - INTERVAL '1 second'",
        "UPDATE hcode_artifact_sources SET revoked_at = NOW()",
        "UPDATE vpn_peer_enrollments SET enrollment_id = gen_random_uuid()",
        "DELETE FROM vpn_peer_enrollments",
        "DELETE FROM vpn_peers",
        "UPDATE general_compute_artifacts SET expires_at = NOW() - INTERVAL '1 second'",
        "DELETE FROM general_compute_artifacts",
        "UPDATE tasks SET status = 'COMPLETED'",
        "UPDATE tasks SET owner = 'bob'",
        "UPDATE users SET is_active = false WHERE username = 'alice'",
        "DELETE FROM users WHERE username = 'alice'",
    ] {
        let fixture = Fixture::new("hcode_source_live_fences").await;
        let source = fixture.register(7).await;
        sqlx::query(mutation).execute(&fixture.db.pool).await.unwrap();
        assert!(fixture.service.get_source(Request::new(fixture.lookup(&source.source_id))).await.is_err(), "{mutation}");
        fixture.finish().await;
    }
}

#[tokio::test]
async fn source_registration_accepts_null_task_deadline_without_dropping_other_expiry_caps() {
    let fixture = Fixture::new("hcode_source_null_deadline").await;
    sqlx::query("UPDATE tasks SET deadline = NULL WHERE task_id = 'source-task'")
        .execute(&fixture.db.pool)
        .await
        .unwrap();
    let artifact_deadline: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE general_compute_artifacts
         SET expires_at = clock_timestamp() + INTERVAL '20 seconds'
         WHERE id = $1 RETURNING expires_at",
    )
    .bind(fixture.artifact)
    .fetch_one(&fixture.db.pool)
    .await
    .unwrap();
    let source = fixture.register(7).await;
    assert!(source.expires_at_unix_ms > Utc::now().timestamp_millis());
    assert!(source.expires_at_unix_ms <= artifact_deadline.timestamp_millis());
    let current = fixture
        .service
        .get_source(Request::new(fixture.lookup(&source.source_id)))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(current.registration_id, source.registration_id);
    fixture.finish().await;
}

#[tokio::test]
async fn source_registration_caps_ttl_at_every_authority_deadline() {
    for gate in ["token", "node", "artifact", "task"] {
        let mut fixture = Fixture::new("hcode_source_ttl").await;
        let deadline = Utc::now() + chrono::Duration::seconds(20);
        match gate {
            "token" => fixture.token = self::token(fixture.owner_id, "alice", None, deadline),
            "node" => *fixture.controller.expiry.lock().unwrap() = Some(deadline),
            "artifact" => {
                sqlx::query("UPDATE general_compute_artifacts SET expires_at = $1")
                    .bind(deadline)
                    .execute(&fixture.db.pool)
                    .await
                    .unwrap();
            }
            "task" => {
                sqlx::query("UPDATE tasks SET deadline = $1")
                    .bind(deadline)
                    .execute(&fixture.db.pool)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let source = fixture.register(7).await;
        assert!(
            source.expires_at_unix_ms <= deadline.timestamp_millis(),
            "{gate}"
        );
        fixture.finish().await;
    }
}

#[tokio::test]
async fn source_registration_quota_is_atomic_and_revocation_frees_only_its_slot() {
    let fixture = Fixture::new("hcode_source_quota").await;
    for node in 8..16 {
        fixture.peer(node).await;
    }
    let mut calls = tokio::task::JoinSet::new();
    for node in 7..16 {
        let service = fixture.service.clone();
        let request = fixture.registration(node);
        calls.spawn(async move { (node, service.register_source(Request::new(request)).await) });
    }
    let mut accepted = Vec::new();
    let mut denied = Vec::new();
    while let Some(call) = calls.join_next().await {
        let (node, result) = call.unwrap();
        match result {
            Ok(response) => accepted.push((node, response.into_inner())),
            Err(error) => {
                assert_eq!(error.code(), tonic::Code::ResourceExhausted);
                denied.push(node);
            }
        }
    }
    assert_eq!(accepted.len(), MAX_SOURCES_PER_ARTIFACT as usize);
    assert_eq!(denied.len(), 1);
    let renewed = fixture.register(accepted[0].0).await;
    assert_eq!(renewed.source_id, accepted[0].1.source_id);
    assert_ne!(renewed.registration_id, accepted[0].1.registration_id);
    assert!(
        fixture
            .service
            .revoke_source(Request::new(fixture.lookup(&renewed.source_id)))
            .await
            .unwrap()
            .into_inner()
            .revoked
    );
    fixture.register(denied[0]).await;
    assert!(fixture
        .service
        .get_source(Request::new(fixture.lookup(&accepted[1].1.source_id)))
        .await
        .is_ok());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hcode_artifact_sources")
        .fetch_one(&fixture.db.pool)
        .await
        .unwrap();
    assert_eq!(count, MAX_SOURCES_PER_ARTIFACT);
    fixture.finish().await;
}

#[tokio::test]
async fn source_registration_recreated_username_cannot_claim_old_artifacts() {
    let mut fixture = Fixture::new("hcode_source_recreated_owner").await;
    let source = fixture.register(7).await;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(fixture.owner_id)
        .execute(&fixture.db.pool)
        .await
        .unwrap();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ('alice', 'new') RETURNING id",
    )
    .fetch_one(&fixture.db.pool)
    .await
    .unwrap();
    fixture.token = self::token(id, "alice", None, Utc::now() + chrono::Duration::hours(1));
    assert_eq!(
        fixture
            .service
            .get_source(Request::new(fixture.lookup(&source.source_id)))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );
    assert_eq!(
        fixture
            .service
            .register_source(Request::new(fixture.registration(7)))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );
    assert_eq!(fixture.controller.requests.load(Ordering::SeqCst), 1);
    fixture.finish().await;
}

#[tokio::test]
async fn source_registration_persistence_rechecks_enrollment_and_opaque_owner() {
    let fixture = Fixture::new("hcode_source_persist_fence").await;
    let owner = fixture.service.owner(&fixture.token).await.unwrap();
    let other = fixture.service.owner(&fixture.other_token).await.unwrap();
    let peer = fixture
        .service
        .vpn
        .verify_user_vpn_peer(&fixture.token, "user:alice:device-7", 7)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .service
            .persist(&other, &peer, fixture.artifact)
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::PermissionDenied
    );
    sqlx::query("UPDATE vpn_peer_enrollments SET enrollment_id = gen_random_uuid()")
        .execute(&fixture.db.pool)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .service
            .persist(&owner, &peer, fixture.artifact)
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::NotFound
    );
    fixture.finish().await;
}

/// One entry of the shared-namespace listing. Its single-use key ID is the
/// enrollment key the node joined with.
fn listed_node(node: i64) -> serde_json::Value {
    json!({
        "id": node.to_string(), "user": {"name": namespace()},
        "preAuthKey": {"id": node.to_string(), "reusable": false},
        "ipAddresses": [format!("100.64.42.{node}")],
        "online": true, "expiry": null
    })
}

#[tokio::test]
async fn owner_vpn_node_query_resolves_only_the_callers_enrolled_node() {
    let fixture = Fixture::new("hcode_owner_vpn_node").await;
    fixture
        .controller
        .set_listing(vec![listed_node(3), listed_node(7)]);
    let response = fixture
        .service
        .resolve_owner_vpn_node(Request::new(HcodeOwnerVpnNodeRequest {
            token: fixture.token.clone(),
            vpn_client_id: "user:alice:device-7".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(response.vpn_client_id, "user:alice:device-7");
    assert_eq!(response.headscale_node_id, 7);
    assert_eq!(response.overlay_ipv4, "100.64.42.7");
    // Another account cannot read this enrollment; the reply reveals nothing about it.
    let denied = fixture
        .service
        .resolve_owner_vpn_node(Request::new(HcodeOwnerVpnNodeRequest {
            token: fixture.other_token.clone(),
            vpn_client_id: "user:alice:device-7".into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(denied.code(), tonic::Code::FailedPrecondition);
    fixture.finish().await;
}

#[tokio::test]
async fn owner_vpn_node_query_reports_an_unjoined_key_and_validates_before_network() {
    let fixture = Fixture::new("hcode_owner_vpn_node_unjoined").await;
    fixture.controller.set_listing(vec![listed_node(3)]);
    let unjoined = fixture
        .service
        .resolve_owner_vpn_node(Request::new(HcodeOwnerVpnNodeRequest {
            token: fixture.token.clone(),
            vpn_client_id: "user:alice:device-7".into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(unjoined.code(), tonic::Code::FailedPrecondition);
    assert!(unjoined.message().contains("not joined"));
    let requests_before = fixture.controller.requests.load(Ordering::SeqCst);
    let invalid = fixture
        .service
        .resolve_owner_vpn_node(Request::new(HcodeOwnerVpnNodeRequest {
            token: fixture.token.clone(),
            vpn_client_id: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(invalid.code(), tonic::Code::InvalidArgument);
    assert_eq!(
        fixture.controller.requests.load(Ordering::SeqCst),
        requests_before,
        "an invalid identity never reaches the control plane"
    );
    fixture.finish().await;
}

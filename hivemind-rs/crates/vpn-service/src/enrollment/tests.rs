use super::*;
use crate::headscale_client::HeadscaleClient;
use hivemind_config::HivemindConfig;
use hivemind_database::postgres::{create_isolated_test_pool, run_migrations, IsolatedTestPool};
use hivemind_database::DatabaseManager;
use hivemind_models::Claims;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

struct Step {
    response: Vec<u8>,
    arrived: Option<oneshot::Sender<()>>,
    resume: Option<oneshot::Receiver<()>>,
}

impl Step {
    fn json(value: Value) -> Self {
        Self::raw(200, &serde_json::to_vec(&value).unwrap(), "")
    }

    fn raw(status: u16, body: &[u8], extra_headers: &str) -> Self {
        let mut response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n",
            body.len()
        ).into_bytes();
        response.extend_from_slice(body);
        Self {
            response,
            arrived: None,
            resume: None,
        }
    }

    fn paused(value: Value) -> (Self, oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (arrived_tx, arrived_rx) = oneshot::channel();
        let (resume_tx, resume_rx) = oneshot::channel();
        let mut step = Self::json(value);
        step.arrived = Some(arrived_tx);
        step.resume = Some(resume_rx);
        (step, arrived_rx, resume_tx)
    }
}

struct Controller {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Controller {
    async fn start(steps: Vec<Step>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let task = tokio::spawn(async move {
            for step in steps {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                let header_end = loop {
                    let len = stream.read(&mut buffer).await.unwrap();
                    assert!(len > 0 && request.len() < 16 * 1024);
                    request.extend_from_slice(&buffer[..len]);
                    if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = String::from_utf8(request[..header_end].to_vec()).unwrap();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while request.len() < header_end + content_length {
                    let len = stream.read(&mut buffer).await.unwrap();
                    assert!(len > 0 && request.len() < 16 * 1024);
                    request.extend_from_slice(&buffer[..len]);
                }
                recorded
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                if let Some(arrived) = step.arrived {
                    let _ = arrived.send(());
                }
                if let Some(resume) = step.resume {
                    resume.await.unwrap();
                }
                // A size-bound rejection is allowed to close before the body is sent.
                let _ = stream.write_all(&step.response).await;
            }
        });
        Self {
            url,
            requests,
            task: Some(task),
        }
    }

    async fn finish(mut self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(3), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

fn issued(id: i64) -> Value {
    json!({"preAuthKey": {"id": id.to_string(), "key": format!("issued-secret-{id}"), "reusable": false}})
}

fn node(key: i64) -> Value {
    json!({"node": {
        "id": "7", "user": {"name": crate::shared_headscale_client_user()},
        "online": true, "ipAddresses": ["100.64.1.2"], "expiry": null,
        "preAuthKey": {"id": key.to_string(), "key": "masked***", "reusable": false}
    }})
}

fn user_list() -> Value {
    json!({"users": [
        {"name": crate::shared_headscale_client_user(), "id": "3"},
        {"name": "user:alice:desktop", "id": "5"}
    ]})
}

fn issuance_steps(id: i64) -> Vec<Step> {
    vec![
        Step::json(json!({})),
        Step::json(user_list()),
        Step::json(issued(id)),
    ]
}

async fn fixture(
    name: &str,
    controller: &Controller,
) -> (Arc<VpnService>, IsolatedTestPool, Uuid, String) {
    let fixture = create_isolated_test_pool(name)
        .await
        .expect("isolated test database required");
    run_migrations(&fixture.pool).await.unwrap();
    let mut config = HivemindConfig::for_test();
    config.auth.jwt_secret = "vpn-enrollment-test-secret".into();
    config.vpn.headscale_url = controller.url.clone();
    config.vpn.headscale_api_key = "control-test-key".into();
    let service = Arc::new(
        VpnService::new(
            config,
            DatabaseManager {
                pool: fixture.pool.clone(),
            },
        )
        .unwrap(),
    );
    let (id, token) = account(&service, "alice").await;
    (service, fixture, id, token)
}

async fn account(service: &VpnService, username: &str) -> (Uuid, String) {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'test-hash') RETURNING id",
    )
    .bind(username)
    .fetch_one(&service.db.pool)
    .await
    .unwrap();
    let token = token(
        service,
        username,
        &id.to_string(),
        None,
        Utc::now().timestamp() + 3600,
    );
    (id, token)
}

fn token(
    service: &VpnService,
    username: &str,
    id: &str,
    role: Option<&str>,
    expiry: i64,
) -> String {
    JwtService::new(&service.config.auth.jwt_secret, 24)
        .encode_claims(&Claims {
            sub: username.into(),
            user_id: id.into(),
            role: role.map(String::from),
            task_id: None,
            worker_id: None,
            exp: expiry as usize,
            iat: (Utc::now().timestamp() - 60) as usize,
        })
        .unwrap()
}

async fn binding(service: &VpnService, client_id: &str) -> (Uuid, Uuid, Uuid, i64) {
    sqlx::query_as(
        "SELECT e.enrollment_id, e.peer_id, e.user_id, e.headscale_key_id
         FROM vpn_peer_enrollments e JOIN vpn_peers p ON p.id = e.peer_id WHERE p.worker_id = $1",
    )
    .bind(client_id)
    .fetch_one(&service.db.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn enrollment_persists_controller_identity_and_ignores_claimed_ip() {
    let mut steps = issuance_steps(11);
    steps.push(Step::json(node(11)));
    let controller = Controller::start(steps).await;
    let (service, fixture, owner_id, owner_token) =
        fixture("vpn_enrollment_identity", &controller).await;
    let config = service
        .issue_user_vpn_config_from_token(&owner_token, "Desktop")
        .await
        .unwrap();
    assert_eq!(config.auth_key, "issued-secret-11");
    let saved = binding(&service, &config.client_id).await;
    assert_eq!(saved.2, owner_id);
    assert_eq!(saved.3, 11);
    assert!(!saved.0.is_nil());
    run_migrations(&fixture.pool).await.unwrap();
    assert_eq!(saved, binding(&service, &config.client_id).await);
    sqlx::query(
        "UPDATE vpn_peers SET virtual_ip = '127.0.0.1', online = true WHERE worker_id = $1",
    )
    .bind(&config.client_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let peer = service
        .verify_user_vpn_peer(&owner_token, &config.client_id, 7)
        .await
        .unwrap();
    assert_eq!(peer.owner_id(), owner_id);
    assert_eq!(peer.peer_id(), saved.1);
    assert_eq!(peer.enrollment_id(), saved.0);
    assert_eq!(peer.node().ipv4().to_string(), "100.64.1.2");
    assert!(peer.authorization_expires_at() > Utc::now());
    let (_, other_token) = account(&service, "bob").await;
    assert!(!service
        .revoke_user_vpn_enrollment(&other_token, &config.client_id)
        .await
        .unwrap());
    assert!(service
        .revoke_user_vpn_enrollment(&owner_token, &config.client_id)
        .await
        .unwrap());
    assert!(!service
        .revoke_user_vpn_enrollment(&owner_token, &config.client_id)
        .await
        .unwrap());
    assert!(service
        .verify_user_vpn_peer(&owner_token, &config.client_id, 7)
        .await
        .is_err());
    let requests = controller.finish().await;
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("POST /api/v1/user "));
    assert!(requests[1].starts_with("GET /api/v1/user "));
    assert!(requests[2].starts_with("POST /api/v1/preauthkey "));
    assert!(requests[3].starts_with("GET /api/v1/node/7 "));
    for request in &requests {
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer control-test-key\r\n"));
        assert!(!request.contains(&owner_token));
    }
    let payload: Value =
        serde_json::from_str(requests[2].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        payload["user"], "3",
        "the API requires an ID, not the namespace name"
    );
    assert_eq!(payload["reusable"], false);
    assert_eq!(payload["ephemeral"], false);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_rejects_non_account_and_mismatched_identities_before_http() {
    let controller = Controller::start(vec![Step::json(issued(11))]).await;
    let (service, fixture, owner_id, owner_token) =
        fixture("vpn_enrollment_auth", &controller).await;
    let (other_id, other_token) = account(&service, "bob").await;
    let future = Utc::now().timestamp() + 3600;
    for invalid in [
        "not-a-token".into(),
        token(&service, "alice", &other_id.to_string(), None, future),
        token(&service, "alice", "not-a-uuid", None, future),
        token(
            &service,
            "alice",
            &owner_id.to_string(),
            Some("worker"),
            future,
        ),
        token(
            &service,
            "alice",
            &owner_id.to_string(),
            Some("worker-execution"),
            future,
        ),
        token(
            &service,
            "alice",
            &owner_id.to_string(),
            None,
            Utc::now().timestamp() - 1,
        ),
    ] {
        assert!(service
            .issue_user_vpn_config_from_token(&invalid, "desktop")
            .await
            .is_err());
        assert!(service
            .verify_user_vpn_peer(&invalid, "user:alice:desktop", 7)
            .await
            .is_err());
    }
    assert!(service
        .issue_user_vpn_config("alice", "desktop", &other_token)
        .await
        .is_err());
    sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
        .bind(owner_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(service
        .issue_user_vpn_config_from_token(&owner_token, "desktop")
        .await
        .is_err());
    assert!(controller.requests.lock().unwrap().is_empty());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vpn_peers")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_does_not_upgrade_legacy_or_foreign_controller_records() {
    let controller = Controller::start(issuance_steps(11)).await;
    let (service, fixture, _, owner_token) = fixture("vpn_enrollment_legacy", &controller).await;
    sqlx::query(
        "INSERT INTO vpn_peers (worker_id, hostname, virtual_ip, auth_key, online)
        VALUES ('user:alice:legacy', 'legacy', '100.64.9.1', 'unverified-secret', true)",
    )
    .execute(&fixture.pool)
    .await
    .unwrap();
    run_migrations(&fixture.pool).await.unwrap();
    assert!(service
        .verify_user_vpn_peer(&owner_token, "user:alice:legacy", 7)
        .await
        .is_err());
    let config = service
        .issue_user_vpn_config_from_token(&owner_token, "desktop")
        .await
        .unwrap();
    let (_, other_token) = account(&service, "bob").await;
    assert!(service
        .verify_user_vpn_peer(&other_token, &config.client_id, 7)
        .await
        .is_err());
    sqlx::query("UPDATE vpn_peer_enrollments SET namespace = 'other-namespace'")
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(service
        .verify_user_vpn_peer(&owner_token, &config.client_id, 7)
        .await
        .is_err());
    sqlx::query("UPDATE vpn_peer_enrollments SET namespace = $1, controller_url = 'http://another-controller/api/v1/'")
        .bind(&service.headscale_user).execute(&fixture.pool).await.unwrap();
    assert!(service
        .verify_user_vpn_peer(&owner_token, &config.client_id, 7)
        .await
        .is_err());
    assert_eq!(controller.finish().await.len(), 3);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_reissuance_fences_old_nodes_and_duplicate_keys_rollback() {
    let mut steps = issuance_steps(11);
    steps.extend(issuance_steps(12));
    steps.push(Step::json(node(11)));
    steps.push(Step::json(node(12)));
    steps.extend(issuance_steps(12));
    let controller = Controller::start(steps).await;
    let (service, fixture, _, owner_token) = fixture("vpn_enrollment_rotate", &controller).await;
    let first = service
        .issue_user_vpn_config_from_token(&owner_token, "desktop")
        .await
        .unwrap();
    let old = binding(&service, &first.client_id).await;
    sqlx::query("UPDATE vpn_peers SET online = true")
        .execute(&fixture.pool)
        .await
        .unwrap();
    let second = service
        .issue_user_vpn_config_from_token(&owner_token, "desktop")
        .await
        .unwrap();
    let current = binding(&service, &second.client_id).await;
    assert_eq!(old.1, current.1);
    assert_ne!(old.0, current.0);
    assert_eq!(current.3, 12);
    let (online, key): (bool, String) =
        sqlx::query_as("SELECT online, auth_key FROM vpn_peers WHERE id = $1")
            .bind(current.1)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert!(!online);
    assert_eq!(key, "issued-secret-12");
    assert!(service
        .verify_user_vpn_peer(&owner_token, &first.client_id, 7)
        .await
        .is_err());
    assert!(service
        .verify_user_vpn_peer(&owner_token, &second.client_id, 7)
        .await
        .is_ok());
    assert!(service
        .issue_user_vpn_config_from_token(&owner_token, "other-device")
        .await
        .is_err());
    assert_eq!(binding(&service, &second.client_id).await, current);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vpn_peers")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "failed binding must roll back the peer insert");
    assert_eq!(controller.finish().await.len(), 11);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_rechecks_all_fences_after_controller_observation() {
    for mutation in [
        "DELETE FROM vpn_peer_enrollments",
        "UPDATE vpn_peer_enrollments SET headscale_key_id = 12",
        "UPDATE vpn_peer_enrollments SET enrollment_id = gen_random_uuid()",
        "UPDATE users SET is_active = false",
        "DELETE FROM users",
        "DELETE FROM vpn_peers",
    ] {
        let (step, arrived, resume) = Step::paused(node(11));
        let mut steps = issuance_steps(11);
        steps.push(step);
        let controller = Controller::start(steps).await;
        let (service, fixture, _, owner_token) =
            fixture("vpn_enrollment_read_fence", &controller).await;
        let config = service
            .issue_user_vpn_config_from_token(&owner_token, "desktop")
            .await
            .unwrap();
        let verifier = service.clone();
        let verification = tokio::spawn(async move {
            verifier
                .verify_user_vpn_peer(&owner_token, &config.client_id, 7)
                .await
        });
        tokio::time::timeout(Duration::from_secs(3), arrived)
            .await
            .unwrap()
            .unwrap();
        sqlx::query(mutation).execute(&fixture.pool).await.unwrap();
        resume.send(()).unwrap();
        let error = verification.await.unwrap().unwrap_err();
        assert!(
            error.to_string().contains("changed or expired"),
            "{mutation}: {error}"
        );
        assert_eq!(controller.finish().await.len(), 4);
        if mutation == "DELETE FROM users" || mutation == "DELETE FROM vpn_peers" {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vpn_peer_enrollments")
                .fetch_one(&fixture.pool)
                .await
                .unwrap();
            assert_eq!(count, 0);
        }
        fixture.cleanup().await.unwrap();
    }
}

#[tokio::test]
async fn enrollment_rechecks_owner_after_key_issuance() {
    let (step, arrived, resume) = Step::paused(issued(11));
    let controller =
        Controller::start(vec![Step::json(json!({})), Step::json(user_list()), step]).await;
    let (service, fixture, _, owner_token) =
        fixture("vpn_enrollment_write_fence", &controller).await;
    let issuer = service.clone();
    let issuance = tokio::spawn(async move {
        issuer
            .issue_user_vpn_config_from_token(&owner_token, "desktop")
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), arrived)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE users SET is_active = false")
        .execute(&fixture.pool)
        .await
        .unwrap();
    resume.send(()).unwrap();
    assert!(issuance
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("changed or expired"));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vpn_peers")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(controller.finish().await.len(), 3);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_legacy_worker_join_invalidates_the_identified_binding() {
    let mut steps = issuance_steps(11);
    steps.push(Step::json(user_list()));
    steps.push(Step::json(issued(12)));
    let controller = Controller::start(steps).await;
    let (service, fixture, _, owner_token) =
        fixture("vpn_enrollment_legacy_replace", &controller).await;
    let config = service
        .issue_user_vpn_config_from_token(&owner_token, "desktop")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
        VALUES ($1, 'alice', '127.0.0.1', 1, 1)",
    )
    .bind(&config.client_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    service
        .join_vpn(&config.client_id, "worker-replacement", &owner_token)
        .await
        .unwrap();
    assert!(service
        .verify_user_vpn_peer(&owner_token, &config.client_id, 7)
        .await
        .is_err());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vpn_peer_enrollments")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(controller.finish().await.len(), 5);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_missing_key_identity_does_not_publish_a_vpn_record() {
    let controller = Controller::start(vec![
        Step::json(json!({})),
        Step::json(user_list()),
        Step::json(json!({"preAuthKey": {"key": "secret-without-id", "reusable": false}})),
    ])
    .await;
    let (service, fixture, _, owner_token) =
        fixture("vpn_enrollment_missing_id", &controller).await;
    let error = service
        .issue_user_vpn_config_from_token(&owner_token, "desktop")
        .await
        .unwrap_err();
    assert!(!format!("{error:?}").contains("secret-without-id"));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vpn_peers")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(controller.finish().await.len(), 3);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_concurrent_persistence_keeps_the_key_and_binding_atomic() {
    let controller = Controller::start(vec![
        Step::json(user_list()),
        Step::json(issued(11)),
        Step::json(user_list()),
        Step::json(issued(12)),
    ])
    .await;
    let (service, fixture, _, owner_token) =
        fixture("vpn_enrollment_concurrent", &controller).await;
    let first_owner = service.authenticate_vpn_owner(&owner_token).await.unwrap();
    let second_owner = service.authenticate_vpn_owner(&owner_token).await.unwrap();
    let first_key = service
        .client
        .create_identified_user_key(&service.headscale_user)
        .await
        .unwrap();
    let second_key = service
        .client
        .create_identified_user_key(&service.headscale_user)
        .await
        .unwrap();
    let (first, second) = tokio::join!(
        service.persist_user_vpn_enrollment(
            &first_owner,
            "user:alice:desktop",
            "alice-desktop",
            "100.64.2.1",
            &first_key
        ),
        service.persist_user_vpn_enrollment(
            &second_owner,
            "user:alice:desktop",
            "alice-desktop",
            "100.64.2.2",
            &second_key
        ),
    );
    first.unwrap();
    second.unwrap();
    let saved = binding(&service, "user:alice:desktop").await;
    let (key, ip): (String, String) =
        sqlx::query_as("SELECT auth_key, virtual_ip FROM vpn_peers WHERE id = $1")
            .bind(saved.1)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(key, format!("issued-secret-{}", saved.3));
    assert_eq!(
        ip,
        if saved.3 == 11 {
            "100.64.2.1"
        } else {
            "100.64.2.2"
        }
    );
    assert!([11, 12].contains(&saved.3));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vpn_peer_enrollments")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(controller.finish().await.len(), 4);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_account_recreation_cannot_inherit_an_old_binding() {
    let controller = Controller::start(issuance_steps(11)).await;
    let (service, fixture, old_id, old_token) =
        fixture("vpn_enrollment_recreated_user", &controller).await;
    let config = service
        .issue_user_vpn_config_from_token(&old_token, "desktop")
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(old_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let (new_id, new_token) = account(&service, "alice").await;
    assert_ne!(new_id, old_id);
    assert!(matches!(
        service
            .verify_user_vpn_peer(&old_token, &config.client_id, 7)
            .await,
        Err(VpnEnrollmentError::Unauthorized(_))
    ));
    assert!(matches!(
        service
            .verify_user_vpn_peer(&new_token, &config.client_id, 7)
            .await,
        Err(VpnEnrollmentError::NotEnrolled)
    ));
    assert_eq!(controller.finish().await.len(), 3);
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable localhost Headscale and isolated PostgreSQL"]
async fn enrollment_live_headscale_issues_and_persists_numeric_id_keys() {
    let url = std::env::var("HIVEMIND_TEST_HEADSCALE_URL").expect("test Headscale URL required");
    let parsed = reqwest::Url::parse(&url).unwrap();
    assert_eq!(
        parsed.host_str(),
        Some("127.0.0.1"),
        "use a disposable localhost controller"
    );
    let key =
        std::env::var("HIVEMIND_TEST_HEADSCALE_API_KEY").expect("test Headscale key required");
    let fixture = create_isolated_test_pool("vpn_enrollment_live_headscale")
        .await
        .unwrap();
    run_migrations(&fixture.pool).await.unwrap();
    let mut config = HivemindConfig::for_test();
    config.auth.jwt_secret = "vpn-live-enrollment-test-secret".into();
    config.vpn.headscale_url = url.clone();
    config.vpn.headscale_login_server = url;
    config.vpn.headscale_api_key = key;
    let service = VpnService::new(
        config,
        DatabaseManager {
            pool: fixture.pool.clone(),
        },
    )
    .unwrap();
    let (owner_id, token) = account(&service, "alice").await;
    let first = service
        .issue_user_vpn_config_from_token(&token, "live-api")
        .await
        .unwrap();
    let old = binding(&service, &first.client_id).await;
    assert_eq!(old.2, owner_id);
    assert!(old.3 > 0);
    assert!(!first.auth_key.is_empty());
    let second = service
        .issue_user_vpn_config_from_token(&token, "live-api")
        .await
        .unwrap();
    let current = binding(&service, &second.client_id).await;
    assert_eq!(old.1, current.1);
    assert_ne!(old.0, current.0);
    assert_ne!(old.3, current.3);
    assert!(first.auth_key != second.auth_key);
    assert!(service
        .revoke_user_vpn_enrollment(&token, &second.client_id)
        .await
        .unwrap());
    fixture.cleanup().await.unwrap();
}

#[tokio::test]
async fn enrollment_control_transport_is_bounded_and_does_not_leak_secrets() {
    let trap = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = format!("Location: http://{}/leak\r\n", trap.local_addr().unwrap());
    let oversized = vec![b'x'; 64 * 1024 + 1];
    let mut chunked = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
        oversized.len()
    )
    .into_bytes();
    chunked.extend_from_slice(&oversized);
    chunked.extend_from_slice(b"\r\n0\r\n\r\n");
    for step in [
        Step::raw(302, b"", &redirect),
        Step::raw(500, b"controller-secret", ""),
        Step::raw(200, &oversized, ""),
        Step {
            response: chunked,
            arrived: None,
            resume: None,
        },
        Step::raw(200, b"not-json", ""),
    ] {
        let controller = Controller::start(vec![step]).await;
        let client = HeadscaleClient::new(&controller.url, "control-test-key").unwrap();
        let error = client
            .create_identified_user_key("nodepool")
            .await
            .unwrap_err();
        assert!(!format!("{error:?}").contains("controller-secret"));
        assert!(!format!("{error:?}").contains("control-test-key"));
        assert_eq!(controller.finish().await.len(), 1);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(100), trap.accept())
            .await
            .is_err()
    );
}

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use crate::handlers::AppState;
use crate::routes::create_router;
use hivemind_auth::AuthManager;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_task_scheduler::TaskScheduler;
use tempfile::TempDir;

async fn setup_app() -> Option<axum::Router> {
    let config = hivemind_config::HivemindConfig::default();
    let db = DatabaseManager::new(&config).await.ok()?;
    db.run_migrations().await.ok()?;
    if let Err(e) = hivemind_database::postgres::seed_default_user(&db.pool).await {
        eprintln!("seed_default_user failed: {:?}", e);
        return None;
    }

    let auth = AuthManager::new(&db, "test-secret", 24);
    let scheduler = TaskScheduler::new(db.clone(), auth.clone());

    let state = AppState {
        db,
        auth,
        scheduler,
        nodepool_grpc_addr: "localhost:50051".into(),
        config,
    };

    Some(create_router(state))
}

async fn setup_app_with_config(config: HivemindConfig) -> Option<(axum::Router, DatabaseManager)> {
    let db = DatabaseManager::new(&config).await.ok()?;
    db.run_migrations().await.ok()?;
    if let Err(e) = hivemind_database::postgres::seed_default_user(&db.pool).await {
        eprintln!("seed_default_user failed: {:?}", e);
        return None;
    }

    let auth = AuthManager::new(&db, "test-secret", 24);
    let scheduler = TaskScheduler::new(db.clone(), auth.clone());

    let state = AppState {
        db: db.clone(),
        auth,
        scheduler,
        nodepool_grpc_addr: "localhost:50051".into(),
        config,
    };

    Some((create_router(state), db))
}

async fn login_token(app: &axum::Router) -> String {
    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "username": "testuser",
                        "password": "testpass123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    login_json["token"]
        .as_str()
        .expect("token should be a string")
        .to_string()
}

fn multipart_upload_body(
    boundary: &str,
    task_id: &str,
    filename: Option<&str>,
    file_bytes: Option<&[u8]>,
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"task_id\"\r\n\r\n");
    body.extend_from_slice(task_id.as_bytes());
    body.extend_from_slice(b"\r\n");

    if let Some(file_bytes) = file_bytes {
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
                filename.unwrap_or("task.zip")
            )
            .as_bytes(),
        );
        body.extend_from_slice(b"Content-Type: application/zip\r\n\r\n");
        body.extend_from_slice(file_bytes);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
    body
}

fn multipart_upload_body_with_fields(
    boundary: &str,
    task_id: &str,
    filename: Option<&str>,
    file_bytes: Option<&[u8]>,
    fields: &[(&str, &str)],
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"task_id\"\r\n\r\n");
    body.extend_from_slice(task_id.as_bytes());
    body.extend_from_slice(b"\r\n");

    for (name, value) in fields {
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }

    if let Some(file_bytes) = file_bytes {
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
                filename.unwrap_or("task.zip")
            )
            .as_bytes(),
        );
        body.extend_from_slice(b"Content-Type: application/zip\r\n\r\n");
        body.extend_from_slice(file_bytes);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
    body
}

async fn setup_upload_app() -> Option<(axum::Router, DatabaseManager, TempDir)> {
    let tmp = TempDir::new().ok()?;
    let mut config = HivemindConfig::default();
    config.torrent.api_dir = tmp.path().join("api").display().to_string();
    config.torrent.bt_dir = tmp.path().join("bt").display().to_string();

    let (app, db) = setup_app_with_config(config).await?;
    sqlx::query("DELETE FROM tasks WHERE task_id LIKE 'upload-test-%'")
        .execute(&db.pool)
        .await
        .ok();

    Some((app, db, tmp))
}

fn priced_task_payload(task_id: &str, max_cpt: Option<i64>) -> serde_json::Value {
    let mut payload = json!({
        "task_id": task_id,
        "torrent": "magnet:?xt=urn:btih:quote-test",
        "cpu_score": 250,
        "gpu_score": 100,
        "memory_gb": 8,
        "gpu_memory_gb": 4,
        "storage_gb": 100,
        "host_count": 2
    });
    if let Some(max_cpt) = max_cpt {
        payload["max_cpt"] = json!(max_cpt);
    }
    payload
}

#[tokio::test]
async fn test_health_endpoint() {
    let app = match setup_app().await {
        Some(app) => app,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_login_invalid_credentials() {
    let app = match setup_app().await {
        Some(app) => app,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "username": "nonexistent",
                        "password": "wrong"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    eprintln!("login_invalid: status={}, body={}", status, body_str);
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "Expected 401 but got {}, body: {}",
        status,
        body_str
    );
}

#[tokio::test]
async fn test_login_valid_credentials() {
    let app = match setup_app().await {
        Some(app) => app,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "username": "testuser",
                        "password": "testpass123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    eprintln!("login_valid: status={}, body={}", status, body_str);

    assert_eq!(
        status,
        StatusCode::OK,
        "Expected 200 but got {}, body: {}",
        status,
        body_str
    );

    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert!(json["token"].is_string(), "Should return a JWT token");
}

#[tokio::test]
async fn test_tasks_unauthorized() {
    let app = match setup_app().await {
        Some(app) => app,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_tasks_with_valid_token() {
    let app = match setup_app().await {
        Some(app) => app,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };

    // First login to get token
    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "username": "testuser",
                        "password": "testpass123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let login_status = login_response.status();
    let body = axum::body::to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    eprintln!(
        "login_for_tasks: status={}, body={}",
        login_status, body_str
    );

    if login_status != StatusCode::OK {
        panic!("Login failed with status {}: {}", login_status, body_str);
    }

    let login_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = login_json["token"]
        .as_str()
        .expect("token should be a string");

    // Now list tasks
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tasks")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["tasks"].is_array(), "Should return tasks array");
}

#[tokio::test]
async fn test_task_quote_endpoint_returns_deterministic_quote_and_breakdown() {
    let (app, _db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks/quote")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&priced_task_payload("quote-test-task", None)).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["success"], true);
    assert_eq!(json["quoted_cpt"], 104);
    assert_eq!(json["currency"], "CPT");
    assert_eq!(json["breakdown"]["base"], 10);
    assert_eq!(json["breakdown"]["cpu"], 2);
    assert_eq!(json["breakdown"]["gpu"], 2);
    assert_eq!(json["breakdown"]["memory"], 16);
    assert_eq!(json["breakdown"]["gpu_memory"], 12);
    assert_eq!(json["breakdown"]["storage"], 10);
    assert_eq!(json["breakdown"]["host_count"], 2);
}

#[tokio::test]
async fn test_create_task_rejects_max_cpt_below_quote() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let task_id = "quote-budget-too-low";
    sqlx::query("DELETE FROM tasks WHERE task_id = $1")
        .bind(task_id)
        .execute(&db.pool)
        .await
        .ok();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&priced_task_payload(task_id, Some(103))).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        status,
        StatusCode::PAYMENT_REQUIRED,
        "response body: {}",
        json
    );
    assert_eq!(json["success"], false);
    assert!(json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("quote exceeds max_cpt"));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE task_id = $1")
        .bind(task_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_create_task_stores_quoted_value_instead_of_oversized_max_cpt() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let task_id = "quote-budget-oversized";
    sqlx::query("DELETE FROM tasks WHERE task_id = $1")
        .bind(task_id)
        .execute(&db.pool)
        .await
        .ok();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&priced_task_payload(task_id, Some(9999))).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::CREATED, "response body: {}", json);

    let stored_max_cpt: i64 = sqlx::query_scalar("SELECT max_cpt FROM tasks WHERE task_id = $1")
        .bind(task_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(stored_max_cpt, 104);

    sqlx::query("DELETE FROM tasks WHERE task_id = $1")
        .bind(task_id)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_create_task_rate_limit_rejects_second_submit_within_one_minute() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let task_one = format!("rate-limit-task-1-{}", uuid::Uuid::new_v4());
    let task_two = format!("rate-limit-task-2-{}", uuid::Uuid::new_v4());

    sqlx::query("DELETE FROM tasks WHERE task_id IN ($1, $2)")
        .bind(&task_one)
        .bind(&task_two)
        .execute(&db.pool)
        .await
        .ok();

    unsafe {
        std::env::set_var("HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE", "1");
    }

    let req_one = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks")
                .header("authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "task_id": task_one,
                        "torrent": "magnet:?xt=urn:btih:rate-limit-a",
                        "memory_gb": 1,
                        "cpu_score": 100,
                        "storage_gb": 1,
                        "max_cpt": 100
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(req_one.status(), StatusCode::CREATED);

    let req_two = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks")
                .header("authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "task_id": task_two,
                        "torrent": "magnet:?xt=urn:btih:rate-limit-b",
                        "memory_gb": 1,
                        "cpu_score": 100,
                        "storage_gb": 1,
                        "max_cpt": 100
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(req_two.status(), StatusCode::TOO_MANY_REQUESTS);

    unsafe {
        std::env::remove_var("HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE");
    }

    sqlx::query("DELETE FROM tasks WHERE task_id IN ($1, $2)")
        .bind(&task_one)
        .bind(&task_two)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_provider_earnings_lists_only_own_provider_credit_with_total_and_limit() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let unique = uuid::Uuid::new_v4().to_string();
    let task_a = format!("earnings-a-{unique}");
    let task_b = format!("earnings-b-{unique}");
    let task_own_debit = format!("earnings-debit-{unique}");
    let task_other = format!("earnings-other-{unique}");
    let id_prefix = format!("earnings-{unique}");

    sqlx::query(
        "DELETE FROM ledger_entries
         WHERE provider_user = 'testuser' OR payer_user = 'testuser' OR task_id LIKE 'earnings-%'",
    )
    .execute(&db.pool)
    .await
    .ok();

    sqlx::query(
        "INSERT INTO ledger_entries (
            task_id, payer_user, provider_worker_id, provider_user, kind,
            amount_cpt, currency, status, idempotency_key, created_at
         )
         VALUES
            ($1, 'payer-a', 'worker-a', 'testuser', 'provider_credit', 25, 'CPT', 'settled', $2, NOW() - INTERVAL '2 minutes'),
            ($3, 'payer-b', 'worker-b', 'testuser', 'provider_credit', 40, 'CPT', 'settled', $4, NOW() - INTERVAL '1 minute'),
            ($5, 'testuser', 'worker-own-debit', 'testuser', 'payer_debit', 900, 'CPT', 'settled', $6, NOW()),
            ($7, 'payer-other', 'worker-other', 'someone-else', 'provider_credit', 700, 'CPT', 'settled', $8, NOW())",
    )
    .bind(&task_a)
    .bind(format!("{id_prefix}-a"))
    .bind(&task_b)
    .bind(format!("{id_prefix}-b"))
    .bind(&task_own_debit)
    .bind(format!("{id_prefix}-own-debit"))
    .bind(&task_other)
    .bind(format!("{id_prefix}-other"))
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/provider/earnings?limit=1")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["success"], true);
    assert_eq!(json["currency"], "CPT");
    assert_eq!(json["total_earned_cpt"], 65);

    let entries = json["entries"].as_array().expect("entries should be array");
    assert_eq!(entries.len(), 1, "limit should apply to entries: {}", json);
    assert_eq!(entries[0]["task_id"], task_b);
    assert_eq!(entries[0]["payer_user"], "payer-b");
    assert_eq!(entries[0]["provider_worker_id"], "worker-b");
    assert_eq!(entries[0]["amount_cpt"], 40);
    assert_eq!(entries[0]["status"], "settled");
    assert!(
        entries[0]["created_at"].as_str().is_some(),
        "created_at should be serialized"
    );

    sqlx::query(
        "DELETE FROM ledger_entries
         WHERE provider_user = 'testuser' OR payer_user = 'testuser' OR idempotency_key LIKE $1",
    )
    .bind(format!("{id_prefix}%"))
    .execute(&db.pool)
    .await
    .ok();
}

#[tokio::test]
async fn test_admin_billing_overview_returns_settled_totals_and_pending_count() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;

    let task_done = format!("admin-billing-done-{}", uuid::Uuid::new_v4());
    let task_pending = format!("admin-billing-pending-{}", uuid::Uuid::new_v4());
    let debit_key = format!("{}:payer_debit", task_done);
    let provider_key = format!("{}:provider_credit", task_done);
    let fee_key = format!("{}:platform_fee", task_done);

    sqlx::query(
        "INSERT INTO tasks (task_id, owner, status, billing_settled, max_cpt)
         VALUES ($1, 'testuser', 'COMPLETED', false, 100)",
    )
    .bind(&task_pending)
    .execute(&db.pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO ledger_entries (
            task_id, payer_user, provider_worker_id, provider_user, kind,
            amount_cpt, currency, status, idempotency_key
        )
         VALUES
            ($1, 'testuser', 'w1', 'provider-a', 'payer_debit', 100, 'CPT', 'settled', $2),
            ($1, 'testuser', 'w1', 'provider-a', 'provider_credit', 90, 'CPT', 'settled', $3),
            ($1, 'testuser', 'w1', 'provider-a', 'platform_fee', 10, 'CPT', 'settled', $4)",
    )
    .bind(&task_done)
    .bind(&debit_key)
    .bind(&provider_key)
    .bind(&fee_key)
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/billing/overview")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["total_payer_debit_cpt"], 100);
    assert_eq!(json["total_provider_credit_cpt"], 90);
    assert_eq!(json["total_platform_fee_cpt"], 10);
    assert_eq!(json["pending_billing_tasks"], 1);

    sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
        .bind(&task_done)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM tasks WHERE task_id IN ($1, $2)")
        .bind(&task_done)
        .bind(&task_pending)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_provider_worker_trust_profile_returns_reputation() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let worker_id = format!("trust-worker-{}", uuid::Uuid::new_v4());

    sqlx::query(
        "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, status, available_memory_gb, queue_capacity)
         VALUES ($1, 'testuser', '10.0.0.9', 4, 16, 200, 0, 0, '', 'IDLE', 16, 2)",
    )
    .bind(&worker_id)
    .execute(&db.pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
         VALUES ($1, 11, 2, 145, false)",
    )
    .bind(&worker_id)
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/provider/workers/{}/trust", worker_id))
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["trust"]["worker_id"], worker_id);
    assert_eq!(json["trust"]["successful_tasks"], 11);
    assert_eq!(json["trust"]["failed_tasks"], 2);
    assert_eq!(json["trust"]["score"], 145);
    assert_eq!(json["trust"]["banned"], false);

    sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_worker_trust_control_updates_ban_and_score() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let worker_id = format!("trust-control-worker-{}", uuid::Uuid::new_v4());

    sqlx::query(
        "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, status, available_memory_gb, queue_capacity)
         VALUES ($1, 'testuser', '10.0.0.9', 4, 16, 200, 0, 0, '', 'IDLE', 16, 2)",
    )
    .bind(&worker_id)
    .execute(&db.pool)
    .await
    .unwrap();

    let control = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/admin/workers/{}/trust-control", worker_id))
                .header("authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from("{\"banned\":true,\"score\":15}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(control.status(), StatusCode::OK);

    let trust = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/provider/workers/{}/trust", worker_id))
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(trust.status(), StatusCode::OK);
    let body = axum::body::to_bytes(trust.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["trust"]["banned"], true);
    assert_eq!(json["trust"]["score"], 15);

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_audit_logs
         WHERE action = 'worker_trust_control'
           AND target_type = 'worker'
           AND target_id = $1",
    )
    .bind(&worker_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(audit_count >= 1);

    sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM admin_audit_logs WHERE target_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_audit_logs_endpoint_returns_recent_actions() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let worker_id = format!("audit-worker-{}", uuid::Uuid::new_v4());

    sqlx::query(
        "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, status, available_memory_gb, queue_capacity)
         VALUES ($1, 'testuser', '10.0.0.19', 4, 16, 200, 0, 0, '', 'IDLE', 16, 2)",
    )
    .bind(&worker_id)
    .execute(&db.pool)
    .await
    .unwrap();

    let control = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/admin/workers/{}/trust-control", worker_id))
                .header("authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from("{\"banned\":false,\"score\":123}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(control.status(), StatusCode::OK);

    let logs = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/audit/logs?limit=20")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs.status(), StatusCode::OK);
    let body = axum::body::to_bytes(logs.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    let entries = json["entries"].as_array().cloned().unwrap_or_default();
    assert!(entries.iter().any(|e| {
        e["action"] == "worker_trust_control"
            && e["target_type"] == "worker"
            && e["target_id"] == worker_id
            && e["detail"]["score"] == 123
    }));

    sqlx::query("DELETE FROM admin_audit_logs WHERE target_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_worker_trust_list_returns_joined_worker_and_reputation() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let worker_a = format!("trust-list-a-{}", uuid::Uuid::new_v4());
    let worker_b = format!("trust-list-b-{}", uuid::Uuid::new_v4());

    sqlx::query(
        "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, status, available_memory_gb, queue_capacity)
         VALUES
            ($1, 'testuser', '10.0.0.11', 4, 16, 200, 0, 0, '', 'IDLE', 16, 2),
            ($2, 'testuser', '10.0.0.12', 4, 16, 200, 0, 0, '', 'ACTIVE', 16, 2)",
    )
    .bind(&worker_a)
    .bind(&worker_b)
    .execute(&db.pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
         VALUES
            ($1, 3, 1, 20, true),
            ($2, 10, 0, 210, false)",
    )
    .bind(&worker_a)
    .bind(&worker_b)
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/workers/trust")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);

    let entries = json["entries"].as_array().cloned().unwrap_or_default();
    assert!(entries.iter().any(|e| e["worker_id"] == worker_a && e["banned"] == true && e["score"] == 20));
    assert!(entries.iter().any(|e| e["worker_id"] == worker_b && e["banned"] == false && e["score"] == 210));

    sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
        .bind(&worker_a)
        .bind(&worker_b)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
        .bind(&worker_a)
        .bind(&worker_b)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_artifact_overview_returns_lifecycle_metrics() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    sqlx::query("DELETE FROM artifacts")
        .execute(&db.pool)
        .await
        .ok();
    let task_id = format!("artifact-overview-task-{}", uuid::Uuid::new_v4());
    let key1 = format!("artifact-1-{}", uuid::Uuid::new_v4());
    let key2 = format!("artifact-2-{}", uuid::Uuid::new_v4());

    sqlx::query(
        "INSERT INTO artifacts (task_id, artifact_key, checksum_sha1, size_bytes, storage_path, dedup_hit, resume_supported, expires_at)
         VALUES
            ($1, $2, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 1024, '/tmp/a1', true, true, NOW() + INTERVAL '2 hours'),
            ($1, $3, 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 2048, '/tmp/a2', false, false, NOW() + INTERVAL '30 hours')",
    )
    .bind(&task_id)
    .bind(&key1)
    .bind(&key2)
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/artifacts/overview")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["total_artifacts"], 2);
    assert_eq!(json["total_size_bytes"], 3072);
    assert_eq!(json["dedup_hits"], 1);
    assert_eq!(json["resumable_artifacts"], 1);
    assert_eq!(json["expiring_in_24h"], 1);

    sqlx::query("DELETE FROM artifacts WHERE task_id = $1")
        .bind(&task_id)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_artifact_cleanup_dry_run_reports_candidates_without_deleting() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let task_id = format!("artifact-cleanup-dry-{}", uuid::Uuid::new_v4());
    let key = format!("artifact-cleanup-dry-key-{}", uuid::Uuid::new_v4());

    sqlx::query(
        "INSERT INTO artifacts (task_id, artifact_key, checksum_sha1, size_bytes, storage_path, expires_at)
         VALUES ($1, $2, 'cccccccccccccccccccccccccccccccccccccccc', 64, '/tmp/missing-dry', NOW() - INTERVAL '1 hour')",
    )
    .bind(&task_id)
    .bind(&key)
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/artifacts/cleanup")
                .header("authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from("{\"dry_run\":true}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["dry_run"], true);
    assert!(json["expired_candidates"].as_i64().unwrap_or(0) >= 1);
    assert_eq!(json["deleted_rows"], 0);

    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM artifacts WHERE artifact_key = $1")
            .bind(&key)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(remaining, 1);

    sqlx::query("DELETE FROM artifacts WHERE artifact_key = $1")
        .bind(&key)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_artifact_cleanup_execute_deletes_expired_rows_and_file() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let task_id = format!("artifact-cleanup-exec-{}", uuid::Uuid::new_v4());
    let key = format!("artifact-cleanup-exec-key-{}", uuid::Uuid::new_v4());
    let tmp = tempfile::TempDir::new().unwrap();
    let artifact_path = tmp.path().join("expired.bin");
    std::fs::write(&artifact_path, b"expired").unwrap();

    sqlx::query(
        "INSERT INTO artifacts (task_id, artifact_key, checksum_sha1, size_bytes, storage_path, expires_at)
         VALUES ($1, $2, 'dddddddddddddddddddddddddddddddddddddddd', 7, $3, NOW() - INTERVAL '2 hours')",
    )
    .bind(&task_id)
    .bind(&key)
    .bind(artifact_path.display().to_string())
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/artifacts/cleanup")
                .header("authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from("{\"dry_run\":false}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["dry_run"], false);
    assert!(json["expired_candidates"].as_i64().unwrap_or(0) >= 1);
    assert!(json["deleted_rows"].as_i64().unwrap_or(0) >= 1);
    assert!(json["deleted_files"].as_i64().unwrap_or(0) >= 1);

    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM artifacts WHERE artifact_key = $1")
            .bind(&key)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    assert!(!artifact_path.exists());
}

#[tokio::test]
async fn test_admin_scheduling_cache_metrics_returns_totals_and_top_workers() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let task_a = format!("cache-metrics-a-{}", uuid::Uuid::new_v4());
    let task_b = format!("cache-metrics-b-{}", uuid::Uuid::new_v4());
    let task_c = format!("cache-metrics-c-{}", uuid::Uuid::new_v4());
    let worker_a = format!("cache-worker-a-{}", uuid::Uuid::new_v4());
    let worker_b = format!("cache-worker-b-{}", uuid::Uuid::new_v4());

    sqlx::query("DELETE FROM tasks WHERE task_id LIKE 'cache-metrics-%'")
        .execute(&db.pool)
        .await
        .ok();

    sqlx::query(
        "INSERT INTO tasks (
            task_id, owner, worker_id, worker_ip, status, torrent_source,
            req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
            host_count, max_cpt, billing_settled, billed_amount, max_retries,
            deterministic, side_effects, priority, cache_hits, created_at, last_update, completed_at
         ) VALUES
            ($1, 'testuser', $2, '127.0.0.1', 'COMPLETED', 'magnet:?xt=urn:btih:cm-a',
             100, 0, 4, 0, 10, 1, 1000, true, 1000, 3, false, false, 0, 3, NOW(), NOW(), NOW()),
            ($3, 'testuser', $2, '127.0.0.1', 'COMPLETED', 'magnet:?xt=urn:btih:cm-b',
             100, 0, 4, 0, 10, 1, 1000, true, 1000, 3, false, false, 0, 2, NOW(), NOW(), NOW()),
            ($4, 'testuser', $5, '127.0.0.1', 'COMPLETED', 'magnet:?xt=urn:btih:cm-c',
             100, 0, 4, 0, 10, 1, 1000, true, 1000, 3, false, false, 0, 1, NOW(), NOW(), NOW())",
    )
    .bind(&task_a)
    .bind(&worker_a)
    .bind(&task_b)
    .bind(&task_c)
    .bind(&worker_b)
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/scheduling/cache-metrics")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["total_completed_tasks"], 3);
    assert_eq!(json["total_cache_hits"], 6);
    assert_eq!(json["cache_hit_rate"], 2.0);
    assert_eq!(json["top_workers"][0]["worker_id"], worker_a);
    assert_eq!(json["top_workers"][0]["completed_tasks"], 2);
    assert_eq!(json["top_workers"][0]["cache_hits"], 5);

    sqlx::query("DELETE FROM tasks WHERE task_id IN ($1, $2, $3)")
        .bind(&task_a)
        .bind(&task_b)
        .bind(&task_c)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_scheduling_cache_alert_returns_normal_within_threshold() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let task_id = format!("cache-alert-normal-{}", uuid::Uuid::new_v4());

    sqlx::query("DELETE FROM tasks WHERE task_id LIKE 'cache-alert-%'")
        .execute(&db.pool)
        .await
        .ok();

    sqlx::query(
        "INSERT INTO tasks (
            task_id, owner, worker_id, worker_ip, status, torrent_source,
            req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
            host_count, max_cpt, billing_settled, billed_amount, max_retries,
            deterministic, side_effects, priority, cache_hits, created_at, last_update, completed_at
         ) VALUES (
            $1, 'testuser', 'cache-alert-worker', '127.0.0.1', 'COMPLETED', 'magnet:?xt=urn:btih:ca',
            100, 0, 4, 0, 10, 1, 1000, true, 1000, 3, false, false, 0, 1, NOW(), NOW(), NOW()
         )",
    )
    .bind(&task_id)
    .execute(&db.pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/scheduling/cache-alert?low=0.5&high=2.0")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["severity"], "normal");
    assert_eq!(json["cache_hit_rate"], 1.0);

    sqlx::query("DELETE FROM tasks WHERE task_id = $1")
        .bind(&task_id)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_scheduling_cache_alert_rejects_invalid_thresholds() {
    let app = match setup_app().await {
        Some(app) => app,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/scheduling/cache-alert?low=2.0&high=1.0")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], false);
    assert_eq!(json["severity"], "invalid_threshold");
}

#[tokio::test]
async fn test_admin_scheduling_cache_alert_persists_low_anomaly_and_list_endpoint_returns_it() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let task_id = format!("cache-alert-anomaly-{}", uuid::Uuid::new_v4());

    sqlx::query("DELETE FROM tasks WHERE task_id LIKE 'cache-alert-anomaly-%'")
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM cache_alert_anomalies")
        .execute(&db.pool)
        .await
        .ok();

    sqlx::query(
        "INSERT INTO tasks (
            task_id, owner, worker_id, worker_ip, status, torrent_source,
            deterministic, side_effects, priority, cache_hits, created_at, last_update, completed_at
         )
         VALUES (
            $1, 'testuser', 'cache-anomaly-worker', '127.0.0.1', 'COMPLETED', 'magnet:?xt=urn:btih:ca2',
            true, false, 0, 0, NOW() - INTERVAL '10 minutes', NOW() - INTERVAL '5 minutes', NOW() - INTERVAL '2 minutes'
         )",
    )
    .bind(&task_id)
    .execute(&db.pool)
    .await
    .unwrap();

    let alert_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/scheduling/cache-alert?low=0.5&high=2.0")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(alert_response.status(), StatusCode::OK);
    let alert_body = axum::body::to_bytes(alert_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let alert_json: serde_json::Value = serde_json::from_slice(&alert_body).unwrap();
    assert_eq!(alert_json["severity"], "low");

    let list_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/scheduling/cache-anomalies?limit=10")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = axum::body::to_bytes(list_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_json: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_json["success"], true);
    let entries = list_json["entries"].as_array().cloned().unwrap_or_default();
    assert!(entries.iter().any(|e| e["severity"] == "low"));

    sqlx::query("DELETE FROM tasks WHERE task_id = $1")
        .bind(&task_id)
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM cache_alert_anomalies")
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_admin_endpoints_reject_non_admin_user() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };

    let non_admin = format!("non-admin-{}", uuid::Uuid::new_v4());
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'testuser'")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, $2, 100)")
        .bind(&non_admin)
        .bind(&hash)
        .execute(&db.pool)
        .await
        .unwrap();

    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "username": non_admin,
                        "password": "testpass123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = login_json["token"].as_str().unwrap().to_string();

    let read_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/scheduling/cache-metrics")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read_response.status(), StatusCode::FORBIDDEN);

    let audit_read_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/audit/logs")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(audit_read_response.status(), StatusCode::FORBIDDEN);

    let anomaly_read_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/scheduling/cache-anomalies")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anomaly_read_response.status(), StatusCode::FORBIDDEN);

    let worker_id = format!("non-admin-worker-{}", uuid::Uuid::new_v4());
    let write_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/admin/workers/{}/trust-control", worker_id))
                .header("authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from("{\"banned\":true,\"score\":10}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(write_response.status(), StatusCode::FORBIDDEN);

    let rep_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(rep_count, 0);

    sqlx::query("DELETE FROM users WHERE username = $1")
        .bind(&non_admin)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_register_worker_rejects_username_that_does_not_match_token_subject() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;

    sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ('someone-else', 'testuser')")
        .execute(&db.pool)
        .await
        .ok();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/register-worker")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "username": "someone-else",
                        "ip": "127.0.0.1:50100",
                        "cpu_cores": 4,
                        "memory_gb": 16,
                        "cpu_score": 100,
                        "gpu_score": 0,
                        "gpu_memory_gb": 0,
                        "location": "local"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let someone_else_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM worker_nodes WHERE username = $1")
            .bind("someone-else")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(someone_else_count, 0);
}

#[tokio::test]
async fn test_register_worker_uses_token_subject_as_worker_owner() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;

    sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'testuser'")
        .execute(&db.pool)
        .await
        .ok();

    for username in [Some(json!("testuser")), Some(json!("")), None] {
        let mut payload = json!({
            "ip": "127.0.0.1:50101",
            "cpu_cores": 4,
            "memory_gb": 16,
            "cpu_score": 100,
            "gpu_score": 0,
            "gpu_memory_gb": 0,
            "location": "local"
        });
        if let Some(username) = username {
            payload["username"] = username;
        }

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/register-worker")
                    .header("Authorization", format!("Bearer {}", token))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK, "payload: {}", payload);

        let username: String =
            sqlx::query_scalar("SELECT username FROM worker_nodes WHERE worker_id = $1")
                .bind("testuser")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(username, "testuser");
    }

    sqlx::query("DELETE FROM worker_nodes WHERE worker_id = 'testuser'")
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_provider_can_update_and_read_own_worker_settings() {
    let (app, db) = match setup_app_with_config(HivemindConfig::default()).await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;

    sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ('testuser', 'settings-other')")
        .execute(&db.pool)
        .await
        .ok();

    let register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/register-worker")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "ip": "127.0.0.1:50102",
                        "cpu_cores": 8,
                        "memory_gb": 32,
                        "cpu_score": 300,
                        "gpu_score": 250,
                        "gpu_memory_gb": 12,
                        "location": "local"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(register_response.status(), StatusCode::OK);

    let update_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/provider/workers/testuser/settings")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "enabled": true,
                        "cpu_cores_limit": 6,
                        "memory_gb_limit": 24,
                        "gpu_memory_gb_limit": 8,
                        "storage_gb_limit": 200,
                        "min_cpt_per_hour": 75
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = update_response.status();
    let body = axum::body::to_bytes(update_response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["worker_id"], "testuser");
    assert_eq!(json["settings"]["enabled"], true);
    assert_eq!(json["settings"]["cpu_cores_limit"], 6);
    assert_eq!(json["settings"]["memory_gb_limit"], 24);
    assert_eq!(json["settings"]["gpu_memory_gb_limit"], 8);
    assert_eq!(json["settings"]["storage_gb_limit"], 200);
    assert_eq!(json["settings"]["min_cpt_per_hour"], 75);

    let get_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/provider/workers/testuser/settings")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["settings"]["cpu_cores_limit"], 6);
    assert_eq!(json["settings"]["min_cpt_per_hour"], 75);

    let listed_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/workers?include_offline=true")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(listed_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let worker = json["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["worker_id"] == "testuser")
        .expect("registered worker should be listed");
    assert_eq!(worker["cpu_cores_limit"], 6);
    assert_eq!(worker["min_cpt_per_hour"], 75);

    sqlx::query(
        "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb, cpu_score)
         VALUES ('settings-other', 'someone-else', '127.0.0.1:50103', 4, 8, 100)",
    )
    .execute(&db.pool)
    .await
    .unwrap();

    let forbidden_response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/provider/workers/settings-other/settings")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "enabled": true,
                        "cpu_cores_limit": 2,
                        "memory_gb_limit": 4,
                        "gpu_memory_gb_limit": 0,
                        "storage_gb_limit": 0,
                        "min_cpt_per_hour": 1
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_response.status(), StatusCode::FORBIDDEN);

    sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ('testuser', 'settings-other')")
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_authenticated_multipart_upload_creates_task_with_magnet_source() {
    let (app, db, tmp) = match setup_upload_app().await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let boundary = "upload-boundary";
    let task_id = "upload-test-success";
    let body = multipart_upload_body(
        boundary,
        task_id,
        Some("browser-task.zip"),
        Some(b"fake-task-data-for-browser-upload"),
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks/upload")
                .header("Authorization", format!("Bearer {}", token))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={}", boundary),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::CREATED, "response body: {}", json);
    assert_eq!(json["success"], true);
    assert_eq!(json["task"]["task_id"], task_id);

    let (torrent_source, expected_btih): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT torrent_source, expected_btih FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(torrent_source
        .as_deref()
        .unwrap_or_default()
        .starts_with("magnet:?xt=urn:btih:"));
    assert!(expected_btih.is_some());
    assert!(tmp
        .path()
        .join("api")
        .join("upload-test-success.zip")
        .exists());

    sqlx::query("DELETE FROM tasks WHERE task_id = $1")
        .bind(task_id)
        .execute(&db.pool)
        .await
        .ok();
}

#[tokio::test]
async fn test_multipart_upload_rejects_low_max_cpt_without_creating_task() {
    let (app, db, _tmp) = match setup_upload_app().await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let boundary = "upload-low-budget-boundary";
    let task_id = "upload-test-low-budget";
    let body = multipart_upload_body_with_fields(
        boundary,
        task_id,
        Some("browser-task.zip"),
        Some(b"fake-task-data-for-browser-upload"),
        &[
            ("cpu_score", "250"),
            ("gpu_score", "100"),
            ("memory_gb", "8"),
            ("gpu_memory_gb", "4"),
            ("storage_gb", "100"),
            ("host_count", "2"),
            ("max_cpt", "103"),
        ],
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks/upload")
                .header("Authorization", format!("Bearer {}", token))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={}", boundary),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        status,
        StatusCode::PAYMENT_REQUIRED,
        "response body: {}",
        json
    );
    assert!(json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("quote exceeds max_cpt"));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE task_id = $1")
        .bind(task_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_multipart_upload_missing_file_returns_400() {
    let (app, _db, _tmp) = match setup_upload_app().await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let boundary = "missing-file-boundary";
    let body = multipart_upload_body(boundary, "upload-test-missing-file", None, None);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks/upload")
                .header("Authorization", format!("Bearer {}", token))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={}", boundary),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_multipart_upload_non_zip_filename_returns_400() {
    let (app, _db, _tmp) = match setup_upload_app().await {
        Some(v) => v,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };
    let token = login_token(&app).await;
    let boundary = "non-zip-boundary";
    let body = multipart_upload_body(
        boundary,
        "upload-test-non-zip",
        Some("not-a-zip.txt"),
        Some(b"not zip"),
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks/upload")
                .header("Authorization", format!("Bearer {}", token))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={}", boundary),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_workers_include_offline_lists_offline_workers() {
    let app = match setup_app().await {
        Some(app) => app,
        None => {
            eprintln!("Skipping: setup failed");
            return;
        }
    };

    let config = hivemind_config::HivemindConfig::default();
    let db = match DatabaseManager::new(&config).await {
        Ok(db) => db,
        Err(_) => {
            eprintln!("Skipping: database unavailable");
            return;
        }
    };
    db.run_migrations().await.ok();
    sqlx::query("DELETE FROM worker_nodes WHERE worker_id LIKE 'api-worker-%'")
        .execute(&db.pool)
        .await
        .ok();
    sqlx::query(
        "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb,
         cpu_score, gpu_score, gpu_memory_gb, gpu_name, vram_mb,
         storage_total_gb, storage_available_gb, location, status)
         VALUES
         ('api-worker-active','test','127.0.0.1:50053',4,16,400,0,0,NULL,0,500,200,'local','IDLE'),
         ('api-worker-offline','test','127.0.0.2:50053',4,16,400,0,0,NULL,0,500,200,'local','OFFLINE')",
    )
    .execute(&db.pool)
    .await
    .unwrap();

    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "username": "testuser",
                        "password": "testpass123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = login_json["token"]
        .as_str()
        .expect("token should be a string");

    let online_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/workers")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(online_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(online_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let online_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let online_workers = online_json["workers"].as_array().unwrap();
    assert!(online_workers
        .iter()
        .any(|w| w["worker_id"] == "api-worker-active"));
    assert!(!online_workers
        .iter()
        .any(|w| w["worker_id"] == "api-worker-offline"));

    let all_response = app
        .oneshot(
            Request::builder()
                .uri("/api/workers?include_offline=1")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(all_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(all_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let all_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let all_workers = all_json["workers"].as_array().unwrap();
    assert!(all_workers
        .iter()
        .any(|w| w["worker_id"] == "api-worker-active"));
    assert!(all_workers
        .iter()
        .any(|w| w["worker_id"] == "api-worker-offline"));

    sqlx::query("DELETE FROM worker_nodes WHERE worker_id LIKE 'api-worker-%'")
        .execute(&db.pool)
        .await
        .ok();
}

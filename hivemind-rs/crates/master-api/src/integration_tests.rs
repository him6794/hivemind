//! Integration tests simulating end-to-end user flow.
//! These tests require a running PostgreSQL instance.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use serde_json::json;

use crate::handlers::AppState;
use crate::routes::create_router;
use hivemind_auth::AuthManager;
use hivemind_database::DatabaseManager;
use hivemind_task_scheduler::TaskScheduler;

async fn setup_app() -> Option<(axum::Router, DatabaseManager)> {
    let config = hivemind_config::HivemindConfig::default();
    let db = DatabaseManager::new(&config).await.ok()?;
    db.run_migrations().await.ok()?;

    let auth = AuthManager::new(&db, "integration-test-secret", 24);
    let scheduler = TaskScheduler::new(db.clone(), auth.clone());

    let state = AppState {
        db: db.clone(),
        auth,
        scheduler,
        nodepool_grpc_addr: "localhost:50051".into(),
    };

    Some((create_router(state), db))
}

async fn login(app: &axum::Router, username: &str, password: &str) -> Option<String> {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&json!({
                    "username": username,
                    "password": password
                })).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["token"].as_str().map(|s| s.to_string())
}

/// Simulates a complete user journey:
/// 1. Register (seeded default user)
/// 2. Login
/// 3. Check balance
/// 4. Upload task
/// 5. List tasks
/// 6. Get task result
/// 7. Stop task
#[tokio::test]
async fn test_full_user_journey() {
    let (app, db) = match setup_app().await {
        Some(v) => v,
        None => {
            eprintln!("Skipping integration test: no database available");
            return;
        }
    };

    // Step 1: Login with seeded user
    let token = login(&app, "testuser", "testpass123").await;
    assert!(token.is_some(), "Login should succeed for seeded user");
    let token = token.unwrap();

    // Step 2: Check balance
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/balance")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert!(json["balance"].as_i64().unwrap() > 0, "Should have positive balance");

    // Step 3: Upload a task
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&json!({
                    "task_id": "integration-task-1",
                    "torrent": "magnet:?xt=urn:btih:integration-test",
                    "memory_gb": 4,
                    "cpu_score": 100,
                })).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["task_id"], "integration-task-1");

    // Step 4: List tasks
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tasks")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let tasks = json["tasks"].as_array().unwrap();
    assert!(tasks.len() >= 1, "Should have at least 1 task");
    let found = tasks.iter().any(|t| t["task_id"] == "integration-task-1");
    assert!(found, "Should find integration-task-1 in list");

    // Step 5: Get task result
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tasks/integration-task-1/result")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["status"], "PENDING");

    // Step 6: Stop task
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks/integration-task-1/stop")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);

    // Step 7: Duplicate task should fail
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tasks")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&json!({
                    "task_id": "integration-task-1",
                    "torrent": "magnet:?xt=urn:btih:duplicate",
                })).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], false, "Duplicate task should be rejected");

    // Cleanup
    sqlx::query("DELETE FROM tasks WHERE owner = 'testuser' AND task_id LIKE 'integration-%'")
        .execute(&db.pool).await.ok();

    println!("=== Full user journey test PASSED ===");
}

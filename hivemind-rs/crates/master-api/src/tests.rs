use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use serde_json::json;

use crate::handlers::AppState;
use crate::routes::create_router;
use hivemind_auth::AuthManager;
use hivemind_database::DatabaseManager;
use hivemind_task_scheduler::TaskScheduler;
use hivemind_models::LoginRequest;

async fn setup_app() -> Option<axum::Router> {
    let config = hivemind_config::HivemindConfig::default();
    let db = DatabaseManager::new(&config).await.ok()?;
    db.run_migrations().await.ok()?;

    let auth = AuthManager::new(&db, "test-secret", 24);
    let scheduler = TaskScheduler::new(db.clone(), auth.clone());

    let state = AppState {
        db,
        auth,
        scheduler,
        nodepool_grpc_addr: "localhost:50051".into(),
    };

    Some(create_router(state))
}

#[tokio::test]
async fn test_health_endpoint() {
    let app = match setup_app().await {
        Some(app) => app,
        None => return,
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
        None => return,
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

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], false);
}

#[tokio::test]
async fn test_login_valid_credentials() {
    let app = match setup_app().await {
        Some(app) => app,
        None => return,
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

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["success"], true);
    assert!(json["token"].is_string(), "Should return a JWT token");
}

#[tokio::test]
async fn test_tasks_unauthorized() {
    let app = match setup_app().await {
        Some(app) => app,
        None => return,
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
        None => return,
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

    let body = axum::body::to_bytes(login_response.into_body(), usize::MAX).await.unwrap();
    let login_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = login_json["token"].as_str().unwrap();

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

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["tasks"].is_array(), "Should return tasks array");
}

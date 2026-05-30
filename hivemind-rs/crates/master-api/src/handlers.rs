use axum::{
    extract::State, http::StatusCode, Json,
};
use hivemind_auth::AuthManager;
use hivemind_database::DatabaseManager;
use hivemind_models::{Task, TaskStatus, TaskInfo};
use hivemind_task_scheduler::TaskScheduler;
use serde::{Deserialize, Serialize};

use crate::middleware::AuthClaims;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseManager,
    pub auth: AuthManager,
    pub scheduler: TaskScheduler,
    pub nodepool_grpc_addr: String,
}

// --- Request/Response types ---

#[derive(Debug, Deserialize)]
pub struct LoginBody {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub message: String,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskBody {
    pub task_id: String,
    pub torrent: Option<String>,
    pub memory_gb: Option<i32>,
    pub cpu_score: Option<i32>,
    pub gpu_score: Option<i32>,
    pub gpu_memory_gb: Option<i32>,
    pub storage_gb: Option<i64>,
    pub location: Option<String>,
    pub host_count: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub success: bool,
    pub message: String,
    pub task: Option<TaskInfo>,
}

#[derive(Debug, Serialize)]
pub struct TaskListResponse {
    pub success: bool,
    pub tasks: Vec<TaskInfo>,
}

/// POST /api/login
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> (StatusCode, Json<LoginResponse>) {
    match state.auth.authenticate(&body.username, &body.password).await {
        Ok(Some(token)) => (
            StatusCode::OK,
            Json(LoginResponse { success: true, message: "Login successful".into(), token: Some(token) }),
        ),
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse { success: false, message: "Invalid credentials".into(), token: None }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse { success: false, message: format!("Error: {}", e), token: None }),
        ),
    }
}

/// POST /api/tasks — Master submits a task to the nodepool
pub async fn create_task(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    Json(body): Json<CreateTaskBody>,
) -> (StatusCode, Json<TaskResponse>) {
    let task = Task {
        id: uuid::Uuid::new_v4(),
        task_id: body.task_id,
        owner: claims.sub.clone(),
        worker_id: None,
        worker_ip: None,
        status: TaskStatus::Pending,
        status_message: None,
        output: None,
        result_torrent: None,
        torrent_source: body.torrent,
        expected_btih: None,
        cpu_usage: 0.0, memory_usage: 0.0, gpu_usage: 0.0, gpu_memory_usage: 0.0,
        req_cpu_score: body.cpu_score.unwrap_or(0),
        req_gpu_score: body.gpu_score.unwrap_or(0),
        req_memory_gb: body.memory_gb.unwrap_or(0),
        req_gpu_memory_gb: body.gpu_memory_gb.unwrap_or(0),
        req_storage_gb: body.storage_gb.unwrap_or(0),
        host_count: body.host_count.unwrap_or(1),
        max_cpt: 0, billing_settled: false, billed_amount: 0,
        retry_count: 0, max_retries: 3, deadline: None,
        deterministic: false, side_effects: false, priority: 0,
        cpu_time_ms: 0, wall_time_ms: 0, peak_memory_mb: 0,
        download_bytes: 0, cache_hits: 0,
        created_at: chrono::Utc::now(), last_update: chrono::Utc::now(),
        completed_at: None,
    };

    match state.scheduler.create_task(&task).await {
        Ok(t) => {
            tracing::info!("Master submitted task {} for user {}", t.task_id, claims.sub);
            (StatusCode::CREATED, Json(TaskResponse { success: true, message: format!("Task {} created", t.task_id), task: Some(t.into()) }))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(TaskResponse { success: false, message: format!("Failed: {}", e), task: None }),
        ),
    }
}

/// GET /api/tasks — List all tasks for the authenticated user
pub async fn list_tasks(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
) -> (StatusCode, Json<TaskListResponse>) {
    match state.scheduler.list_user_tasks(&claims.sub).await {
        Ok(tasks) => {
            let infos: Vec<TaskInfo> = tasks.into_iter().map(TaskInfo::from).collect();
            (StatusCode::OK, Json(TaskListResponse { success: true, tasks: infos }))
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(TaskListResponse { success: false, tasks: vec![] })),
    }
}

/// GET /api/tasks/:task_id/log — Get task execution log
pub async fn get_task_log(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.scheduler.get_task(&task_id).await {
        Ok(Some(task)) => {
            if task.owner != claims.sub {
                return (StatusCode::FORBIDDEN, Json(serde_json::json!({"success": false, "message": "Not authorized"})));
            }
            (StatusCode::OK, Json(serde_json::json!({
                "success": true, "task_id": task.task_id,
                "status": task.status.as_str(), "output": task.output,
                "status_message": task.status_message,
                "cpu_usage": task.cpu_usage, "memory_usage": task.memory_usage,
                "gpu_usage": task.gpu_usage,
                "wall_time_ms": task.wall_time_ms, "peak_memory_mb": task.peak_memory_mb,
            })))
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"success": false, "message": "Task not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success": false, "message": format!("Error: {}", e)}))),
    }
}

/// POST /api/tasks/:task_id/stop — Stop a running task
pub async fn stop_task(
    State(state): State<AppState>,
    AuthClaims(claims): AuthClaims,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.scheduler.get_task(&task_id).await {
        Ok(Some(task)) => {
            if task.owner != claims.sub {
                return (StatusCode::FORBIDDEN, Json(serde_json::json!({"success": false, "message": "Not authorized"})));
            }
            match task.status {
                TaskStatus::Pending | TaskStatus::Queued | TaskStatus::Assigned | TaskStatus::Running => {
                    match state.scheduler.cancel_task(&task_id).await {
                        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"success": true, "message": "Task stopped"}))),
                        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success": false, "message": format!("Failed: {}", e)}))),
                    }
                }
                _ => (StatusCode::CONFLICT, Json(serde_json::json!({"success": false, "message": format!("Already in terminal state: {}", task.status.as_str())}))),
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"success": false, "message": "Task not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success": false, "message": format!("Error: {}", e)}))),
    }
}

/// GET /health
pub async fn health_check() -> &'static str {
    "OK"
}

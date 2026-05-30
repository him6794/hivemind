use axum::{extract::State, http::StatusCode, Json};
use hivemind_models::{LoginRequest, LoginResponse, Task, TaskInfo, TaskStatus};

#[derive(Clone)]
pub struct AppState {
    pub db: hivemind_database::DatabaseManager,
    pub auth: hivemind_auth::AuthManager,
    pub scheduler: hivemind_task_scheduler::TaskScheduler,
    pub nodepool_grpc_addr: String,
}

#[derive(serde::Deserialize)]
pub struct LoginPayload {
    pub username: String,
    pub password: String,
}

pub async fn login_handler(
    State(state): State<AppState>,
    Json(payload): Json<LoginPayload>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let req = LoginRequest { username: payload.username, password: payload.password };
    state.auth.login(&req).await.map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(serde::Serialize)]
pub struct BalanceResponse {
    pub success: bool,
    pub balance: i64,
}

pub async fn get_balance_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<BalanceResponse>, StatusCode> {
    let token = extract_token(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = state.auth.validate_token(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let balance: Option<i64> = sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
        .bind(&claims.sub)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(BalanceResponse { success: true, balance: balance.unwrap_or(0) }))
}

#[derive(serde::Deserialize)]
pub struct UploadTaskPayload {
    pub task_id: Option<String>,
    pub torrent: String,
    pub memory_gb: Option<i32>,
    pub cpu_score: Option<i32>,
    pub gpu_score: Option<i32>,
    pub gpu_memory_gb: Option<i32>,
    pub location: Option<String>,
    pub host_count: Option<i32>,
    pub max_cpt: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct UploadTaskResponse {
    pub success: bool,
    pub task_id: String,
    pub status_message: String,
}

pub async fn upload_task_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<UploadTaskPayload>,
) -> Result<Json<UploadTaskResponse>, StatusCode> {
    let token = extract_token(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = state.auth.validate_token(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let task_id = payload.task_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Ok(Some(_)) = state.scheduler.get_task(&task_id).await {
        return Ok(Json(UploadTaskResponse { success: false, task_id, status_message: "Task already exists".into() }));
    }
    let task = Task {
        id: uuid::Uuid::new_v4(),
        task_id: task_id.clone(),
        owner: claims.sub.clone(),
        worker_id: None,
        worker_ip: None,
        status: TaskStatus::Pending,
        status_message: Some("Task created, waiting for scheduling".into()),
        output: None,
        result_torrent: None,
        torrent_source: Some(payload.torrent),
        expected_btih: None,
        cpu_usage: 0.0,
        memory_usage: 0.0,
        gpu_usage: 0.0,
        gpu_memory_usage: 0.0,
        req_cpu_score: payload.cpu_score.unwrap_or(100),
        req_gpu_score: payload.gpu_score.unwrap_or(0),
        req_memory_gb: payload.memory_gb.unwrap_or(4),
        req_gpu_memory_gb: payload.gpu_memory_gb.unwrap_or(0),
        host_count: payload.host_count.unwrap_or(1),
        max_cpt: payload.max_cpt.unwrap_or(1000),
        billing_settled: false,
        billed_amount: 0,
        retry_count: 0,
        max_retries: 3,
        deadline: None,
        deterministic: false,
        side_effects: false,
        priority: 0,
        cpu_time_ms: 0,
        wall_time_ms: 0,
        peak_memory_mb: 0,
        download_bytes: 0,
        cache_hits: 0,
        created_at: chrono::Utc::now(),
        last_update: chrono::Utc::now(),
        completed_at: None,
    };
    match state.scheduler.create_task(&task).await {
        Ok(_) => Ok(Json(UploadTaskResponse { success: true, task_id, status_message: "Task created successfully".into() })),
        Err(e) => Ok(Json(UploadTaskResponse { success: false, task_id, status_message: format!("Failed: {}", e) })),
    }
}

#[derive(serde::Serialize)]
pub struct TaskListResponse { pub tasks: Vec<TaskInfo> }

pub async fn list_tasks_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<TaskListResponse>, StatusCode> {
    let token = extract_token(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = state.auth.validate_token(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let tasks = state.scheduler.list_user_tasks(&claims.sub).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let task_infos: Vec<TaskInfo> = tasks.into_iter().map(TaskInfo::from).collect();
    Ok(Json(TaskListResponse { tasks: task_infos }))
}

#[derive(serde::Deserialize)]
pub struct TaskIdParam { pub task_id: String }

pub async fn get_task_result_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(params): axum::extract::Path<TaskIdParam>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let token = extract_token(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = state.auth.validate_token(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let task = state.scheduler.get_task(&params.task_id).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match task {
        Some(t) if t.owner == claims.sub => Ok(Json(serde_json::json!({
            "success": true, "status_message": t.status_message.unwrap_or_default(),
            "result_torrent": t.result_torrent, "status": t.status.as_str(),
        }))),
        Some(_) => Err(StatusCode::FORBIDDEN),
        None => Ok(Json(serde_json::json!({
            "success": false, "status_message": "Task not found", "result_torrent": null, "status": "UNKNOWN",
        }))),
    }
}

pub async fn stop_task_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(params): axum::extract::Path<TaskIdParam>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let token = extract_token(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = state.auth.validate_token(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let task = state.scheduler.get_task(&params.task_id).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match task {
        Some(t) if t.owner == claims.sub && t.status.is_active() => {
            state.scheduler.cancel_task(&params.task_id).await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(Json(serde_json::json!({"success": true, "status_message": "Task cancelled"})))
        }
        Some(t) if t.owner != claims.sub => Err(StatusCode::FORBIDDEN),
        _ => Ok(Json(serde_json::json!({"success": false, "status_message": "Task not found or already completed"}))),
    }
}

pub async fn health_handler() -> &'static str { "OK" }

fn extract_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let auth_header = headers.get(axum::http::header::AUTHORIZATION)?;
    auth_header.to_str().ok()?.strip_prefix("Bearer ")
}

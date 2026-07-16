use axum::{extract::State, http::StatusCode, Json};
use hivemind_config::HivemindConfig;
use serde::{Deserialize, Serialize};

use crate::grpc_client::GrpcClient;
use crate::middleware::AuthUser;

#[derive(Clone)]
pub struct AppState {
    pub jwt_secret: String,
    pub token_expiry_hours: i64,
    pub grpc_client: GrpcClient,
    pub config: HivemindConfig,
}

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
pub struct RegisterBody {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub success: bool,
    pub balance: i64,
}

#[derive(Debug, Deserialize)]
pub struct TransferBody {
    pub to_username: String,
    pub amount_cpt: i64,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TransferResponse {
    pub success: bool,
    pub message: String,
    pub from_balance: i64,
    pub to_balance: i64,
    pub transfer_id: String,
}

#[derive(Debug, Deserialize)]
pub struct VpnConfigBody {
    pub client_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VpnConfigResponse {
    pub success: bool,
    pub message: String,
    pub login_server: String,
    pub auth_key: String,
    pub virtual_ip: String,
    pub client_id: String,
    pub config_text: String,
    pub expires_at: String,
}

fn is_reserved_admin_username(username: &str) -> bool {
    std::env::var("HIVEMIND_ADMIN_USERS")
        .ok()
        .map(|users| {
            users
                .split(',')
                .map(str::trim)
                .filter(|configured| !configured.is_empty())
                .any(|configured| configured == username)
        })
        .unwrap_or(false)
}

pub async fn health_check() -> &'static str {
    "OK"
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> (StatusCode, Json<LoginResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.login(&body.username, &body.password).await {
        Ok(resp) if resp.success => (
            StatusCode::OK,
            Json(LoginResponse {
                success: true,
                message: "Login successful".into(),
                token: Some(resp.token),
            }),
        ),
        Ok(resp) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                message: resp.status_message,
                token: None,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse {
                success: false,
                message: format!("gRPC error: {}", e),
                token: None,
            }),
        ),
    }
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterBody>,
) -> (StatusCode, Json<RegisterResponse>) {
    let username = body.username.trim();
    if username.len() < 3 {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                success: false,
                message: "Username must be at least 3 characters".into(),
            }),
        );
    }
    if is_reserved_admin_username(username) {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                success: false,
                message: "Username is unavailable".into(),
            }),
        );
    }
    if body.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                success: false,
                message: "Password must be at least 8 characters".into(),
            }),
        );
    }

    let mut grpc = state.grpc_client.clone();
    match grpc.register_user(username, &body.password).await {
        Ok(resp) if resp.success => (
            StatusCode::CREATED,
            Json(RegisterResponse {
                success: true,
                message: resp.status_message,
            }),
        ),
        Ok(resp) => (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                success: false,
                message: resp.status_message,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RegisterResponse {
                success: false,
                message: format!("gRPC error: {}", e),
            }),
        ),
    }
}

pub async fn get_balance(
    State(state): State<AppState>,
    AuthUser { claims, token }: AuthUser,
) -> (StatusCode, Json<BalanceResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc.get_balance(&claims.sub, &token).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(BalanceResponse {
                success: resp.success,
                balance: resp.balance,
            }),
        ),
        Err(_e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(BalanceResponse {
                success: false,
                balance: 0,
            }),
        ),
    }
}

pub async fn transfer_cpt(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Json(body): Json<TransferBody>,
) -> (StatusCode, Json<TransferResponse>) {
    let mut grpc = state.grpc_client.clone();
    match grpc
        .transfer_cpt(
            &token,
            body.to_username.trim(),
            body.amount_cpt,
            body.idempotency_key.as_deref().unwrap_or(""),
        )
        .await
    {
        Ok(resp) => (
            if resp.success {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            },
            Json(TransferResponse {
                success: resp.success,
                message: resp.status_message,
                from_balance: resp.from_balance,
                to_balance: resp.to_balance,
                transfer_id: resp.transfer_id,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(TransferResponse {
                success: false,
                message: e.to_string(),
                from_balance: 0,
                to_balance: 0,
                transfer_id: String::new(),
            }),
        ),
    }
}

pub async fn issue_vpn_config(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Json(body): Json<VpnConfigBody>,
) -> (StatusCode, Json<VpnConfigResponse>) {
    let mut grpc = state.grpc_client.clone();
    let client_name = body.client_name.unwrap_or_default();
    match grpc.issue_user_vpn_config(&token, client_name.trim()).await {
        Ok(resp) => (
            if resp.success {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            },
            Json(VpnConfigResponse {
                success: resp.success,
                message: resp.status_message,
                login_server: resp.login_server,
                auth_key: resp.auth_key,
                virtual_ip: resp.virtual_ip,
                client_id: resp.client_id,
                config_text: resp.config_text,
                expires_at: resp.expires_at,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(VpnConfigResponse {
                success: false,
                message: e.to_string(),
                login_server: String::new(),
                auth_key: String::new(),
                virtual_ip: String::new(),
                client_id: String::new(),
                config_text: String::new(),
                expires_at: String::new(),
            }),
        ),
    }
}

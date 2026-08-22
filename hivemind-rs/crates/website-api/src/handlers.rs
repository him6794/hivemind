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

#[derive(Debug, Deserialize)]
pub struct EnrollmentCredentialBody {
    pub role: String,
    pub client_instance_id: String,
}

#[derive(Debug, Serialize)]
pub struct EnrollmentCredentialResponse {
    pub success: bool,
    pub message: String,
    pub credential: Option<String>,
    pub credential_id: Option<String>,
    pub owner: Option<String>,
    pub role: Option<String>,
    pub expires_at_unix: i64,
}

#[derive(Debug, Deserialize)]
pub struct RedeemEnrollmentCredentialBody {
    pub credential: String,
}

#[derive(Debug, Serialize)]
pub struct RedeemEnrollmentResponse {
    pub success: bool,
    pub message: String,
    pub credential_id: Option<String>,
    pub identity_id: Option<String>,
    pub owner: Option<String>,
    pub role: Option<String>,
    pub client_instance_id: Option<String>,
    pub worker_id: Option<String>,
    pub expires_at_unix: i64,
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

pub async fn issue_enrollment_credential(
    State(state): State<AppState>,
    AuthUser { token, .. }: AuthUser,
    Json(body): Json<EnrollmentCredentialBody>,
) -> (StatusCode, Json<EnrollmentCredentialResponse>) {
    let role = body.role.trim();
    let client_instance_id = body.client_instance_id.trim();
    if role.is_empty() || client_instance_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(EnrollmentCredentialResponse {
                success: false,
                message: "role and client_instance_id are required".into(),
                credential: None,
                credential_id: None,
                owner: None,
                role: None,
                expires_at_unix: 0,
            }),
        );
    }
    let mut grpc = state.grpc_client.clone();
    match grpc
        .issue_enrollment_credential(&token, role, client_instance_id)
        .await
    {
        Ok(resp) if resp.success => (
            StatusCode::OK,
            Json(EnrollmentCredentialResponse {
                success: true,
                message: resp.status_message,
                credential: (!resp.credential.is_empty()).then_some(resp.credential),
                credential_id: (!resp.credential_id.is_empty()).then_some(resp.credential_id),
                owner: (!resp.owner.is_empty()).then_some(resp.owner),
                role: (!resp.role.is_empty()).then_some(resp.role),
                expires_at_unix: resp.expires_at_unix,
            }),
        ),
        Ok(resp) => (
            StatusCode::BAD_REQUEST,
            Json(EnrollmentCredentialResponse {
                success: false,
                message: resp.status_message,
                credential: None,
                credential_id: None,
                owner: None,
                role: None,
                expires_at_unix: 0,
            }),
        ),
        Err(_error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(EnrollmentCredentialResponse {
                success: false,
                message: "enrollment credential service unavailable".into(),
                credential: None,
                credential_id: None,
                owner: None,
                role: None,
                expires_at_unix: 0,
            }),
        ),
    }
}

pub async fn redeem_enrollment_credential(
    State(state): State<AppState>,
    Json(body): Json<RedeemEnrollmentCredentialBody>,
) -> (StatusCode, Json<RedeemEnrollmentResponse>) {
    if body.credential.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RedeemEnrollmentResponse {
                success: false,
                message: "credential is required".into(),
                credential_id: None,
                identity_id: None,
                owner: None,
                role: None,
                client_instance_id: None,
                worker_id: None,
                expires_at_unix: 0,
            }),
        );
    }
    let mut grpc = state.grpc_client.clone();
    match grpc
        .redeem_enrollment_credential(body.credential.trim())
        .await
    {
        Ok(resp) if resp.success => (
            StatusCode::OK,
            Json(RedeemEnrollmentResponse {
                success: true,
                message: resp.status_message,
                credential_id: (!resp.credential_id.is_empty()).then_some(resp.credential_id),
                identity_id: (!resp.identity_id.is_empty()).then_some(resp.identity_id),
                owner: (!resp.owner.is_empty()).then_some(resp.owner),
                role: (!resp.role.is_empty()).then_some(resp.role),
                client_instance_id: (!resp.client_instance_id.is_empty())
                    .then_some(resp.client_instance_id),
                worker_id: (!resp.worker_id.is_empty()).then_some(resp.worker_id),
                expires_at_unix: resp.expires_at_unix,
            }),
        ),
        Ok(_resp) => (
            StatusCode::UNAUTHORIZED,
            Json(RedeemEnrollmentResponse {
                success: false,
                message: "invalid, expired, or already redeemed enrollment credential".into(),
                credential_id: None,
                identity_id: None,
                owner: None,
                role: None,
                client_instance_id: None,
                worker_id: None,
                expires_at_unix: 0,
            }),
        ),
        Err(_error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RedeemEnrollmentResponse {
                success: false,
                message: "enrollment credential service unavailable".into(),
                credential_id: None,
                identity_id: None,
                owner: None,
                role: None,
                client_instance_id: None,
                worker_id: None,
                expires_at_unix: 0,
            }),
        ),
    }
}

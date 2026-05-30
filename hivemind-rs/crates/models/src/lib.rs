use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// --- User Models ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub balance: i64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub username: String,
    pub balance: i64,
}

impl From<User> for UserInfo {
    fn from(u: User) -> Self {
        Self { id: u.id, username: u.username, balance: u.balance }
    }
}

// --- Worker/Node Models ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkerNode {
    pub id: Uuid,
    pub worker_id: String,
    pub username: String,
    pub ip: String,
    pub virtual_ip: Option<String>,
    pub hostname: Option<String>,
    pub cpu_cores: i32,
    pub memory_gb: i32,
    pub cpu_score: i32,
    pub gpu_score: i32,
    pub gpu_memory_gb: i32,
    pub location: String,
    pub status: WorkerStatus,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub gpu_memory_usage: f64,
    pub available_memory_gb: i32,
    pub queue_capacity: i32,
    pub last_heartbeat: DateTime<Utc>,
    pub registered_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum WorkerStatus {
    Active,
    Idle,
    Busy,
    Offline,
    Error,
}

impl WorkerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Idle => "IDLE",
            Self::Busy => "BUSY",
            Self::Offline => "OFFLINE",
            Self::Error => "ERROR",
        }
    }
}

impl std::str::FromStr for WorkerStatus {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_uppercase().as_str() {
            "ACTIVE" => Self::Active,
            "IDLE" => Self::Idle,
            "BUSY" => Self::Busy,
            "OFFLINE" => Self::Offline,
            _ => Self::Error,
        })
    }
}

impl sqlx::Type<sqlx::Postgres> for WorkerStatus {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("VARCHAR")
    }
    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for WorkerStatus {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
        Ok(s.parse().unwrap())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for WorkerStatus {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> sqlx::encode::IsNull {
        <String as sqlx::Encode<'q, sqlx::Postgres>>::encode(self.as_str().to_string(), buf)
    }
}

// --- Task Models ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Task {
    pub id: Uuid,
    pub task_id: String,
    pub owner: String,
    pub worker_id: Option<String>,
    pub worker_ip: Option<String>,
    pub status: TaskStatus,
    pub status_message: Option<String>,
    pub output: Option<String>,
    pub result_torrent: Option<String>,
    pub torrent_source: Option<String>,
    pub expected_btih: Option<String>,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub gpu_memory_usage: f64,
    pub req_cpu_score: i32,
    pub req_gpu_score: i32,
    pub req_memory_gb: i32,
    pub req_gpu_memory_gb: i32,
    pub host_count: i32,
    pub max_cpt: i64,
    pub billing_settled: bool,
    pub billed_amount: i64,
    pub retry_count: i32,
    pub max_retries: i32,
    pub deadline: Option<DateTime<Utc>>,
    pub deterministic: bool,
    pub side_effects: bool,
    pub priority: i32,
    pub cpu_time_ms: i64,
    pub wall_time_ms: i64,
    pub peak_memory_mb: i64,
    pub download_bytes: i64,
    pub cache_hits: i64,
    pub created_at: DateTime<Utc>,
    pub last_update: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum TaskStatus {
    Pending,
    Queued,
    Assigned,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Queued => "QUEUED",
            Self::Assigned => "ASSIGNED",
            Self::Running => "RUNNING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::TimedOut => "TIMED_OUT",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Pending | Self::Queued | Self::Assigned | Self::Running)
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_uppercase().as_str() {
            "PENDING" => Self::Pending,
            "QUEUED" => Self::Queued,
            "ASSIGNED" => Self::Assigned,
            "RUNNING" => Self::Running,
            "COMPLETED" => Self::Completed,
            "FAILED" => Self::Failed,
            "CANCELLED" => Self::Cancelled,
            "TIMED_OUT" => Self::TimedOut,
            _ => Self::Pending,
        })
    }
}

impl sqlx::Type<sqlx::Postgres> for TaskStatus {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("VARCHAR")
    }
    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for TaskStatus {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
        Ok(s.parse().unwrap())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for TaskStatus {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> sqlx::encode::IsNull {
        <String as sqlx::Encode<'q, sqlx::Postgres>>::encode(self.as_str().to_string(), buf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub task_id: String,
    pub status: String,
    pub status_message: String,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub gpu_memory_usage: f64,
    pub worker_ip: String,
}

impl From<Task> for TaskInfo {
    fn from(t: Task) -> Self {
        Self {
            task_id: t.task_id,
            status: t.status.as_str().into(),
            status_message: t.status_message.unwrap_or_default(),
            cpu_usage: t.cpu_usage,
            memory_usage: t.memory_usage,
            gpu_usage: t.gpu_usage,
            gpu_memory_usage: t.gpu_memory_usage,
            worker_ip: t.worker_ip.unwrap_or_default(),
        }
    }
}

// --- VPN Models ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct VpnPeer {
    pub id: Uuid,
    pub worker_id: String,
    pub hostname: String,
    pub virtual_ip: String,
    pub auth_key: String,
    pub online: bool,
    pub last_seen: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// --- DAG Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagIr {
    pub job_id: String,
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    pub task_id: String,
    pub artifact_inputs: Vec<String>,
    pub resource_requirements: ResourceRequirements,
    pub max_retries: i32,
    pub deadline_unix: i64,
    pub deterministic: bool,
    pub side_effects: bool,
    pub priority: i32,
    pub execution_package_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagEdge {
    pub from_task_id: String,
    pub to_task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    pub cpu_cores: i32,
    pub memory_gb: i32,
    pub gpu_score: i32,
    pub gpu_memory_gb: i32,
}

// --- Batch Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchLease {
    pub batch_id: String,
    pub worker_id: String,
    pub tasks: Vec<BatchTask>,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchTask {
    pub task_id: String,
    pub torrent: String,
    pub resource_limits: ResourceRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedTask {
    pub task_id: String,
    pub status: String,
    pub stdout_artifact_ref: Option<String>,
    pub stderr_artifact_ref: Option<String>,
    pub result_artifact_refs: Vec<String>,
    pub metrics: Option<ExecutionMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    pub cpu_time_ms: i64,
    pub wall_time_ms: i64,
    pub peak_memory_mb: i64,
    pub download_bytes: i64,
    pub cache_hits: i64,
}

// --- Auth / Token Models ---

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub user_id: String,
    pub exp: usize,
    pub iat: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub success: bool,
    pub status_message: String,
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

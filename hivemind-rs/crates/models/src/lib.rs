use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

type SqlxEncodeResult = Result<sqlx::encode::IsNull, sqlx::error::BoxDynError>;

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
        Self {
            id: u.id,
            username: u.username,
            balance: u.balance,
        }
    }
}

// --- Ledger Models ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LedgerEntry {
    pub id: Uuid,
    pub task_id: String,
    pub payer_user: String,
    pub provider_worker_id: Option<String>,
    pub provider_user: Option<String>,
    pub kind: LedgerEntryKind,
    pub amount_cpt: i64,
    pub currency: String,
    pub status: LedgerEntryStatus,
    pub idempotency_key: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerEntryKind {
    PayerDebit,
    ProviderCredit,
    PlatformFee,
    UserTransferDebit,
    UserTransferCredit,
}

impl LedgerEntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PayerDebit => "payer_debit",
            Self::ProviderCredit => "provider_credit",
            Self::PlatformFee => "platform_fee",
            Self::UserTransferDebit => "user_transfer_debit",
            Self::UserTransferCredit => "user_transfer_credit",
        }
    }
}

impl std::str::FromStr for LedgerEntryKind {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "provider_credit" => Self::ProviderCredit,
            "platform_fee" => Self::PlatformFee,
            "user_transfer_debit" => Self::UserTransferDebit,
            "user_transfer_credit" => Self::UserTransferCredit,
            _ => Self::PayerDebit,
        })
    }
}

impl sqlx::Type<sqlx::Postgres> for LedgerEntryKind {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("VARCHAR")
    }
    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for LedgerEntryKind {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
        Ok(s.parse().unwrap())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for LedgerEntryKind {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> SqlxEncodeResult {
        <String as sqlx::Encode<'q, sqlx::Postgres>>::encode(self.as_str().to_string(), buf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerEntryStatus {
    Settled,
}

impl LedgerEntryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Settled => "settled",
        }
    }
}

impl std::str::FromStr for LedgerEntryStatus {
    type Err = std::convert::Infallible;
    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        Ok(Self::Settled)
    }
}

impl sqlx::Type<sqlx::Postgres> for LedgerEntryStatus {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("VARCHAR")
    }
    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for LedgerEntryStatus {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
        Ok(s.parse().unwrap())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for LedgerEntryStatus {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> SqlxEncodeResult {
        <String as sqlx::Encode<'q, sqlx::Postgres>>::encode(self.as_str().to_string(), buf)
    }
}

// --- Resource Models: CPU / GPU / RAM / VRAM / Storage ---

/// Resource specification (total available on a worker, or required by a task)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    pub cpu_cores: i32,
    pub memory_mb: i64, // RAM in MB
    pub gpu_count: i32,
    pub gpu_name: String,
    pub vram_mb: i64, // VRAM in MB
    pub cpu_score: i32,
    pub gpu_score: i32,
    pub storage_total_gb: i64,
    pub storage_available_gb: i64,
}

/// Current resource usage (percentages)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub gpu_percent: f64,
    pub vram_percent: f64,
    pub storage_percent: f64,
}

/// Resource specification for DAG nodes / task scheduling (backward compat alias)
pub type ResourceRequirements = ResourceSpec;

// --- Worker/Node Models ---

pub const PUBLIC_DYNAMIC_ADMISSION_MODE: &str = "public_dynamic";
pub const PRIVATE_STATIC_ADMISSION_MODE: &str = "private_static";
pub const WORKER_CAPABILITY_REPORT_VERSION: u32 = 1;
pub const WORKER_CAPABILITY_REPORT_MAX_BYTES: usize = 16 * 1024;
pub const WORKER_CAPABILITY_MAX_ENTRIES: usize = 32;
pub const WORKER_CAPABILITY_MAX_USAGE_UNITS: u64 = 1_000_000;
pub const WORKER_CAPABILITY_MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
pub const WORKER_CAPABILITY_MAX_READINESS_REASON_BYTES: usize = 255;

/// A bounded, self-reported capability used only for public dynamic admission.
/// It carries no typed GPU identity, executable path, image trust, or host
/// command; those remain private/operator-owned capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerRuntimeCapability {
    pub runtime: String,
    #[serde(default)]
    pub backend_id: String,
    #[serde(default)]
    pub semantics_manifest_sha256: String,
    pub max_usage_units: u64,
    pub max_output_bytes: u64,
}

/// Versioned public capability/readiness observation sent by a Worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerCapabilityReport {
    pub protocol_version: u32,
    pub capabilities: Vec<WorkerRuntimeCapability>,
    pub ready: bool,
    #[serde(default)]
    pub readiness_reason: String,
}

impl WorkerCapabilityReport {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version != WORKER_CAPABILITY_REPORT_VERSION {
            return Err("worker capability report protocol version is unsupported".into());
        }
        if self.capabilities.len() > WORKER_CAPABILITY_MAX_ENTRIES {
            return Err("worker capability report has too many entries".into());
        }
        if self.readiness_reason.len() > WORKER_CAPABILITY_MAX_READINESS_REASON_BYTES {
            return Err("worker capability readiness reason is too long".into());
        }
        if self.ready && self.capabilities.is_empty() {
            return Err("ready worker capability report has no capabilities".into());
        }
        if !self.ready && self.readiness_reason.trim().is_empty() {
            return Err("not-ready worker capability report needs a reason".into());
        }
        let mut previous: Option<(&str, &str, &str)> = None;
        for capability in &self.capabilities {
            if capability.runtime != "managed-function-v0"
                && capability.runtime != "production_sandboxed_dsl"
            {
                return Err("worker capability report contains an unsupported runtime".into());
            }
            if capability.runtime == "managed-function-v0"
                && (!capability.backend_id.is_empty()
                    || !capability.semantics_manifest_sha256.is_empty())
            {
                return Err("managed-function-v0 capability identity is invalid".into());
            }
            if capability.runtime == "production_sandboxed_dsl"
                && (capability.backend_id.trim().is_empty()
                    || capability.semantics_manifest_sha256.trim().is_empty())
            {
                return Err("production DSL capability identity is incomplete".into());
            }
            if capability.runtime.len() > 64
                || capability.backend_id.len() > 255
                || capability.semantics_manifest_sha256.len() > 71
                || !(1..=WORKER_CAPABILITY_MAX_USAGE_UNITS).contains(&capability.max_usage_units)
                || !(1..=WORKER_CAPABILITY_MAX_OUTPUT_BYTES).contains(&capability.max_output_bytes)
            {
                return Err("worker capability report entry is outside its bounds".into());
            }
            let key = (
                capability.runtime.as_str(),
                capability.backend_id.as_str(),
                capability.semantics_manifest_sha256.as_str(),
            );
            if previous.is_some_and(|prior| prior >= key) {
                return Err("worker capability report is not canonical or has duplicates".into());
            }
            previous = Some(key);
        }
        Ok(())
    }

    pub fn validate_public_dynamic(&self) -> Result<(), String> {
        self.validate()?;
        if self
            .capabilities
            .iter()
            .any(|capability| capability.runtime != "managed-function-v0")
        {
            return Err(
                "public dynamic admission supports only managed-function-v0 capabilities".into(),
            );
        }
        Ok(())
    }

    pub fn capabilities_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(&self.capabilities)
            .map_err(|error| format!("serialize worker capabilities: {error}"))
    }

    /// The default public closed-DSL claim. It expresses only interpreter
    /// capability; Nodepool still controls task budget, liveness, reputation,
    /// and per-attempt proof authorization.
    pub fn public_managed_dsl() -> Self {
        Self {
            protocol_version: WORKER_CAPABILITY_REPORT_VERSION,
            capabilities: vec![WorkerRuntimeCapability {
                runtime: "managed-function-v0".into(),
                backend_id: String::new(),
                semantics_manifest_sha256: String::new(),
                max_usage_units: WORKER_CAPABILITY_MAX_USAGE_UNITS,
                max_output_bytes: WORKER_CAPABILITY_MAX_OUTPUT_BYTES,
            }],
            ready: true,
            readiness_reason: String::new(),
        }
    }
}

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
    pub gpu_name: Option<String>,
    pub vram_mb: i64,
    pub storage_total_gb: i64,
    pub storage_available_gb: i64,
    pub provider_enabled: bool,
    pub cpu_cores_limit: i32,
    pub memory_gb_limit: i32,
    pub gpu_memory_gb_limit: i32,
    pub storage_gb_limit: i64,
    pub min_cpt_per_hour: i64,
    pub location: String,
    pub status: WorkerStatus,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub gpu_memory_usage: f64,
    pub available_memory_gb: i32,
    pub queue_capacity: i32,
    /// Nodepool-persisted, operator-approved general-compute registration.
    /// It is never accepted from a worker registration request.
    pub general_compute_capabilities_json: Option<String>,
    /// Nodepool-persisted, operator-approved closed-DSL registration.
    /// It is never accepted from a worker registration request.
    pub managed_dsl_capabilities_json: Option<String>,
    /// Admission mode selected by Nodepool for this Worker record.
    pub admission_mode: String,
    /// Bounded public capability observation, kept separate from trusted
    /// operator snapshots and used only when admission_mode is public_dynamic.
    pub dynamic_capabilities_json: Option<String>,
    pub dynamic_capabilities_digest: Option<String>,
    pub dynamic_admission_ready: bool,
    pub dynamic_readiness_reason: Option<String>,
    pub dynamic_observed_at: Option<DateTime<Utc>>,
    pub last_heartbeat: DateTime<Utc>,
    pub registered_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkerNode {
    /// Derive a ResourceSpec from this worker node for scheduling
    pub fn to_resource_spec(&self) -> ResourceSpec {
        ResourceSpec {
            cpu_cores: self.cpu_cores,
            memory_mb: self.memory_gb as i64 * 1024,
            gpu_count: if self.gpu_score > 0 { 1 } else { 0 },
            gpu_name: self.gpu_name.clone().unwrap_or_default(),
            vram_mb: self.gpu_memory_gb as i64 * 1024,
            cpu_score: self.cpu_score,
            gpu_score: self.gpu_score,
            storage_total_gb: self.storage_total_gb,
            storage_available_gb: self.storage_available_gb,
        }
    }

    pub fn to_resource_usage(&self) -> ResourceUsage {
        ResourceUsage {
            cpu_percent: self.cpu_usage,
            memory_percent: self.memory_usage,
            gpu_percent: self.gpu_usage,
            vram_percent: self.gpu_memory_usage,
            storage_percent: 0.0,
        }
    }
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
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> SqlxEncodeResult {
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
    pub runtime: Option<String>,
    pub task_source: Option<String>,
    pub general_compute_manifest_json: Option<Vec<u8>>,
    pub managed_dsl_backend_id: Option<String>,
    pub managed_dsl_semantics_manifest_sha256: Option<String>,
    pub expected_btih: Option<String>,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub gpu_memory_usage: f64,
    pub req_cpu_score: i32,
    pub req_gpu_score: i32,
    pub req_memory_gb: i32,
    pub req_gpu_memory_gb: i32,
    pub req_storage_gb: i64,
    pub host_count: i32,
    pub max_cpt: i64,
    pub billing_settled: bool,
    pub billed_amount: i64,
    pub managed_executed_ops: i64,
    pub managed_output_bytes: i64,
    pub managed_receipt_json: Option<String>,
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

impl Task {
    /// Derive required ResourceSpec from task requirements
    pub fn to_resource_requirements(&self) -> ResourceSpec {
        ResourceSpec {
            cpu_cores: 1, // minimum
            memory_mb: self.req_memory_gb as i64 * 1024,
            gpu_count: if self.req_gpu_score > 0 { 1 } else { 0 },
            gpu_name: String::new(),
            vram_mb: self.req_gpu_memory_gb as i64 * 1024,
            cpu_score: self.req_cpu_score,
            gpu_score: self.req_gpu_score,
            storage_total_gb: self.req_storage_gb,
            storage_available_gb: 0,
        }
    }
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
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> SqlxEncodeResult {
        <String as sqlx::Encode<'q, sqlx::Postgres>>::encode(self.as_str().to_string(), buf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub task_id: String,
    pub owner: String,
    pub status: String,
    pub status_message: String,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub gpu_memory_usage: f64,
    pub worker_ip: String,
    pub retry_count: i32,
    pub wall_time_ms: i64,
    pub peak_memory_mb: i64,
    pub billed_amount: i64,
    pub billing_settled: bool,
    pub deterministic: bool,
    pub worker_id: String,
    pub provider_user: String,
    pub dispatch_status: String,
    pub usage_units: i64,
    pub max_cpt: i64,
}

/// Conversion from Task to TaskInfo
impl From<Task> for TaskInfo {
    fn from(t: Task) -> Self {
        let has_worker = t.worker_id.is_some();
        Self {
            task_id: t.task_id,
            owner: t.owner,
            status: t.status.as_str().into(),
            status_message: t.status_message.unwrap_or_default(),
            cpu_usage: t.cpu_usage,
            memory_usage: t.memory_usage,
            gpu_usage: t.gpu_usage,
            gpu_memory_usage: t.gpu_memory_usage,
            worker_ip: t.worker_ip.unwrap_or_default(),
            retry_count: t.retry_count,
            wall_time_ms: t.wall_time_ms,
            peak_memory_mb: t.peak_memory_mb,
            billed_amount: t.billed_amount,
            billing_settled: t.billing_settled,
            deterministic: t.deterministic,
            worker_id: t.worker_id.unwrap_or_default(),
            provider_user: String::new(),
            dispatch_status: if t.retry_count > 0 {
                "REDISPATCHED".into()
            } else if has_worker {
                "DISPATCHED".into()
            } else {
                "NOT_DISPATCHED".into()
            },
            usage_units: t.managed_executed_ops,
            max_cpt: t.max_cpt,
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
    pub resource_requirements: ResourceSpec,
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
    pub resource_limits: ResourceSpec,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub user_id: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
    pub exp: usize,
    pub iat: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LoginResponse {
    pub success: bool,
    pub status_message: String,
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub const SESSION_PROTOCOL_VERSION: u32 = 1;
pub const SESSION_TOKEN_MAX_BYTES: usize = 8 * 1024;
pub const SESSION_ID_MAX_BYTES: usize = 255;
pub const SESSION_CLIENT_INSTANCE_MAX_BYTES: usize = 128;
pub const SESSION_CAPABILITY_REPORT_MAX_BYTES: usize = 64 * 1024;
pub const SESSION_RESUME_TOKEN_MAX_BYTES: usize = 256;
pub const SESSION_FRAME_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_RESUME_TTL_SECS: u64 = 4 * 60;
pub const DEFAULT_SESSION_HEARTBEAT_TIMEOUT_SECS: u64 = 3 * 60;
pub const DEFAULT_MAX_PENDING_DELIVERIES: usize = 128;
pub const DEFAULT_MAX_COMPLETED_DELIVERIES: usize = 512;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionError {
    #[error("session protocol version is unsupported")]
    UnsupportedProtocol,
    #[error("session identity is incomplete")]
    InvalidIdentity,
    #[error("session field exceeds its byte limit")]
    FieldTooLarge,
    #[error("session bearer token is required")]
    MissingToken,
    #[error("session is already active")]
    SessionAlreadyActive,
    #[error("session resume token is invalid or expired")]
    InvalidResumeToken,
    #[error("session is not active")]
    InactiveSession,
    #[error("session delivery queue is full")]
    QueueFull,
    #[error("session delivery was not found")]
    UnknownDelivery,
    #[error("session delivery identity does not match")]
    DeliveryIdentityMismatch,
    #[error("session delivery was cancelled")]
    DeliveryCancelled,
    #[error("session cancellation was not requested for this delivery")]
    CancellationNotRequested,
    #[error("session result conflicts with an earlier result")]
    ConflictingResult,
    #[error("session has expired")]
    Expired,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SessionHello {
    pub protocol_version: u32,
    pub token: String,
    pub worker_id: String,
    pub owner: String,
    pub client_instance_id: String,
    pub capability_report_json: String,
    pub ready: bool,
    pub readiness_reason: String,
    pub resume_token: Option<String>,
    pub last_received_sequence: u64,
}

impl fmt::Debug for SessionHello {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionHello")
            .field("protocol_version", &self.protocol_version)
            .field("token", &"<redacted>")
            .field("worker_id", &self.worker_id)
            .field("owner", &self.owner)
            .field("client_instance_id", &self.client_instance_id)
            .field(
                "capability_report_bytes",
                &self.capability_report_json.len(),
            )
            .field("ready", &self.ready)
            .field("readiness_reason", &self.readiness_reason)
            .field(
                "resume_token",
                &self.resume_token.as_ref().map(|_| "<redacted>"),
            )
            .field("last_received_sequence", &self.last_received_sequence)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SessionWelcome {
    pub protocol_version: u32,
    pub session_id: String,
    pub worker_id: String,
    pub owner: String,
    pub client_instance_id: String,
    pub resume_token: String,
    pub next_delivery_sequence: u64,
    pub resume_after_sequence: u64,
}

impl fmt::Debug for SessionWelcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionWelcome")
            .field("protocol_version", &self.protocol_version)
            .field("session_id", &self.session_id)
            .field("worker_id", &self.worker_id)
            .field("owner", &self.owner)
            .field("client_instance_id", &self.client_instance_id)
            .field("resume_token", &"<redacted>")
            .field("next_delivery_sequence", &self.next_delivery_sequence)
            .field("resume_after_sequence", &self.resume_after_sequence)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionIdentity {
    pub worker_id: String,
    pub owner: String,
    pub client_instance_id: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SessionTask {
    pub task_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub payload: Vec<u8>,
}

impl fmt::Debug for SessionTask {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionTask")
            .field("task_id", &self.task_id)
            .field("execution_id", &self.execution_id)
            .field("attempt_id", &self.attempt_id)
            .field("idempotency_key", &self.idempotency_key)
            .field("request_digest", &self.request_digest)
            .field("payload_bytes", &self.payload.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDelivery {
    pub delivery_sequence: u64,
    pub task: SessionTask,
    pub cancellation_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResult {
    pub delivery_sequence: u64,
    pub task_id: String,
    pub success: bool,
    pub response_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckDisposition {
    Accepted,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationAckDisposition {
    Accepted,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultDisposition {
    Accepted,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectOutcome {
    pub welcome: SessionWelcome,
    pub redeliveries: Vec<SessionDelivery>,
    pub replaced_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueueOutcome {
    pub delivery: SessionDelivery,
    pub active_session_id: Option<String>,
    pub duplicate: bool,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub resume_ttl_secs: u64,
    pub heartbeat_timeout_secs: u64,
    pub max_pending_deliveries: usize,
    pub max_completed_deliveries: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            resume_ttl_secs: DEFAULT_RESUME_TTL_SECS,
            heartbeat_timeout_secs: DEFAULT_SESSION_HEARTBEAT_TIMEOUT_SECS,
            max_pending_deliveries: DEFAULT_MAX_PENDING_DELIVERIES,
            max_completed_deliveries: DEFAULT_MAX_COMPLETED_DELIVERIES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TaskKey {
    task_id: String,
    execution_id: String,
    attempt_id: String,
    idempotency_key: String,
}

impl From<&SessionTask> for TaskKey {
    fn from(task: &SessionTask) -> Self {
        Self {
            task_id: task.task_id.clone(),
            execution_id: task.execution_id.clone(),
            attempt_id: task.attempt_id.clone(),
            idempotency_key: task.idempotency_key.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct DeliveryState {
    delivery: SessionDelivery,
    acknowledged: bool,
    cancellation_acknowledged: bool,
    result_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkerSession {
    resume_token_sha256: String,
    resume_expires_at: u64,
    active_session_id: Option<String>,
    next_delivery_sequence: u64,
    deliveries: BTreeMap<u64, DeliveryState>,
    task_sequences: HashMap<TaskKey, u64>,
    last_heartbeat_at: u64,
}

/// In-memory, bounded session authority used by Nodepool and test transports.
///
/// Only the hash of a resume token is retained. The raw token appears in a
/// `SessionWelcome` exactly once per successful connect and is expected to be
/// kept by the platform adapter in memory or secure ephemeral storage.
#[derive(Debug, Clone)]
pub struct SessionRegistry {
    config: SessionConfig,
    workers: HashMap<SessionIdentity, WorkerSession>,
    session_workers: HashMap<String, SessionIdentity>,
}

pub type SharedSessionRegistry = Arc<Mutex<SessionRegistry>>;

impl SessionRegistry {
    #[must_use]
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            workers: HashMap::new(),
            session_workers: HashMap::new(),
        }
    }

    #[must_use]
    pub fn shared(config: SessionConfig) -> SharedSessionRegistry {
        Arc::new(Mutex::new(Self::new(config)))
    }

    pub fn connect(
        &mut self,
        hello: SessionHello,
        now: u64,
    ) -> Result<ConnectOutcome, SessionError> {
        validate_hello(&hello)?;
        let identity = SessionIdentity {
            worker_id: hello.worker_id.clone(),
            owner: hello.owner.clone(),
            client_instance_id: hello.client_instance_id.clone(),
        };
        if self.workers.iter().any(|(existing_identity, worker)| {
            existing_identity.worker_id == identity.worker_id
                && existing_identity != &identity
                && worker.active_session_id.is_some()
        }) {
            return Err(SessionError::SessionAlreadyActive);
        }
        let existing = self.workers.remove(&identity);
        let mut replaced_session_id = None;
        let mut worker = if let Some(existing) = existing {
            if existing.resume_expires_at <= now {
                if let Some(session_id) = existing.active_session_id.as_ref() {
                    self.session_workers.remove(session_id);
                }
                WorkerSession {
                    resume_token_sha256: String::new(),
                    resume_expires_at: now,
                    active_session_id: None,
                    next_delivery_sequence: 1,
                    deliveries: BTreeMap::new(),
                    task_sequences: HashMap::new(),
                    last_heartbeat_at: now,
                }
            } else {
                let Some(resume_token) = hello.resume_token.as_deref() else {
                    self.workers.insert(identity, existing);
                    return Err(SessionError::InvalidResumeToken);
                };
                if existing.resume_token_sha256 != token_sha256(resume_token) {
                    self.workers.insert(identity, existing);
                    return Err(SessionError::InvalidResumeToken);
                }
                if let Some(session_id) = existing.active_session_id.as_ref() {
                    replaced_session_id = Some(session_id.clone());
                    self.session_workers.remove(session_id);
                }
                existing
            }
        } else {
            WorkerSession {
                resume_token_sha256: String::new(),
                resume_expires_at: now,
                active_session_id: None,
                next_delivery_sequence: 1,
                deliveries: BTreeMap::new(),
                task_sequences: HashMap::new(),
                last_heartbeat_at: now,
            }
        };

        let session_id = new_opaque_id();
        let resume_token = new_resume_token();
        worker.resume_token_sha256 = token_sha256(&resume_token);
        worker.resume_expires_at = now.saturating_add(self.config.resume_ttl_secs);
        worker.active_session_id = Some(session_id.clone());
        worker.last_heartbeat_at = now;
        self.session_workers
            .insert(session_id.clone(), identity.clone());
        let next_delivery_sequence = worker.next_delivery_sequence;
        let redeliveries = worker
            .deliveries
            .values()
            .filter(|delivery| {
                delivery.result_fingerprint.is_none() && !delivery.cancellation_acknowledged
            })
            .map(|delivery| delivery.delivery.clone())
            .collect();
        self.workers.insert(identity.clone(), worker);

        Ok(ConnectOutcome {
            welcome: SessionWelcome {
                protocol_version: SESSION_PROTOCOL_VERSION,
                session_id,
                worker_id: identity.worker_id,
                owner: identity.owner,
                client_instance_id: identity.client_instance_id,
                resume_token,
                next_delivery_sequence,
                resume_after_sequence: hello.last_received_sequence,
            },
            redeliveries,
            replaced_session_id,
        })
    }

    pub fn enqueue(
        &mut self,
        identity: &SessionIdentity,
        task: SessionTask,
    ) -> Result<EnqueueOutcome, SessionError> {
        validate_task(&task)?;
        let worker = self
            .workers
            .get_mut(identity)
            .ok_or(SessionError::InactiveSession)?;
        let key = TaskKey::from(&task);
        if let Some(sequence) = worker.task_sequences.get(&key).copied() {
            let delivery = worker
                .deliveries
                .get(&sequence)
                .ok_or(SessionError::UnknownDelivery)?
                .delivery
                .clone();
            return Ok(EnqueueOutcome {
                delivery,
                active_session_id: worker.active_session_id.clone(),
                duplicate: true,
            });
        }
        let outstanding = worker
            .deliveries
            .values()
            .filter(|delivery| {
                delivery.result_fingerprint.is_none() && !delivery.cancellation_acknowledged
            })
            .count();
        if outstanding >= self.config.max_pending_deliveries {
            return Err(SessionError::QueueFull);
        }
        let delivery = SessionDelivery {
            delivery_sequence: worker.next_delivery_sequence,
            task,
            cancellation_requested: false,
        };
        worker.next_delivery_sequence = worker.next_delivery_sequence.saturating_add(1);
        worker
            .task_sequences
            .insert(key, delivery.delivery_sequence);
        worker.deliveries.insert(
            delivery.delivery_sequence,
            DeliveryState {
                delivery: delivery.clone(),
                acknowledged: false,
                cancellation_acknowledged: false,
                result_fingerprint: None,
            },
        );
        Ok(EnqueueOutcome {
            delivery,
            active_session_id: worker.active_session_id.clone(),
            duplicate: false,
        })
    }

    pub fn acknowledge(
        &mut self,
        session_id: &str,
        sequence: u64,
        task_id: &str,
        attempt_id: &str,
        idempotency_key: &str,
    ) -> Result<AckDisposition, SessionError> {
        let worker = self.worker_for_session_mut(session_id)?;
        let delivery = worker
            .deliveries
            .get_mut(&sequence)
            .ok_or(SessionError::UnknownDelivery)?;
        if delivery.delivery.task.task_id != task_id
            || delivery.delivery.task.attempt_id != attempt_id
            || delivery.delivery.task.idempotency_key != idempotency_key
        {
            return Err(SessionError::DeliveryIdentityMismatch);
        }
        if delivery.acknowledged {
            return Ok(AckDisposition::Duplicate);
        }
        delivery.acknowledged = true;
        Ok(AckDisposition::Accepted)
    }

    pub fn acknowledge_cancellation(
        &mut self,
        session_id: &str,
        sequence: u64,
        task_id: &str,
        attempt_id: &str,
        idempotency_key: &str,
    ) -> Result<CancellationAckDisposition, SessionError> {
        let max_completed_deliveries = self.config.max_completed_deliveries;
        let worker = self.worker_for_session_mut(session_id)?;
        let delivery = worker
            .deliveries
            .get_mut(&sequence)
            .ok_or(SessionError::UnknownDelivery)?;
        if delivery.delivery.task.task_id != task_id
            || delivery.delivery.task.attempt_id != attempt_id
            || delivery.delivery.task.idempotency_key != idempotency_key
        {
            return Err(SessionError::DeliveryIdentityMismatch);
        }
        if !delivery.delivery.cancellation_requested {
            return Err(SessionError::CancellationNotRequested);
        }
        if delivery.cancellation_acknowledged {
            return Ok(CancellationAckDisposition::Duplicate);
        }
        delivery.cancellation_acknowledged = true;
        prune_terminal_deliveries(worker, max_completed_deliveries);
        Ok(CancellationAckDisposition::Accepted)
    }

    pub fn record_result(
        &mut self,
        session_id: &str,
        result: SessionResult,
    ) -> Result<ResultDisposition, SessionError> {
        let max_completed_deliveries = self.config.max_completed_deliveries;
        let worker = self.worker_for_session_mut(session_id)?;
        let delivery = worker
            .deliveries
            .get_mut(&result.delivery_sequence)
            .ok_or(SessionError::UnknownDelivery)?;
        if delivery.delivery.task.task_id != result.task_id {
            return Err(SessionError::DeliveryIdentityMismatch);
        }
        if delivery.delivery.cancellation_requested {
            return Err(SessionError::DeliveryCancelled);
        }
        if let Some(previous) = delivery.result_fingerprint.as_deref() {
            if previous == result.response_digest {
                return Ok(ResultDisposition::Duplicate);
            }
            return Err(SessionError::ConflictingResult);
        }
        delivery.acknowledged = true;
        delivery.result_fingerprint = Some(result.response_digest);
        prune_terminal_deliveries(worker, max_completed_deliveries);
        Ok(ResultDisposition::Accepted)
    }

    pub fn heartbeat(
        &mut self,
        session_id: &str,
        _last_received_sequence: u64,
        now: u64,
    ) -> Result<(), SessionError> {
        let worker = self.worker_for_session_mut(session_id)?;
        worker.last_heartbeat_at = now;
        Ok(())
    }

    pub fn disconnect(&mut self, session_id: &str, now: u64) -> Result<(), SessionError> {
        let identity = self
            .session_workers
            .remove(session_id)
            .ok_or(SessionError::InactiveSession)?;
        let worker = self
            .workers
            .get_mut(&identity)
            .ok_or(SessionError::InactiveSession)?;
        if worker.active_session_id.as_deref() != Some(session_id) {
            return Err(SessionError::InactiveSession);
        }
        worker.active_session_id = None;
        worker.resume_expires_at = now.saturating_add(self.config.resume_ttl_secs);
        Ok(())
    }

    pub fn expire(&mut self, now: u64) -> usize {
        let heartbeat_timeout_secs = self.config.heartbeat_timeout_secs;
        let expired: Vec<(SessionIdentity, Option<String>)> = self
            .workers
            .iter()
            .filter(|(_, worker)| {
                let resume_expired =
                    worker.active_session_id.is_none() && worker.resume_expires_at <= now;
                let heartbeat_expired = worker.active_session_id.is_some()
                    && worker
                        .last_heartbeat_at
                        .saturating_add(heartbeat_timeout_secs)
                        <= now;
                resume_expired || heartbeat_expired
            })
            .map(|(identity, worker)| (identity.clone(), worker.active_session_id.clone()))
            .collect();
        let count = expired.len();
        for (identity, session_id) in expired {
            self.workers.remove(&identity);
            if let Some(session_id) = session_id {
                self.session_workers.remove(&session_id);
            }
        }
        count
    }

    #[must_use]
    pub fn active_session_id(&self, identity: &SessionIdentity) -> Option<String> {
        self.workers
            .get(identity)
            .and_then(|worker| worker.active_session_id.clone())
    }

    #[must_use]
    pub fn is_session_active(&self, session_id: &str) -> bool {
        self.session_workers.contains_key(session_id)
    }

    #[must_use]
    pub fn active_identity_for_worker(&self, worker_id: &str) -> Option<SessionIdentity> {
        self.workers
            .iter()
            .find(|(identity, worker)| {
                identity.worker_id == worker_id && worker.active_session_id.is_some()
            })
            .map(|(identity, _)| identity.clone())
    }

    #[must_use]
    pub fn delivery(&self, session_id: &str, sequence: u64) -> Option<SessionDelivery> {
        let identity = self.session_workers.get(session_id)?;
        let worker = self.workers.get(identity)?;
        if worker.active_session_id.as_deref() != Some(session_id) {
            return None;
        }
        worker
            .deliveries
            .get(&sequence)
            .map(|state| state.delivery.clone())
    }

    #[must_use]
    pub fn acknowledged_deliveries(&self, session_id: &str) -> Vec<SessionDelivery> {
        let Some(identity) = self.session_workers.get(session_id) else {
            return Vec::new();
        };
        let Some(worker) = self.workers.get(identity) else {
            return Vec::new();
        };
        if worker.active_session_id.as_deref() != Some(session_id) {
            return Vec::new();
        }
        worker
            .deliveries
            .values()
            .filter(|delivery| {
                delivery.acknowledged
                    && !delivery.delivery.cancellation_requested
                    && delivery.result_fingerprint.is_none()
            })
            .map(|delivery| delivery.delivery.clone())
            .collect()
    }

    pub fn cancel_task_attempt(&mut self, task_id: &str, attempt_id: &str) -> usize {
        let mut marked = 0;
        for worker in self.workers.values_mut() {
            for delivery in worker.deliveries.values_mut() {
                if delivery.delivery.task.task_id == task_id
                    && delivery.delivery.task.attempt_id == attempt_id
                    && delivery.result_fingerprint.is_none()
                    && !delivery.delivery.cancellation_requested
                {
                    delivery.delivery.cancellation_requested = true;
                    marked += 1;
                }
            }
        }
        marked
    }

    #[must_use]
    pub fn cancellation_deliveries(&self, session_id: &str) -> Vec<SessionDelivery> {
        let Some(identity) = self.session_workers.get(session_id) else {
            return Vec::new();
        };
        let Some(worker) = self.workers.get(identity) else {
            return Vec::new();
        };
        if worker.active_session_id.as_deref() != Some(session_id) {
            return Vec::new();
        }
        worker
            .deliveries
            .values()
            .filter(|delivery| {
                delivery.delivery.cancellation_requested
                    && !delivery.cancellation_acknowledged
                    && delivery.result_fingerprint.is_none()
            })
            .map(|delivery| delivery.delivery.clone())
            .collect()
    }

    #[must_use]
    pub fn pending_deliveries(&self, identity: &SessionIdentity) -> Vec<SessionDelivery> {
        self.workers
            .get(identity)
            .map(|worker| {
                worker
                    .deliveries
                    .values()
                    .filter(|delivery| {
                        !delivery.delivery.cancellation_requested
                            && delivery.result_fingerprint.is_none()
                    })
                    .map(|delivery| delivery.delivery.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn worker_for_session_mut(
        &mut self,
        session_id: &str,
    ) -> Result<&mut WorkerSession, SessionError> {
        let identity = self
            .session_workers
            .get(session_id)
            .cloned()
            .ok_or(SessionError::InactiveSession)?;
        let worker = self
            .workers
            .get_mut(&identity)
            .ok_or(SessionError::InactiveSession)?;
        if worker.active_session_id.as_deref() != Some(session_id) {
            return Err(SessionError::InactiveSession);
        }
        Ok(worker)
    }
}

fn prune_terminal_deliveries(worker: &mut WorkerSession, max_completed_deliveries: usize) {
    let mut completed = worker
        .deliveries
        .values()
        .filter(|delivery| {
            delivery.result_fingerprint.is_some() || delivery.cancellation_acknowledged
        })
        .count();
    while completed > max_completed_deliveries {
        let Some(sequence) = worker
            .deliveries
            .iter()
            .find(|(_, delivery)| {
                delivery.result_fingerprint.is_some() || delivery.cancellation_acknowledged
            })
            .map(|(sequence, _)| *sequence)
        else {
            break;
        };
        if let Some(delivery) = worker.deliveries.remove(&sequence) {
            worker
                .task_sequences
                .remove(&TaskKey::from(&delivery.delivery.task));
            completed -= 1;
        }
    }
}

fn validate_hello(hello: &SessionHello) -> Result<(), SessionError> {
    if hello.protocol_version != SESSION_PROTOCOL_VERSION {
        return Err(SessionError::UnsupportedProtocol);
    }
    if hello.token.trim().is_empty() {
        return Err(SessionError::MissingToken);
    }
    if hello.token.len() > SESSION_TOKEN_MAX_BYTES
        || hello.worker_id.len() > SESSION_ID_MAX_BYTES
        || hello.owner.len() > SESSION_ID_MAX_BYTES
        || hello.client_instance_id.len() > SESSION_CLIENT_INSTANCE_MAX_BYTES
        || hello.capability_report_json.len() > SESSION_CAPABILITY_REPORT_MAX_BYTES
        || hello.readiness_reason.len() > SESSION_ID_MAX_BYTES
        || hello
            .resume_token
            .as_ref()
            .is_some_and(|token| token.len() > SESSION_RESUME_TOKEN_MAX_BYTES)
    {
        return Err(SessionError::FieldTooLarge);
    }
    if hello.worker_id.trim().is_empty()
        || hello.owner.trim().is_empty()
        || hello.client_instance_id.trim().is_empty()
    {
        return Err(SessionError::InvalidIdentity);
    }
    Ok(())
}

fn validate_task(task: &SessionTask) -> Result<(), SessionError> {
    for value in [
        task.task_id.as_str(),
        task.execution_id.as_str(),
        task.attempt_id.as_str(),
        task.idempotency_key.as_str(),
        task.request_digest.as_str(),
    ] {
        if value.trim().is_empty() {
            return Err(SessionError::InvalidIdentity);
        }
        if value.len() > SESSION_ID_MAX_BYTES {
            return Err(SessionError::FieldTooLarge);
        }
    }
    if task.payload.len() > SESSION_FRAME_MAX_BYTES {
        return Err(SessionError::FieldTooLarge);
    }
    Ok(())
}

fn new_opaque_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn new_resume_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("hm-session-v1.{}", hex::encode(bytes))
}

fn token_sha256(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(resume_token: Option<String>) -> SessionHello {
        SessionHello {
            protocol_version: SESSION_PROTOCOL_VERSION,
            token: "jwt-in-memory".into(),
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
            capability_report_json: "{\"runtime\":\"managed-function-v0\"}".into(),
            ready: true,
            readiness_reason: String::new(),
            resume_token,
            last_received_sequence: 0,
        }
    }

    fn task(task_id: &str) -> SessionTask {
        SessionTask {
            task_id: task_id.into(),
            execution_id: format!("execution-{task_id}"),
            attempt_id: format!("attempt-{task_id}"),
            idempotency_key: format!("idempotency-{task_id}"),
            request_digest: "a".repeat(64),
            payload: b"bounded-task".to_vec(),
        }
    }

    #[test]
    fn connect_assigns_identity_and_redacts_bearer_material() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        let outcome = registry.connect(hello(None), 10).unwrap();
        assert_eq!(outcome.welcome.protocol_version, SESSION_PROTOCOL_VERSION);
        assert!(registry.is_session_active(&outcome.welcome.session_id));
        let debug = format!("{:?}", hello(Some(outcome.welcome.resume_token.clone())));
        assert!(!debug.contains("jwt-in-memory"));
        assert!(!debug.contains(&outcome.welcome.resume_token));
        assert!(!format!("{:?}", outcome.welcome).contains(&outcome.welcome.resume_token));
    }

    #[test]
    fn delivery_ack_and_result_are_idempotent_and_identity_bound() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        let outcome = registry.connect(hello(None), 10).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        let delivery = registry.enqueue(&identity, task("task-1")).unwrap();
        assert!(!delivery.duplicate);
        assert_eq!(
            registry
                .acknowledge(
                    &outcome.welcome.session_id,
                    delivery.delivery.delivery_sequence,
                    "task-1",
                    "attempt-task-1",
                    "idempotency-task-1",
                )
                .unwrap(),
            AckDisposition::Accepted
        );
        assert_eq!(
            registry
                .acknowledge(
                    &outcome.welcome.session_id,
                    delivery.delivery.delivery_sequence,
                    "task-1",
                    "attempt-task-1",
                    "idempotency-task-1",
                )
                .unwrap(),
            AckDisposition::Duplicate
        );
        assert!(matches!(
            registry.acknowledge(
                &outcome.welcome.session_id,
                delivery.delivery.delivery_sequence,
                "other-task",
                "attempt-task-1",
                "idempotency-task-1",
            ),
            Err(SessionError::DeliveryIdentityMismatch)
        ));
        let result = SessionResult {
            delivery_sequence: delivery.delivery.delivery_sequence,
            task_id: "task-1".into(),
            success: true,
            response_digest: "result-a".into(),
        };
        assert_eq!(
            registry
                .record_result(&outcome.welcome.session_id, result.clone())
                .unwrap(),
            ResultDisposition::Accepted
        );
        assert_eq!(
            registry
                .record_result(&outcome.welcome.session_id, result)
                .unwrap(),
            ResultDisposition::Duplicate
        );
        assert!(matches!(
            registry.record_result(
                &outcome.welcome.session_id,
                SessionResult {
                    delivery_sequence: delivery.delivery.delivery_sequence,
                    task_id: "task-1".into(),
                    success: false,
                    response_digest: "result-b".into(),
                }
            ),
            Err(SessionError::ConflictingResult)
        ));
    }

    #[test]
    fn reconnect_replaces_old_session_and_redelivers_unacknowledged_tasks() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        let first = registry.connect(hello(None), 10).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        let queued = registry.enqueue(&identity, task("task-1")).unwrap();
        registry.disconnect(&first.welcome.session_id, 11).unwrap();
        let resumed = registry
            .connect(
                SessionHello {
                    resume_token: Some(first.welcome.resume_token),
                    last_received_sequence: 0,
                    ..hello(None)
                },
                12,
            )
            .unwrap();
        assert_eq!(
            resumed.replaced_session_id.as_deref(),
            None,
            "disconnect removes the old active session before resume"
        );
        assert_eq!(resumed.redeliveries, vec![queued.delivery.clone()]);
        assert!(!registry.is_session_active(&first.welcome.session_id));
        assert!(registry.is_session_active(&resumed.welcome.session_id));
        assert!(matches!(
            registry.acknowledge(
                &first.welcome.session_id,
                queued.delivery.delivery_sequence,
                "task-1",
                "attempt-task-1",
                "idempotency-task-1",
            ),
            Err(SessionError::InactiveSession)
        ));
    }

    #[test]
    fn connecting_again_replaces_an_active_session_only_with_the_resume_token() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        let first = registry.connect(hello(None), 10).unwrap();
        assert!(matches!(
            registry.connect(hello(None), 11),
            Err(SessionError::InvalidResumeToken)
        ));
        let resumed = registry
            .connect(
                SessionHello {
                    resume_token: Some(first.welcome.resume_token),
                    ..hello(None)
                },
                11,
            )
            .unwrap();
        assert_eq!(
            resumed.replaced_session_id.as_deref(),
            Some(first.welcome.session_id.as_str())
        );
        assert!(!registry.is_session_active(&first.welcome.session_id));
    }

    #[test]
    fn expiry_removes_disconnected_resume_state() {
        let mut registry = SessionRegistry::new(SessionConfig {
            resume_ttl_secs: 5,
            ..SessionConfig::default()
        });
        let first = registry.connect(hello(None), 10).unwrap();
        registry.disconnect(&first.welcome.session_id, 11).unwrap();
        assert_eq!(registry.expire(15), 0);
        assert_eq!(registry.expire(16), 1);
        assert!(registry
            .connect(
                SessionHello {
                    resume_token: Some(first.welcome.resume_token),
                    ..hello(None)
                },
                17,
            )
            .is_ok());
    }

    #[test]
    fn duplicate_enqueue_returns_the_original_delivery_without_growing_the_queue() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        registry.connect(hello(None), 1).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        let first = registry.enqueue(&identity, task("task-1")).unwrap();
        let second = registry.enqueue(&identity, task("task-1")).unwrap();
        assert_eq!(first.delivery, second.delivery);
        assert!(second.duplicate);
        assert_eq!(registry.pending_deliveries(&identity).len(), 1);
    }

    #[test]
    fn reconnect_at_exact_resume_expiry_starts_without_old_delivery_state() {
        let mut registry = SessionRegistry::new(SessionConfig {
            resume_ttl_secs: 5,
            ..SessionConfig::default()
        });
        let first = registry.connect(hello(None), 10).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        registry.enqueue(&identity, task("expired-task")).unwrap();
        registry.disconnect(&first.welcome.session_id, 11).unwrap();

        let resumed = registry
            .connect(
                SessionHello {
                    resume_token: Some(first.welcome.resume_token),
                    ..hello(None)
                },
                16,
            )
            .unwrap();
        assert!(resumed.redeliveries.is_empty());
        assert_eq!(resumed.welcome.next_delivery_sequence, 1);
    }

    #[test]
    fn active_session_expires_without_a_heartbeat() {
        let mut registry = SessionRegistry::new(SessionConfig {
            heartbeat_timeout_secs: 5,
            ..SessionConfig::default()
        });
        let first = registry.connect(hello(None), 10).unwrap();

        assert_eq!(registry.expire(14), 0);
        assert_eq!(registry.expire(15), 1);
        assert!(!registry.is_session_active(&first.welcome.session_id));
    }

    #[test]
    fn another_client_instance_cannot_create_an_ambiguous_active_worker_session() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        registry.connect(hello(None), 10).unwrap();
        let second = SessionHello {
            client_instance_id: "device-2".into(),
            ..hello(None)
        };

        assert_eq!(
            registry.connect(second, 11),
            Err(SessionError::SessionAlreadyActive)
        );
    }

    #[test]
    fn cancellation_is_redelivered_as_a_cancel_frame_and_rejects_late_results() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        let outcome = registry.connect(hello(None), 10).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        let delivery = registry.enqueue(&identity, task("cancelled-task")).unwrap();

        assert_eq!(
            registry.cancel_task_attempt("cancelled-task", "attempt-cancelled-task"),
            1
        );
        assert!(registry.pending_deliveries(&identity).is_empty());
        let cancellations = registry.cancellation_deliveries(&outcome.welcome.session_id);
        assert_eq!(
            cancellations,
            vec![SessionDelivery {
                cancellation_requested: true,
                ..delivery.delivery.clone()
            }]
        );
        assert_eq!(
            registry.record_result(
                &outcome.welcome.session_id,
                SessionResult {
                    delivery_sequence: delivery.delivery.delivery_sequence,
                    task_id: "cancelled-task".into(),
                    success: true,
                    response_digest: "late-result".into(),
                },
            ),
            Err(SessionError::DeliveryCancelled)
        );
    }

    #[test]
    fn cancellation_ack_retires_redelivery_and_is_idempotent() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        let first = registry.connect(hello(None), 10).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        let delivery = registry
            .enqueue(&identity, task("cancel-ack-task"))
            .unwrap();
        assert_eq!(
            registry.cancel_task_attempt("cancel-ack-task", "attempt-cancel-ack-task"),
            1
        );
        assert_eq!(
            registry
                .acknowledge_cancellation(
                    &first.welcome.session_id,
                    delivery.delivery.delivery_sequence,
                    "cancel-ack-task",
                    "attempt-cancel-ack-task",
                    "idempotency-cancel-ack-task",
                )
                .unwrap(),
            CancellationAckDisposition::Accepted
        );
        assert_eq!(
            registry
                .acknowledge_cancellation(
                    &first.welcome.session_id,
                    delivery.delivery.delivery_sequence,
                    "cancel-ack-task",
                    "attempt-cancel-ack-task",
                    "idempotency-cancel-ack-task",
                )
                .unwrap(),
            CancellationAckDisposition::Duplicate
        );
        assert!(registry
            .cancellation_deliveries(&first.welcome.session_id)
            .is_empty());
        registry.disconnect(&first.welcome.session_id, 11).unwrap();
        let resumed = registry
            .connect(
                SessionHello {
                    resume_token: Some(first.welcome.resume_token),
                    ..hello(None)
                },
                12,
            )
            .unwrap();
        assert!(resumed.redeliveries.is_empty());
        assert_eq!(
            registry.record_result(
                &resumed.welcome.session_id,
                SessionResult {
                    delivery_sequence: delivery.delivery.delivery_sequence,
                    task_id: "cancel-ack-task".into(),
                    success: true,
                    response_digest: "late-result".into(),
                },
            ),
            Err(SessionError::DeliveryCancelled)
        );
    }

    #[test]
    fn cancellation_ack_requires_a_marked_delivery() {
        let mut registry = SessionRegistry::new(SessionConfig::default());
        let outcome = registry.connect(hello(None), 10).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        let delivery = registry
            .enqueue(&identity, task("uncancelled-task"))
            .unwrap();
        assert_eq!(
            registry.acknowledge_cancellation(
                &outcome.welcome.session_id,
                delivery.delivery.delivery_sequence,
                "uncancelled-task",
                "attempt-uncancelled-task",
                "idempotency-uncancelled-task",
            ),
            Err(SessionError::CancellationNotRequested)
        );
    }

    #[test]
    fn acknowledged_cancellation_does_not_consume_pending_capacity() {
        let mut registry = SessionRegistry::new(SessionConfig {
            max_pending_deliveries: 1,
            ..SessionConfig::default()
        });
        let outcome = registry.connect(hello(None), 10).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        let delivery = registry
            .enqueue(&identity, task("cancel-capacity-task"))
            .unwrap();
        assert_eq!(
            registry.cancel_task_attempt("cancel-capacity-task", "attempt-cancel-capacity-task"),
            1
        );
        registry
            .acknowledge_cancellation(
                &outcome.welcome.session_id,
                delivery.delivery.delivery_sequence,
                "cancel-capacity-task",
                "attempt-cancel-capacity-task",
                "idempotency-cancel-capacity-task",
            )
            .unwrap();

        assert!(registry.enqueue(&identity, task("next-task")).is_ok());
    }

    #[test]
    fn terminal_delivery_history_is_bounded() {
        let mut registry = SessionRegistry::new(SessionConfig {
            max_completed_deliveries: 1,
            ..SessionConfig::default()
        });
        let outcome = registry.connect(hello(None), 10).unwrap();
        let identity = SessionIdentity {
            worker_id: "worker-1".into(),
            owner: "alice".into(),
            client_instance_id: "device-1".into(),
        };
        for task_id in ["completed-one", "completed-two"] {
            let delivery = registry.enqueue(&identity, task(task_id)).unwrap();
            registry
                .record_result(
                    &outcome.welcome.session_id,
                    SessionResult {
                        delivery_sequence: delivery.delivery.delivery_sequence,
                        task_id: task_id.into(),
                        success: true,
                        response_digest: format!("result-{task_id}"),
                    },
                )
                .unwrap();
        }

        assert!(registry.delivery(&outcome.welcome.session_id, 1).is_none());
        assert!(registry.delivery(&outcome.welcome.session_id, 2).is_some());
    }
}

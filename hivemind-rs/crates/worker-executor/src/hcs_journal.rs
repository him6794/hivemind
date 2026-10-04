//! Durable, nonsecret state for native Windows HCS executions.
//!
//! The journal is deliberately a host-side recovery record, not an execution
//! proof and not a result store. It contains identities, digests, operator
//! paths, and bounded lifecycle observations only. Credentials, bearer tokens,
//! source/input bytes, and private keys are never accepted by this schema.

use general_compute_runtime::windows_hcs::{
    HcsLifecycleEvent, HcsSystemSummary, HIVEMIND_HCS_OWNER, HIVEMIND_HCS_SYSTEM_ID_PREFIX,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const HCS_JOURNAL_SCHEMA_VERSION: u32 = 1;
const HCS_JOURNAL_KEY_VERSION: u8 = 1;
const MAX_FIELD_BYTES: usize = 4096;
const MAX_RECORD_BYTES: u64 = 1024 * 1024;
const MAX_ERROR_BYTES: usize = 2048;
const MAX_ENUMERATED_SYSTEMS: usize = 4096;
#[cfg(windows)]
const REPARSE_POINT_ATTRIBUTE: u32 = 0x0400;
const TERMINAL_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
const TEMPORARY_SNAPSHOT_MIN_AGE_MS: u64 = 60 * 60 * 1_000;

/// Terminal HCS records are retained for this long after authoritative cleanup
/// and delivery. Non-terminal or unreconciled records are never removed by GC.
pub const HCS_JOURNAL_TERMINAL_RETENTION_MS: u64 = TERMINAL_RETENTION_MS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HcsExecutionIdentity {
    pub task_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub transfer_generation: Option<i64>,
}

impl HcsExecutionIdentity {
    pub fn validate(&self) -> Result<(), HcsJournalError> {
        validate_component("task_id", &self.task_id, MAX_FIELD_BYTES)?;
        validate_component("execution_id", &self.execution_id, MAX_FIELD_BYTES)?;
        validate_component("attempt_id", &self.attempt_id, MAX_FIELD_BYTES)?;
        validate_component("idempotency_key", &self.idempotency_key, MAX_FIELD_BYTES)?;
        validate_digest("request_digest", &self.request_digest)?;
        if self
            .transfer_generation
            .is_some_and(|generation| generation <= 0)
        {
            return Err(HcsJournalError::InvalidIntent(
                "transfer_generation must be positive when present".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HcsExecutionIntent {
    pub identity: HcsExecutionIdentity,
    pub worker_id: String,
    pub backend_id: String,
    pub guest_image_digest: String,
    pub runner_sha256: String,
    pub policy_digest: String,
    pub spec_digest: String,
    pub input_sha256: String,
    pub container_id: String,
    pub scratch_path: PathBuf,
    pub result_path: PathBuf,
}

impl HcsExecutionIntent {
    pub fn validate(&self) -> Result<(), HcsJournalError> {
        self.identity.validate()?;
        for (name, value) in [
            ("worker_id", self.worker_id.as_str()),
            ("backend_id", self.backend_id.as_str()),
            ("container_id", self.container_id.as_str()),
        ] {
            validate_component(name, value, MAX_FIELD_BYTES)?;
        }
        for (name, value) in [
            ("guest_image_digest", self.guest_image_digest.as_str()),
            ("runner_sha256", self.runner_sha256.as_str()),
            ("policy_digest", self.policy_digest.as_str()),
            ("spec_digest", self.spec_digest.as_str()),
            ("input_sha256", self.input_sha256.as_str()),
        ] {
            validate_digest(name, value)?;
        }
        validate_operator_path("scratch_path", &self.scratch_path)?;
        validate_operator_path("result_path", &self.result_path)?;
        if !is_safe_component(&self.container_id) {
            return Err(HcsJournalError::InvalidIntent(
                "container_id contains unsafe characters".into(),
            ));
        }
        if !self.container_id.starts_with(HIVEMIND_HCS_SYSTEM_ID_PREFIX) {
            return Err(HcsJournalError::InvalidIntent(
                "container_id is outside the Hivemind HCS namespace".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcsLifecycleState {
    Prepared,
    Creating,
    Created,
    Running,
    Waiting,
    GuestExited,
    ShuttingDown,
    Terminating,
    ResultRead,
    Closed,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    RecoveryRequired,
    Orphaned,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcsCleanupState {
    Pending,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcsDeliveryState {
    NotRequired,
    AwaitingValidation,
    Validated,
    Delivered,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcsReconciliationOutcome {
    MatchedRunning,
    MatchedCompleted,
    MissingSystem,
    OrphanSystem,
    WorkerIdentityMismatch,
    HcsIdentityMismatch,
    LeaseRequired,
    CleanupRequired,
    CleanupFailed,
    Corrupt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HcsJournalRecord {
    pub schema_version: u32,
    pub identity: HcsExecutionIdentity,
    pub worker_id: String,
    pub backend_id: String,
    pub guest_image_digest: String,
    pub runner_sha256: String,
    pub policy_digest: String,
    pub spec_digest: String,
    pub input_sha256: String,
    pub container_id: String,
    pub scratch_path: PathBuf,
    pub result_path: PathBuf,
    pub hcs_system_id: Option<String>,
    pub lifecycle: HcsLifecycleState,
    pub cleanup: HcsCleanupState,
    pub delivery: HcsDeliveryState,
    pub guest_exit_code: Option<i32>,
    pub hcs_exit_status: Option<i32>,
    pub hcs_exit_type: Option<String>,
    pub result_sha256: Option<String>,
    pub result_size: Option<u64>,
    pub reconciliation: Option<HcsReconciliationOutcome>,
    pub last_error: Option<String>,
    pub sequence: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl HcsJournalRecord {
    fn from_intent(intent: HcsExecutionIntent, now_ms: u64) -> Self {
        Self {
            schema_version: HCS_JOURNAL_SCHEMA_VERSION,
            identity: intent.identity,
            worker_id: intent.worker_id,
            backend_id: intent.backend_id,
            guest_image_digest: intent.guest_image_digest,
            runner_sha256: intent.runner_sha256,
            policy_digest: intent.policy_digest,
            spec_digest: intent.spec_digest,
            input_sha256: intent.input_sha256,
            container_id: intent.container_id,
            scratch_path: intent.scratch_path,
            result_path: intent.result_path,
            hcs_system_id: None,
            lifecycle: HcsLifecycleState::Prepared,
            cleanup: HcsCleanupState::Pending,
            delivery: HcsDeliveryState::NotRequired,
            guest_exit_code: None,
            hcs_exit_status: None,
            hcs_exit_type: None,
            result_sha256: None,
            result_size: None,
            reconciliation: None,
            last_error: None,
            sequence: 1,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    pub fn needs_reconciliation(&self) -> bool {
        match self.lifecycle {
            HcsLifecycleState::Prepared | HcsLifecycleState::Abandoned => false,
            HcsLifecycleState::Completed => {
                self.cleanup != HcsCleanupState::Succeeded
                    || self.delivery != HcsDeliveryState::Delivered
            }
            HcsLifecycleState::Failed
            | HcsLifecycleState::Cancelled
            | HcsLifecycleState::TimedOut => self.cleanup != HcsCleanupState::Succeeded,
            HcsLifecycleState::Creating
            | HcsLifecycleState::Created
            | HcsLifecycleState::Running
            | HcsLifecycleState::Waiting
            | HcsLifecycleState::GuestExited
            | HcsLifecycleState::ShuttingDown
            | HcsLifecycleState::Terminating
            | HcsLifecycleState::ResultRead
            | HcsLifecycleState::Closed
            | HcsLifecycleState::RecoveryRequired
            | HcsLifecycleState::Orphaned => true,
        }
    }

    fn validate(&self) -> Result<(), HcsJournalError> {
        if self.schema_version != HCS_JOURNAL_SCHEMA_VERSION {
            return Err(HcsJournalError::Corrupt(format!(
                "unsupported HCS journal schema version {}",
                self.schema_version
            )));
        }
        let intent = HcsExecutionIntent {
            identity: self.identity.clone(),
            worker_id: self.worker_id.clone(),
            backend_id: self.backend_id.clone(),
            guest_image_digest: self.guest_image_digest.clone(),
            runner_sha256: self.runner_sha256.clone(),
            policy_digest: self.policy_digest.clone(),
            spec_digest: self.spec_digest.clone(),
            input_sha256: self.input_sha256.clone(),
            container_id: self.container_id.clone(),
            scratch_path: self.scratch_path.clone(),
            result_path: self.result_path.clone(),
        };
        intent.validate().map_err(|error| {
            HcsJournalError::Corrupt(format!("invalid HCS journal identity: {error}"))
        })?;
        if let Some(system_id) = &self.hcs_system_id {
            validate_component("hcs_system_id", system_id, MAX_FIELD_BYTES)?;
            if !is_safe_component(system_id) || system_id != &self.container_id {
                return Err(HcsJournalError::Corrupt(
                    "HCS system identity does not match the operator container id".into(),
                ));
            }
        }
        if matches!(
            self.lifecycle,
            HcsLifecycleState::Prepared | HcsLifecycleState::Creating
        ) && self.hcs_system_id.is_some()
        {
            return Err(HcsJournalError::Corrupt(
                "HCS system identity is present before a successful create event".into(),
            ));
        }
        if matches!(
            self.lifecycle,
            HcsLifecycleState::Created
                | HcsLifecycleState::Running
                | HcsLifecycleState::Waiting
                | HcsLifecycleState::GuestExited
                | HcsLifecycleState::ShuttingDown
                | HcsLifecycleState::Terminating
                | HcsLifecycleState::ResultRead
                | HcsLifecycleState::Closed
                | HcsLifecycleState::Completed
                | HcsLifecycleState::Cancelled
                | HcsLifecycleState::TimedOut
        ) && self.hcs_system_id.is_none()
        {
            return Err(HcsJournalError::Corrupt(
                "HCS lifecycle state is missing the created system identity".into(),
            ));
        }
        if let Some(exit_type) = &self.hcs_exit_type {
            if exit_type.len() > MAX_ERROR_BYTES
                || !matches!(exit_type.as_str(), "GracefulExit" | "ForcedExit")
            {
                return Err(HcsJournalError::Corrupt(
                    "HCS exit type is not authoritative".into(),
                ));
            }
            if self.hcs_exit_status != Some(0) {
                return Err(HcsJournalError::Corrupt(
                    "HCS exit type is present without a successful HCS status".into(),
                ));
            }
        } else if self.hcs_exit_status.is_some() {
            return Err(HcsJournalError::Corrupt(
                "HCS exit status is missing its exit type".into(),
            ));
        }
        if self.result_sha256.is_some() != self.result_size.is_some() {
            return Err(HcsJournalError::Corrupt(
                "HCS result digest and size must be recorded together".into(),
            ));
        }
        if self.result_size.is_some_and(|size| size > MAX_RECORD_BYTES) {
            return Err(HcsJournalError::Corrupt(
                "HCS result size exceeds the journal bound".into(),
            ));
        }
        if let Some(result_sha256) = &self.result_sha256 {
            validate_digest("result_sha256", result_sha256)?;
        }
        if matches!(
            self.lifecycle,
            HcsLifecycleState::Prepared
                | HcsLifecycleState::Creating
                | HcsLifecycleState::Created
                | HcsLifecycleState::Running
                | HcsLifecycleState::Waiting
                | HcsLifecycleState::GuestExited
                | HcsLifecycleState::ShuttingDown
                | HcsLifecycleState::Terminating
                | HcsLifecycleState::Cancelled
                | HcsLifecycleState::TimedOut
        ) && !matches!(self.delivery, HcsDeliveryState::NotRequired)
        {
            return Err(HcsJournalError::Corrupt(
                "HCS delivery state is present before a result can be delivered".into(),
            ));
        }
        if self.lifecycle == HcsLifecycleState::ResultRead
            && self.delivery != HcsDeliveryState::AwaitingValidation
        {
            return Err(HcsJournalError::Corrupt(
                "result-read HCS journal entry has an invalid delivery state".into(),
            ));
        }
        if self.lifecycle == HcsLifecycleState::Completed
            && self.delivery == HcsDeliveryState::NotRequired
        {
            return Err(HcsJournalError::Corrupt(
                "completed HCS journal entry has no delivery state".into(),
            ));
        }
        if matches!(
            self.delivery,
            HcsDeliveryState::Validated | HcsDeliveryState::Delivered
        ) {
            let terminal_cleanup = self.lifecycle == HcsLifecycleState::Completed
                && (self.cleanup == HcsCleanupState::Succeeded
                    || matches!(
                        self.reconciliation,
                        Some(
                            HcsReconciliationOutcome::CleanupRequired
                                | HcsReconciliationOutcome::CleanupFailed
                        )
                    ));
            let recovery_cleanup = self.lifecycle == HcsLifecycleState::RecoveryRequired
                && matches!(
                    self.reconciliation,
                    Some(
                        HcsReconciliationOutcome::MissingSystem
                            | HcsReconciliationOutcome::CleanupRequired
                            | HcsReconciliationOutcome::CleanupFailed
                            | HcsReconciliationOutcome::LeaseRequired
                    )
                );
            let abandoned_cleanup = self.lifecycle == HcsLifecycleState::Abandoned
                && self.cleanup == HcsCleanupState::Succeeded
                && self.delivery != HcsDeliveryState::Delivered;
            if !terminal_cleanup && !recovery_cleanup && !abandoned_cleanup {
                return Err(HcsJournalError::Corrupt(
                    "validated HCS delivery is not bound to a completed cleanup state".into(),
                ));
            }
        }
        if let Some(error) = &self.last_error {
            if error.len() > MAX_ERROR_BYTES || error.chars().any(char::is_control) {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal error is too large or contains control characters".into(),
                ));
            }
        }
        if self.lifecycle == HcsLifecycleState::Completed
            && (self.guest_exit_code != Some(0)
                || self.hcs_exit_status != Some(0)
                || self.result_sha256.is_none()
                || self.result_size.is_none()
                || (self.cleanup != HcsCleanupState::Succeeded
                    && !matches!(
                        self.reconciliation,
                        Some(
                            HcsReconciliationOutcome::CleanupRequired
                                | HcsReconciliationOutcome::CleanupFailed
                        )
                    )))
        {
            return Err(HcsJournalError::Corrupt(
                "completed HCS journal entry is missing authoritative state".into(),
            ));
        }
        if self.sequence == 0 {
            return Err(HcsJournalError::Corrupt(
                "HCS journal sequence must be positive".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HcsJournalEvent {
    CreateRequested,
    Created {
        system_id: String,
    },
    Started,
    Waiting,
    GuestExited {
        exit_code: Option<i32>,
    },
    Cancelled,
    TimedOut,
    StartFailed {
        error: String,
    },
    WaitFailed {
        error: String,
    },
    ShutdownStarted,
    ShutdownCompleted {
        status: i32,
        exit_type: String,
    },
    ShutdownFailed {
        error: String,
    },
    TerminateStarted,
    Terminated {
        status: i32,
        exit_type: String,
    },
    TerminateFailed {
        error: String,
    },
    ResultRead {
        sha256: String,
        size: usize,
    },
    ResultReadFailed {
        error: String,
    },
    Closed,
    Completed,
    Failed {
        error: String,
    },
    DeliveryValidated,
    Delivered,
    DeliveryFailed {
        error: String,
    },
    /// The successful authoritative HCS enumeration confirmed that this
    /// journaled system is absent, so no further native cleanup is required.
    CleanupConfirmedAbsent,
    Abandoned {
        reason: String,
    },
    Reconciled {
        outcome: HcsReconciliationOutcome,
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HcsReconciliationAction {
    ReattachCandidate {
        record: HcsJournalRecord,
        system: HcsSystemSummary,
    },
    RecoverCompletedResult {
        record: HcsJournalRecord,
    },
    MissingSystem {
        record: HcsJournalRecord,
    },
    CleanupRecordedSystem {
        record: HcsJournalRecord,
        system: HcsSystemSummary,
    },
    IdentityMismatch {
        record: HcsJournalRecord,
        system: HcsSystemSummary,
    },
    WorkerIdentityMismatch {
        record: HcsJournalRecord,
    },
    /// The HCS provider returned an entry whose ownership cannot be proven.
    /// Reconciliation must leave it untouched and keep the Worker unavailable.
    QuarantineSystem {
        system: HcsSystemSummary,
        reason: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum HcsJournalError {
    #[error("HCS journal root is invalid: {0}")]
    InvalidRoot(String),
    #[error("HCS journal intent is invalid: {0}")]
    InvalidIntent(String),
    #[error("HCS journal identity already belongs to different immutable metadata")]
    IdentityConflict,
    #[error("HCS journal entry is unknown")]
    UnknownEntry,
    #[error("HCS journal is corrupt: {0}")]
    Corrupt(String),
    #[error("HCS journal sequence conflict")]
    SequenceConflict,
    #[error("HCS journal I/O failed: {0}")]
    Io(String),
    #[error("HCS lifecycle event cannot be recorded: {0}")]
    Event(String),
}

#[derive(Clone)]
pub struct HcsExecutionJournal {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
    index: Arc<Mutex<HashMap<String, PathBuf>>>,
}

impl HcsExecutionJournal {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, HcsJournalError> {
        let root = root.into();
        if !root.is_absolute() {
            return Err(HcsJournalError::InvalidRoot(
                "operator Worker state root must be absolute".into(),
            ));
        }
        ensure_safe_directory(&root)?;
        ensure_safe_directory(&root.join("entries"))?;
        let journal = Self {
            root,
            lock: Arc::new(Mutex::new(())),
            index: Arc::new(Mutex::new(HashMap::new())),
        };
        {
            let _guard = journal
                .lock
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            journal.cleanup_temporary_snapshots_locked()?;
            journal.rebuild_index_locked()?;
            journal.prune_terminal_records_locked(now_ms(), TERMINAL_RETENTION_MS)?;
        }
        Ok(journal)
    }

    pub fn from_environment() -> Result<Option<Self>, HcsJournalError> {
        match std::env::var_os("HIVEMIND_WORKER_STATE_ROOT") {
            None => Ok(None),
            Some(value) => {
                let value = value.to_string_lossy().trim().to_owned();
                if value.is_empty() {
                    return Err(HcsJournalError::InvalidRoot(
                        "HIVEMIND_WORKER_STATE_ROOT must not be blank".into(),
                    ));
                }
                Self::open(value).map(Some)
            }
        }
    }

    /// Open the operator-owned journal using the package's zero-configuration
    /// Windows state root when no explicit deployment override is present.
    /// Non-Windows workers do not create an HCS journal because HCS is not an
    /// available execution provider there.
    pub fn from_environment_or_default() -> Result<Option<Self>, HcsJournalError> {
        if std::env::var_os("HIVEMIND_WORKER_STATE_ROOT").is_some() {
            return Self::from_environment();
        }

        #[cfg(windows)]
        {
            let root = Self::default_windows_worker_state_root()?.join("hcs-journal");
            Self::open(root).map(Some)
        }

        #[cfg(not(windows))]
        {
            Ok(None)
        }
    }

    #[cfg(windows)]
    pub fn default_windows_worker_state_root() -> Result<PathBuf, HcsJournalError> {
        let local_app_data = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
            HcsJournalError::InvalidRoot(
                "LOCALAPPDATA is required for the default Worker state root".into(),
            )
        })?;
        let local_app_data = PathBuf::from(local_app_data);
        if !local_app_data.is_absolute()
            || local_app_data.as_os_str().is_empty()
            || local_app_data
                .to_string_lossy()
                .chars()
                .any(char::is_control)
        {
            return Err(HcsJournalError::InvalidRoot(
                "LOCALAPPDATA must be an absolute path without control characters".into(),
            ));
        }
        Ok(local_app_data.join("Hivemind").join("Worker"))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn begin(&self, intent: HcsExecutionIntent) -> Result<HcsJournalRecord, HcsJournalError> {
        intent.validate()?;
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let directory = self.entry_directory(&intent.identity)?;
        if let Some(existing) = self.read_latest_locked(&directory)? {
            if immutable_fields_match(&existing, &intent) {
                return Ok(existing);
            }
            return Err(HcsJournalError::IdentityConflict);
        }
        let record = HcsJournalRecord::from_intent(intent, now_ms());
        self.append_snapshot_locked(&directory, &record)?;
        self.index
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(immutable_key(&record.identity), directory);
        Ok(record)
    }

    pub fn load(
        &self,
        identity: &HcsExecutionIdentity,
    ) -> Result<HcsJournalRecord, HcsJournalError> {
        identity.validate()?;
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let directory = self.entry_directory(identity)?;
        let record = self
            .read_latest_locked(&directory)?
            .ok_or(HcsJournalError::UnknownEntry)?;
        if record.identity != *identity {
            return Err(HcsJournalError::IdentityConflict);
        }
        Ok(record)
    }

    pub fn append_event(
        &self,
        identity: &HcsExecutionIdentity,
        event: HcsJournalEvent,
    ) -> Result<HcsJournalRecord, HcsJournalError> {
        identity.validate()?;
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let directory = self.entry_directory(identity)?;
        let mut record = self
            .read_latest_locked(&directory)?
            .ok_or(HcsJournalError::UnknownEntry)?;
        if record.identity != *identity {
            return Err(HcsJournalError::IdentityConflict);
        }
        apply_event(&mut record, event)?;
        record.sequence = record.sequence.saturating_add(1);
        record.updated_at_ms = now_ms();
        record.validate()?;
        self.append_snapshot_locked(&directory, &record)?;
        Ok(record)
    }

    pub fn append_runtime_event(
        &self,
        identity: &HcsExecutionIdentity,
        event: HcsLifecycleEvent,
    ) -> Result<HcsJournalRecord, HcsJournalError> {
        self.append_event(identity, runtime_event(event))
    }

    pub fn list_records(&self) -> Result<Vec<HcsJournalRecord>, HcsJournalError> {
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let entries = self.root.join("entries");
        ensure_safe_directory(&entries)?;
        let mut records = Vec::new();
        let mut identity_keys = HashSet::new();
        for entry in fs::read_dir(&entries).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if is_reparse_point(&metadata) || !metadata.is_dir() {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entries contains a non-directory or reparse point".into(),
                ));
            }
            let Some(record) = self.read_latest_locked(&path)? else {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entry directory has no snapshot".into(),
                ));
            };
            let directory_name = entry.file_name().to_string_lossy().into_owned();
            if directory_name != immutable_key(&record.identity)
                && directory_name != legacy_immutable_key(&record.identity)
            {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entry directory does not match its immutable identity".into(),
                ));
            }
            if !identity_keys.insert(immutable_key(&record.identity)) {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal contains duplicate immutable execution identities".into(),
                ));
            }
            records.push(record);
        }
        records.sort_by(|left, right| {
            left.identity
                .task_id
                .cmp(&right.identity.task_id)
                .then_with(|| left.identity.attempt_id.cmp(&right.identity.attempt_id))
        });
        Ok(records)
    }

    pub fn plan_reconciliation(
        &self,
        systems: &[HcsSystemSummary],
        worker_id: &str,
    ) -> Result<Vec<HcsReconciliationAction>, HcsJournalError> {
        validate_component("worker_id", worker_id, MAX_FIELD_BYTES)?;
        if systems.len() > MAX_ENUMERATED_SYSTEMS {
            return Err(HcsJournalError::Corrupt(
                "HCS enumeration returned too many compute systems".into(),
            ));
        }
        let mut system_ids = HashSet::with_capacity(systems.len());
        for system in systems {
            validate_component("hcs_system_id", &system.id, MAX_FIELD_BYTES).map_err(|_| {
                HcsJournalError::Corrupt("HCS enumeration returned an invalid system id".into())
            })?;
            if let Some(owner) = system.owner.as_deref() {
                validate_component("hcs_owner", owner, MAX_FIELD_BYTES).map_err(|_| {
                    HcsJournalError::Corrupt("HCS enumeration returned an invalid owner".into())
                })?;
            }
            if let Some(state) = system.state.as_deref() {
                validate_component("hcs_state", state, MAX_FIELD_BYTES).map_err(|_| {
                    HcsJournalError::Corrupt("HCS enumeration returned an invalid state".into())
                })?;
            }
            if !system_ids.insert(system.id.as_str()) {
                return Err(HcsJournalError::Corrupt(
                    "HCS enumeration returned duplicate system identities".into(),
                ));
            }
        }
        let all_records = self.list_records()?;
        let pending = all_records
            .iter()
            .filter(|record| record.needs_reconciliation())
            .collect::<Vec<_>>();
        let mut actions = Vec::new();

        for record in &pending {
            if record.worker_id != worker_id {
                actions.push(HcsReconciliationAction::WorkerIdentityMismatch {
                    record: (*record).clone(),
                });
                continue;
            }
            if record.lifecycle == HcsLifecycleState::Completed
                && record.cleanup == HcsCleanupState::Succeeded
                && record.hcs_system_id.is_none()
            {
                actions.push(HcsReconciliationAction::RecoverCompletedResult {
                    record: (*record).clone(),
                });
                continue;
            }
            let Some(system_id) = record.hcs_system_id.as_deref() else {
                // A matching container name is not enough to establish that a
                // successful HCS create event produced this system. Leave the
                // second pass to quarantine it instead of mutating the record.
                if systems
                    .iter()
                    .any(|system| system.id == record.container_id)
                {
                    continue;
                }
                actions.push(HcsReconciliationAction::MissingSystem {
                    record: (*record).clone(),
                });
                continue;
            };
            let Some(system) = systems.iter().find(|system| system.id == system_id) else {
                actions.push(HcsReconciliationAction::MissingSystem {
                    record: (*record).clone(),
                });
                continue;
            };
            if system.id != record.container_id || !trusted_system_summary(system) {
                actions.push(HcsReconciliationAction::IdentityMismatch {
                    record: (*record).clone(),
                    system: system.clone(),
                });
            } else if record.lifecycle == HcsLifecycleState::Completed
                && record.delivery == HcsDeliveryState::Delivered
            {
                // A delivered result must never be replayed. If the exact
                // system is still live, cleanup is the only safe action.
                actions.push(HcsReconciliationAction::CleanupRecordedSystem {
                    record: (*record).clone(),
                    system: system.clone(),
                });
            } else if record.lifecycle == HcsLifecycleState::Completed {
                actions.push(HcsReconciliationAction::RecoverCompletedResult {
                    record: (*record).clone(),
                });
            } else {
                actions.push(HcsReconciliationAction::ReattachCandidate {
                    record: (*record).clone(),
                    system: system.clone(),
                });
            }
        }

        for system in systems {
            let matching_records = all_records
                .iter()
                .filter(|record| {
                    record.hcs_system_id.as_deref() == Some(system.id.as_str())
                        || record.container_id == system.id
                })
                .collect::<Vec<_>>();
            let Some(record) = matching_records.first().copied() else {
                actions.push(HcsReconciliationAction::QuarantineSystem {
                    system: system.clone(),
                    reason: "HCS system has no exact journal identity on this Worker".into(),
                });
                continue;
            };
            if matching_records.len() != 1 {
                actions.push(HcsReconciliationAction::QuarantineSystem {
                    system: system.clone(),
                    reason: "HCS system identity matches multiple journal records".into(),
                });
                continue;
            }
            if record.hcs_system_id.as_deref() != Some(system.id.as_str()) {
                actions.push(HcsReconciliationAction::QuarantineSystem {
                    system: system.clone(),
                    reason: "HCS system identity was not recorded by a successful create event"
                        .into(),
                });
                continue;
            }
            if record.worker_id != worker_id {
                if !pending
                    .iter()
                    .any(|pending| pending.identity == record.identity)
                {
                    actions.push(HcsReconciliationAction::QuarantineSystem {
                        system: system.clone(),
                        reason: "HCS system is recorded for another Worker; this Worker will not terminate it".into(),
                    });
                }
                continue;
            }
            if !trusted_system_summary(system) {
                if !pending
                    .iter()
                    .any(|pending| pending.identity == record.identity)
                {
                    actions.push(HcsReconciliationAction::QuarantineSystem {
                        system: system.clone(),
                        reason: "HCS ownership metadata is absent or unexpected".into(),
                    });
                }
                continue;
            }
            if !record.needs_reconciliation() && record.lifecycle != HcsLifecycleState::Prepared {
                // A terminal journal record with a still-live HCS system is a
                // recorded leak. It may be cleaned only by exact identity.
                actions.push(HcsReconciliationAction::CleanupRecordedSystem {
                    record: record.clone(),
                    system: system.clone(),
                });
            }
        }
        Ok(actions)
    }
}

impl general_compute_runtime::windows_hcs::HcsLifecycleObserver for HcsJournalObserver {
    fn on_event(&mut self, event: HcsLifecycleEvent) -> Result<(), String> {
        self.journal
            .append_runtime_event(&self.identity, event)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub struct HcsJournalObserver {
    journal: HcsExecutionJournal,
    identity: HcsExecutionIdentity,
}

impl HcsJournalObserver {
    #[must_use]
    pub fn new(journal: HcsExecutionJournal, identity: HcsExecutionIdentity) -> Self {
        Self { journal, identity }
    }
}

fn runtime_event(event: HcsLifecycleEvent) -> HcsJournalEvent {
    match event {
        HcsLifecycleEvent::Created { system_id } => HcsJournalEvent::Created { system_id },
        HcsLifecycleEvent::Started => HcsJournalEvent::Started,
        HcsLifecycleEvent::Waiting => HcsJournalEvent::Waiting,
        HcsLifecycleEvent::GuestExited { exit_code } => HcsJournalEvent::GuestExited { exit_code },
        HcsLifecycleEvent::Cancelled => HcsJournalEvent::Cancelled,
        HcsLifecycleEvent::TimedOut => HcsJournalEvent::TimedOut,
        HcsLifecycleEvent::StartFailed { error } => HcsJournalEvent::StartFailed { error },
        HcsLifecycleEvent::WaitFailed { error } => HcsJournalEvent::WaitFailed { error },
        HcsLifecycleEvent::ShutdownStarted => HcsJournalEvent::ShutdownStarted,
        HcsLifecycleEvent::ShutdownCompleted { status, exit_type } => {
            HcsJournalEvent::ShutdownCompleted { status, exit_type }
        }
        HcsLifecycleEvent::ShutdownFailed { error } => HcsJournalEvent::ShutdownFailed { error },
        HcsLifecycleEvent::TerminateStarted => HcsJournalEvent::TerminateStarted,
        HcsLifecycleEvent::Terminated { status, exit_type } => {
            HcsJournalEvent::Terminated { status, exit_type }
        }
        HcsLifecycleEvent::TerminateFailed { error } => HcsJournalEvent::TerminateFailed { error },
        HcsLifecycleEvent::ResultRead { sha256, size } => {
            HcsJournalEvent::ResultRead { sha256, size }
        }
        HcsLifecycleEvent::ResultReadFailed { error } => {
            HcsJournalEvent::ResultReadFailed { error }
        }
        HcsLifecycleEvent::Closed => HcsJournalEvent::Closed,
    }
}

fn apply_event(
    record: &mut HcsJournalRecord,
    event: HcsJournalEvent,
) -> Result<(), HcsJournalError> {
    match event {
        HcsJournalEvent::CreateRequested => {
            require_state(record, &[HcsLifecycleState::Prepared], "create request")?;
            record.lifecycle = HcsLifecycleState::Creating;
        }
        HcsJournalEvent::Created { system_id } => {
            require_state(record, &[HcsLifecycleState::Creating], "created")?;
            validate_system_id(&system_id)
                .map_err(|_| HcsJournalError::Event("HCS system identity is invalid".into()))?;
            if system_id != record.container_id {
                return Err(HcsJournalError::Event(
                    "HCS system identity does not match the operator container id".into(),
                ));
            }
            if record
                .hcs_system_id
                .as_deref()
                .is_some_and(|existing| existing != system_id)
            {
                return Err(HcsJournalError::IdentityConflict);
            }
            record.hcs_system_id = Some(system_id);
            record.lifecycle = HcsLifecycleState::Created;
        }
        HcsJournalEvent::Started => {
            require_state(record, &[HcsLifecycleState::Created], "started")?;
            record.lifecycle = HcsLifecycleState::Running;
        }
        HcsJournalEvent::Waiting => {
            require_state(
                record,
                &[HcsLifecycleState::Running, HcsLifecycleState::Waiting],
                "waiting",
            )?;
            record.lifecycle = HcsLifecycleState::Waiting;
        }
        HcsJournalEvent::GuestExited { exit_code } => {
            require_state(
                record,
                &[HcsLifecycleState::Running, HcsLifecycleState::Waiting],
                "guest exit",
            )?;
            record.guest_exit_code = exit_code;
            record.lifecycle = HcsLifecycleState::GuestExited;
        }
        HcsJournalEvent::Cancelled => {
            require_state(
                record,
                &[
                    HcsLifecycleState::Created,
                    HcsLifecycleState::Running,
                    HcsLifecycleState::Waiting,
                    HcsLifecycleState::GuestExited,
                ],
                "cancellation",
            )?;
            record.lifecycle = HcsLifecycleState::Cancelled;
            record.cleanup = HcsCleanupState::Pending;
        }
        HcsJournalEvent::TimedOut => {
            require_state(
                record,
                &[
                    HcsLifecycleState::Created,
                    HcsLifecycleState::Running,
                    HcsLifecycleState::Waiting,
                    HcsLifecycleState::GuestExited,
                ],
                "timeout",
            )?;
            record.lifecycle = HcsLifecycleState::TimedOut;
            record.cleanup = HcsCleanupState::Pending;
        }
        HcsJournalEvent::StartFailed { error } => {
            require_state(
                record,
                &[HcsLifecycleState::Creating, HcsLifecycleState::Created],
                "start failure",
            )?;
            record.lifecycle = HcsLifecycleState::Failed;
            record.last_error = Some(bound_error(error));
        }
        HcsJournalEvent::WaitFailed { error } => {
            require_state(
                record,
                &[HcsLifecycleState::Running, HcsLifecycleState::Waiting],
                "wait failure",
            )?;
            record.lifecycle = HcsLifecycleState::Failed;
            record.last_error = Some(bound_error(error));
        }
        HcsJournalEvent::ShutdownStarted => {
            require_state(record, &[HcsLifecycleState::GuestExited], "shutdown")?;
            record.lifecycle = HcsLifecycleState::ShuttingDown;
        }
        HcsJournalEvent::ShutdownCompleted { status, exit_type } => {
            require_state(
                record,
                &[HcsLifecycleState::ShuttingDown],
                "shutdown completion",
            )?;
            validate_hcs_exit(status, &exit_type)?;
            record.hcs_exit_status = Some(status);
            record.hcs_exit_type = Some(exit_type);
            record.cleanup = HcsCleanupState::Succeeded;
        }
        HcsJournalEvent::ShutdownFailed { error } => {
            require_state(
                record,
                &[HcsLifecycleState::ShuttingDown],
                "shutdown failure",
            )?;
            record.lifecycle = HcsLifecycleState::RecoveryRequired;
            record.cleanup = HcsCleanupState::Failed;
            record.last_error = Some(bound_error(error));
        }
        HcsJournalEvent::TerminateStarted => {
            require_state(
                record,
                &[
                    HcsLifecycleState::Created,
                    HcsLifecycleState::Running,
                    HcsLifecycleState::Waiting,
                    HcsLifecycleState::GuestExited,
                    HcsLifecycleState::ShuttingDown,
                    HcsLifecycleState::Failed,
                    HcsLifecycleState::Cancelled,
                    HcsLifecycleState::TimedOut,
                    HcsLifecycleState::RecoveryRequired,
                    HcsLifecycleState::Completed,
                    HcsLifecycleState::Abandoned,
                ],
                "termination",
            )?;
            record.cleanup = HcsCleanupState::Pending;
            record.reconciliation = Some(HcsReconciliationOutcome::CleanupRequired);
        }
        HcsJournalEvent::Terminated { status, exit_type } => {
            require_state(
                record,
                &[
                    HcsLifecycleState::Terminating,
                    HcsLifecycleState::RecoveryRequired,
                    HcsLifecycleState::Failed,
                    HcsLifecycleState::Cancelled,
                    HcsLifecycleState::TimedOut,
                    HcsLifecycleState::Created,
                    HcsLifecycleState::Running,
                    HcsLifecycleState::Waiting,
                    HcsLifecycleState::GuestExited,
                    HcsLifecycleState::ShuttingDown,
                    HcsLifecycleState::Completed,
                    HcsLifecycleState::Abandoned,
                ],
                "termination completion",
            )?;
            validate_hcs_exit(status, &exit_type)?;
            record.hcs_exit_status = Some(status);
            record.hcs_exit_type = Some(exit_type);
            record.cleanup = HcsCleanupState::Succeeded;
        }
        HcsJournalEvent::TerminateFailed { error } => {
            require_state(
                record,
                &[
                    HcsLifecycleState::Created,
                    HcsLifecycleState::Running,
                    HcsLifecycleState::Waiting,
                    HcsLifecycleState::GuestExited,
                    HcsLifecycleState::ShuttingDown,
                    HcsLifecycleState::Failed,
                    HcsLifecycleState::Cancelled,
                    HcsLifecycleState::TimedOut,
                    HcsLifecycleState::RecoveryRequired,
                    HcsLifecycleState::Completed,
                    HcsLifecycleState::Abandoned,
                ],
                "termination failure",
            )?;
            record.lifecycle = HcsLifecycleState::RecoveryRequired;
            record.cleanup = HcsCleanupState::Failed;
            record.last_error = Some(bound_error(error));
        }
        HcsJournalEvent::ResultRead { sha256, size } => {
            require_state(record, &[HcsLifecycleState::Closed], "result read")?;
            if record.cleanup != HcsCleanupState::Succeeded
                || record.guest_exit_code != Some(0)
                || record.hcs_exit_status != Some(0)
            {
                return Err(HcsJournalError::Event(
                    "result cannot be recorded before authoritative HCS cleanup".into(),
                ));
            }
            validate_digest("result_sha256", &sha256)?;
            if u64::try_from(size).unwrap_or(u64::MAX) > MAX_RECORD_BYTES {
                return Err(HcsJournalError::Event(
                    "result size exceeds the journal bound".into(),
                ));
            }
            record.result_sha256 = Some(sha256);
            record.result_size = Some(size as u64);
            record.lifecycle = HcsLifecycleState::ResultRead;
            record.delivery = HcsDeliveryState::AwaitingValidation;
        }
        HcsJournalEvent::ResultReadFailed { error } => {
            require_state(record, &[HcsLifecycleState::Closed], "result read failure")?;
            record.lifecycle = HcsLifecycleState::Failed;
            record.last_error = Some(bound_error(error));
        }
        HcsJournalEvent::Closed => {
            if matches!(
                record.lifecycle,
                HcsLifecycleState::Completed | HcsLifecycleState::ResultRead
            ) {
                return Err(HcsJournalError::Event(
                    "closed event cannot follow a terminal result".into(),
                ));
            }
            if !matches!(
                record.lifecycle,
                HcsLifecycleState::Failed
                    | HcsLifecycleState::Cancelled
                    | HcsLifecycleState::TimedOut
                    | HcsLifecycleState::RecoveryRequired
                    | HcsLifecycleState::Prepared
            ) {
                record.lifecycle = HcsLifecycleState::Closed;
            }
        }
        HcsJournalEvent::Completed => {
            require_state(record, &[HcsLifecycleState::ResultRead], "completion")?;
            if record.cleanup != HcsCleanupState::Succeeded
                || record.guest_exit_code != Some(0)
                || record.hcs_exit_status != Some(0)
                || record.result_sha256.is_none()
                || record.result_size.is_none()
            {
                return Err(HcsJournalError::Event(
                    "completed execution is missing authoritative lifecycle or result state".into(),
                ));
            }
            record.lifecycle = HcsLifecycleState::Completed;
        }
        HcsJournalEvent::Failed { error } => {
            if record.lifecycle == HcsLifecycleState::Completed {
                return Err(HcsJournalError::Event(
                    "completed execution cannot be changed to failed".into(),
                ));
            }
            if record.cleanup == HcsCleanupState::Failed
                || record.lifecycle == HcsLifecycleState::RecoveryRequired
            {
                record.lifecycle = HcsLifecycleState::RecoveryRequired;
            } else {
                record.lifecycle = HcsLifecycleState::Failed;
            }
            record.last_error = Some(bound_error(error));
        }
        HcsJournalEvent::DeliveryValidated => {
            if record.lifecycle != HcsLifecycleState::Completed
                || record.cleanup != HcsCleanupState::Succeeded
                || !matches!(
                    record.delivery,
                    HcsDeliveryState::AwaitingValidation
                        | HcsDeliveryState::Validated
                        | HcsDeliveryState::Delivered
                )
            {
                return Err(HcsJournalError::Event(
                    "delivery validation requires a completed, cleaned execution".into(),
                ));
            }
            record.delivery = HcsDeliveryState::Validated;
        }
        HcsJournalEvent::Delivered => {
            if record.lifecycle != HcsLifecycleState::Completed
                || record.cleanup != HcsCleanupState::Succeeded
                || !matches!(
                    record.delivery,
                    HcsDeliveryState::Validated | HcsDeliveryState::Delivered
                )
            {
                return Err(HcsJournalError::Event(
                    "delivery requires validated completed execution".into(),
                ));
            }
            record.delivery = HcsDeliveryState::Delivered;
        }
        HcsJournalEvent::DeliveryFailed { error } => {
            if record.lifecycle != HcsLifecycleState::Completed {
                return Err(HcsJournalError::Event(
                    "delivery failure requires a completed execution".into(),
                ));
            }
            record.delivery = HcsDeliveryState::Failed;
            record.last_error = Some(bound_error(error));
        }
        HcsJournalEvent::CleanupConfirmedAbsent => {
            require_state(
                record,
                &[
                    HcsLifecycleState::Creating,
                    HcsLifecycleState::Created,
                    HcsLifecycleState::Running,
                    HcsLifecycleState::Waiting,
                    HcsLifecycleState::GuestExited,
                    HcsLifecycleState::ShuttingDown,
                    HcsLifecycleState::Terminating,
                    HcsLifecycleState::ResultRead,
                    HcsLifecycleState::Closed,
                    HcsLifecycleState::Failed,
                    HcsLifecycleState::Cancelled,
                    HcsLifecycleState::TimedOut,
                    HcsLifecycleState::RecoveryRequired,
                    HcsLifecycleState::Orphaned,
                ],
                "cleanup confirmation",
            )?;
            if record.reconciliation != Some(HcsReconciliationOutcome::MissingSystem)
                || record.lifecycle != HcsLifecycleState::RecoveryRequired
            {
                return Err(HcsJournalError::Event(
                    "cleanup absence requires an authoritative missing-system reconciliation"
                        .into(),
                ));
            }
            record.cleanup = HcsCleanupState::Succeeded;
            if record.delivery == HcsDeliveryState::Delivered {
                // A delivered completed result remains terminal after the
                // authoritative absence confirmation. It is not abandoned or
                // replayed merely because its HCS system is already gone.
                record.lifecycle = HcsLifecycleState::Completed;
            }
        }
        HcsJournalEvent::Abandoned { reason } => {
            if record.cleanup != HcsCleanupState::Succeeded
                || (record.lifecycle == HcsLifecycleState::Completed
                    && record.delivery == HcsDeliveryState::Delivered)
            {
                return Err(HcsJournalError::Event(
                    "an execution cannot be abandoned before cleanup or after delivery".into(),
                ));
            }
            record.lifecycle = HcsLifecycleState::Abandoned;
            record.last_error = Some(bound_error(reason));
        }
        HcsJournalEvent::Reconciled { outcome, detail } => {
            record.reconciliation = Some(outcome);
            if let Some(detail) = detail {
                record.last_error = Some(bound_error(detail));
            }
            if matches!(
                outcome,
                HcsReconciliationOutcome::MissingSystem
                    | HcsReconciliationOutcome::CleanupRequired
                    | HcsReconciliationOutcome::CleanupFailed
                    | HcsReconciliationOutcome::WorkerIdentityMismatch
                    | HcsReconciliationOutcome::HcsIdentityMismatch
                    | HcsReconciliationOutcome::LeaseRequired
                    | HcsReconciliationOutcome::Corrupt
            ) {
                record.lifecycle = HcsLifecycleState::RecoveryRequired;
            }
        }
    }
    Ok(())
}

fn require_state(
    record: &HcsJournalRecord,
    allowed: &[HcsLifecycleState],
    event: &str,
) -> Result<(), HcsJournalError> {
    if allowed.contains(&record.lifecycle) {
        Ok(())
    } else {
        Err(HcsJournalError::Event(format!(
            "{event} event is invalid after {:?}",
            record.lifecycle
        )))
    }
}

fn validate_system_id(value: &str) -> Result<(), HcsJournalError> {
    validate_component("hcs_system_id", value, MAX_FIELD_BYTES)
        .map_err(|_| HcsJournalError::Event("HCS system identity is invalid".into()))?;
    if !is_safe_component(value) {
        return Err(HcsJournalError::Event(
            "HCS system identity contains unsafe characters".into(),
        ));
    }
    Ok(())
}

fn trusted_system_summary(system: &HcsSystemSummary) -> bool {
    system.owner.as_deref() == Some(HIVEMIND_HCS_OWNER)
        && system.id.starts_with(HIVEMIND_HCS_SYSTEM_ID_PREFIX)
        && validate_system_id(&system.id).is_ok()
}

fn validate_hcs_exit(status: i32, exit_type: &str) -> Result<(), HcsJournalError> {
    if status != 0 {
        return Err(HcsJournalError::Event(
            "HCS cleanup returned a non-success HRESULT".into(),
        ));
    }
    if !matches!(exit_type, "GracefulExit" | "ForcedExit") {
        return Err(HcsJournalError::Event(
            "HCS cleanup exit type is not authoritative".into(),
        ));
    }
    Ok(())
}

fn immutable_fields_match(record: &HcsJournalRecord, intent: &HcsExecutionIntent) -> bool {
    record.identity == intent.identity
        && record.worker_id == intent.worker_id
        && record.backend_id == intent.backend_id
        && record.guest_image_digest == intent.guest_image_digest
        && record.runner_sha256 == intent.runner_sha256
        && record.policy_digest == intent.policy_digest
        && record.spec_digest == intent.spec_digest
        && record.input_sha256 == intent.input_sha256
        && record.container_id == intent.container_id
        && record.scratch_path == intent.scratch_path
        && record.result_path == intent.result_path
}

fn immutable_identity_matches_without_generation(
    left: &HcsExecutionIdentity,
    right: &HcsExecutionIdentity,
) -> bool {
    left.task_id == right.task_id
        && left.execution_id == right.execution_id
        && left.attempt_id == right.attempt_id
        && left.idempotency_key == right.idempotency_key
        && left.request_digest == right.request_digest
}

fn validate_component(name: &str, value: &str, max_bytes: usize) -> Result<(), HcsJournalError> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(HcsJournalError::InvalidIntent(format!(
            "{name} must be non-empty, bounded, and free of control characters"
        )));
    }
    Ok(())
}

fn validate_digest(name: &str, value: &str) -> Result<(), HcsJournalError> {
    validate_component(name, value, 128)?;
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(HcsJournalError::InvalidIntent(format!(
            "{name} must use sha256:<64 hex characters>"
        )));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(HcsJournalError::InvalidIntent(format!(
            "{name} must use sha256:<64 hex characters>"
        )));
    }
    Ok(())
}

fn validate_operator_path(name: &str, path: &Path) -> Result<(), HcsJournalError> {
    let value = path.to_string_lossy();
    validate_component(name, &value, MAX_FIELD_BYTES)?;
    if !path.is_absolute() || value.contains('\0') {
        return Err(HcsJournalError::InvalidIntent(format!(
            "{name} must be an absolute operator-owned path"
        )));
    }
    Ok(())
}

fn is_safe_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn bound_error(value: String) -> String {
    if value.len() <= MAX_ERROR_BYTES {
        return value;
    }
    let mut end = MAX_ERROR_BYTES;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].to_owned()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn is_temporary_snapshot_name(name: &str) -> bool {
    let Some(name) = name.strip_prefix('.') else {
        return false;
    };
    let Some(name) = name.strip_suffix(".tmp") else {
        return false;
    };
    let mut fields = name.split('.');
    fields
        .next()
        .is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        && fields.next().is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
        })
        && fields.next().is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
        })
        && fields.next().is_none()
}

fn immutable_key(identity: &HcsExecutionIdentity) -> String {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(b"hivemind-hcs-journal-key");
    canonical.push(0);
    canonical.push(HCS_JOURNAL_KEY_VERSION);
    for value in [
        &identity.task_id,
        &identity.execution_id,
        &identity.attempt_id,
        &identity.idempotency_key,
        &identity.request_digest,
    ] {
        append_key_field(&mut canonical, value.as_bytes());
    }
    // Lease generations are checked as immutable record metadata, but they do
    // not select a different journal directory. This prevents a refreshed
    // lease from creating a second entry for the same execution attempt.
    general_compute_runtime::sha256_digest(&canonical)
        .strip_prefix("sha256:")
        .expect("sha256 digest has a prefix")
        .to_owned()
}

fn append_key_field(canonical: &mut Vec<u8>, value: &[u8]) {
    canonical.extend_from_slice(&(value.len() as u64).to_be_bytes());
    canonical.extend_from_slice(value);
}

fn legacy_immutable_key(identity: &HcsExecutionIdentity) -> String {
    let bytes = serde_json::to_vec(identity).expect("HCS identity serialization is infallible");
    general_compute_runtime::sha256_digest(&bytes)
        .strip_prefix("sha256:")
        .expect("sha256 digest has a prefix")
        .to_owned()
}

fn ensure_safe_directory(path: &Path) -> Result<(), HcsJournalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if is_reparse_point(&metadata) || !metadata.is_dir() {
                return Err(HcsJournalError::InvalidRoot(format!(
                    "{} is not a regular directory",
                    path.display()
                )));
            }
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }

    if let Some(parent) = path.parent().filter(|parent| *parent != path) {
        ensure_safe_directory(parent)?;
    }
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if is_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(HcsJournalError::InvalidRoot(format!(
            "{} was not created as a regular directory",
            path.display()
        )));
    }
    Ok(())
}

fn ensure_safe_regular_file(path: &Path) -> Result<(), HcsJournalError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if is_reparse_point(&metadata) || !metadata.is_file() {
        return Err(HcsJournalError::Corrupt(format!(
            "{} is not a regular journal snapshot",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & REPARSE_POINT_ATTRIBUTE != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn io_error(error: std::io::Error) -> HcsJournalError {
    HcsJournalError::Io(error.to_string())
}

impl HcsExecutionJournal {
    /// Remove crash-orphaned snapshot writers before rebuilding the index.
    fn cleanup_temporary_snapshots_locked(&self) -> Result<(), HcsJournalError> {
        let entries = self.root.join("entries");
        for entry in fs::read_dir(&entries).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let directory = entry.path();
            let metadata = fs::symlink_metadata(&directory).map_err(io_error)?;
            if is_reparse_point(&metadata) || !metadata.is_dir() {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entries contains a non-directory or reparse point".into(),
                ));
            }
            for snapshot in fs::read_dir(&directory).map_err(io_error)? {
                let snapshot = snapshot.map_err(io_error)?;
                let path = snapshot.path();
                let name = snapshot.file_name().to_string_lossy().into_owned();
                if !is_temporary_snapshot_name(&name) {
                    continue;
                }
                let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
                if is_reparse_point(&metadata) || !metadata.is_file() {
                    return Err(HcsJournalError::Corrupt(
                        "HCS journal temporary snapshot is not a regular file".into(),
                    ));
                }
                let age_ms = metadata
                    .modified()
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .and_then(|age| u64::try_from(age.as_millis()).ok())
                    .unwrap_or_default();
                if age_ms >= TEMPORARY_SNAPSHOT_MIN_AGE_MS {
                    fs::remove_file(path).map_err(io_error)?;
                }
            }
        }
        Ok(())
    }

    fn rebuild_index_locked(&self) -> Result<(), HcsJournalError> {
        let entries = self.root.join("entries");
        let mut rebuilt = HashMap::new();
        for entry in fs::read_dir(&entries).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if is_reparse_point(&metadata) || !metadata.is_dir() {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entries contains a non-directory or reparse point".into(),
                ));
            }
            let Some(record) = self.read_latest_locked(&path)? else {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entry directory has no snapshot".into(),
                ));
            };
            let directory_name = entry.file_name().to_string_lossy().into_owned();
            if directory_name != immutable_key(&record.identity)
                && directory_name != legacy_immutable_key(&record.identity)
            {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entry directory does not match its immutable identity".into(),
                ));
            }
            if rebuilt
                .insert(immutable_key(&record.identity), path)
                .is_some()
            {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal contains duplicate immutable execution identities".into(),
                ));
            }
        }
        *self.index.lock().unwrap_or_else(|error| error.into_inner()) = rebuilt;
        Ok(())
    }

    pub fn prune_terminal_records(&self) -> Result<usize, HcsJournalError> {
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        self.prune_terminal_records_locked(now_ms(), TERMINAL_RETENTION_MS)
    }

    fn prune_terminal_records_locked(
        &self,
        now_ms: u64,
        retention_ms: u64,
    ) -> Result<usize, HcsJournalError> {
        let entries = self.root.join("entries");
        let mut compacted_snapshots = 0;
        for entry in fs::read_dir(&entries).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if is_reparse_point(&metadata) || !metadata.is_dir() {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entries contains a non-directory or reparse point".into(),
                ));
            }
            let Some(record) = self.read_latest_locked(&path)? else {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entry directory has no snapshot".into(),
                ));
            };
            let eligible = matches!(
                record.lifecycle,
                HcsLifecycleState::Completed
                    | HcsLifecycleState::Failed
                    | HcsLifecycleState::Cancelled
                    | HcsLifecycleState::TimedOut
                    | HcsLifecycleState::Abandoned
            ) && !record.needs_reconciliation()
                && record.cleanup == HcsCleanupState::Succeeded
                && now_ms.saturating_sub(record.updated_at_ms) >= retention_ms;
            if !eligible {
                continue;
            }
            for snapshot in fs::read_dir(&path).map_err(io_error)? {
                let snapshot = snapshot.map_err(io_error)?;
                let snapshot_path = snapshot.path();
                let name = snapshot.file_name().to_string_lossy().into_owned();
                let Some(sequence_text) = name.strip_suffix(".json") else {
                    continue;
                };
                if !sequence_text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(HcsJournalError::Corrupt(
                        "HCS journal snapshot name is invalid".into(),
                    ));
                }
                let sequence = sequence_text.parse::<u64>().map_err(|_| {
                    HcsJournalError::Corrupt("HCS journal snapshot sequence is invalid".into())
                })?;
                if sequence < record.sequence {
                    fs::remove_file(snapshot_path).map_err(io_error)?;
                    compacted_snapshots += 1;
                }
            }
        }
        Ok(compacted_snapshots)
    }

    fn entry_directory(&self, identity: &HcsExecutionIdentity) -> Result<PathBuf, HcsJournalError> {
        let entries = self.root.join("entries");
        ensure_safe_directory(&entries)?;
        let current = entries.join(immutable_key(identity));
        let legacy = entries.join(legacy_immutable_key(identity));
        let key = immutable_key(identity);

        if let Some(directory) = self
            .index
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
            .cloned()
        {
            if directory.exists() {
                ensure_safe_directory(&directory)?;
                return Ok(directory);
            }
            self.index
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&key);
        }

        let current_exists = current.exists();
        let legacy_exists = legacy.exists();
        if current_exists && legacy_exists {
            return Err(HcsJournalError::Corrupt(
                "HCS journal identity has both current and legacy entry directories".into(),
            ));
        }
        let directory = if current_exists {
            current
        } else if legacy_exists {
            legacy
        } else {
            current
        };
        if directory.exists() {
            ensure_safe_directory(&directory)?;
            let Some(record) = self.read_latest_locked(&directory)? else {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entry directory has no snapshot".into(),
                ));
            };
            if !immutable_identity_matches_without_generation(&record.identity, identity) {
                return Err(HcsJournalError::IdentityConflict);
            }
            self.index
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(key, directory.clone());
        }
        Ok(directory)
    }

    fn read_latest_locked(
        &self,
        directory: &Path,
    ) -> Result<Option<HcsJournalRecord>, HcsJournalError> {
        if !directory.exists() {
            return Ok(None);
        }
        ensure_safe_directory(directory)?;
        let mut latest: Option<(u64, PathBuf)> = None;
        for entry in fs::read_dir(directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if is_reparse_point(&metadata) {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entry contains a reparse point".into(),
                ));
            }
            if metadata.is_dir() {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal entry contains a nested directory".into(),
                ));
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(sequence_text) = name.strip_suffix(".json") else {
                continue;
            };
            if !sequence_text.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(HcsJournalError::Corrupt(
                    "HCS journal snapshot name is invalid".into(),
                ));
            }
            let sequence = sequence_text.parse::<u64>().map_err(|_| {
                HcsJournalError::Corrupt("HCS journal snapshot sequence is invalid".into())
            })?;
            if latest
                .as_ref()
                .is_none_or(|(current, _)| sequence > *current)
            {
                latest = Some((sequence, path));
            }
        }
        let Some((sequence, path)) = latest else {
            return Ok(None);
        };
        ensure_safe_regular_file(&path)?;
        let metadata = fs::metadata(&path).map_err(io_error)?;
        if metadata.len() > MAX_RECORD_BYTES {
            return Err(HcsJournalError::Corrupt(
                "HCS journal snapshot exceeds the size limit".into(),
            ));
        }
        let bytes = fs::read(&path).map_err(io_error)?;
        let record: HcsJournalRecord = serde_json::from_slice(&bytes).map_err(|error| {
            HcsJournalError::Corrupt(format!("HCS journal snapshot is invalid JSON: {error}"))
        })?;
        if record.sequence != sequence {
            return Err(HcsJournalError::Corrupt(
                "HCS journal snapshot sequence does not match its filename".into(),
            ));
        }
        record.validate()?;
        Ok(Some(record))
    }

    fn append_snapshot_locked(
        &self,
        directory: &Path,
        record: &HcsJournalRecord,
    ) -> Result<(), HcsJournalError> {
        ensure_safe_directory(directory)?;
        let final_path = directory.join(format!("{:020}.json", record.sequence));
        if final_path.exists() {
            return Err(HcsJournalError::SequenceConflict);
        }
        let temporary_path = directory.join(format!(
            ".{:020}.{}.{}.tmp",
            record.sequence,
            std::process::id(),
            now_ms()
        ));
        let bytes =
            serde_json::to_vec(record).map_err(|error| HcsJournalError::Io(error.to_string()))?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(HcsJournalError::Corrupt(
                "HCS journal snapshot exceeds the size limit".into(),
            ));
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .map_err(io_error)?;
        file.write_all(&bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        if final_path.exists() {
            let _ = fs::remove_file(&temporary_path);
            return Err(HcsJournalError::SequenceConflict);
        }
        fs::rename(&temporary_path, &final_path).map_err(|error| {
            let _ = fs::remove_file(&temporary_path);
            io_error(error)
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use general_compute_runtime::windows_hcs::HcsLifecycleObserver;
    use tempfile::TempDir;

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn intent() -> HcsExecutionIntent {
        let root =
            std::env::temp_dir().join(format!("hivemind-journal-test-{}", std::process::id()));
        HcsExecutionIntent {
            identity: HcsExecutionIdentity {
                task_id: "task-1".into(),
                execution_id: "execution-1".into(),
                attempt_id: "attempt-1".into(),
                idempotency_key: "idempotency-1".into(),
                request_digest: digest('a'),
                transfer_generation: Some(1),
            },
            worker_id: "worker-1".into(),
            backend_id: "windows-backend".into(),
            guest_image_digest: digest('b'),
            runner_sha256: digest('c'),
            policy_digest: digest('d'),
            spec_digest: digest('e'),
            input_sha256: digest('f'),
            container_id: "hivemind-task-1".into(),
            scratch_path: root.join("scratch"),
            result_path: root.join("scratch").join("result.json"),
        }
    }

    fn system(id: &str) -> HcsSystemSummary {
        HcsSystemSummary {
            id: id.into(),
            owner: Some("hivemind".into()),
            state: Some("Running".into()),
        }
    }

    fn complete_without_delivery(journal: &HcsExecutionJournal, identity: &HcsExecutionIdentity) {
        journal
            .append_event(identity, HcsJournalEvent::CreateRequested)
            .unwrap();
        journal
            .append_event(
                identity,
                HcsJournalEvent::Created {
                    system_id: "hivemind-task-1".into(),
                },
            )
            .unwrap();
        journal
            .append_event(identity, HcsJournalEvent::Started)
            .unwrap();
        journal
            .append_event(
                identity,
                HcsJournalEvent::GuestExited { exit_code: Some(0) },
            )
            .unwrap();
        journal
            .append_event(identity, HcsJournalEvent::ShutdownStarted)
            .unwrap();
        journal
            .append_event(
                identity,
                HcsJournalEvent::ShutdownCompleted {
                    status: 0,
                    exit_type: "GracefulExit".into(),
                },
            )
            .unwrap();
        journal
            .append_event(identity, HcsJournalEvent::Closed)
            .unwrap();
        journal
            .append_event(
                identity,
                HcsJournalEvent::ResultRead {
                    sha256: digest('1'),
                    size: 42,
                },
            )
            .unwrap();
        journal
            .append_event(identity, HcsJournalEvent::Completed)
            .unwrap();
    }

    fn complete_and_deliver(journal: &HcsExecutionJournal, identity: &HcsExecutionIdentity) {
        complete_without_delivery(journal, identity);
        journal
            .append_event(identity, HcsJournalEvent::DeliveryValidated)
            .unwrap();
        journal
            .append_event(identity, HcsJournalEvent::Delivered)
            .unwrap();
    }

    #[test]
    fn journal_writes_atomic_snapshots_and_reloads_latest_record() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        journal
            .append_event(&identity, HcsJournalEvent::CreateRequested)
            .unwrap();
        let record = journal
            .append_event(
                &identity,
                HcsJournalEvent::Created {
                    system_id: "hivemind-task-1".into(),
                },
            )
            .unwrap();
        assert_eq!(record.sequence, 3);
        assert_eq!(
            journal.load(&identity).unwrap().lifecycle,
            HcsLifecycleState::Created
        );
        assert_eq!(journal.list_records().unwrap().len(), 1);
    }

    #[test]
    fn later_task_cannot_replace_immutable_identity_or_paths() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        journal.begin(intent()).unwrap();
        let mut changed = intent();
        changed.backend_id = "different-backend".into();
        assert_eq!(
            journal.begin(changed).unwrap_err().to_string(),
            HcsJournalError::IdentityConflict.to_string()
        );
    }

    #[test]
    fn start_failed_after_created_keeps_the_exact_system_for_recovery() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        journal
            .append_event(&identity, HcsJournalEvent::CreateRequested)
            .unwrap();
        journal
            .append_event(
                &identity,
                HcsJournalEvent::Created {
                    system_id: "hivemind-task-1".into(),
                },
            )
            .unwrap();
        let record = journal
            .append_event(
                &identity,
                HcsJournalEvent::StartFailed {
                    error: "guest process creation failed".into(),
                },
            )
            .unwrap();
        assert_eq!(record.lifecycle, HcsLifecycleState::Failed);
        assert_eq!(record.hcs_system_id.as_deref(), Some("hivemind-task-1"));
        assert_eq!(record.cleanup, HcsCleanupState::Pending);
        assert!(record.needs_reconciliation());
    }

    #[test]
    fn lease_generation_change_cannot_replace_an_existing_attempt() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        journal.begin(intent()).unwrap();
        let mut refreshed = intent();
        refreshed.identity.transfer_generation = Some(2);
        assert!(matches!(
            journal.begin(refreshed),
            Err(HcsJournalError::IdentityConflict)
        ));
    }

    #[test]
    fn malformed_delivery_state_is_fail_closed() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        let directory = journal.entry_directory(&identity).unwrap();
        let mut record = journal.load(&identity).unwrap();
        record.sequence = 2;
        record.lifecycle = HcsLifecycleState::Prepared;
        record.cleanup = HcsCleanupState::Succeeded;
        record.delivery = HcsDeliveryState::Delivered;
        fs::write(
            directory.join("00000000000000000002.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            journal.load(&identity),
            Err(HcsJournalError::Corrupt(_))
        ));
    }

    #[test]
    fn corrupt_snapshot_is_fail_closed_and_temporary_files_are_ignored() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        let directory = journal.entry_directory(&identity).unwrap();
        fs::write(directory.join(".00000000000000000002.tmp"), b"not-json").unwrap();
        let record = journal.load(&identity).unwrap();
        assert_eq!(record.sequence, 1);
        fs::write(directory.join("00000000000000000002.json"), b"not-json").unwrap();
        assert!(matches!(
            journal.load(&identity),
            Err(HcsJournalError::Corrupt(_))
        ));
    }

    #[test]
    fn runtime_events_record_cleanup_and_result_without_payload_bytes() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        journal
            .append_event(&identity, HcsJournalEvent::CreateRequested)
            .unwrap();
        let mut observer = HcsJournalObserver::new(journal.clone(), identity.clone());
        observer
            .on_event(HcsLifecycleEvent::Created {
                system_id: "hivemind-task-1".into(),
            })
            .unwrap();
        observer.on_event(HcsLifecycleEvent::Started).unwrap();
        observer
            .on_event(HcsLifecycleEvent::GuestExited { exit_code: Some(0) })
            .unwrap();
        observer
            .on_event(HcsLifecycleEvent::ShutdownStarted)
            .unwrap();
        observer
            .on_event(HcsLifecycleEvent::ShutdownCompleted {
                status: 0,
                exit_type: "GracefulExit".into(),
            })
            .unwrap();
        observer.on_event(HcsLifecycleEvent::Closed).unwrap();
        observer
            .on_event(HcsLifecycleEvent::ResultRead {
                sha256: digest('1'),
                size: 42,
            })
            .unwrap();
        let record = journal.load(&identity).unwrap();
        assert_eq!(record.guest_exit_code, Some(0));
        assert_eq!(record.cleanup, HcsCleanupState::Succeeded);
        assert_eq!(record.result_size, Some(42));
        assert!(record.result_sha256.is_some());
    }

    #[test]
    fn completed_delivered_live_system_is_cleanup_only() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        complete_and_deliver(&journal, &identity);
        journal
            .append_event(&identity, HcsJournalEvent::TerminateStarted)
            .unwrap();

        let actions = journal
            .plan_reconciliation(&[system("hivemind-task-1")], "worker-1")
            .unwrap();
        assert!(matches!(
            actions.as_slice(),
            [HcsReconciliationAction::CleanupRecordedSystem { record, system }]
                if record.lifecycle == HcsLifecycleState::Completed
                    && record.cleanup == HcsCleanupState::Pending
                    && record.delivery == HcsDeliveryState::Delivered
                    && system.id == "hivemind-task-1"
        ));
    }

    #[test]
    fn completed_delivered_missing_system_stays_terminal_after_absence_confirmation() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        complete_and_deliver(&journal, &identity);
        journal
            .append_event(&identity, HcsJournalEvent::TerminateStarted)
            .unwrap();

        let actions = journal.plan_reconciliation(&[], "worker-1").unwrap();
        assert!(matches!(
            actions.as_slice(),
            [HcsReconciliationAction::MissingSystem { .. }]
        ));
        journal
            .append_event(
                &identity,
                HcsJournalEvent::Reconciled {
                    outcome: HcsReconciliationOutcome::MissingSystem,
                    detail: Some("system is absent".into()),
                },
            )
            .unwrap();
        journal
            .append_event(&identity, HcsJournalEvent::CleanupConfirmedAbsent)
            .unwrap();
        let record = journal.load(&identity).unwrap();
        assert_eq!(record.lifecycle, HcsLifecycleState::Completed);
        assert_eq!(record.cleanup, HcsCleanupState::Succeeded);
        assert_eq!(record.delivery, HcsDeliveryState::Delivered);
        assert!(!record.needs_reconciliation());
        assert!(matches!(
            journal.append_event(
                &identity,
                HcsJournalEvent::Abandoned {
                    reason: "must not abandon delivered result".into(),
                }
            ),
            Err(HcsJournalError::Event(_))
        ));
    }

    #[test]
    fn enumeration_bounds_and_duplicate_identities_fail_closed() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let systems = (0..=MAX_ENUMERATED_SYSTEMS)
            .map(|index| system(&format!("hivemind-system-{index}")))
            .collect::<Vec<_>>();
        assert!(matches!(
            journal.plan_reconciliation(&systems, "worker-1"),
            Err(HcsJournalError::Corrupt(_))
        ));

        let duplicate = system("hivemind-duplicate");
        assert!(matches!(
            journal.plan_reconciliation(&[duplicate.clone(), duplicate], "worker-1"),
            Err(HcsJournalError::Corrupt(_))
        ));
    }

    #[test]
    fn malformed_enumerated_metadata_fails_closed() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let mut invalid_owner = system("hivemind-invalid-owner");
        invalid_owner.owner = Some("bad\nowner".into());
        assert!(matches!(
            journal.plan_reconciliation(&[invalid_owner], "worker-1"),
            Err(HcsJournalError::Corrupt(_))
        ));

        let mut invalid_state = system("hivemind-invalid-state");
        invalid_state.state = Some("bad\u{0000}state".into());
        assert!(matches!(
            journal.plan_reconciliation(&[invalid_state], "worker-1"),
            Err(HcsJournalError::Corrupt(_))
        ));
    }

    #[test]
    fn reconciliation_never_adopts_without_exact_hcs_identity() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        journal
            .append_event(&identity, HcsJournalEvent::CreateRequested)
            .unwrap();
        journal
            .append_event(
                &identity,
                HcsJournalEvent::Created {
                    system_id: "hivemind-task-1".into(),
                },
            )
            .unwrap();
        let actions = journal
            .plan_reconciliation(&[system("different-system")], "worker-1")
            .unwrap();
        assert!(matches!(
            actions.as_slice(),
            [
                HcsReconciliationAction::MissingSystem { .. },
                HcsReconciliationAction::QuarantineSystem { .. }
            ]
        ));
    }

    #[test]
    fn worker_identity_mismatch_is_not_a_reattach_candidate() {
        let temp = TempDir::new().unwrap();
        let journal = HcsExecutionJournal::open(temp.path()).unwrap();
        let intent = intent();
        let identity = intent.identity.clone();
        journal.begin(intent).unwrap();
        journal
            .append_event(&identity, HcsJournalEvent::CreateRequested)
            .unwrap();
        journal
            .append_event(
                &identity,
                HcsJournalEvent::Created {
                    system_id: "hivemind-task-1".into(),
                },
            )
            .unwrap();
        let actions = journal
            .plan_reconciliation(&[system("hivemind-task-1")], "another-worker")
            .unwrap();
        assert!(matches!(
            actions.as_slice(),
            [HcsReconciliationAction::WorkerIdentityMismatch { .. }]
        ));
    }
}

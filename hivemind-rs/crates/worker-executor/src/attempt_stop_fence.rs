//! Durable, nonsecret attempt-bound Worker stop fences.
//!
//! The store records only task/attempt identity and the final second in which
//! the signed stop token is valid. It never accepts or persists bearer tokens.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::ActiveTaskKey;

const SCHEMA_VERSION: u32 = 1;
const MAX_FIELD_BYTES: usize = 4096;
const MAX_FENCES: usize = 100_000;
const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
const SEQUENCE_WIDTH: usize = 20;
#[cfg(windows)]
const REPARSE_POINT_ATTRIBUTE: u32 = 0x0400;
static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FenceRecord {
    task_id: String,
    attempt_id: String,
    expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FenceSnapshot {
    schema_version: u32,
    sequence: u64,
    fences: Vec<FenceRecord>,
}

#[derive(Default)]
struct FenceState {
    sequence: u64,
    fences: HashMap<ActiveTaskKey, i64>,
}

/// A disk-backed stop-fence store. Unit tests may use the in-memory variant;
/// production workers always open the operator-owned Worker state root.
#[derive(Clone)]
pub(crate) struct AttemptStopFenceStore {
    root: Option<PathBuf>,
    state: Arc<Mutex<FenceState>>,
}

impl AttemptStopFenceStore {
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        Self {
            root: None,
            state: Arc::new(Mutex::new(FenceState::default())),
        }
    }

    #[cfg(not(test))]
    pub(crate) fn from_environment_or_default() -> Result<Self> {
        let state_root = match std::env::var_os("HIVEMIND_WORKER_STATE_ROOT") {
            Some(value) => {
                let value = value.to_string_lossy().trim().to_owned();
                if value.is_empty() {
                    bail!("HIVEMIND_WORKER_STATE_ROOT must not be blank");
                }
                PathBuf::from(value)
            }
            None => {
                #[cfg(windows)]
                {
                    super::hcs_journal::HcsExecutionJournal::default_windows_worker_state_root()
                        .context("resolve the default Worker state root")?
                }
                #[cfg(not(windows))]
                {
                    let state_home = std::env::var_os("XDG_STATE_HOME")
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from)
                        .or_else(|| {
                            std::env::var_os("HOME").map(|home| {
                                PathBuf::from(home).join(".local").join("state")
                            })
                        })
                        .context(
                            "set HIVEMIND_WORKER_STATE_ROOT or HOME to a persistent Worker state location",
                        )?;
                    state_home.join("hivemind").join("worker")
                }
            }
        };
        Self::open(state_root)
    }

    /// Open the stop-fence store under a stable Worker state root and load its
    /// last committed snapshot before the executor accepts work.
    pub(crate) fn open(worker_state_root: impl Into<PathBuf>) -> Result<Self> {
        let worker_state_root = worker_state_root.into();
        if !worker_state_root.is_absolute()
            || worker_state_root.as_os_str().is_empty()
            || worker_state_root
                .to_string_lossy()
                .chars()
                .any(char::is_control)
        {
            bail!("Worker state root must be absolute and contain no control characters");
        }
        ensure_safe_directory(&worker_state_root).context("validate Worker state root")?;
        let root = worker_state_root.join("attempt-stop-fences");
        ensure_safe_directory(&root).context("create or validate Worker stop-fence directory")?;

        let mut state = read_latest_snapshot(&root)?;
        let now = chrono::Utc::now().timestamp();
        let previous_len = state.fences.len();
        state
            .fences
            .retain(|_, expires_at| is_active(*expires_at, now));
        if state.fences.len() != previous_len {
            persist_snapshot(&root, &mut state)?;
        }

        Ok(Self {
            root: Some(root),
            state: Arc::new(Mutex::new(state)),
        })
    }

    pub(crate) fn fences(&self) -> HashMap<ActiveTaskKey, i64> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .fences
            .clone()
    }

    /// Persist before the caller publishes the fence or acknowledges the Stop.
    /// Repeated stops preserve the later expiry while it remains active.
    pub(crate) fn record_fence(
        &self,
        task_id: &str,
        attempt_id: &str,
        expires_at: i64,
    ) -> Result<i64> {
        validate_component("task_id", task_id)?;
        validate_component("attempt_id", attempt_id)?;
        let now = chrono::Utc::now().timestamp();
        if expires_at < now {
            bail!("Worker stop-fence expiry has already elapsed");
        }

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = ActiveTaskKey::new(task_id, attempt_id);
        let mut next = FenceState {
            sequence: state.sequence,
            fences: state.fences.clone(),
        };
        next.fences
            .retain(|_, existing_expiry| is_active(*existing_expiry, now));
        let effective_expiry = next
            .fences
            .get(&key)
            .copied()
            .unwrap_or(expires_at)
            .max(expires_at);
        next.fences.insert(key, effective_expiry);

        if let Some(root) = &self.root {
            persist_snapshot(root, &mut next).context("persist Worker stop fence")?;
        }
        *state = next;
        Ok(effective_expiry)
    }
}

pub(super) fn is_active(expires_at: i64, now: i64) -> bool {
    expires_at >= now
}

fn validate_component(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > MAX_FIELD_BYTES
        || value.chars().any(char::is_control)
    {
        bail!("Worker stop-fence {name} is empty, too large, or contains control characters");
    }
    Ok(())
}

fn read_latest_snapshot(root: &Path) -> Result<FenceState> {
    let mut latest: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(root).context("read Worker stop-fence directory")? {
        let entry = entry.context("read Worker stop-fence directory entry")?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).context("inspect Worker stop-fence entry")?;
        if is_reparse_point(&metadata) {
            bail!("Worker stop-fence directory contains a reparse point");
        }
        if !metadata.is_file() {
            bail!("Worker stop-fence directory contains a non-file entry");
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_temporary_name(&name) {
            continue;
        }
        let Some(sequence_text) = name.strip_suffix(".json") else {
            bail!("Worker stop-fence snapshot name is invalid");
        };
        if sequence_text.len() != SEQUENCE_WIDTH
            || !sequence_text.bytes().all(|byte| byte.is_ascii_digit())
        {
            bail!("Worker stop-fence snapshot name is invalid");
        }
        let sequence = sequence_text
            .parse::<u64>()
            .context("parse Worker stop-fence snapshot sequence")?;
        if latest
            .as_ref()
            .is_none_or(|(current, _)| sequence > *current)
        {
            latest = Some((sequence, path));
        }
    }

    let Some((sequence, path)) = latest else {
        return Ok(FenceState::default());
    };
    let metadata =
        fs::symlink_metadata(&path).context("inspect latest Worker stop-fence snapshot")?;
    if is_reparse_point(&metadata) || !metadata.is_file() || metadata.len() > MAX_SNAPSHOT_BYTES {
        bail!("latest Worker stop-fence snapshot is not a bounded regular file");
    }
    let bytes = fs::read(&path).context("read latest Worker stop-fence snapshot")?;
    let snapshot: FenceSnapshot =
        serde_json::from_slice(&bytes).context("parse latest Worker stop-fence snapshot")?;
    if snapshot.schema_version != SCHEMA_VERSION || snapshot.sequence != sequence {
        bail!("latest Worker stop-fence snapshot version or sequence is invalid");
    }
    if snapshot.fences.len() > MAX_FENCES {
        bail!("latest Worker stop-fence snapshot contains too many entries");
    }

    let mut fences = HashMap::with_capacity(snapshot.fences.len());
    for record in snapshot.fences {
        validate_component("task_id", &record.task_id)?;
        validate_component("attempt_id", &record.attempt_id)?;
        if record.expires_at <= 0 {
            bail!("Worker stop-fence expiry is invalid");
        }
        let key = ActiveTaskKey::new(&record.task_id, &record.attempt_id);
        if fences.insert(key, record.expires_at).is_some() {
            bail!("Worker stop-fence snapshot contains a duplicate identity");
        }
    }

    Ok(FenceState { sequence, fences })
}

fn persist_snapshot(root: &Path, state: &mut FenceState) -> Result<()> {
    loop {
        if state.fences.len() > MAX_FENCES {
            bail!("Worker stop-fence store contains too many entries");
        }
        let sequence = state
            .sequence
            .checked_add(1)
            .context("Worker stop-fence snapshot sequence exhausted")?;
        let mut fences = state
            .fences
            .iter()
            .map(|(key, expires_at)| FenceRecord {
                task_id: key.task_id.clone(),
                attempt_id: key.attempt_id.clone(),
                expires_at: *expires_at,
            })
            .collect::<Vec<_>>();
        fences.sort_by(|left, right| {
            left.task_id
                .cmp(&right.task_id)
                .then_with(|| left.attempt_id.cmp(&right.attempt_id))
        });
        let snapshot = FenceSnapshot {
            schema_version: SCHEMA_VERSION,
            sequence,
            fences,
        };
        let bytes =
            serde_json::to_vec(&snapshot).context("serialize Worker stop-fence snapshot")?;
        if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
            bail!("Worker stop-fence snapshot exceeds the size limit");
        }

        let final_path = root.join(format!("{sequence:0width$}.json", width = SEQUENCE_WIDTH));
        let temporary_path = root.join(format!(
            ".{sequence:0width$}.{}.{}.tmp",
            std::process::id(),
            TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            width = SEQUENCE_WIDTH,
        ));
        let write_result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary_path)
                .context("create temporary Worker stop-fence snapshot")?;
            file.write_all(&bytes)
                .context("write temporary Worker stop-fence snapshot")?;
            file.sync_all()
                .context("sync temporary Worker stop-fence snapshot")?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary_path);
            return Err(error);
        }

        match publish_snapshot(&temporary_path, &final_path, root) {
            Ok(()) => {
                state.sequence = sequence;
                cleanup_older_snapshots(root, sequence);
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let latest = read_latest_snapshot(root)
                    .context("reload Worker stop-fence snapshot after concurrent publication")?;
                if latest.sequence < sequence {
                    let _ = fs::remove_file(&temporary_path);
                    return Err(error)
                        .context("Worker stop-fence sequence conflicted without a newer snapshot");
                }
                merge_fence_state(state, latest, chrono::Utc::now().timestamp());
                let _ = fs::remove_file(&temporary_path);
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary_path);
                return Err(error).context("publish durable Worker stop-fence snapshot");
            }
        }
    }
}

fn merge_fence_state(state: &mut FenceState, latest: FenceState, now: i64) {
    state.fences.retain(|_, expiry| is_active(*expiry, now));
    for (key, expiry) in latest.fences {
        if is_active(expiry, now) {
            state
                .fences
                .entry(key)
                .and_modify(|current| *current = (*current).max(expiry))
                .or_insert(expiry);
        }
    }
    state.sequence = latest.sequence;
}

/// Publish without replacing an existing sequence so concurrent writers cannot
/// overwrite an acknowledged fence snapshot.
fn publish_snapshot(temporary_path: &Path, final_path: &Path, root: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let _ = root;

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

        let temporary_wide = temporary_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let final_wide = final_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let moved = unsafe {
            MoveFileExW(
                temporary_wide.as_ptr(),
                final_wide.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved == 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    #[cfg(not(windows))]
    {
        fs::hard_link(temporary_path, final_path)?;
        #[cfg(unix)]
        std::fs::File::open(root)?.sync_all()?;
        let _ = fs::remove_file(temporary_path);
    }

    Ok(())
}

/// A committed snapshot is authoritative; failure to reclaim older committed
/// snapshots does not invalidate the new fence and must not suppress its ack.
fn cleanup_older_snapshots(root: &Path, latest_sequence: u64) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(sequence_text) = name.strip_suffix(".json") else {
            continue;
        };
        let Ok(sequence) = sequence_text.parse::<u64>() else {
            continue;
        };
        if sequence < latest_sequence {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn ensure_safe_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if is_reparse_point(&metadata) || !metadata.is_dir() {
                bail!("{} is not a regular directory", path.display());
            }
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
    }

    if let Some(parent) = path.parent().filter(|parent| *parent != path) {
        ensure_safe_directory(parent)?;
    }
    let created = match fs::create_dir(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error).with_context(|| format!("create {}", path.display())),
    };
    let metadata = fs::symlink_metadata(path).context("inspect newly created directory")?;
    if is_reparse_point(&metadata) || !metadata.is_dir() {
        bail!("{} was not created as a regular directory", path.display());
    }
    #[cfg(unix)]
    if created {
        let parent = path
            .parent()
            .context("new Worker state directory has no parent")?;
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("sync parent directory {}", parent.display()))?;
    }
    #[cfg(not(unix))]
    let _ = created;
    Ok(())
}

fn is_temporary_name(name: &str) -> bool {
    let Some(name) = name.strip_prefix('.') else {
        return false;
    };
    let Some(name) = name.strip_suffix(".tmp") else {
        return false;
    };
    let mut fields = name.split('.');
    fields.next().is_some_and(|sequence| {
        sequence.len() == SEQUENCE_WIDTH && sequence.bytes().all(|byte| byte.is_ascii_digit())
    }) && fields
        .next()
        .is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        && fields.next().is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
        })
        && fields.next().is_none()
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & REPARSE_POINT_ATTRIBUTE != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn reloads_fences_and_keeps_expiry_inclusive() {
        let root = TempDir::new().expect("temporary root should be created");
        let store = AttemptStopFenceStore::open(root.path()).expect("store should open");
        let expiry = chrono::Utc::now().timestamp() + 300;
        store
            .record_fence("task-1", "attempt-1", expiry)
            .expect("fence should persist");

        let reopened = AttemptStopFenceStore::open(root.path()).expect("store should reopen");
        let fences = reopened.fences();
        assert_eq!(
            fences.get(&ActiveTaskKey::new("task-1", "attempt-1")),
            Some(&expiry)
        );
        assert!(is_active(expiry, expiry));
        assert!(!is_active(expiry, expiry + 1));
    }

    #[test]
    fn stale_store_publication_preserves_committed_fences() {
        let root = TempDir::new().expect("temporary root should be created");
        let first = AttemptStopFenceStore::open(root.path()).expect("first store should open");
        let second = AttemptStopFenceStore::open(root.path()).expect("second store should open");
        let expiry = chrono::Utc::now().timestamp() + 300;

        first
            .record_fence("task-1", "attempt-1", expiry)
            .expect("first fence should persist");
        second
            .record_fence("task-2", "attempt-2", expiry)
            .expect("stale store should merge and persist its fence");

        let reopened = AttemptStopFenceStore::open(root.path()).expect("store should reopen");
        let fences = reopened.fences();
        assert_eq!(
            fences.get(&ActiveTaskKey::new("task-1", "attempt-1")),
            Some(&expiry)
        );
        assert_eq!(
            fences.get(&ActiveTaskKey::new("task-2", "attempt-2")),
            Some(&expiry)
        );
    }

    #[test]
    fn open_discards_expired_fences() {
        let root = TempDir::new().expect("temporary root should be created");
        let store = AttemptStopFenceStore::open(root.path()).expect("store should open");
        store
            .record_fence("task-1", "attempt-old", chrono::Utc::now().timestamp() + 30)
            .expect("initial fence should persist");

        let mut state = store
            .state
            .lock()
            .expect("store lock should not be poisoned");
        state.fences.insert(
            ActiveTaskKey::new("task-1", "attempt-old"),
            chrono::Utc::now().timestamp() - 1,
        );
        if let Some(path) = &store.root {
            persist_snapshot(path, &mut state).expect("expired snapshot should persist");
        }
        drop(state);

        let reopened = AttemptStopFenceStore::open(root.path()).expect("store should reopen");
        assert!(reopened.fences().is_empty());
    }

    #[test]
    fn corrupt_latest_snapshot_fails_closed() {
        let root = TempDir::new().expect("temporary root should be created");
        let store = AttemptStopFenceStore::open(root.path()).expect("store should open");
        store
            .record_fence("task-1", "attempt-1", chrono::Utc::now().timestamp() + 300)
            .expect("fence should persist");
        let snapshot_dir = root.path().join("attempt-stop-fences");
        let snapshot = fs::read_dir(&snapshot_dir)
            .expect("snapshot directory should read")
            .next()
            .expect("snapshot should exist")
            .expect("snapshot entry should read")
            .path();
        fs::write(snapshot, b"not-json").expect("snapshot should be corrupted");

        assert!(AttemptStopFenceStore::open(root.path()).is_err());
    }

    #[test]
    fn record_rejects_elapsed_expiry() {
        let root = TempDir::new().expect("temporary root should be created");
        let store = AttemptStopFenceStore::open(root.path()).expect("store should open");

        assert!(store
            .record_fence("task-1", "attempt-1", chrono::Utc::now().timestamp() - 1)
            .is_err());
        assert!(store.fences().is_empty());
    }

    #[test]
    fn worker_state_root_must_be_absolute() {
        assert!(AttemptStopFenceStore::open("relative-worker-state").is_err());
    }
}

//! Durable retry records for operator-owned task-root cleanup.
//!
//! Cleanup is deliberately best-effort at the terminal execution boundary:
//! result delivery must not be turned into a false failure just because a
//! directory is temporarily held open. This module makes that best effort
//! durable and retries only execution-scoped roots below a configured operator
//! root. It never follows symlinks or accepts an arbitrary path from a record.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const QUEUE_DIRECTORY: &str = ".hivemind-maintenance";
const RECORD_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanupRecord {
    version: u32,
    root: PathBuf,
    operator_root: PathBuf,
    created_at_ms: u128,
}

/// Persist a cleanup retry for an execution-scoped directory.
///
/// The record is keyed by the exact path and is create-new, so repeated
/// failures cannot grow an unbounded append-only log. A pre-existing record is
/// already sufficient for the scavenger and is intentionally left untouched.
pub fn record_cleanup_failure(root: &Path) -> Result<(), String> {
    let operator_root = root
        .parent()
        .ok_or_else(|| "cleanup root has no operator parent".to_owned())?;
    validate_execution_root(root, operator_root)?;
    ensure_real_directory(operator_root)?;

    let queue_root = operator_root.join(QUEUE_DIRECTORY);
    ensure_or_create_directory(&queue_root)?;
    let key = general_compute_runtime::sha256_digest(root.to_string_lossy().as_bytes());
    let key = key.strip_prefix("sha256:").unwrap_or(&key);
    let record_path = queue_root.join(format!("cleanup-{key}.json"));
    if let Ok(metadata) = fs::symlink_metadata(&record_path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("cleanup record path is not a regular file".into());
        }
        return Ok(());
    }

    let record = CleanupRecord {
        version: RECORD_VERSION,
        root: root.to_path_buf(),
        operator_root: operator_root.to_path_buf(),
        created_at_ms: unix_time_ms(),
    };
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    let temporary = queue_root.join(format!(
        ".cleanup-{}-{}.tmp",
        std::process::id(),
        unix_time_ms()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        match fs::rename(&temporary, &record_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Retry all durable cleanup records below one operator-owned root.
///
/// A successful or already-absent root removes its record. A failed removal
/// remains durable for the next startup or explicit maintenance pass.
pub fn scavenge_operator_root(operator_root: &Path) -> Result<usize, String> {
    if !operator_root.is_absolute() {
        return Err("operator root must be absolute".into());
    }
    let operator_metadata = match fs::symlink_metadata(operator_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    if operator_metadata.file_type().is_symlink() || !operator_metadata.is_dir() {
        return Err("operator root is not a real directory".into());
    }
    ensure_no_symlink_ancestors(operator_root)?;

    let queue_root = operator_root.join(QUEUE_DIRECTORY);
    let metadata = match fs::symlink_metadata(&queue_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("cleanup queue is not a real directory".into());
    }
    let mut cleaned = 0usize;
    for entry in fs::read_dir(&queue_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(key) = name
            .strip_prefix("cleanup-")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let record: CleanupRecord = match serde_json::from_slice(&bytes) {
            Ok(record) => record,
            Err(_) => continue,
        };
        if record.version != RECORD_VERSION || record.operator_root != operator_root {
            continue;
        }
        if validate_execution_root(&record.root, operator_root).is_err() {
            continue;
        }
        let root_metadata = match fs::symlink_metadata(&record.root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let _ = fs::remove_file(&path);
                cleaned += 1;
                continue;
            }
            Err(_) => continue,
        };
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            continue;
        }
        if fs::remove_dir_all(&record.root).is_ok() {
            let _ = fs::remove_file(&path);
            cleaned += 1;
        }
    }
    Ok(cleaned)
}

/// Run the scavenger for a set of independently configured operator roots.
pub fn scavenge_operator_roots<'a, I>(roots: I) -> Result<usize, String>
where
    I: IntoIterator<Item = &'a Path>,
{
    let mut cleaned = 0;
    for root in roots {
        cleaned += scavenge_operator_root(root)?;
    }
    Ok(cleaned)
}

fn validate_execution_root(root: &Path, operator_root: &Path) -> Result<(), String> {
    if !root.is_absolute() || !operator_root.is_absolute() || root.parent() != Some(operator_root) {
        return Err("cleanup root is not a direct absolute child of its operator root".into());
    }
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "cleanup root name is invalid".to_owned())?;
    let suffix = name
        .strip_prefix("exec-")
        .ok_or_else(|| "cleanup root is not execution-scoped".to_owned())?;
    if suffix.len() != 59 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("cleanup root execution scope is invalid".into());
    }
    ensure_no_symlink_ancestors(root)
}

fn ensure_real_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("operator root is not a real directory".into());
    }
    ensure_no_symlink_ancestors(path)
}

fn ensure_or_create_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("cleanup queue is not a real directory".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    ensure_no_symlink_ancestors(path)
}

fn ensure_no_symlink_ancestors(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("cleanup path contains a symlink boundary".into());
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("cleanup path contains a non-directory boundary".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operator_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hivemind-maintenance-{label}-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("operator root should be created");
        root
    }

    fn execution_root(operator_root: &Path) -> PathBuf {
        operator_root.join(format!("exec-{}", "a".repeat(59)))
    }

    #[test]
    fn cleanup_records_are_deduplicated_and_scavenged() {
        let operator_root = operator_root("dedupe");
        let task_root = execution_root(&operator_root);
        fs::create_dir_all(task_root.join("nested")).expect("task root should be created");
        fs::write(task_root.join("nested/output"), b"output").expect("output should be written");

        record_cleanup_failure(&task_root).expect("cleanup failure should be durable");
        record_cleanup_failure(&task_root).expect("repeated cleanup failure should be idempotent");
        let queue = operator_root.join(QUEUE_DIRECTORY);
        let records = fs::read_dir(&queue)
            .expect("maintenance queue should be readable")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|kind| kind.is_file())
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(records, 1, "one root should have one retry record");

        assert_eq!(
            scavenge_operator_root(&operator_root).expect("scavenger should succeed"),
            1
        );
        assert!(
            !task_root.exists(),
            "successful retry should remove the task root"
        );
        assert_eq!(
            fs::read_dir(&queue)
                .expect("maintenance queue should remain readable")
                .count(),
            0
        );

        let _ = fs::remove_dir_all(operator_root);
    }

    #[test]
    fn scavenger_removes_records_for_already_absent_roots() {
        let operator_root = operator_root("absent");
        let task_root = execution_root(&operator_root);
        record_cleanup_failure(&task_root).expect("missing root cleanup should be durable");

        assert_eq!(
            scavenge_operator_root(&operator_root).expect("scavenger should succeed"),
            1
        );
        assert!(!operator_root.join(QUEUE_DIRECTORY).join("cleanup").exists());
        assert_eq!(
            fs::read_dir(operator_root.join(QUEUE_DIRECTORY))
                .expect("maintenance queue should remain readable")
                .count(),
            0
        );

        let _ = fs::remove_dir_all(operator_root);
    }

    #[test]
    fn malformed_or_outside_records_are_retained_without_following_paths() {
        let operator_root = operator_root("invalid");
        let queue = operator_root.join(QUEUE_DIRECTORY);
        fs::create_dir_all(&queue).expect("maintenance queue should be created");
        let outside = operator_root
            .parent()
            .expect("temporary root should have a parent")
            .join(format!("hivemind-maintenance-outside-{}", unix_time_ms()));
        fs::create_dir_all(&outside).expect("outside directory should be created");
        fs::write(outside.join("sentinel"), b"keep").expect("sentinel should be written");

        let valid_key = "b".repeat(64);
        fs::write(queue.join(format!("cleanup-{valid_key}.json")), b"not-json")
            .expect("malformed record should be written");
        let outside_record = serde_json::json!({
            "version": RECORD_VERSION,
            "root": outside,
            "operator_root": operator_root,
            "created_at_ms": unix_time_ms(),
        });
        fs::write(
            queue.join(format!("cleanup-{}.json", "c".repeat(64))),
            serde_json::to_vec(&outside_record).expect("record should serialize"),
        )
        .expect("outside record should be written");

        assert_eq!(
            scavenge_operator_root(&operator_root)
                .expect("scavenger should ignore invalid records"),
            0
        );
        assert!(outside.exists(), "outside paths must never be followed");
        assert!(outside.join("sentinel").exists());
        assert!(queue.join(format!("cleanup-{valid_key}.json")).exists());
        assert!(queue
            .join(format!("cleanup-{}.json", "c".repeat(64)))
            .exists());

        let _ = fs::remove_dir_all(operator_root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn invalid_execution_scopes_are_rejected_before_queue_creation() {
        let operator_root = operator_root("scope");
        let invalid = operator_root.join("task-not-execution-scoped");
        fs::create_dir_all(&invalid).expect("invalid root should be created");
        assert!(record_cleanup_failure(&invalid).is_err());
        assert!(!operator_root.join(QUEUE_DIRECTORY).exists());
        let _ = fs::remove_dir_all(operator_root);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cleanup_roots_are_not_followed() {
        use std::os::unix::fs::symlink;

        let operator_root = operator_root("symlink");
        let outside = operator_root
            .parent()
            .expect("temporary root should have a parent")
            .join(format!(
                "hivemind-maintenance-symlink-outside-{}",
                unix_time_ms()
            ));
        fs::create_dir_all(&outside).expect("outside directory should be created");
        let linked = execution_root(&operator_root);
        symlink(&outside, &linked).expect("test symlink should be created");
        assert!(record_cleanup_failure(&linked).is_err());
        let _ = fs::remove_file(linked);
        let _ = fs::remove_dir_all(&operator_root);
        let _ = fs::remove_dir_all(outside);
    }
}

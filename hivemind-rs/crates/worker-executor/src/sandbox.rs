//! Task identifier validation helpers for the managed-function worker.

/// Returns true when `task_id` is safe to use as a filesystem or key component.
///
/// Managed-function tasks no longer touch the host filesystem, but task IDs are
/// still echoed into gRPC responses and logs, so we keep the conservative
/// allowlist (alphanumeric plus `-`, `_`, `.`) and reject path-traversal forms.
pub fn is_safe_task_id(task_id: &str) -> bool {
    if task_id.len() == 1 && task_id.as_bytes()[0] == b'.' {
        return false;
    }
    !task_id.trim().is_empty()
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !task_id.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_normalizing_task_ids() {
        assert!(!is_safe_task_id("../escape"));
        assert!(!is_safe_task_id("."));
        assert!(!is_safe_task_id(".."));
        assert!(!is_safe_task_id(""));
        assert!(!is_safe_task_id("has/slash"));
    }

    #[test]
    fn accepts_normal_task_ids() {
        assert!(is_safe_task_id("task-123"));
        assert!(is_safe_task_id("managed_function.v0"));
        assert!(is_safe_task_id("abc123"));
    }
}

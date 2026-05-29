use std::fs;
use std::path::PathBuf;

/// Ensure the sandbox directory exists and is clean
pub fn prepare_sandbox(base_dir: &str, task_id: &str) -> std::io::Result<PathBuf> {
    let sandbox_path = PathBuf::from(base_dir).join(task_id);
    if sandbox_path.exists() {
        fs::remove_dir_all(&sandbox_path)?;
    }
    fs::create_dir_all(&sandbox_path)?;
    Ok(sandbox_path)
}

/// Clean up sandbox after task completion
pub fn cleanup_sandbox(task_id: &str, sandbox_dir: &str) -> std::io::Result<()> {
    let sandbox_path = PathBuf::from(sandbox_dir).join(task_id);
    if sandbox_path.exists() {
        fs::remove_dir_all(&sandbox_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_prepare_sandbox_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();

        let result = prepare_sandbox(base, "task-1");
        assert!(result.is_ok(), "prepare_sandbox should succeed");
        let path = result.unwrap();
        assert!(path.exists(), "sandbox directory should exist");
        assert!(path.is_dir(), "sandbox should be a directory");
    }

    #[test]
    fn test_prepare_sandbox_cleans_existing() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();

        // Create initial sandbox with a file
        let path = prepare_sandbox(base, "task-2").unwrap();
        fs::write(path.join("old_file.txt"), "old data").unwrap();
        assert!(path.join("old_file.txt").exists());

        // Re-prepare should clean it
        let path2 = prepare_sandbox(base, "task-2").unwrap();
        assert!(path2.exists());
        assert!(!path2.join("old_file.txt").exists(), "Old files should be removed");
    }

    #[test]
    fn test_cleanup_sandbox_removes_directory() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();

        let path = prepare_sandbox(base, "task-3").unwrap();
        fs::write(path.join("result.txt"), "done").unwrap();
        assert!(path.exists());

        cleanup_sandbox("task-3", base).unwrap();
        assert!(!path.exists(), "sandbox should be removed after cleanup");
    }

    #[test]
    fn test_cleanup_sandbox_nonexistent_is_ok() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();

        // Cleaning up a non-existent sandbox should succeed
        let result = cleanup_sandbox("nonexistent-task", base);
        assert!(result.is_ok(), "cleanup of nonexistent sandbox should succeed");
    }

    #[test]
    fn test_sandbox_lifecycle() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();

        // Create
        let path = prepare_sandbox(base, "lifecycle-task").unwrap();
        assert!(path.exists());

        // Write some data
        fs::write(path.join("input.dat"), b"test data").unwrap();
        fs::write(path.join("output.dat"), b"result data").unwrap();
        assert!(path.join("input.dat").exists());
        assert!(path.join("output.dat").exists());

        // Cleanup
        cleanup_sandbox("lifecycle-task", base).unwrap();
        assert!(!path.exists(), "All files should be gone after cleanup");
    }
}

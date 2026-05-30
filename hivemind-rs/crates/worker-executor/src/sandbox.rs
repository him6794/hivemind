use std::fs;
use std::path::PathBuf;

/// Sandbox resource limits for Rust-based task execution
#[derive(Debug, Clone)]
pub struct SandboxLimits {
    pub max_cpu_percent: f64,
    pub max_memory_mb: u64,
    pub max_storage_mb: u64,
    pub max_wall_time_secs: u64,
    pub gpu_required: bool,
    pub vram_required_mb: i64,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            max_cpu_percent: 80.0,
            max_memory_mb: 4096,
            max_storage_mb: 10240,  // 10 GB
            max_wall_time_secs: 3600,
            gpu_required: false,
            vram_required_mb: 0,
        }
    }
}

/// Ensure the sandbox directory exists and is clean
pub fn prepare_sandbox(base_dir: &str, task_id: &str) -> std::io::Result<PathBuf> {
    let sandbox_path = PathBuf::from(base_dir).join(task_id);
    if sandbox_path.exists() {
        fs::remove_dir_all(&sandbox_path)?;
    }
    fs::create_dir_all(&sandbox_path)?;
    Ok(sandbox_path)
}

/// Check if a sandbox directory has enough disk space available
pub fn check_storage(sandbox_dir: &str, required_mb: u64) -> bool {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();

    // Find the disk that contains the sandbox directory
    let sandbox_path = std::path::Path::new(sandbox_dir);
    let canonical = sandbox_path.canonicalize().unwrap_or_else(|_| sandbox_path.to_path_buf());

    for disk in disks.iter() {
        if canonical.starts_with(disk.mount_point()) {
            let available_mb = disk.available_space() / (1024 * 1024);
            return available_mb >= required_mb;
        }
    }

    // Fallback: check total available
    let total_available: u64 = disks.iter().map(|d| d.available_space()).sum();
    total_available / (1024 * 1024) >= required_mb
}

/// Clean up sandbox after task completion
pub fn cleanup_sandbox(task_id: &str, sandbox_dir: &str) -> std::io::Result<()> {
    let sandbox_path = PathBuf::from(sandbox_dir).join(task_id);
    if sandbox_path.exists() {
        fs::remove_dir_all(&sandbox_path)?;
    }
    Ok(())
}

/// Estimate sandbox storage usage
pub fn sandbox_storage_used(task_id: &str, sandbox_dir: &str) -> u64 {
    let sandbox_path = PathBuf::from(sandbox_dir).join(task_id);
    if !sandbox_path.exists() {
        return 0;
    }
    dir_size(&sandbox_path).unwrap_or(0)
}

fn dir_size(path: &std::path::Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                total += dir_size(&entry.path())?;
            } else {
                total += entry.metadata()?.len();
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sandbox_limits_default() {
        let limits = SandboxLimits::default();
        assert_eq!(limits.max_cpu_percent, 80.0);
        assert_eq!(limits.max_memory_mb, 4096);
        assert_eq!(limits.max_wall_time_secs, 3600);
    }

    #[test]
    fn test_prepare_sandbox_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();
        let result = prepare_sandbox(base, "task-1");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.exists());
        assert!(path.is_dir());
    }

    #[test]
    fn test_prepare_sandbox_cleans_existing() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();
        let path = prepare_sandbox(base, "task-2").unwrap();
        fs::write(path.join("old_file.txt"), "old data").unwrap();
        assert!(path.join("old_file.txt").exists());
        let path2 = prepare_sandbox(base, "task-2").unwrap();
        assert!(path2.exists());
        assert!(!path2.join("old_file.txt").exists());
    }

    #[test]
    fn test_cleanup_sandbox_removes_directory() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();
        let path = prepare_sandbox(base, "task-3").unwrap();
        fs::write(path.join("result.txt"), "done").unwrap();
        assert!(path.exists());
        cleanup_sandbox("task-3", base).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_cleanup_sandbox_nonexistent_is_ok() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();
        let result = cleanup_sandbox("nonexistent-task", base);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_storage() {
        // Should always have some storage
        let has_space = check_storage(".", 1); // 1 MB
        assert!(has_space, "Should have at least 1MB available");

        let insane = check_storage(".", 1_000_000_000); // 1 PB
        assert!(!insane, "Should NOT have 1PB available");
    }

    #[test]
    fn test_sandbox_lifecycle() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().to_str().unwrap();
        let path = prepare_sandbox(base, "lifecycle-task").unwrap();
        assert!(path.exists());
        fs::write(path.join("input.dat"), b"test data").unwrap();
        fs::write(path.join("output.dat"), b"result data").unwrap();
        assert!(path.join("input.dat").exists());
        assert!(path.join("output.dat").exists());
        cleanup_sandbox("lifecycle-task", base).unwrap();
        assert!(!path.exists());
    }
}

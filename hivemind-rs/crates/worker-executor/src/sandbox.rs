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
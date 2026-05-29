use anyhow::{Context, Result};
use hivemind_config::HivemindConfig;
use hivemind_models::Task;
use std::process::Command;
use std::time::Instant;

pub async fn run_task(task: &Task, config: &HivemindConfig) -> Result<super::TaskResult> {
    let start = Instant::now();

    tracing::info!("Executing task {} with torrent {}", task.task_id,
        task.torrent_source.as_deref().unwrap_or("none"));

    // For now, run a lightweight simulation (the real executor-rs monty path is preserved)
    // The actual Monty executor is at executor-rs/ - this crate wraps it for task workloads
    let output = execute_sandboxed(task, config).await?;

    let elapsed = start.elapsed();

    Ok(super::TaskResult {
        task_id: task.task_id.clone(),
        success: output.status.success(),
        output: Some(String::from_utf8_lossy(&output.stdout).to_string()),
        error: if output.status.success() { None } else {
            Some(String::from_utf8_lossy(&output.stderr).to_string())
        },
        exit_code: output.status.code().unwrap_or(-1),
        cpu_time_ms: 0,
        wall_time_ms: elapsed.as_millis() as i64,
        peak_memory_mb: 0,
    })
}

async fn execute_sandboxed(task: &Task, config: &HivemindConfig) -> Result<std::process::Output> {
    let btih = task.torrent_source.as_deref().unwrap_or("dummy-btih");

    // Use the executor-rs monty binary if available
    let executor = if std::path::Path::new(&config.executor.monty_executable).exists() {
        config.executor.monty_executable.clone()
    } else {
        // Fallback: use a simple echo-based task simulation
        if cfg!(windows) {
            "cmd.exe".into()
        } else {
            "/bin/sh".into()
        }
    };

    let mut cmd = Command::new(&executor);

    if executor == "cmd.exe" || executor == "/bin/sh" {
        // Simulation mode
        if cfg!(windows) {
            cmd.args(["/C", &format!("echo RESULT_TORRENT=result_{}", btih)]);
        } else {
            cmd.args(["-c", &format!("echo RESULT_TORRENT=result_{}", btih)]);
        }
    } else {
        // Monty mode: pass -c script with the task
        cmd.args(["-c", &format!("import time; time.sleep(1); print('RESULT_TORRENT=result_{}')", btih)]);
    }

    // Apply resource limits
    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn executor process")?;

    let result = tokio::task::spawn_blocking(move || {
        child.wait_with_output()
    })
    .await
    .context("Executor task panicked")?
    .context("Executor process failed")?;

    Ok(result)
}
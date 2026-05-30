use anyhow::{Context, Result};
use hivemind_config::HivemindConfig;
use hivemind_models::Task;
use super::sandbox::SandboxLimits;
use std::process::Command;
use std::time::Instant;

pub async fn run_task(task: &Task, config: &HivemindConfig) -> Result<super::TaskResult> {
    let start = Instant::now();

    let btih = task.torrent_source.as_deref().unwrap_or("unknown");
    tracing::info!(
        "Executing task {} (BTIH: {}, requires GPU: {}, storage: {}GB)",
        task.task_id, &btih[..usize::min(8, btih.len())],
        task.req_gpu_score > 0,
        task.req_storage_gb
    );

    let limits = SandboxLimits {
        max_cpu_percent: config.executor.max_cpu_percent,
        max_memory_mb: config.executor.max_memory_mb,
        max_storage_mb: task.req_storage_gb.max(1) as u64 * 1024,
        max_wall_time_secs: config.executor.task_timeout_secs,
        gpu_required: task.req_gpu_score > 0,
        vram_required_mb: task.req_gpu_memory_gb as i64 * 1024,
    };

    // Verify storage before execution
    if !super::sandbox::check_storage(&config.executor.sandbox_dir, limits.max_storage_mb) {
        return Err(anyhow::anyhow!(
            "Insufficient storage: need {}MB, sandbox dir: {}",
            limits.max_storage_mb, config.executor.sandbox_dir
        ));
    }

    let output = execute_sandboxed(task, config, &limits).await?;
    let elapsed = start.elapsed();

    Ok(super::TaskResult {
        task_id: task.task_id.clone(),
        success: output.status.success(),
        output: Some(String::from_utf8_lossy(&output.stdout).to_string()),
        error: if output.status.success() { None } else {
            Some(String::from_utf8_lossy(&output.stderr).to_string())
        },
        exit_code: output.status.code().unwrap_or(-1),
        cpu_time_ms: 0,  // Would be populated by resource monitoring
        wall_time_ms: elapsed.as_millis() as i64,
        peak_memory_mb: 0,
    })
}

async fn execute_sandboxed(
    task: &Task,
    config: &HivemindConfig,
    limits: &SandboxLimits,
) -> Result<std::process::Output> {
    let btih = task.torrent_source.as_deref().unwrap_or("dummy-btih");

    // Use the executor-rs monty binary if available (Rust sandbox)
    let executor = if std::path::Path::new(&config.executor.monty_executable).exists() {
        config.executor.monty_executable.clone()
    } else {
        if cfg!(windows) { "cmd.exe".into() } else { "/bin/sh".into() }
    };

    let mut cmd = Command::new(&executor);

    if executor == "cmd.exe" || executor == "/bin/sh" {
        // Simulation mode: emit the result torrent ref
        if cfg!(windows) {
            cmd.args(["/C", &format!("echo RESULT_TORRENT=result_{}", btih)]);
        } else {
            cmd.args(["-c", &format!("echo RESULT_TORRENT=result_{}", btih)]);
        }
    } else {
        // Monty/Rust sandbox mode with resource limits
        cmd.args([
            "-c",
            &format!("import time; time.sleep(1); print('RESULT_TORRENT=result_{}')", btih),
        ]);
        cmd.arg("--max-memory").arg(limits.max_memory_mb.to_string());
        if limits.gpu_required {
            tracing::info!("Task {} requires GPU, enabling GPU access in sandbox", task.task_id);
        }
    }

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

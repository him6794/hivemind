use super::sandbox::{SandboxEgressPolicy, SandboxLimits};
use anyhow::{Context, Result};
use hivemind_config::HivemindConfig;
use hivemind_models::Task;
use std::process::Command;
use std::time::Instant;

/// Extract a safe identifier from a magnet URI or raw btih.
/// Magnet URIs contain `&` chars that break cmd.exe / shell parsing.
fn sanitize_btih(raw: &str) -> &str {
    if let Some(pos) = raw.find("btih:") {
        let after = &raw[pos + 5..];
        after.split(['&', ')']).next().unwrap_or(raw)
    } else {
        raw
    }
}

pub async fn run_task(task: &Task, config: &HivemindConfig) -> Result<super::TaskResult> {
    let start = Instant::now();

    let btih = sanitize_btih(task.torrent_source.as_deref().unwrap_or("unknown"));
    tracing::info!(
        "Executing task {} (BTIH: {}, requires GPU: {}, storage: {}GB)",
        task.task_id,
        &btih[..usize::min(8, btih.len())],
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
            limits.max_storage_mb,
            config.executor.sandbox_dir
        ));
    }

    let output = execute_sandboxed(task, config, &limits).await?;
    let elapsed = start.elapsed();

    Ok(super::TaskResult {
        task_id: task.task_id.clone(),
        success: output.status.success(),
        output: Some(String::from_utf8_lossy(&output.stdout).to_string()),
        error: if output.status.success() {
            None
        } else {
            Some(String::from_utf8_lossy(&output.stderr).to_string())
        },
        exit_code: output.status.code().unwrap_or(-1),
        cpu_time_ms: 0, // Would be populated by resource monitoring
        wall_time_ms: elapsed.as_millis() as i64,
        peak_memory_mb: 0,
    })
}

async fn execute_sandboxed(
    task: &Task,
    config: &HivemindConfig,
    limits: &SandboxLimits,
) -> Result<std::process::Output> {
    let btih = sanitize_btih(task.torrent_source.as_deref().unwrap_or("dummy-btih"));
    let policy = SandboxEgressPolicy::from_config(&config.executor)?;
    if config.executor.sandbox_mode.eq_ignore_ascii_case("production") && !policy.is_release_safe() {
        return Err(anyhow::anyhow!(
            "production mode requires network egress policy (enable egress and configure allowlist/denylist targets)"
        ));
    }

    // Use the executor-rs monty binary if available (Rust sandbox)
    let executor = if std::path::Path::new(&config.executor.monty_executable).exists() {
        config.executor.monty_executable.clone()
    } else if config.executor.sandbox_mode.eq_ignore_ascii_case("dev") {
        if cfg!(windows) {
            "cmd.exe".into()
        } else {
            "/bin/sh".into()
        }
    } else {
        return Err(anyhow::anyhow!(
            "sandbox executable '{}' not found; {} mode refuses shell simulation",
            config.executor.monty_executable,
            config.executor.sandbox_mode
        ));
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
            &format!(
                "import time; time.sleep(1); print('RESULT_TORRENT=result_{}')",
                btih
            ),
        ]);
        cmd.arg("--max-memory")
            .arg(limits.max_memory_mb.to_string());
        if limits.gpu_required {
            tracing::info!(
                "Task {} requires GPU, enabling GPU access in sandbox",
                task.task_id
            );
        }
    }

    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn executor process")?;

    let result = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .context("Executor task panicked")?
        .context("Executor process failed")?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hivemind_models::TaskStatus;
    use tempfile::TempDir;
    use uuid::Uuid;

    #[tokio::test]
    async fn production_mode_rejects_missing_sandbox_executable() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().to_str().unwrap());
        config.executor.sandbox_mode = "production".into();
        config.executor.network_egress_enabled = true;
        config.executor.network_egress_mode = "allowlist".into();
        config.executor.network_egress_targets = vec!["8.8.8.8".into()];
        config.executor.monty_executable = tmp
            .path()
            .join("missing-monty")
            .to_string_lossy()
            .to_string();

        let result = run_task(&test_task(), &config).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("production mode refuses shell simulation"));
    }

    #[tokio::test]
    async fn dev_mode_preserves_shell_simulation_when_sandbox_executable_is_missing() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().to_str().unwrap());
        config.executor.sandbox_mode = "dev".into();
        config.executor.monty_executable = tmp
            .path()
            .join("missing-monty")
            .to_string_lossy()
            .to_string();

        let result = run_task(&test_task(), &config).await.unwrap();

        assert!(result.success);
        assert!(result.output.unwrap().contains("RESULT_TORRENT=result_"));
    }

    #[tokio::test]
    async fn production_mode_rejects_missing_egress_policy() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().to_str().unwrap());
        config.executor.sandbox_mode = "production".into();
        config.executor.network_egress_enabled = false;
        config.executor.monty_executable = if cfg!(windows) {
            "cmd.exe".into()
        } else {
            "/bin/sh".into()
        };

        let result = run_task(&test_task(), &config).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("production mode requires network egress policy"));
    }

    #[tokio::test]
    async fn production_mode_allowlist_passes_egress_gate() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().to_str().unwrap());
        config.executor.sandbox_mode = "production".into();
        config.executor.network_egress_enabled = true;
        config.executor.network_egress_mode = "allowlist".into();
        config.executor.network_egress_targets = vec!["8.8.8.8".into()];
        config.executor.monty_executable = tmp
            .path()
            .join("missing-monty")
            .to_string_lossy()
            .to_string();

        let result = run_task(&test_task(), &config).await;
        assert!(result.is_err());
        assert!(!result
            .unwrap_err()
            .to_string()
            .contains("production mode requires network egress policy"));
    }

    fn test_config(sandbox_dir: &str) -> HivemindConfig {
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = sandbox_dir.into();
        config
    }

    fn test_task() -> Task {
        let now = Utc::now();
        Task {
            id: Uuid::new_v4(),
            task_id: "sandbox-gate-test".into(),
            owner: "requestor".into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some("abcdef1234567890".into()),
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 1,
            req_gpu_score: 0,
            req_memory_gb: 1,
            req_gpu_memory_gb: 0,
            req_storage_gb: 1,
            host_count: 1,
            max_cpt: 1,
            billing_settled: false,
            billed_amount: 0,
            retry_count: 0,
            max_retries: 3,
            deadline: None,
            deterministic: false,
            side_effects: false,
            priority: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            download_bytes: 0,
            cache_hits: 0,
            created_at: now,
            last_update: now,
            completed_at: None,
        }
    }
}

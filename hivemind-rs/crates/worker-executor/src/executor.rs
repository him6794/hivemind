use super::sandbox::{SandboxEgressPolicy, SandboxLimits};
use anyhow::{Context, Result};
use hivemind_config::HivemindConfig;
use hivemind_models::Task;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tokio::sync::oneshot;

const BYTES_PER_MB: u64 = 1024 * 1024;
const MAX_TASK_PACKAGE_ENTRIES: usize = 10_000;

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

fn validate_production_requirements(
    config: &HivemindConfig,
    policy: &SandboxEgressPolicy,
) -> Result<()> {
    if !config
        .executor
        .sandbox_mode
        .eq_ignore_ascii_case("production")
    {
        return Ok(());
    }

    if config.auth.jwt_secret.trim().is_empty()
        || config.auth.jwt_secret == "CHANGE_ME_IN_PRODUCTION"
        || config.auth.jwt_secret == "change-me-in-production"
    {
        return Err(anyhow::anyhow!(
            "production mode requires a non-default JWT_SECRET"
        ));
    }
    if !policy.is_release_safe() {
        return Err(anyhow::anyhow!(
            "production mode requires network egress policy (enable egress and configure allowlist/denylist targets)"
        ));
    }

    Ok(())
}

fn task_result_from_output(
    task_id: String,
    output: std::process::Output,
    elapsed_ms: i64,
) -> super::TaskResult {
    super::TaskResult {
        task_id,
        success: output.status.success(),
        output: Some(String::from_utf8_lossy(&output.stdout).to_string()),
        error: if output.status.success() {
            None
        } else {
            Some(String::from_utf8_lossy(&output.stderr).to_string())
        },
        exit_code: output.status.code().unwrap_or(-1),
        cpu_time_ms: 0,
        wall_time_ms: elapsed_ms,
        peak_memory_mb: 0,
    }
}

fn decode_magnet_display_name(source: &str) -> Option<String> {
    let query = source.strip_prefix("magnet:?")?;
    query.split('&').find_map(|part| {
        let value = part.strip_prefix("dn=")?;
        Some(percent_decode(value))
    })
}

fn decode_magnet_btih(source: &str) -> Option<String> {
    let query = source.strip_prefix("magnet:?")?;
    query.split('&').find_map(|part| {
        let decoded = percent_decode(part);
        let lower = decoded.to_ascii_lowercase();
        let prefix = "xt=urn:btih:";
        if lower.starts_with(prefix) {
            Some(decoded[prefix.len()..].to_string())
        } else {
            None
        }
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                decoded.push((high << 4) | low);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn candidate_task_paths(source: &str, config: &HivemindConfig) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let trimmed = source.trim();
    if !trimmed.is_empty() && !trimmed.starts_with("magnet:?") {
        candidates.push(PathBuf::from(trimmed));
    }

    if let Some(name) = decode_magnet_display_name(trimmed) {
        let file_name = Path::new(&name)
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(name));
        candidates.push(PathBuf::from(&config.torrent.api_dir).join(&file_name));
        candidates.push(
            PathBuf::from(&config.torrent.api_dir)
                .join("uploads")
                .join(&file_name),
        );
    }

    candidates
}

fn resolve_task_source(task: &Task, config: &HivemindConfig) -> Result<PathBuf> {
    let source = task.torrent_source.as_deref().unwrap_or("");
    let expected_btih = decode_magnet_btih(source);
    for path in candidate_task_paths(source, config) {
        if path.exists() {
            let path = canonicalize_task_artifact(&path, config)?;
            if let Some(expected_btih) = expected_btih.as_deref() {
                verify_task_artifact_btih(&path, expected_btih, config)?;
            }
            return Ok(path);
        }
    }

    anyhow::bail!(
        "task source '{}' is not available locally; remote torrent/artifact fetch is not implemented for worker-executor",
        source
    )
}

fn canonicalize_task_artifact(path: &Path, config: &HivemindConfig) -> Result<PathBuf> {
    let artifact_root = fs::canonicalize(&config.torrent.api_dir).with_context(|| {
        format!(
            "configured task artifact directory '{}' is not available",
            config.torrent.api_dir
        )
    })?;
    let artifact_path = fs::canonicalize(path)
        .with_context(|| format!("task source '{}' is not available", path.display()))?;

    if !artifact_path.starts_with(&artifact_root) {
        anyhow::bail!(
            "task source '{}' is outside configured task artifact directory '{}'",
            path.display(),
            artifact_root.display()
        );
    }

    Ok(artifact_path)
}

fn verify_task_artifact_btih(
    path: &Path,
    expected_btih: &str,
    config: &HivemindConfig,
) -> Result<()> {
    let data = fs::read(path)
        .with_context(|| format!("Failed to read task artifact {}", path.display()))?;
    let metainfo = hivemind_torrent_service::metainfo::create_metainfo(
        &data,
        path,
        &config.torrent.announce_url,
    )?;

    if !metainfo.info_hash.eq_ignore_ascii_case(expected_btih) {
        anyhow::bail!(
            "task artifact '{}' info hash '{}' does not match magnet BTIH '{}'",
            path.display(),
            metainfo.info_hash,
            expected_btih
        );
    }

    Ok(())
}

fn effective_memory_limit_mb(task: &Task, config: &HivemindConfig) -> u64 {
    if task.req_memory_gb > 0 {
        config
            .executor
            .max_memory_mb
            .min(task.req_memory_gb as u64 * 1024)
    } else {
        config.executor.max_memory_mb
    }
}

fn prepare_task_script(
    task: &Task,
    config: &HivemindConfig,
    limits: &SandboxLimits,
) -> Result<PathBuf> {
    let source = resolve_task_source(task, config)?;
    if source.is_file()
        && source
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        return Ok(source);
    }

    if source.is_file()
        && source
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        let sandbox = super::sandbox::prepare_sandbox(&config.executor.sandbox_dir, &task.task_id)
            .context("Failed to prepare task sandbox")?;
        extract_zip_safely(
            &source,
            &sandbox,
            limits.max_storage_mb.saturating_mul(BYTES_PER_MB),
            MAX_TASK_PACKAGE_ENTRIES,
        )?;
        let main_py = sandbox.join("main.py");
        if main_py.is_file() {
            return Ok(main_py);
        }
        anyhow::bail!(
            "task package '{}' does not contain top-level main.py",
            source.display()
        );
    }

    anyhow::bail!(
        "unsupported task source '{}'; expected a local .py file or .zip package",
        source.display()
    )
}

fn extract_zip_safely(
    zip_path: &Path,
    destination: &Path,
    max_uncompressed_bytes: u64,
    max_entries: usize,
) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("Failed to open task package {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Failed to read task package {}", zip_path.display()))?;

    if archive.len() > max_entries {
        anyhow::bail!(
            "task package contains {} entries, exceeding limit {}",
            archive.len(),
            max_entries
        );
    }

    let plans = validate_zip_entries(&mut archive, max_uncompressed_bytes)?;

    for (index, plan) in plans.into_iter().enumerate() {
        let mut entry = archive.by_index(index)?;
        let out_path = destination.join(&plan.path);

        if plan.is_dir {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(&out_path)?;
        io::copy(&mut entry, &mut output)?;
    }

    Ok(())
}

struct ZipEntryPlan {
    path: PathBuf,
    is_dir: bool,
}

fn validate_zip_entries<R: io::Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    max_uncompressed_bytes: u64,
) -> Result<Vec<ZipEntryPlan>> {
    let mut seen_paths = HashSet::new();
    let mut file_paths = HashSet::new();
    let mut directory_paths = HashSet::new();
    let mut total_uncompressed = 0u64;
    let mut plans = Vec::with_capacity(archive.len());

    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let Some(enclosed_name) = entry.enclosed_name() else {
            anyhow::bail!("task package contains unsafe path '{}'", entry.name());
        };
        let enclosed_name = normalize_zip_entry_path(&enclosed_name);
        if enclosed_name.as_os_str().is_empty() {
            anyhow::bail!("task package contains empty path '{}'", entry.name());
        }
        if !seen_paths.insert(enclosed_name.clone()) {
            anyhow::bail!(
                "task package contains duplicate path '{}'",
                enclosed_name.display()
            );
        }
        total_uncompressed = total_uncompressed
            .checked_add(entry.size())
            .context("task package uncompressed size overflow")?;
        if total_uncompressed > max_uncompressed_bytes {
            anyhow::bail!(
                "task package uncompressed size {} bytes exceeds task storage limit {} bytes",
                total_uncompressed,
                max_uncompressed_bytes
            );
        }

        if entry.is_dir() {
            reject_zip_directory_conflict(&enclosed_name, &file_paths)?;
            directory_paths.insert(enclosed_name.clone());
        } else {
            reject_zip_file_conflict(&enclosed_name, &file_paths, &directory_paths)?;
            file_paths.insert(enclosed_name.clone());
        }
        mark_parent_directories(&enclosed_name, &mut directory_paths);
        plans.push(ZipEntryPlan {
            path: enclosed_name,
            is_dir: entry.is_dir(),
        });
    }

    Ok(plans)
}

fn reject_zip_file_conflict(
    path: &Path,
    file_paths: &HashSet<PathBuf>,
    directory_paths: &HashSet<PathBuf>,
) -> Result<()> {
    if file_paths.contains(path) || directory_paths.contains(path) {
        anyhow::bail!(
            "task package contains conflicting file and directory paths at '{}'",
            path.display()
        );
    }
    if let Some(file_ancestor) = file_ancestor(path, file_paths) {
        anyhow::bail!(
            "task package contains conflicting file and directory paths at '{}' and '{}'",
            file_ancestor.display(),
            path.display()
        );
    }
    Ok(())
}

fn reject_zip_directory_conflict(path: &Path, file_paths: &HashSet<PathBuf>) -> Result<()> {
    if file_paths.contains(path) {
        anyhow::bail!(
            "task package contains conflicting file and directory paths at '{}'",
            path.display()
        );
    }
    if let Some(file_ancestor) = file_ancestor(path, file_paths) {
        anyhow::bail!(
            "task package contains conflicting file and directory paths at '{}' and '{}'",
            file_ancestor.display(),
            path.display()
        );
    }
    Ok(())
}

fn file_ancestor(path: &Path, file_paths: &HashSet<PathBuf>) -> Option<PathBuf> {
    let mut ancestor = path.parent();
    while let Some(parent) = ancestor {
        if parent.as_os_str().is_empty() {
            break;
        }
        if file_paths.contains(parent) {
            return Some(parent.to_path_buf());
        }
        ancestor = parent.parent();
    }
    None
}

fn mark_parent_directories(path: &Path, directory_paths: &mut HashSet<PathBuf>) {
    let mut parent = path.parent();
    while let Some(path) = parent {
        if path.as_os_str().is_empty() {
            break;
        }
        directory_paths.insert(path.to_path_buf());
        parent = path.parent();
    }
}

fn normalize_zip_entry_path(path: &Path) -> PathBuf {
    path.components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::Normal(part) => Some(PathBuf::from(part)),
            _ => None,
        })
        .fold(PathBuf::new(), |mut normalized, part| {
            normalized.push(part);
            normalized
        })
}

pub async fn run_task(task: &Task, config: &HivemindConfig) -> Result<super::TaskResult> {
    let (_cancel_tx, cancel_rx) = oneshot::channel();
    run_task_with_cancel(task, config, cancel_rx).await
}

pub async fn run_task_with_cancel(
    task: &Task,
    config: &HivemindConfig,
    cancel_rx: oneshot::Receiver<()>,
) -> Result<super::TaskResult> {
    let start = Instant::now();
    let btih = sanitize_btih(task.torrent_source.as_deref().unwrap_or(""));
    tracing::info!(
        "Executing task {} (BTIH: {}, requires GPU: {}, storage: {}GB)",
        task.task_id,
        &btih[..usize::min(8, btih.len())],
        task.req_gpu_score > 0,
        task.req_storage_gb
    );

    let limits = SandboxLimits {
        max_cpu_percent: config.executor.max_cpu_percent,
        max_memory_mb: effective_memory_limit_mb(task, config),
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

    let policy = SandboxEgressPolicy::from_config(&config.executor)?;
    validate_production_requirements(config, &policy)?;

    let output = execute_sandboxed(task, config, &limits, cancel_rx).await?;
    let elapsed = start.elapsed();
    match output {
        ExecutionOutput::Completed(output) => Ok(task_result_from_output(
            task.task_id.clone(),
            output,
            elapsed.as_millis() as i64,
        )),
        ExecutionOutput::Stopped { stdout, stderr } => Ok(super::TaskResult {
            task_id: task.task_id.clone(),
            success: false,
            output: Some(stdout),
            error: Some(if stderr.trim().is_empty() {
                "Task execution stopped".to_string()
            } else {
                format!("Task execution stopped\n{}", stderr)
            }),
            exit_code: -1,
            cpu_time_ms: 0,
            wall_time_ms: elapsed.as_millis() as i64,
            peak_memory_mb: 0,
        }),
    }
}

enum ExecutionOutput {
    Completed(std::process::Output),
    Stopped { stdout: String, stderr: String },
}

async fn execute_sandboxed(
    task: &Task,
    config: &HivemindConfig,
    limits: &SandboxLimits,
    cancel_rx: oneshot::Receiver<()>,
) -> Result<ExecutionOutput> {
    if !std::path::Path::new(&config.executor.monty_executable).exists() {
        return Err(anyhow::anyhow!(
            "sandbox executable '{}' not found",
            config.executor.monty_executable
        ));
    }

    let task_script = prepare_task_script(task, config, limits)?;

    let mut cmd = Command::new(&config.executor.monty_executable);
    cmd.arg("--max-duration")
        .arg(limits.max_wall_time_secs.to_string())
        .arg("--max-memory")
        .arg(format!("{}MB", limits.max_memory_mb))
        .arg(&task_script);
    configure_process_tree(&mut cmd);
    if limits.gpu_required {
        tracing::info!(
            "Task {} requires GPU, enabling GPU access in sandbox",
            task.task_id
        );
    }

    let child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn executor process")?;

    wait_for_process_output(child, cancel_rx).await
}

async fn wait_for_process_output(
    child: std::process::Child,
    cancel_rx: oneshot::Receiver<()>,
) -> Result<ExecutionOutput> {
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let mut join = tokio::task::spawn_blocking(move || {
        let mut child = child;
        loop {
            if stop_rx.try_recv().is_ok() {
                terminate_process_tree(&mut child);
                let output = child.wait_with_output()?;
                return Ok::<_, std::io::Error>(ExecutionOutput::Stopped {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                return Ok(ExecutionOutput::Completed(output));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    tokio::select! {
        result = &mut join => {
            result
                .context("Executor task panicked")?
                .context("Executor process failed")
        }
        _ = cancel_rx => {
            let _ = stop_tx.send(());
            join.await
                .context("Executor task panicked")?
                .context("Executor process failed")
        }
    }
}

#[cfg(windows)]
fn configure_process_tree(_cmd: &mut Command) {}

#[cfg(unix)]
fn configure_process_tree(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(windows)]
fn terminate_process_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = child.kill();
}

#[cfg(unix)]
fn terminate_process_tree(child: &mut std::process::Child) {
    let process_group = format!("-{}", child.id());
    send_unix_process_group_signal("-TERM", &process_group);
    std::thread::sleep(std::time::Duration::from_millis(200));
    send_unix_process_group_signal("-KILL", &process_group);
    std::thread::sleep(std::time::Duration::from_millis(100));
    let _ = child.kill();
}

#[cfg(unix)]
fn send_unix_process_group_signal(signal: &str, process_group: &str) {
    let _ = std::process::Command::new("kill")
        .args([signal, "--", process_group])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
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
            .contains("sandbox executable"));
    }

    #[tokio::test]
    async fn real_executor_binary_controls_task_output() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let script = write_test_executor_script(tmp.path(), "deterministic output", "", 0);
        config.executor.monty_executable = script.to_string_lossy().to_string();
        let task_file = write_task_file(std::path::Path::new(&config.torrent.api_dir));

        let result = run_task(&test_task_with_source(task_file), &config)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some("deterministic output"));
        assert_eq!(result.error, None);
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn local_python_task_uses_monty_cli_file_contract() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let script = write_monty_contract_executor_script(tmp.path());
        let task_file = std::path::Path::new(&config.torrent.api_dir).join("main.py");
        std::fs::write(&task_file, "print('hello from task')\n").unwrap();
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let mut task = test_task();
        task.torrent_source = Some(task_file.to_string_lossy().to_string());

        let result = run_task(&task, &config).await.unwrap();

        assert!(result.success, "stderr: {:?}", result.error);
        assert_eq!(result.output.as_deref(), Some("monty-file-ok"));
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn task_requested_memory_caps_monty_memory_argument() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.executor.max_memory_mb = 4096;
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let (script, args_file) = write_arg_capture_executor_script(tmp.path());
        let task_file = std::path::Path::new(&config.torrent.api_dir).join("main.py");
        std::fs::write(&task_file, "print('hello from task')\n").unwrap();
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let mut task = test_task();
        task.req_memory_gb = 1;
        task.torrent_source = Some(task_file.to_string_lossy().to_string());

        let result = run_task(&task, &config).await.unwrap();

        assert!(result.success, "stderr: {:?}", result.error);
        let args = std::fs::read_to_string(args_file).unwrap();
        assert!(args.contains("--max-memory"), "args: {args}");
        assert!(args.contains("1024MB"), "args: {args}");
        assert!(!args.contains("4096MB"), "args: {args}");
    }

    #[tokio::test]
    async fn unspecified_task_memory_uses_global_executor_memory_argument() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.executor.max_memory_mb = 4096;
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let (script, args_file) = write_arg_capture_executor_script(tmp.path());
        let task_file = std::path::Path::new(&config.torrent.api_dir).join("main.py");
        std::fs::write(&task_file, "print('hello from task')\n").unwrap();
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let mut task = test_task();
        task.req_memory_gb = 0;
        task.torrent_source = Some(task_file.to_string_lossy().to_string());

        let result = run_task(&task, &config).await.unwrap();

        assert!(result.success, "stderr: {:?}", result.error);
        let args = std::fs::read_to_string(args_file).unwrap();
        assert!(args.contains("4096MB"), "args: {args}");
        assert!(!args.contains("1024MB"), "args: {args}");
    }

    #[tokio::test]
    async fn local_zip_task_extracts_main_py_and_uses_monty_file_contract() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let script = write_monty_contract_executor_script(tmp.path());
        let package = write_task_zip(std::path::Path::new(&config.torrent.api_dir), "task.zip");
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let result = run_task(&test_task_with_source(package.to_string_lossy()), &config)
            .await
            .unwrap();

        assert!(result.success, "stderr: {:?}", result.error);
        assert_eq!(result.output.as_deref(), Some("monty-file-ok"));
        assert!(
            tmp.path()
                .join("sandbox")
                .join("sandbox-gate-test")
                .join("main.py")
                .exists(),
            "main.py should be materialized in the task sandbox"
        );
    }

    #[tokio::test]
    async fn magnet_display_name_resolves_local_seed_package() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let package = write_task_zip(
            std::path::Path::new(&config.torrent.api_dir),
            "task name.zip",
        );
        let data = std::fs::read(&package).unwrap();
        let metainfo = hivemind_torrent_service::metainfo::create_metainfo(
            &data,
            &package,
            &config.torrent.announce_url,
        )
        .unwrap();
        let script = write_monty_contract_executor_script(tmp.path());
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let source = format!(
            "magnet:?xt=urn:btih:{}&dn=task%20name.zip&tr=",
            metainfo.info_hash
        );
        let result = run_task(&test_task_with_source(source), &config)
            .await
            .unwrap();

        assert!(result.success, "stderr: {:?}", result.error);
        assert_eq!(result.output.as_deref(), Some("monty-file-ok"));
        assert!(package.exists());
    }

    #[tokio::test]
    async fn local_task_source_rejects_paths_outside_artifact_dir() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let script = write_monty_contract_executor_script(tmp.path());
        let outside_task = write_task_file(tmp.path());
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let result = run_task(&test_task_with_source(outside_task), &config).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("is outside configured task artifact directory"));
    }

    #[tokio::test]
    async fn magnet_display_name_rejects_mismatched_btih() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        write_task_zip(std::path::Path::new(&config.torrent.api_dir), "task.zip");
        let script = write_monty_contract_executor_script(tmp.path());
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let source = "magnet:?xt=urn:btih:0000000000000000000000000000000000000000&dn=task.zip&tr=";
        let result = run_task(&test_task_with_source(source), &config).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("does not match magnet BTIH"));
    }

    #[tokio::test]
    async fn zip_task_rejects_unsafe_paths() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let script = write_monty_contract_executor_script(tmp.path());
        let package =
            write_unsafe_task_zip(std::path::Path::new(&config.torrent.api_dir), "unsafe.zip");
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let result = run_task(&test_task_with_source(package.to_string_lossy()), &config).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("task package contains unsafe path '../main.py'"));
        assert!(!tmp.path().join("main.py").exists());
    }

    #[tokio::test]
    async fn zip_task_rejects_directory_named_main_py() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let script = write_monty_contract_executor_script(tmp.path());
        let package = write_task_zip_with_main_py_directory(
            std::path::Path::new(&config.torrent.api_dir),
            "directory-main.zip",
        );
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let result = run_task(&test_task_with_source(package.to_string_lossy()), &config).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("does not contain top-level main.py"));
    }

    #[tokio::test]
    async fn zip_task_rejects_duplicate_paths() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let script = write_monty_contract_executor_script(tmp.path());
        let package = write_task_zip_with_duplicate_main(
            std::path::Path::new(&config.torrent.api_dir),
            "duplicate-main.zip",
        );
        config.executor.monty_executable = script.to_string_lossy().to_string();

        let result = run_task(&test_task_with_source(package.to_string_lossy()), &config).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("task package contains duplicate path 'main.py'"));
    }

    #[tokio::test]
    async fn zip_task_rejects_too_many_entries() {
        let tmp = TempDir::new().unwrap();
        let package = write_task_zip_with_many_entries(tmp.path(), "too-many-entries.zip", 32);
        let destination = tmp.path().join("sandbox");
        std::fs::create_dir_all(&destination).unwrap();

        let result = extract_zip_safely(&package, &destination, u64::MAX, 32);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("task package contains 33 entries, exceeding limit 32"));
    }

    #[test]
    fn zip_extraction_rejects_uncompressed_size_over_limit() {
        let tmp = TempDir::new().unwrap();
        let package = write_task_zip_with_large_file(tmp.path(), "oversized.zip", 64);
        let destination = tmp.path().join("sandbox");
        std::fs::create_dir_all(&destination).unwrap();

        let result = extract_zip_safely(&package, &destination, 32, MAX_TASK_PACKAGE_ENTRIES);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exceeds task storage limit 32 bytes"));
        assert!(!destination.join("main.py").exists());
    }

    #[test]
    fn zip_extraction_rejects_file_directory_conflict_before_writing() {
        let tmp = TempDir::new().unwrap();
        let package = write_task_zip_with_file_directory_conflict(tmp.path(), "conflict.zip");
        let destination = tmp.path().join("sandbox");
        std::fs::create_dir_all(&destination).unwrap();

        let result = extract_zip_safely(&package, &destination, u64::MAX, MAX_TASK_PACKAGE_ENTRIES);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("conflicting file and directory paths"));
        assert!(!destination.join("main.py").exists());
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
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("sandbox executable"));
    }

    #[tokio::test]
    async fn production_mode_rejects_default_jwt_secret() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().to_str().unwrap());
        config.executor.sandbox_mode = "production".into();
        config.executor.network_egress_enabled = true;
        config.executor.network_egress_mode = "allowlist".into();
        config.executor.network_egress_targets = vec!["8.8.8.8".into()];
        config.auth.jwt_secret = "CHANGE_ME_IN_PRODUCTION".into();
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
            .contains("production mode requires a non-default JWT_SECRET"));
    }

    #[tokio::test]
    async fn real_executor_binary_returns_failure_status_and_stderr() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().to_str().unwrap());
        config.executor.sandbox_mode = "production".into();
        config.executor.network_egress_enabled = true;
        config.executor.network_egress_mode = "allowlist".into();
        config.executor.network_egress_targets = vec!["8.8.8.8".into()];
        let script = write_test_executor_script(tmp.path(), "partial output", "forced failure", 7);
        config.executor.monty_executable = script.to_string_lossy().to_string();
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        let task_file = write_task_file(std::path::Path::new(&config.torrent.api_dir));

        let result = run_task(&test_task_with_source(task_file), &config)
            .await
            .unwrap();

        assert!(!result.success);
        assert_eq!(result.output.as_deref(), Some("partial output"));
        assert_eq!(result.error.as_deref(), Some("forced failure"));
        assert_eq!(result.exit_code, 7);
    }

    fn test_config(sandbox_dir: &str) -> HivemindConfig {
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = sandbox_dir.into();
        config.auth.jwt_secret = "unit-test-jwt-secret".into();
        config
    }

    fn test_task() -> Task {
        test_task_with_source("abcdef1234567890")
    }

    fn test_task_with_source(source: impl Into<String>) -> Task {
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
            torrent_source: Some(source.into()),
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

    fn write_task_file(dir: &std::path::Path) -> String {
        let path = dir.join("main.py");
        std::fs::write(&path, "print('hello from worker task')\n").unwrap();
        path.to_string_lossy().to_string()
    }

    fn write_task_zip(dir: &std::path::Path, file_name: &str) -> std::path::PathBuf {
        let path = dir.join(file_name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("main.py", options).unwrap();
        use std::io::Write;
        zip.write_all(b"print('hello from zipped worker task')\n")
            .unwrap();
        zip.finish().unwrap();
        path
    }

    fn write_unsafe_task_zip(dir: &std::path::Path, file_name: &str) -> std::path::PathBuf {
        let path = dir.join(file_name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("../main.py", options).unwrap();
        use std::io::Write;
        zip.write_all(b"print('zip slip')\n").unwrap();
        zip.finish().unwrap();
        path
    }

    fn write_task_zip_with_main_py_directory(
        dir: &std::path::Path,
        file_name: &str,
    ) -> std::path::PathBuf {
        let path = dir.join(file_name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.add_directory("main.py/", options).unwrap();
        zip.finish().unwrap();
        path
    }

    fn write_task_zip_with_duplicate_main(
        dir: &std::path::Path,
        file_name: &str,
    ) -> std::path::PathBuf {
        let path = dir.join(file_name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("main.py", options).unwrap();
        use std::io::Write;
        zip.write_all(b"print('first')\n").unwrap();
        zip.start_file("./main.py", options).unwrap();
        zip.write_all(b"print('second')\n").unwrap();
        zip.finish().unwrap();
        path
    }

    fn write_task_zip_with_many_entries(
        dir: &std::path::Path,
        file_name: &str,
        extra_entries: usize,
    ) -> std::path::PathBuf {
        let path = dir.join(file_name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("main.py", options).unwrap();
        use std::io::Write;
        zip.write_all(b"print('hello')\n").unwrap();
        for index in 0..extra_entries {
            zip.start_file(format!("data/{index}.txt"), options)
                .unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn write_task_zip_with_large_file(
        dir: &std::path::Path,
        file_name: &str,
        bytes: usize,
    ) -> std::path::PathBuf {
        let path = dir.join(file_name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("main.py", options).unwrap();
        use std::io::Write;
        zip.write_all(&vec![b'x'; bytes]).unwrap();
        zip.finish().unwrap();
        path
    }

    fn write_task_zip_with_file_directory_conflict(
        dir: &std::path::Path,
        file_name: &str,
    ) -> std::path::PathBuf {
        let path = dir.join(file_name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("main.py", options).unwrap();
        use std::io::Write;
        zip.write_all(b"print('first')\n").unwrap();
        zip.start_file("main.py/child.py", options).unwrap();
        zip.write_all(b"print('child')\n").unwrap();
        zip.finish().unwrap();
        path
    }

    // Test-only shim that writes a temporary executor script for deterministic assertions.
    fn write_test_executor_script(
        dir: &std::path::Path,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> std::path::PathBuf {
        let path = if cfg!(windows) {
            dir.join("mock-executor.cmd")
        } else {
            dir.join("mock-executor.sh")
        };

        let script = if cfg!(windows) {
            let mut script = String::from("@echo off\r\n");
            if !stdout.is_empty() {
                script.push_str(&format!("<nul set /p \"={}\"\r\n", stdout));
            }
            if !stderr.is_empty() {
                script.push_str(&format!("<nul set /p \"={}\" 1>&2\r\n", stderr));
            }
            script.push_str(&format!("exit /b {}\r\n", exit_code));
            script
        } else {
            let mut script = String::from("#!/bin/sh\n");
            if !stdout.is_empty() {
                script.push_str(&format!("printf '%s' '{}'\n", stdout));
            }
            if !stderr.is_empty() {
                script.push_str(&format!("printf '%s' '{}' 1>&2\n", stderr));
            }
            script.push_str(&format!("exit {}\n", exit_code));
            script
        };

        std::fs::write(&path, script).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }

        path
    }

    fn write_monty_contract_executor_script(dir: &std::path::Path) -> std::path::PathBuf {
        let path = if cfg!(windows) {
            dir.join("monty-contract.cmd")
        } else {
            dir.join("monty-contract.sh")
        };

        let script = if cfg!(windows) {
            String::from(
                "@echo off\r\n\
for %%A in (%*) do (\r\n\
  if \"%%~A\"==\"--task-id\" (\r\n\
    echo unsupported task flag 1^>^&2\r\n\
    exit /b 64\r\n\
  )\r\n\
)\r\n\
set \"last=\"\r\n\
for %%A in (%*) do set \"last=%%~A\"\r\n\
if not exist \"%last%\" (\r\n\
  echo missing script file 1^>^&2\r\n\
  exit /b 65\r\n\
)\r\n\
<nul set /p \"=monty-file-ok\"\r\n\
exit /b 0\r\n",
            )
        } else {
            String::from(
                "#!/bin/sh\n\
for arg in \"$@\"; do\n\
  if [ \"$arg\" = \"--task-id\" ]; then\n\
    echo 'unsupported task flag' >&2\n\
    exit 64\n\
  fi\n\
done\n\
last=''\n\
for arg in \"$@\"; do last=\"$arg\"; done\n\
if [ ! -f \"$last\" ]; then\n\
  echo 'missing script file' >&2\n\
  exit 65\n\
fi\n\
printf '%s' 'monty-file-ok'\n\
exit 0\n",
            )
        };

        std::fs::write(&path, script).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }

        path
    }

    fn write_arg_capture_executor_script(
        dir: &std::path::Path,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let path = if cfg!(windows) {
            dir.join("arg-capture.cmd")
        } else {
            dir.join("arg-capture.sh")
        };
        let args_file = dir.join("executor-args.txt");

        let script = if cfg!(windows) {
            format!(
                "@echo off\r\necho %* > \"{}\"\r\nset \"last=\"\r\nfor %%A in (%*) do set \"last=%%~A\"\r\nif not exist \"%last%\" exit /b 65\r\n<nul set /p \"=monty-file-ok\"\r\nexit /b 0\r\n",
                args_file.display()
            )
        } else {
            format!(
                "#!/bin/sh\nprintf '%s\n' \"$*\" > '{}'\nlast=''\nfor arg in \"$@\"; do last=\"$arg\"; done\n[ -f \"$last\" ] || exit 65\nprintf '%s' 'monty-file-ok'\nexit 0\n",
                args_file.display()
            )
        };

        std::fs::write(&path, script).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }

        (path, args_file)
    }
}

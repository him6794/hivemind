use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_models::Task;
use managed_function_runtime::{render_output_bounded, ExecutionLimits, ManagedExecutor};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tokio::sync::watch;

fn is_managed_function_task(task: &Task) -> bool {
    task.runtime.as_deref() == Some("managed-function-v0")
}

fn execute_managed_function_task(
    task: &Task,
    elapsed_ms: i64,
    cancelled: &AtomicBool,
) -> Result<super::TaskResult> {
    let source = task
        .task_source
        .as_deref()
        .filter(|source| !source.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("managed-function-v0 task_source is required"))?;
    let input = task
        .torrent_source
        .as_deref()
        .filter(|input| !input.trim().is_empty())
        .unwrap_or("null");
    let limits = ExecutionLimits {
        max_usage_units: (task.max_cpt > 0).then_some(task.max_cpt as u64),
        ..ExecutionLimits::default()
    };
    let max_output_bytes = limits.max_output_bytes;
    let execution =
        match ManagedExecutor.execute_json_input_with_cancel(source, limits, input, cancelled) {
            Ok(execution) => execution,
            Err(error) => {
                let error_message = if error.code() == "cancelled" {
                    "Task execution stopped".to_string()
                } else {
                    error.to_string()
                };
                let receipt = json!({
                    "runtime": "managed-function-v0",
                    "status": "failed",
                    "executed_ops": 0,
                    "output_bytes": 0,
                    "failure_code": error.code(),
                    "failure_message": error_message,
                });
                return Ok(super::TaskResult {
                    task_id: task.task_id.clone(),
                    success: false,
                    output: None,
                    error: Some(error_message),
                    exit_code: 1,
                    cpu_time_ms: 0,
                    wall_time_ms: elapsed_ms,
                    peak_memory_mb: 0,
                    managed_executed_ops: 0,
                    managed_output_bytes: 0,
                    managed_receipt_json: Some(receipt.to_string()),
                    managed_proof: None,
                });
            }
        };
    let output = if execution.output.is_empty() {
        match render_output_bounded(&execution.value, max_output_bytes) {
            Ok(output) => output,
            Err(error) => {
                let error_message = error.to_string();
                let receipt = json!({
                    "runtime": "managed-function-v0",
                    "status": "failed",
                    "executed_ops": execution.receipt.executed_ops,
                    "output_bytes": 0,
                    "failure_code": error.code(),
                    "failure_message": error_message,
                });
                return Ok(super::TaskResult {
                    task_id: task.task_id.clone(),
                    success: false,
                    output: None,
                    error: Some(error_message),
                    exit_code: 1,
                    cpu_time_ms: 0,
                    wall_time_ms: elapsed_ms,
                    peak_memory_mb: 0,
                    managed_executed_ops: execution.receipt.executed_ops as i64,
                    managed_output_bytes: 0,
                    managed_receipt_json: Some(receipt.to_string()),
                    managed_proof: None,
                });
            }
        }
    } else {
        execution.output
    };
    let output_bytes = output.len() as i64;
    let receipt = json!({
        "runtime": "managed-function-v0",
        "status": "completed",
        "usage_units": execution.receipt.usage_units,
        "executed_ops": execution.receipt.executed_ops,
        "function_calls": execution.receipt.function_calls,
        "loop_iterations": execution.receipt.loop_iterations,
        "max_call_depth": execution.receipt.max_call_depth,
        "output_bytes": output_bytes,
        "failure_code": execution.receipt.failure_code,
        "failure_message": execution.receipt.failure_message,
    });

    Ok(super::TaskResult {
        task_id: task.task_id.clone(),
        success: true,
        output: Some(output),
        error: None,
        exit_code: 0,
        cpu_time_ms: 0,
        wall_time_ms: elapsed_ms,
        peak_memory_mb: 0,
        managed_executed_ops: execution.receipt.usage_units.min(i64::MAX as u64) as i64,
        managed_output_bytes: output_bytes,
        managed_receipt_json: Some(receipt.to_string()),
        managed_proof: None,
    })
}

pub async fn run_task(task: &Task, config: &HivemindConfig) -> Result<super::TaskResult> {
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    run_task_with_cancel(task, config, cancel_rx).await
}

pub async fn run_task_with_cancel(
    task: &Task,
    _config: &HivemindConfig,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<super::TaskResult> {
    let start = Instant::now();
    tracing::info!(
        "Executing task {} (runtime: {}, requires GPU: {}, storage: {}GB)",
        task.task_id,
        task.runtime.as_deref().unwrap_or("<none>"),
        task.req_gpu_score > 0,
        task.req_storage_gb
    );

    if is_managed_function_task(task) {
        let task = task.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let execution_cancelled = cancelled.clone();
        let mut execution = tokio::task::spawn_blocking(move || {
            execute_managed_function_task(
                &task,
                start.elapsed().as_millis() as i64,
                &execution_cancelled,
            )
        });
        return tokio::select! {
            result = &mut execution => result.map_err(anyhow::Error::from)?,
            _ = wait_for_cancellation(&mut cancel_rx) => {
                cancelled.store(true, Ordering::Release);
                execution.await.map_err(anyhow::Error::from)?
            }
        };
    }

    Err(anyhow::anyhow!(
        "unsupported runtime {:?}: only managed-function-v0 tasks are supported",
        task.runtime.as_deref().unwrap_or("<none>")
    ))
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    if *cancellation.borrow() {
        return;
    }
    while cancellation.changed().await.is_ok() {
        if *cancellation.borrow() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hivemind_models::TaskStatus;
    use tempfile::TempDir;
    use uuid::Uuid;

    #[tokio::test]
    async fn managed_function_task_executes_without_host_artifact_or_process() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
        config.executor.monty_executable = tmp
            .path()
            .join("must-not-be-called")
            .to_string_lossy()
            .to_string();
        let mut task = test_task_with_source("{\"items\":[1,2,3]}");
        task.runtime = Some("managed-function-v0".into());
        task.task_source = Some(
            "let total = 0; for item in get(input, \"items\") { let total = total + item; } return total;"
                .into(),
        );
        task.max_cpt = 1000;

        let result = run_task(&task, &config).await.unwrap();

        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some("6"));
        assert_eq!(result.exit_code, 0);
        assert!(result.managed_receipt_json.is_some());
        assert!(result.managed_executed_ops > 0);
        assert_eq!(result.managed_output_bytes, 1);
    }

    #[tokio::test]
    async fn managed_function_budget_exhaustion_returns_structured_failure() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let mut task = test_task_with_source("null");
        task.runtime = Some("managed-function-v0".into());
        task.max_cpt = 2;
        task.task_source = Some("return 1 + 2 + 3;".into());

        let result = run_task(&task, &config).await.unwrap();

        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("budget_exhausted"));
        assert!(result
            .managed_receipt_json
            .as_deref()
            .unwrap_or_default()
            .contains("budget_exhausted"));
    }

    #[tokio::test]
    async fn managed_function_task_keeps_its_usage_budget_while_enforcing_default_call_depth() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let mut source = String::new();
        for depth in 0..64 {
            source.push_str(&format!(
                "fn step_{depth}() {{ return step_{}(); }}\n",
                depth + 1
            ));
        }
        source.push_str("fn step_64() { return 0; }\nreturn step_0();");

        let mut task = test_task_with_source("null");
        task.runtime = Some("managed-function-v0".into());
        task.max_cpt = 10_000;
        task.task_source = Some(source);

        let result = run_task(&task, &config).await.unwrap();

        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("call_depth_exceeded"));
        assert!(result
            .managed_receipt_json
            .as_deref()
            .unwrap_or_default()
            .contains("call_depth_exceeded"));
    }

    #[tokio::test]
    async fn managed_function_task_rejects_an_oversized_return_value_before_reporting_success() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let mut expression = "\"x\"".to_string();
        for _ in 0..21 {
            expression = format!("double({expression})");
        }

        let mut task = test_task_with_source("null");
        task.runtime = Some("managed-function-v0".into());
        task.max_cpt = 10_000;
        task.task_source = Some(format!(
            "fn double(value) {{ return value + value; }} return {expression};"
        ));

        let result = run_task(&task, &config).await.unwrap();

        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("value_limit_exceeded"));
        assert!(result
            .managed_receipt_json
            .as_deref()
            .unwrap_or_default()
            .contains("value_limit_exceeded"));
    }

    #[tokio::test]
    async fn unsupported_runtime_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let task = test_task_with_source("payload");

        let error = run_task(&task, &config).await.unwrap_err();
        assert!(error.to_string().contains("unsupported runtime"));
    }

    fn test_config(sandbox_dir: &str) -> HivemindConfig {
        let mut config = HivemindConfig::default();
        config.executor.sandbox_dir = sandbox_dir.into();
        config.auth.jwt_secret = "unit-test-jwt-secret".into();
        // Workers only need the platform public key; the default sample key is valid.
        config
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
            runtime: None,
            task_source: None,
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
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
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

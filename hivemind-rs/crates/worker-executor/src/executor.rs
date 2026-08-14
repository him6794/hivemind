use anyhow::Result;
use general_compute_runtime::artifact::{ArtifactMaterializer, CasChunkStore};
use general_compute_runtime::cp_python::{PythonBackendRegistration, PythonBackendRegistry};
use general_compute_runtime::execution::{ExecutionError, ReferenceBackendExecutor};
use general_compute_runtime::supervisor::Cancellation;
use general_compute_runtime::{
    GeneralComputeRequest, GeneralComputeResult, ResultStatus, TrustedWorkerCapabilityRegistration,
};
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
                    general_compute_result_json: None,
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
                    general_compute_result_json: None,
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
        general_compute_result_json: None,
    })
}

pub async fn run_task(task: &Task, config: &HivemindConfig) -> Result<super::TaskResult> {
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    run_task_with_cancel(task, config, cancel_rx).await
}

pub async fn run_task_with_cancel(
    task: &Task,
    config: &HivemindConfig,
    cancel_rx: watch::Receiver<bool>,
) -> Result<super::TaskResult> {
    run_task_with_cancel_and_reference(task, config, cancel_rx, None).await
}

pub async fn run_task_with_cancel_and_reference(
    task: &Task,
    config: &HivemindConfig,
    cancel_rx: watch::Receiver<bool>,
    reference_executor: Option<Arc<ReferenceBackendExecutor>>,
) -> Result<super::TaskResult> {
    run_task_with_cancel_and_reference_and_cas(task, config, cancel_rx, reference_executor, None)
        .await
}

pub async fn run_task_with_cancel_and_reference_and_cas(
    task: &Task,
    config: &HivemindConfig,
    mut cancel_rx: watch::Receiver<bool>,
    reference_executor: Option<Arc<ReferenceBackendExecutor>>,
    cas_store: Option<Arc<CasChunkStore>>,
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

    if task.runtime.as_deref() == Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION) {
        let task = task.clone();
        let reference_executor = reference_executor.clone();
        let config = config.clone();
        let cas_store = cas_store.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let runtime_cancellation = Arc::new(Cancellation::new());
        let execution_cancelled = cancelled.clone();
        let execution_runtime_cancellation = runtime_cancellation.clone();
        let mut execution = tokio::task::spawn_blocking(move || {
            execute_general_compute_task(
                &task,
                &config,
                reference_executor.as_deref(),
                cas_store.as_deref(),
                &execution_cancelled,
                &execution_runtime_cancellation,
            )
        });
        return tokio::select! {
            result = &mut execution => result.map_err(anyhow::Error::from)?,
            _ = wait_for_cancellation(&mut cancel_rx) => {
                cancelled.store(true, Ordering::Release);
                runtime_cancellation.cancel();
                execution.await.map_err(anyhow::Error::from)?
            }
        };
    }

    Err(anyhow::anyhow!(
        "unsupported runtime {:?}: only managed-function-v0 tasks are supported",
        task.runtime.as_deref().unwrap_or("<none>")
    ))
}

fn execute_general_compute_task(
    task: &Task,
    config: &HivemindConfig,
    reference_executor: Option<&ReferenceBackendExecutor>,
    cas_store: Option<&CasChunkStore>,
    cancelled: &AtomicBool,
    cancellation: &Cancellation,
) -> Result<super::TaskResult> {
    let request = task
        .general_compute_manifest_json
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("general-compute request manifest is required"))
        .and_then(|manifest| {
            serde_json::from_slice::<GeneralComputeRequest>(manifest).map_err(|error| {
                anyhow::anyhow!("general-compute request manifest is malformed: {error}")
            })
        })?;
    if cancelled.load(Ordering::Acquire) {
        cancellation.cancel();
    }
    let result = if let Some(executor) = reference_executor {
        let root = absolute_runtime_root(config, &task.task_id)?;
        let materializer =
            ArtifactMaterializer::new(root).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        match cas_store {
            Some(store) => executor.execute_with_cas_with_cancellation(
                &request,
                &materializer,
                store,
                cancellation,
            ),
            None => executor.execute_with_cancellation(&request, &materializer, cancellation),
        }
    } else {
        Err(ExecutionError::BackendUnavailable(
            "reference backend is not operator-configured".into(),
        ))
    };
    let typed = match result {
        Ok(result) => result,
        Err(error) => failed_general_compute_result(&request, error_code(&error)),
    };
    let encoded = serde_json::to_vec(&typed)?;
    let completed = typed.status == ResultStatus::Completed;
    Ok(super::TaskResult {
        task_id: task.task_id.clone(),
        success: completed,
        output: completed.then(|| typed.stdout.clone()),
        error: (!completed).then(|| {
            typed
                .error_code
                .clone()
                .unwrap_or_else(|| "backend_unavailable".into())
        }),
        exit_code: typed.exit_code.unwrap_or(1),
        cpu_time_ms: typed.usage.cpu_time_ms.min(i64::MAX as u64) as i64,
        wall_time_ms: typed.usage.wall_time_ms.min(i64::MAX as u64) as i64,
        peak_memory_mb: (typed.usage.peak_memory_bytes / (1024 * 1024)).min(i64::MAX as u64) as i64,
        managed_executed_ops: 0,
        managed_output_bytes: typed.usage.output_bytes.min(i64::MAX as u64) as i64,
        managed_receipt_json: None,
        managed_proof: None,
        general_compute_result_json: Some(encoded),
    })
}
pub(crate) async fn run_task_with_cancel_and_reference_and_cas_and_trusted_registration(
    task: &Task,
    config: &HivemindConfig,
    cancel_rx: watch::Receiver<bool>,
    reference_executor: Option<Arc<ReferenceBackendExecutor>>,
    cas_store: Option<Arc<CasChunkStore>>,
    trusted_registration: Option<TrustedWorkerCapabilityRegistration>,
) -> Result<super::TaskResult> {
    let request = task
        .general_compute_manifest_json
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("general-compute request manifest is required"))
        .and_then(|manifest| {
            serde_json::from_slice::<GeneralComputeRequest>(manifest).map_err(|error| {
                anyhow::anyhow!("general-compute request manifest is malformed: {error}")
            })
        })?;
    let selection = match trusted_gpu_selection(&request, trusted_registration.as_ref()) {
        Ok(selection) => selection,
        Err(_error) => {
            return general_compute_task_result(
                task,
                failed_general_compute_result(&request, "gpu_unavailable"),
            );
        }
    };
    let mut result = run_task_with_cancel_and_reference_and_cas(
        task,
        config,
        cancel_rx,
        reference_executor,
        cas_store,
    )
    .await?;
    let Some(encoded) = result.general_compute_result_json.as_deref() else {
        return Ok(result);
    };
    let mut typed = serde_json::from_slice::<GeneralComputeResult>(encoded)?;
    typed.gpu_selection = selection;
    result.general_compute_result_json = Some(serde_json::to_vec(&typed)?);
    Ok(result)
}

fn trusted_gpu_selection(
    request: &GeneralComputeRequest,
    registration: Option<&TrustedWorkerCapabilityRegistration>,
) -> Result<Option<general_compute_runtime::gpu::GpuSelection>, String> {
    match registration {
        Some(registration) => registration
            .select_gpu_for_request(request)
            .map_err(|error| error.message),
        None if request.execution_policy.gpu_required => Err(
            "typed GPU request requires an operator-approved trusted registration".into(),
        ),
        None => Ok(None),
    }
}

fn general_compute_task_result(
    task: &Task,
    typed: GeneralComputeResult,
) -> Result<super::TaskResult> {
    let encoded = serde_json::to_vec(&typed)?;
    let completed = typed.status == ResultStatus::Completed;
    Ok(super::TaskResult {
        task_id: task.task_id.clone(),
        success: completed,
        output: completed.then(|| typed.stdout.clone()),
        error: (!completed).then(|| {
            typed
                .error_code
                .clone()
                .unwrap_or_else(|| "backend_unavailable".into())
        }),
        exit_code: typed.exit_code.unwrap_or(1),
        cpu_time_ms: typed.usage.cpu_time_ms.min(i64::MAX as u64) as i64,
        wall_time_ms: typed.usage.wall_time_ms.min(i64::MAX as u64) as i64,
        peak_memory_mb: (typed.usage.peak_memory_bytes / (1024 * 1024)).min(i64::MAX as u64) as i64,
        managed_executed_ops: 0,
        managed_output_bytes: typed.usage.output_bytes.min(i64::MAX as u64) as i64,
        managed_receipt_json: None,
        managed_proof: None,
        general_compute_result_json: Some(encoded),
    })
}

fn absolute_runtime_root(config: &HivemindConfig, task_id: &str) -> Result<std::path::PathBuf> {
    if !crate::sandbox::is_safe_task_id(task_id) {
        anyhow::bail!("unsafe task id for general-compute artifact root");
    }
    let root = std::path::PathBuf::from(&config.executor.sandbox_dir);
    let root = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()?.join(root)
    };
    Ok(root.join("general-compute").join(task_id))
}

fn error_code(error: &ExecutionError) -> &'static str {
    match error {
        ExecutionError::BackendUnavailable(_) => "backend_unavailable",
        ExecutionError::Capability(_) => "capability_rejected",
        ExecutionError::Artifact(_) => "artifact_invalid",
        ExecutionError::Request(_) => "request_invalid",
        ExecutionError::UnsupportedExecutionMode => "backend_unavailable",
        ExecutionError::UnsupportedEntrypoint => "request_invalid",
        ExecutionError::SourceNotUtf8 | ExecutionError::InputNotUtf8 => "artifact_invalid",
        ExecutionError::MultipleInputArtifacts => "request_invalid",
        ExecutionError::Backend(_) => "backend_failed",
    }
}

fn failed_general_compute_result(
    request: &GeneralComputeRequest,
    code: &str,
) -> GeneralComputeResult {
    GeneralComputeResult {
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        status: ResultStatus::BackendUnavailable,
        exit_code: None,
        error_code: Some(code.into()),
        stdout: String::new(),
        stderr: String::new(),
        output_artifacts: Vec::new(),
        usage: Default::default(),
        runtime_version: request.runtime_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        input_sha256: general_compute_runtime::sha256_digest(&[]),
        determinism: request.determinism.clone(),
        capability_summary: Vec::new(),
        gpu_selection: None,
        output_manifest_root: general_compute_runtime::canonical_artifact_root(&[]),
        evidence: Default::default(),
    }
}

pub fn reference_executor_from_environment(
    admission: &crate::runtime_admission::WorkerRuntimeAdmission,
) -> Option<Arc<ReferenceBackendExecutor>> {
    let registrations = std::env::var("HIVEMIND_GENERAL_COMPUTE_REFERENCE_BACKENDS").ok()?;
    let registrations =
        serde_json::from_str::<Vec<PythonBackendRegistration>>(&registrations).ok()?;
    let registry = PythonBackendRegistry::new(registrations).ok()?;
    Some(Arc::new(ReferenceBackendExecutor::new_with_trusted_registration(
        admission.capability_matrix(),
        admission.worker_capabilities(),
        registry,
        admission.trusted_registration(),
    )))
}

/// Load the optional operator-owned local CAS root. Invalid or relative roots
/// disable CAS materialization rather than widening the worker's filesystem
/// access or falling back to an inferred path.
pub fn cas_store_from_environment() -> Option<Arc<CasChunkStore>> {
    let root = std::env::var("HIVEMIND_GENERAL_COMPUTE_CAS_ROOT").ok()?;
    if root.trim().is_empty() {
        return None;
    }
    match CasChunkStore::new(root) {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::warn!(error = %error, "general-compute CAS root is invalid; CAS execution disabled");
            None
        }
    }
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
    use managed_function_runtime::V0_SEMANTICS_MANIFEST_JSON;
    use serde_json::Value;
    use tempfile::TempDir;
    use uuid::Uuid;

    #[tokio::test]
    async fn managed_function_task_executes_without_host_artifact_or_process() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        config.torrent.api_dir = tmp.path().join("api").to_string_lossy().to_string();
        std::fs::create_dir_all(&config.torrent.api_dir).unwrap();
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

        let manifest: Value = serde_json::from_str(V0_SEMANTICS_MANIFEST_JSON).unwrap();
        let receipt: Value =
            serde_json::from_str(result.managed_receipt_json.as_deref().unwrap()).unwrap();
        assert!(manifest["failure_receipts"]["worker_synthetic_receipt"]
            .as_bool()
            .unwrap());
        assert_eq!(
            manifest["failure_receipts"]["evaluation_failure_counters"],
            "zeroed"
        );
        assert_eq!(receipt["runtime"], manifest["runtime_id"]);
        assert_eq!(receipt["status"], "failed");
        assert_eq!(receipt["executed_ops"], 0);
        assert_eq!(receipt["output_bytes"], 0);
        assert!(result.managed_proof.is_none());
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

    #[tokio::test]
    async fn alpha_without_reference_configuration_returns_typed_backend_unavailable() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let mut request = GeneralComputeRequest {
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest: String::new(),
            runtime_version: general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest: format!("sha256:{}", "a".repeat(64)),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: general_compute_runtime::ArtifactManifest::inline_json(
                "source",
                general_compute_runtime::ArtifactRole::Source,
                b"result = 1",
            ),
            input_artifacts: Vec::new(),
            execution_policy: general_compute_runtime::ExecutionPolicy::default(),
            determinism: general_compute_runtime::DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        let mut task = test_task_with_source("null");
        task.runtime = Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());

        let result = run_task(&task, &config).await.unwrap();

        assert!(!result.success);
        let typed: GeneralComputeResult =
            serde_json::from_slice(result.general_compute_result_json.as_deref().unwrap()).unwrap();
        assert_eq!(typed.status, ResultStatus::BackendUnavailable);
        assert_eq!(typed.error_code.as_deref(), Some("backend_unavailable"));
    }

    #[tokio::test]
    async fn alpha_worker_executes_with_an_operator_provided_cas_store() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let image = format!("sha256:{}", "a".repeat(64));
        let source_bytes = b"result = input['value'] + 1";
        let input_bytes = br#"{"value":4}"#;

        fn cas_manifest(
            artifact_id: &str,
            role: general_compute_runtime::ArtifactRole,
            bytes: &[u8],
        ) -> (general_compute_runtime::ArtifactManifest, Vec<Vec<u8>>) {
            let split = bytes.len() / 2;
            let chunks = vec![bytes[..split].to_vec(), bytes[split..].to_vec()];
            let manifest = general_compute_runtime::ArtifactManifest {
                artifact_id: artifact_id.into(),
                role,
                size_bytes: bytes.len() as u64,
                mime_type: "text/plain".into(),
                sha256: general_compute_runtime::sha256_digest(bytes),
                chunks: chunks
                    .iter()
                    .enumerate()
                    .map(|(index, chunk)| general_compute_runtime::ArtifactChunk {
                        offset: if index == 0 { 0 } else { split as u64 },
                        size_bytes: chunk.len() as u64,
                        sha256: general_compute_runtime::sha256_digest(chunk),
                    })
                    .collect(),
                inline_bytes: None,
            };
            (manifest, chunks)
        }

        let (source, source_chunks) = cas_manifest(
            "worker-cas-source",
            general_compute_runtime::ArtifactRole::Source,
            source_bytes,
        );
        let (input, input_chunks) = cas_manifest(
            "worker-cas-input",
            general_compute_runtime::ArtifactRole::Input,
            input_bytes,
        );
        let mut request = GeneralComputeRequest {
            execution_id: "execution-cas".into(),
            attempt_id: "attempt-cas".into(),
            idempotency_key: "idempotency-cas".into(),
            request_digest: String::new(),
            runtime_version: general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest: image.clone(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: source,
            input_artifacts: vec![input],
            execution_policy: general_compute_runtime::ExecutionPolicy::default(),
            determinism: general_compute_runtime::DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();

        let capabilities = general_compute_runtime::CapabilityMatrix::new(vec![
            general_compute_runtime::BackendRegistration {
                backend_id: request.backend_id.clone(),
                guest_image_digest: image.clone(),
                capabilities: vec!["cpu".into()],
                max_threads: 2,
                network_allowed: false,
                filesystem_read_only: true,
                gpu_allowed: false,
            },
        ]);
        let worker = general_compute_runtime::WorkerCapabilities {
            guest_image_digests: vec![image.clone()],
            capabilities: vec!["cpu".into()],
            max_threads: 2,
            gpu_available: false,
        };
        let python_registry = PythonBackendRegistry::new(vec![PythonBackendRegistration {
            backend_id: request.backend_id.clone(),
            executable: "python".into(),
            runtime_version: "CPython 3.12.9".into(),
            guest_image_digest: image,
            protocol_version: "general-compute-wire-v1".into(),
            max_output_bytes: 1024,
            execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
        }])
        .unwrap();
        let reference = Arc::new(ReferenceBackendExecutor::new(
            capabilities,
            worker,
            python_registry,
        ));
        let cas_root = TempDir::new().unwrap();
        let store = general_compute_runtime::artifact::CasChunkStore::new(cas_root.path()).unwrap();
        for (artifact, chunks) in [
            (&request.source_artifact, source_chunks),
            (&request.input_artifacts[0], input_chunks),
        ] {
            for (manifest_chunk, bytes) in artifact.chunks.iter().zip(chunks) {
                store
                    .put_chunk(&manifest_chunk.sha256, &bytes)
                    .expect("verified chunk should be stored");
            }
        }

        let mut task = test_task_with_source("null");
        task.task_id = "worker-cas-task".into();
        task.runtime = Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let result = run_task_with_cancel_and_reference_and_cas(
            &task,
            &config,
            cancel_rx,
            Some(reference),
            Some(Arc::new(store)),
        )
        .await
        .unwrap();

        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some("5"));
        assert!(result.general_compute_result_json.is_some());
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
            general_compute_manifest_json: None,
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

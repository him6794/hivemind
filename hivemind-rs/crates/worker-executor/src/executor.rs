use anyhow::Result;
use general_compute_runtime::artifact::{ArtifactMaterializer, CasChunkStore};
use general_compute_runtime::cp_python::{PythonBackendRegistration, PythonBackendRegistry};
use general_compute_runtime::execution::{ExecutionError, ReferenceBackendExecutor};
use general_compute_runtime::production::{ProductionBackendConfig, ProductionBackendRegistry};
use general_compute_runtime::sandbox::{BackendExecutionMode, ProductionSandboxLauncher};
use general_compute_runtime::supervisor::{Cancellation, RunResult, RunStatus};
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
    cancel_rx: watch::Receiver<bool>,
    reference_executor: Option<Arc<ReferenceBackendExecutor>>,
    cas_store: Option<Arc<CasChunkStore>>,
) -> Result<super::TaskResult> {
    run_task_with_cancel_and_backends(
        task,
        config,
        cancel_rx,
        reference_executor,
        cas_store,
        production_backends_from_environment()?,
        runtime_capability_matrix_from_environment(),
    )
    .await
}

pub(crate) async fn run_task_with_cancel_and_backends(
    task: &Task,
    config: &HivemindConfig,
    cancel_rx: watch::Receiver<bool>,
    reference_executor: Option<Arc<ReferenceBackendExecutor>>,
    cas_store: Option<Arc<CasChunkStore>>,
    production_backends: Option<Arc<ProductionBackendRegistry>>,
    capability_matrix: Option<Arc<general_compute_runtime::CapabilityMatrix>>,
) -> Result<super::TaskResult> {
    run_task_with_cancel_and_backends_and_trusted_registration(
        task,
        config,
        cancel_rx,
        reference_executor,
        cas_store,
        production_backends,
        capability_matrix,
        None,
    )
    .await
}

pub(crate) async fn run_task_with_cancel_and_backends_and_trusted_registration(
    task: &Task,
    config: &HivemindConfig,
    mut cancel_rx: watch::Receiver<bool>,
    reference_executor: Option<Arc<ReferenceBackendExecutor>>,
    cas_store: Option<Arc<CasChunkStore>>,
    production_backends: Option<Arc<ProductionBackendRegistry>>,
    capability_matrix: Option<Arc<general_compute_runtime::CapabilityMatrix>>,
    trusted_registration: Option<TrustedWorkerCapabilityRegistration>,
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
        let production_backends = production_backends.clone();
        let config = config.clone();
        let cas_store = cas_store.clone();
        let capability_matrix = capability_matrix.clone();
        let trusted_registration = trusted_registration.clone();
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
                production_backends.as_deref(),
                capability_matrix.as_deref(),
                trusted_registration.as_ref(),
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
    production_backends: Option<&ProductionBackendRegistry>,
    capability_matrix: Option<&general_compute_runtime::CapabilityMatrix>,
    trusted_registration: Option<&TrustedWorkerCapabilityRegistration>,
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
    if request.validate().is_err() {
        return typed_task_result(
            task,
            failed_general_compute_result(&request, "request_invalid", None),
        );
    }
    let trusted_gpu_selection = match trusted_gpu_selection(&request, trusted_registration) {
        Ok(selection) => selection,
        Err(_error) => {
            return typed_task_result(
                task,
                failed_general_compute_result(&request, "gpu_unavailable", None),
            );
        }
    };
    let result = match backend_execution_mode(capability_matrix, &request.backend_id) {
        Some(BackendExecutionMode::ProductionSandboxedOci) => {
            let Some(production) = production_backends.and_then(|registry| registry.get(&request.backend_id)) else {
                return typed_task_result(
                    task,
                    failed_general_compute_result(
                        &request,
                        "backend_unavailable",
                        trusted_gpu_selection.clone(),
                    ),
                );
            };
            execute_production_backend_task(
                &request,
                task,
                config,
                production,
                cas_store,
                cancelled,
                cancellation,
                trusted_gpu_selection.clone(),
            )
        }
        Some(BackendExecutionMode::ReferenceDirect) if reference_executor.is_some() => {
            let executor = reference_executor.expect("guarded by is_some");
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
        }
        None if reference_executor.is_some() && production_backends.is_none() => {
            let executor = reference_executor.expect("guarded by is_some");
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
        }
        _ => Err(ExecutionError::BackendUnavailable(
            "backend execution mode is not operator-configured".into(),
        )),
    };
    let typed = match result {
        Ok(mut result) => {
            result.gpu_selection = trusted_gpu_selection;
            result
        }
        Err(error) => {
            tracing::warn!(
                task_id = %task.task_id,
                backend_id = %request.backend_id,
                error = %error,
                "general-compute execution failed"
            );
            failed_general_compute_result_with_error(
                &request,
                &error,
                trusted_gpu_selection,
            )
        }
    };
    typed_task_result(task, typed)
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

fn typed_task_result(
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

fn execute_production_backend_task(
    request: &GeneralComputeRequest,
    task: &Task,
    config: &HivemindConfig,
    backend: &ProductionBackendConfig,
    cas_store: Option<&CasChunkStore>,
    cancelled: &AtomicBool,
    cancellation: &Cancellation,
    trusted_gpu_selection: Option<general_compute_runtime::gpu::GpuSelection>,
) -> Result<GeneralComputeResult, ExecutionError> {
    if backend.backend_id != request.backend_id
        || backend.guest_image_digest != request.guest_image_digest
    {
        return Err(ExecutionError::BackendUnavailable(
            "production backend registration does not match request".into(),
        ));
    }
    if cancelled.load(Ordering::Acquire) {
        cancellation.cancel();
    }
    backend
        .validate_request_mounts(request)
        .map_err(|error| ExecutionError::BackendUnavailable(error.to_string()))?;
    let (bundle_root, artifact_root) = backend
        .materialize_bundle(request, &task.task_id)
        .map_err(|error| ExecutionError::BackendUnavailable(error.to_string()))?;
    let materializer = ArtifactMaterializer::new(&artifact_root)
        .map_err(|error| ExecutionError::BackendUnavailable(error.to_string()))?;
    let mut materialized_bytes = Vec::with_capacity(1 + request.input_artifacts.len());
    for artifact in std::iter::once(&request.source_artifact).chain(request.input_artifacts.iter()) {
        let materialized = match cas_store {
            Some(store) => materializer.materialize_with_cas(artifact, store),
            None => materializer.materialize(artifact),
        }
        .map_err(|error| ExecutionError::BackendUnavailable(error.to_string()))?;
        if materialized.size_bytes != artifact.size_bytes || materialized.sha256 != artifact.sha256 {
            return Err(ExecutionError::BackendUnavailable(
                "materialized production artifact identity mismatch".into(),
            ));
        }
        let bytes = std::fs::read(&materialized.path).map_err(|error| {
            ExecutionError::BackendUnavailable(format!(
                "materialized production artifact cannot be read: {error}"
            ))
        })?;
        if bytes.len() as u64 != artifact.size_bytes
            || general_compute_runtime::sha256_digest(&bytes) != artifact.sha256
        {
            return Err(ExecutionError::BackendUnavailable(
                "materialized production artifact bytes changed after verification".into(),
            ));
        }
        materialized_bytes.push(bytes);
    }
    let Some(source_bytes) = materialized_bytes.first() else {
        return Err(ExecutionError::BackendUnavailable(
            "production source artifact was not materialized".into(),
        ));
    };
    let input_bytes = materialized_bytes
        .iter()
        .skip(1)
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    let input_sha256 = general_compute_runtime::canonical_input_digest(source_bytes, &input_bytes);
    for mount in &backend.policy.mounts {
        if let general_compute_runtime::sandbox::SandboxMount::ReadOnlyArtifact { artifact_id, .. } = mount {
            let path = artifact_root.join(artifact_id);
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                ExecutionError::BackendUnavailable(format!(
                    "declared production artifact mount is unavailable: {error}"
                ))
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(ExecutionError::BackendUnavailable(
                    "declared production artifact mount is not a regular file".into(),
                ));
            }
        }
    }
    let launcher = ProductionSandboxLauncher::with_oci_runner_command(
        backend.runner_executable.clone(),
        backend.runner_prefix_args.clone(),
    )
    .with_runner_state_root(backend.runner_state_root.clone())
    .with_runner_sha256(backend.runner_sha256.clone())
    .with_timeout(std::time::Duration::from_millis(
        request.execution_policy.wall_time_ms,
    ))
    .with_output_limits(backend.max_output_bytes, backend.max_output_bytes.saturating_mul(2));
    let launch = backend.launch();
    let result = launcher
        .run_materialized_bundle(
            &launch,
            &bundle_root,
            &artifact_root,
            &task.task_id,
            cancellation,
        )
        .map_err(|error| ExecutionError::BackendUnavailable(error.to_string()))?;
    production_result(
        request,
        result,
        input_sha256,
        config,
        trusted_gpu_selection,
    )
}

fn production_result(
    request: &GeneralComputeRequest,
    result: RunResult,
    input_sha256: String,
    _config: &HivemindConfig,
    trusted_gpu_selection: Option<general_compute_runtime::gpu::GpuSelection>,
) -> Result<GeneralComputeResult, ExecutionError> {
    let status = match result.status {
        RunStatus::Completed if result.exit_code == Some(0) => ResultStatus::Completed,
        RunStatus::Cancelled => ResultStatus::Cancelled,
        RunStatus::TimedOut => ResultStatus::TimedOut,
        RunStatus::OutputLimitExceeded => ResultStatus::ResourceExhausted,
        RunStatus::Completed | RunStatus::Failed => ResultStatus::Failed,
    };
    let stdout_bytes = result.stdout;
    let _stdout = String::from_utf8(stdout_bytes.clone())
        .map_err(|_| ExecutionError::BackendUnavailable("production stdout is not UTF-8".into()))?;
    let stderr = String::from_utf8(result.stderr)
        .map_err(|_| ExecutionError::BackendUnavailable("production stderr is not UTF-8".into()))?;
    let exit_code = match status {
        ResultStatus::Completed => Some(0),
        ResultStatus::Failed => result.exit_code.or(Some(1)),
        _ => None,
    };
    let envelope: general_compute_runtime::ProductionResultEnvelope = serde_json::from_slice(&stdout_bytes)
        .map_err(|error| {
            ExecutionError::BackendUnavailable(format!(
                "production result decoder rejected stdout: {error}; runner exit_code={:?}; runner stderr: {}",
                result.exit_code,
                stderr.trim()
            ))
        })?;
    let mut typed = envelope
        .into_result_with_input_digest(request, &input_sha256)
        .map_err(|error| {
        ExecutionError::BackendUnavailable(format!("production result validation failed: {}", error.message))
        })?;
    typed.gpu_selection = trusted_gpu_selection;
    if typed.status != status {
        return Err(ExecutionError::BackendUnavailable(
            "production result status disagrees with runner status".into(),
        ));
    }
    typed.stderr = stderr;
    typed.exit_code = exit_code;
    Ok(typed)
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

fn failed_general_compute_result_with_error(
    request: &GeneralComputeRequest,
    error: &ExecutionError,
    gpu_selection: Option<general_compute_runtime::gpu::GpuSelection>,
) -> GeneralComputeResult {
    let mut result = failed_general_compute_result(request, error_code(error), gpu_selection);
    result.stderr = error.to_string();
    result
}

fn failed_general_compute_result(
    request: &GeneralComputeRequest,
    code: &str,
    gpu_selection: Option<general_compute_runtime::gpu::GpuSelection>,
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
        gpu_selection,
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

pub fn production_backends_from_environment(
) -> anyhow::Result<Option<Arc<ProductionBackendRegistry>>> {
    let path = match std::env::var("HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS") {
        Ok(path) if !path.trim().is_empty() => path,
        _ => return Ok(None),
    };
    let bytes = std::fs::read(&path).map_err(|error| {
        anyhow::anyhow!(
            "failed to read operator production backend registry {path:?}: {error}"
        )
    })?;
    let registrations = serde_json::from_slice::<Vec<ProductionBackendConfig>>(&bytes)
        .map_err(|error| anyhow::anyhow!("operator production backend registry is invalid: {error}"))?;
    ProductionBackendRegistry::new(registrations)
        .map(Arc::new)
        .map(Some)
        .map_err(|error| anyhow::anyhow!("operator production backend registry is invalid: {error}"))
}

pub fn runtime_capability_matrix_from_environment(
) -> Option<Arc<general_compute_runtime::CapabilityMatrix>> {
    let registrations = match std::env::var("HIVEMIND_GENERAL_COMPUTE_BACKENDS") {
        Ok(backends) if !backends.trim().is_empty() => {
            serde_json::from_str::<Vec<general_compute_runtime::BackendRegistration>>(&backends)
                .ok()?
        }
        _ => {
            let trusted =
                std::env::var("HIVEMIND_GENERAL_COMPUTE_TRUSTED_REGISTRATION").ok()?;
            serde_json::from_str::<TrustedWorkerCapabilityRegistration>(&trusted)
                .ok()?
                .backends
        }
    };
    Some(Arc::new(general_compute_runtime::CapabilityMatrix::new(
        registrations,
    )))
}

fn backend_execution_mode(
    capability_matrix: Option<&general_compute_runtime::CapabilityMatrix>,
    backend_id: &str,
) -> Option<BackendExecutionMode> {
    capability_matrix.and_then(|matrix| {
        matrix
            .backends
            .iter()
            .find(|backend| backend.backend_id == backend_id)
            .map(|backend| backend.execution_mode)
    })
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

    #[test]
    fn failed_production_result_retains_diagnostic_detail() {
        let request = production_request(
            "oci-success",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "execution-diagnostic-test",
        );
        let error = ExecutionError::BackendUnavailable("runner stderr: permission denied".into());

        let result = failed_general_compute_result_with_error(&request, &error, None);

        assert_eq!(result.error_code.as_deref(), Some("backend_unavailable"));
        assert_eq!(result.stderr, "general-compute backend unavailable: runner stderr: permission denied");
    }

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
    async fn production_capability_without_production_configuration_never_uses_reference_executor() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let image = format!("sha256:{}", "a".repeat(64));
        let mut request = GeneralComputeRequest {
            execution_id: "execution-production-routing".into(),
            attempt_id: "attempt-production-routing".into(),
            idempotency_key: "idempotency-production-routing".into(),
            request_digest: String::new(),
            runtime_version: general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest: image.clone(),
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
        let capabilities = general_compute_runtime::CapabilityMatrix::new(vec![
            general_compute_runtime::BackendRegistration {
                backend_id: request.backend_id.clone(),
                execution_mode: BackendExecutionMode::ProductionSandboxedOci,
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
            execution_mode: BackendExecutionMode::ReferenceDirect,
        }])
        .unwrap();
        let reference = Arc::new(ReferenceBackendExecutor::new(
            capabilities.clone(),
            worker,
            python_registry,
        ));
        let mut task = test_task_with_source("null");
        task.task_id = "production-routing-task".into();
        task.runtime = Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = run_task_with_cancel_and_backends(
            &task,
            &config,
            cancel_rx,
            Some(reference),
            None,
            None,
            Some(Arc::new(capabilities)),
        )
        .await
        .unwrap();

        assert!(!result.success);
        let typed: GeneralComputeResult =
            serde_json::from_slice(result.general_compute_result_json.as_deref().unwrap()).unwrap();
        assert_eq!(typed.status, ResultStatus::BackendUnavailable);
        assert_eq!(typed.error_code.as_deref(), Some("backend_unavailable"));
    }

    #[tokio::test]
    async fn production_worker_missing_pinned_bundle_rootfs_fails_closed() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let image = format!("sha256:{}", "b".repeat(64));
        let registration = production_registration(tmp.path().join("production"), &image);
        let backend_id = registration.backend_id.clone();
        let production = Arc::new(
            ProductionBackendRegistry::new(vec![registration]).unwrap(),
        );
        let request = production_request(&backend_id, &image, "execution-production-rootfs");
        let capabilities = production_capabilities(&backend_id, &image);
        let mut task = test_task_with_source("null");
        task.task_id = "production-rootfs-task".into();
        task.runtime = Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = run_task_with_cancel_and_backends(
            &task,
            &config,
            cancel_rx,
            None,
            None,
            Some(production),
            Some(Arc::new(capabilities)),
        )
        .await
        .unwrap();

        assert!(!result.success);
        let typed: GeneralComputeResult =
            serde_json::from_slice(result.general_compute_result_json.as_deref().unwrap()).unwrap();
        assert_eq!(typed.status, ResultStatus::BackendUnavailable);
        assert_eq!(typed.error_code.as_deref(), Some("backend_unavailable"));
    }

    #[tokio::test]
    async fn production_worker_missing_pinned_runner_fails_closed() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let image = format!("sha256:{}", "c".repeat(64));
        let root = tmp.path().join("production");
        let registration = production_registration(root.clone(), &image);
        std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();
        let backend_id = registration.backend_id.clone();
        let production = Arc::new(
            ProductionBackendRegistry::new(vec![registration]).unwrap(),
        );
        let request = production_request(&backend_id, &image, "execution-production-runner");
        let capabilities = production_capabilities(&backend_id, &image);
        let mut task = test_task_with_source("null");
        task.task_id = "production-runner-task".into();
        task.runtime = Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = run_task_with_cancel_and_backends(
            &task,
            &config,
            cancel_rx,
            None,
            None,
            Some(production),
            Some(Arc::new(capabilities)),
        )
        .await
        .unwrap();

        assert!(!result.success);
        let typed: GeneralComputeResult =
            serde_json::from_slice(result.general_compute_result_json.as_deref().unwrap()).unwrap();
        assert_eq!(typed.status, ResultStatus::BackendUnavailable);
        assert_eq!(typed.error_code.as_deref(), Some("backend_unavailable"));
    }

    #[tokio::test]
    async fn production_worker_routes_materialized_bundle_to_operator_runner() {
        // This is a process-level routing fixture with an operator-owned fake
        // runner. It proves the Worker materializes and validates the bundle,
        // then consumes the versioned result envelope; it is not a claim that
        // this host has real rootless OCI isolation.
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let image = format!("sha256:{}", "d".repeat(64));
        let root = tmp.path().join("production-success");
        let mut registration = production_registration(root.clone(), &image);
        std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();
        std::fs::create_dir_all(&registration.runner_state_root).unwrap();
        std::fs::write(
            &registration.seccomp_profile_path,
            test_seccomp_profile_bytes(),
        )
        .unwrap();
        let request = production_request(
            &registration.backend_id,
            &image,
            "execution-production-success",
        );
        let envelope = general_compute_runtime::ProductionResultEnvelope {
            protocol_version: general_compute_runtime::PRODUCTION_RESULT_PROTOCOL_VERSION.into(),
            status: ResultStatus::Completed,
            exit_code: Some(0),
            error_code: None,
            stdout: "operator runner output".into(),
            stderr: String::new(),
            output_artifacts: Vec::new(),
            usage: Default::default(),
            input_sha256: general_compute_runtime::canonical_input_digest(b"result = 1", &[]),
            output_manifest_root: general_compute_runtime::canonical_artifact_root(&[]),
        };
        let result_path = root.join("runner-result.json");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&result_path, serde_json::to_vec(&envelope).unwrap()).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let runner = root.join("fake-runc.sh");
            let result_literal = result_path.to_string_lossy().replace('\'', "'\\''");
            std::fs::write(
                &runner,
                format!("#!/bin/sh\ncat '{result_literal}'\n"),
            )
            .unwrap();
            let mut permissions = std::fs::metadata(&runner).unwrap().permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(&runner, permissions).unwrap();
            registration.runner_executable = runner;
            registration.runner_prefix_args = Vec::new();
        }
        #[cfg(windows)]
        {
            let script = root.join("fake-runc.cmd");
            std::fs::write(
                &script,
                format!(
                    "@echo off\r\ntype \"{}\"\r\nexit /b 0\r\n",
                    result_path.display()
                ),
            )
            .unwrap();
            registration.runner_executable =
                std::path::PathBuf::from(std::env::var("ComSpec").unwrap());
            registration.runner_prefix_args =
                vec!["/C".into(), script.to_string_lossy().into_owned()];
        }
        registration.runner_sha256 = general_compute_runtime::sha256_digest(
            &std::fs::read(&registration.runner_executable).unwrap(),
        );

        let production = Arc::new(ProductionBackendRegistry::new(vec![registration]).unwrap());
        let capabilities = production_capabilities(&request.backend_id, &image);
        let mut task = test_task_with_source("null");
        task.task_id = "production-success-task".into();
        task.runtime = Some(general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into());
        task.general_compute_manifest_json = Some(serde_json::to_vec(&request).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = run_task_with_cancel_and_backends(
            &task,
            &config,
            cancel_rx,
            None,
            None,
            Some(production),
            Some(Arc::new(capabilities)),
        )
        .await
        .unwrap();

        assert!(
            result.success,
            "production runner fixture failed: {:?} {:?}",
            result.error,
            result.general_compute_result_json
        );
        assert_eq!(result.output.as_deref(), Some("operator runner output"));
        let typed: GeneralComputeResult =
            serde_json::from_slice(result.general_compute_result_json.as_deref().unwrap())
                .unwrap();
        assert_eq!(typed.status, ResultStatus::Completed);
        assert_eq!(
            typed.input_sha256,
            general_compute_runtime::canonical_input_digest(b"result = 1", &[])
        );
    }

    #[test]
    fn production_result_rejects_a_digest_not_bound_to_materialized_source_bytes() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(tmp.path().join("sandbox").to_str().unwrap());
        let image = format!("sha256:{}", "f".repeat(64));
        let request = production_request(
            "python-cpython-312",
            &image,
            "execution-production-result-digest",
        );
        let output = general_compute_runtime::ArtifactManifest::inline_json(
            "stdout",
            general_compute_runtime::ArtifactRole::Output,
            b"ok",
        );
        let envelope = general_compute_runtime::ProductionResultEnvelope {
            protocol_version: general_compute_runtime::PRODUCTION_RESULT_PROTOCOL_VERSION.into(),
            status: ResultStatus::Completed,
            exit_code: Some(0),
            error_code: None,
            stdout: "ok".into(),
            stderr: String::new(),
            output_artifacts: vec![output.clone()],
            usage: Default::default(),
            input_sha256: general_compute_runtime::sha256_digest(&[]),
            output_manifest_root: general_compute_runtime::canonical_artifact_root(&[output]),
        };
        let run = RunResult {
            status: RunStatus::Completed,
            exit_code: Some(0),
            reaped: true,
            stdout: serde_json::to_vec(&envelope).unwrap(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        };
        let expected = general_compute_runtime::canonical_input_digest(b"result = 1", &[]);

        let error = production_result(&request, run, expected, &config, None)
            .expect_err("a runner digest that omits the source bytes must fail closed");
        assert!(error.to_string().contains("input digest"));
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
                execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
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

    fn production_registration(
        root: std::path::PathBuf,
        image: &str,
    ) -> ProductionBackendConfig {
        ProductionBackendConfig {
            backend_id: "python-cpython-312".into(),
            guest_image_digest: image.into(),
            bundle_root: root.join("bundles"),
            artifact_root: root.join("artifacts"),
            runner_executable: root.join("runc"),
            runner_state_root: root.join("runner-state"),
            seccomp_profile_path: root.join("seccomp.json"),
            runner_prefix_args: Vec::new(),
            runner_sha256: format!("sha256:{}", "d".repeat(64)),
            entrypoint: vec!["python".into(), "/runtime/runner.py".into()],
            policy: general_compute_runtime::sandbox::LinuxSandboxPolicy {
                oci_privilege: general_compute_runtime::sandbox::OciPrivilegeMode::Rootless,
                namespaces: vec![
                    general_compute_runtime::sandbox::LinuxNamespace::User,
                    general_compute_runtime::sandbox::LinuxNamespace::Pid,
                    general_compute_runtime::sandbox::LinuxNamespace::Mount,
                    general_compute_runtime::sandbox::LinuxNamespace::Network,
                ],
                cgroup: general_compute_runtime::sandbox::CgroupPolicy::V2,
                seccomp: general_compute_runtime::sandbox::SeccompPolicy::DefaultDeny {
                    profile_sha256: general_compute_runtime::sha256_digest(
                        test_seccomp_profile_bytes(),
                    ),
                },
                privilege_escalation:
                    general_compute_runtime::sandbox::PrivilegeEscalationPolicy::NoNewPrivileges,
                root_filesystem: general_compute_runtime::sandbox::RootFilesystemPolicy::ReadOnly,
                network: general_compute_runtime::sandbox::SandboxNetworkPolicy::DenyAll,
                mounts: vec![general_compute_runtime::sandbox::SandboxMount::ReadOnlyArtifact {
                    artifact_id: "source".into(),
                    destination: "/work/source".into(),
                }],
            },
            max_output_bytes: 1024,
        }
    }

    fn test_seccomp_profile_bytes() -> &'static [u8] {
        br#"{"defaultAction":"SCMP_ACT_ERRNO","syscalls":[{"action":"SCMP_ACT_ALLOW","names":["exit","exit_group"]}]}"#
    }

    fn production_request(
        backend_id: &str,
        image: &str,
        execution_id: &str,
    ) -> GeneralComputeRequest {
        let mut request = GeneralComputeRequest {
            execution_id: execution_id.into(),
            attempt_id: "attempt-production-worker".into(),
            idempotency_key: "idempotency-production-worker".into(),
            request_digest: String::new(),
            runtime_version: general_compute_runtime::GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest: image.into(),
            backend_id: backend_id.into(),
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
        request
    }

    fn production_capabilities(
        backend_id: &str,
        image: &str,
    ) -> general_compute_runtime::CapabilityMatrix {
        general_compute_runtime::CapabilityMatrix::new(vec![
            general_compute_runtime::BackendRegistration {
                backend_id: backend_id.into(),
                execution_mode: BackendExecutionMode::ProductionSandboxedOci,
                guest_image_digest: image.into(),
                capabilities: vec!["cpu".into()],
                max_threads: 2,
                network_allowed: false,
                filesystem_read_only: true,
                gpu_allowed: false,
            },
        ])
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

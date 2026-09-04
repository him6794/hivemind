//! Fixed operator-owned guest runner for `managed-function-gpu-v1`.
//!
//! The OCI image supplies this binary as its pinned entrypoint.  It accepts no
//! task-selected executable, library, device, or command.  The only workload
//! inputs are the four fixed files mounted by the Worker:
//!
//! * `/work/source`
//! * `/work/input`
//! * `/work/manifest`
//! * `/work/selection`

use general_compute_runtime::managed_gpu::{
    ManagedGpuCapability, ManagedGpuEvidence, ManagedGpuRequest, ManagedGpuResult,
    ManagedGpuStatus, ManagedGpuUsage,
};
use general_compute_runtime::sha256_digest;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

#[cfg(all(feature = "cuda", target_os = "linux"))]
use managed_function_runtime::{
    render_output_bounded, CudaGpuBackend, ExecutionLimits, ManagedGpuExecutor, GPU_OPERATION_COST,
};
#[cfg(all(feature = "cuda", target_os = "linux"))]
use std::time::Instant;

const SOURCE_PATH: &str = "/work/source";
const INPUT_PATH: &str = "/work/input";
const MANIFEST_PATH: &str = "/work/manifest";
const SELECTION_PATH: &str = "/work/selection";

#[derive(Debug, Clone)]
struct RunnerPaths {
    source: PathBuf,
    input: PathBuf,
    manifest: PathBuf,
    selection: PathBuf,
}

impl RunnerPaths {
    fn fixed() -> Self {
        Self {
            source: PathBuf::from(SOURCE_PATH),
            input: PathBuf::from(INPUT_PATH),
            manifest: PathBuf::from(MANIFEST_PATH),
            selection: PathBuf::from(SELECTION_PATH),
        }
    }
}

#[derive(Debug)]
struct RunnerOutcome {
    result: ManagedGpuResult,
    process_exit_code: i32,
}

fn main() {
    match run() {
        Ok(exit_code) => process::exit(exit_code),
        Err(error) => {
            // Diagnostics stay on stderr. Do not print source, input, or
            // serialized result data because the runner boundary is also an
            // audit/logging boundary.
            eprintln!("managed GPU runner failed: {error}");
            process::exit(1);
        }
    }
}

fn run() -> Result<i32, String> {
    let outcome = execute(&RunnerPaths::fixed())?;
    emit_result(&outcome.result)?;
    Ok(outcome.process_exit_code)
}

fn execute(paths: &RunnerPaths) -> Result<RunnerOutcome, String> {
    let manifest_bytes = read_file(&paths.manifest)?;
    let request = serde_json::from_slice::<ManagedGpuRequest>(&manifest_bytes)
        .map_err(|error| format!("managed GPU manifest is malformed: {error}"))?;
    request
        .validate()
        .map_err(|error| format!("managed GPU request is invalid: {error:?}"))?;

    let source = read_utf8(&paths.source)?;
    if source != request.source {
        return Err("managed GPU source mount does not match the request manifest".into());
    }
    let input = read_utf8(&paths.input)?;
    if input != request.input_json {
        return Err("managed GPU input mount does not match the request manifest".into());
    }

    let selection_bytes = read_file(&paths.selection)?;
    let selected_gpu = serde_json::from_slice::<ManagedGpuCapability>(&selection_bytes)
        .map_err(|error| format!("managed GPU selection is malformed: {error}"))?;
    selected_gpu
        .validate()
        .map_err(|error| format!("managed GPU selection is invalid: {error}"))?;
    validate_selection(&request, &selected_gpu)?;

    #[cfg(not(all(feature = "cuda", target_os = "linux")))]
    {
        let result = managed_gpu_result(
            &request,
            selected_gpu,
            ManagedGpuStatus::BackendUnavailable,
            Some("backend_unavailable"),
            String::new(),
            usage(&request, 0, 0, 0),
        );
        Ok(RunnerOutcome {
            result,
            process_exit_code: 0,
        })
    }

    #[cfg(all(feature = "cuda", target_os = "linux"))]
    {
        let mut backend = match CudaGpuBackend::new(
            selected_gpu.device_id.clone(),
            selected_gpu.cuda_device_ordinal,
            selected_gpu.cuda_uuid.clone(),
        ) {
            Ok(backend) => backend,
            Err(_) => {
                let result = managed_gpu_result(
                    &request,
                    selected_gpu,
                    ManagedGpuStatus::BackendUnavailable,
                    Some("backend_unavailable"),
                    String::new(),
                    usage(&request, 0, 0, 0),
                );
                return Ok(RunnerOutcome {
                    result,
                    process_exit_code: 0,
                });
            }
        };

        let limits = ExecutionLimits {
            max_ops: request
                .limits
                .max_operations
                .saturating_mul(GPU_OPERATION_COST),
            max_usage_units: Some(
                request
                    .limits
                    .max_operations
                    .saturating_mul(GPU_OPERATION_COST),
            ),
            max_output_bytes: request.limits.max_output_bytes,
            max_loop_iterations: request.limits.max_operations,
            max_value_bytes: request.limits.max_value_bytes,
            max_collection_items: request.limits.max_collection_items,
            max_value_depth: request.limits.max_value_depth,
            max_value_materialization_bytes: request.limits.max_value_materialization_bytes,
            ..ExecutionLimits::default()
        };
        let started = Instant::now();
        let execution = ManagedGpuExecutor::new(&mut backend).execute_json_input_with_cancel(
            &request.source,
            limits,
            &request.input_json,
            None,
        );
        let elapsed_ms = started
            .elapsed()
            .as_millis()
            .min(request.limits.max_wall_time_ms as u128) as u64;

        let (result, process_exit_code) = match execution {
            Ok(execution) if elapsed_ms >= request.limits.max_wall_time_ms => (
                managed_gpu_result(
                    &request,
                    selected_gpu,
                    ManagedGpuStatus::TimedOut,
                    Some("wall_time_exceeded"),
                    String::new(),
                    usage(
                        &request,
                        execution.receipt.gpu_operations,
                        request.limits.max_wall_time_ms,
                        0,
                    ),
                ),
                0,
            ),
            Ok(execution) => {
                let output = if execution.output.is_empty() {
                    render_output_bounded(&execution.value, request.limits.max_output_bytes)
                        .map_err(|error| error.to_string())?
                } else {
                    execution.output
                };
                (
                    managed_gpu_result(
                        &request,
                        selected_gpu,
                        ManagedGpuStatus::Completed,
                        None,
                        output.clone(),
                        usage(
                            &request,
                            execution.receipt.gpu_operations,
                            elapsed_ms,
                            output.len() as u64,
                        ),
                    ),
                    0,
                )
            }
            Err(error) => {
                let (status, error_code, process_exit_code) = runtime_error_status(error.code());
                (
                    managed_gpu_result(
                        &request,
                        selected_gpu,
                        status,
                        Some(error_code),
                        String::new(),
                        usage(&request, 0, elapsed_ms, 0),
                    ),
                    process_exit_code,
                )
            }
        };
        return Ok(RunnerOutcome {
            result,
            process_exit_code,
        });
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("cannot read fixed input {path:?}: {error}"))
}

fn read_utf8(path: &Path) -> Result<String, String> {
    String::from_utf8(read_file(path)?).map_err(|_| format!("fixed input {path:?} is not UTF-8"))
}

fn validate_selection(
    request: &ManagedGpuRequest,
    selected: &ManagedGpuCapability,
) -> Result<(), String> {
    let requirement = &request.gpu_requirement;
    if selected.vendor != requirement.vendor
        || selected.compute_capability != requirement.compute_capability
        || selected.runtime != requirement.runtime
        || selected.runtime_version != requirement.runtime_version
        || selected.driver_abi != requirement.driver_abi
        || selected.vram_bytes < requirement.min_vram_bytes
        || selected.max_streams < requirement.min_streams
        || selected.image_digest != request.guest_image_digest
        || selected.image_digest != requirement.image_digest
    {
        return Err("managed GPU selection does not satisfy the request requirement".into());
    }
    Ok(())
}

#[cfg(all(feature = "cuda", target_os = "linux"))]
fn runtime_error_status(code: &str) -> (ManagedGpuStatus, &'static str, i32) {
    match code {
        "cancelled" => (ManagedGpuStatus::Cancelled, "cancelled", 0),
        "gpu_backend_unavailable" => (
            ManagedGpuStatus::BackendUnavailable,
            "backend_unavailable",
            0,
        ),
        "op_limit_exceeded"
        | "budget_exhausted"
        | "output_limit_exceeded"
        | "value_limit_exceeded" => (ManagedGpuStatus::ResourceExhausted, "resource_exhausted", 0),
        _ => (ManagedGpuStatus::Failed, "execution_failed", 1),
    }
}

fn usage(
    request: &ManagedGpuRequest,
    executed_operations: u64,
    wall_time_ms: u64,
    output_bytes: u64,
) -> ManagedGpuUsage {
    ManagedGpuUsage {
        source_bytes: request.source.len() as u64,
        input_bytes: request.input_json.len() as u64,
        output_bytes,
        executed_operations,
        operation_cost_units: executed_operations
            .saturating_mul(general_compute_runtime::managed_gpu::MANAGED_GPU_OPERATION_COST_UNITS),
        wall_time_ms,
        gpu_time_ms: wall_time_ms.min(request.limits.max_gpu_time_ms),
        gpu_memory_bytes: 0,
    }
}

fn managed_gpu_result(
    request: &ManagedGpuRequest,
    selected_gpu: ManagedGpuCapability,
    status: ManagedGpuStatus,
    error_code: Option<&str>,
    output: String,
    usage: ManagedGpuUsage,
) -> ManagedGpuResult {
    ManagedGpuResult {
        protocol_version: general_compute_runtime::managed_gpu::MANAGED_GPU_RESULT_PROTOCOL_VERSION
            .into(),
        execution_id: request.execution_id.clone(),
        attempt_id: request.attempt_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        runtime_version: request.runtime_version.clone(),
        semantics_manifest_sha256: request.semantics_manifest_sha256.clone(),
        operation_registry_version: request.operation_registry_version.clone(),
        backend_id: request.backend_id.clone(),
        guest_image_digest: request.guest_image_digest.clone(),
        source_sha256: request.source_sha256(),
        input_sha256: request.input_sha256(),
        reservation_cpt: request.reservation_cpt,
        status,
        exit_code: match status {
            ManagedGpuStatus::Completed => Some(0),
            ManagedGpuStatus::Failed => Some(1),
            ManagedGpuStatus::Cancelled
            | ManagedGpuStatus::TimedOut
            | ManagedGpuStatus::ResourceExhausted
            | ManagedGpuStatus::BackendUnavailable => None,
        },
        error_code: error_code.map(str::to_owned),
        output_sha256: sha256_digest(output.as_bytes()),
        output,
        selected_gpu,
        usage,
        evidence: ManagedGpuEvidence::default(),
    }
}

fn emit_result(result: &ManagedGpuResult) -> Result<(), String> {
    let bytes = serde_json::to_vec(result)
        .map_err(|error| format!("managed GPU result serialization failed: {error}"))?;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle
        .write_all(&bytes)
        .and_then(|_| handle.write_all(b"\n"))
        .and_then(|_| handle.flush())
        .map_err(|error| format!("managed GPU result output failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use general_compute_runtime::managed_gpu::{
        ManagedGpuLimits, ManagedGpuProofPolicy, ManagedGpuRequirement,
        MANAGED_GPU_BILLING_VERSION, MANAGED_GPU_COST_MODEL_VERSION,
        MANAGED_GPU_OPERATION_REGISTRY_VERSION, MANAGED_GPU_REQUEST_PROTOCOL_VERSION,
        MANAGED_GPU_RUNTIME_VERSION, MANAGED_GPU_SEMANTICS_MANIFEST_SHA256,
        MANAGED_GPU_SETTLEMENT_BASIS,
    };
    use std::fs;
    use tempfile::TempDir;

    const IMAGE_DIGEST: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn fixture() -> (ManagedGpuRequest, ManagedGpuCapability) {
        let requirement = ManagedGpuRequirement::new(
            "8.9",
            "12.4",
            "535.129",
            1024 * 1024 * 1024,
            1,
            IMAGE_DIGEST,
        )
        .expect("valid GPU requirement");
        let selected = ManagedGpuCapability::new(
            "gpu-0",
            "8.9",
            "12.4",
            "535.129",
            8 * 1024 * 1024 * 1024,
            4,
            IMAGE_DIGEST,
            0,
            "GPU-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        )
        .expect("valid GPU capability");
        let mut request = ManagedGpuRequest {
            protocol_version: MANAGED_GPU_REQUEST_PROTOCOL_VERSION.into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest: String::new(),
            runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
            semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
            operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
            backend_id: "gpu-backend".into(),
            guest_image_digest: IMAGE_DIGEST.into(),
            source: "return 1".into(),
            input_json: "null".into(),
            gpu_requirement: requirement,
            limits: ManagedGpuLimits::default(),
            reservation_cpt: 100,
            billing_version: MANAGED_GPU_BILLING_VERSION.into(),
            cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
            settlement_basis: MANAGED_GPU_SETTLEMENT_BASIS.into(),
            proof_policy: ManagedGpuProofPolicy::None,
        };
        request.request_digest = request.canonical_request_digest();
        (request, selected)
    }

    fn write_fixture(
        temp: &TempDir,
        request: &ManagedGpuRequest,
        selected: &ManagedGpuCapability,
        source: &str,
        input: &str,
    ) -> RunnerPaths {
        let source_path = temp.path().join("source");
        let input_path = temp.path().join("input");
        let manifest_path = temp.path().join("manifest");
        let selection_path = temp.path().join("selection");
        fs::write(&source_path, source).expect("source fixture writes");
        fs::write(&input_path, input).expect("input fixture writes");
        fs::write(
            &manifest_path,
            serde_json::to_vec(request).expect("request fixture serializes"),
        )
        .expect("manifest fixture writes");
        fs::write(
            &selection_path,
            serde_json::to_vec(selected).expect("selection fixture serializes"),
        )
        .expect("selection fixture writes");
        RunnerPaths {
            source: source_path,
            input: input_path,
            manifest: manifest_path,
            selection: selection_path,
        }
    }

    #[test]
    fn fixed_paths_are_operator_owned_and_not_task_selectable() {
        let paths = RunnerPaths::fixed();
        assert_eq!(paths.source, PathBuf::from(SOURCE_PATH));
        assert_eq!(paths.input, PathBuf::from(INPUT_PATH));
        assert_eq!(paths.manifest, PathBuf::from(MANIFEST_PATH));
        assert_eq!(paths.selection, PathBuf::from(SELECTION_PATH));
    }

    #[test]
    fn runner_rejects_source_mount_drift() {
        let temp = TempDir::new().expect("temporary runner root");
        let (request, selected) = fixture();
        let paths = write_fixture(&temp, &request, &selected, "return 2", "null");

        let error = execute(&paths).expect_err("source drift must fail closed");

        assert!(error.contains("source mount does not match"));
    }

    #[test]
    fn runner_rejects_input_mount_drift() {
        let temp = TempDir::new().expect("temporary runner root");
        let (request, selected) = fixture();
        let paths = write_fixture(&temp, &request, &selected, &request.source, "{}");

        let error = execute(&paths).expect_err("input drift must fail closed");

        assert!(error.contains("input mount does not match"));
    }

    #[test]
    fn runner_rejects_selection_requirement_mismatch() {
        let temp = TempDir::new().expect("temporary runner root");
        let (request, mut selected) = fixture();
        selected.compute_capability = "7.5".into();
        let paths = write_fixture(
            &temp,
            &request,
            &selected,
            &request.source,
            &request.input_json,
        );

        let error = execute(&paths).expect_err("GPU selection drift must fail closed");

        assert!(error.contains("selection does not satisfy"));
    }

    #[cfg(all(feature = "cuda", target_os = "linux"))]
    #[test]
    fn runtime_errors_keep_backend_unavailable_terminal_status() {
        assert_eq!(
            runtime_error_status("gpu_backend_unavailable"),
            (
                ManagedGpuStatus::BackendUnavailable,
                "backend_unavailable",
                0
            )
        );
        assert_eq!(
            runtime_error_status("cancelled"),
            (ManagedGpuStatus::Cancelled, "cancelled", 0)
        );
        assert_eq!(
            runtime_error_status("output_limit_exceeded"),
            (ManagedGpuStatus::ResourceExhausted, "resource_exhausted", 0)
        );
        assert_eq!(
            runtime_error_status("gpu_execution_error"),
            (ManagedGpuStatus::Failed, "execution_failed", 1)
        );
    }
}

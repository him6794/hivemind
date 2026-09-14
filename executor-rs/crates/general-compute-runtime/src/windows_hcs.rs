//! Native Windows Host Compute System integration.
//!
//! This module is deliberately separate from the Linux OCI launcher. It never
//! invokes Docker, WSL, a shell, or a direct host process. Windows builds use
//! ComputeCore.dll; non-Windows builds fail closed with `UnsupportedPlatform`.

use crate::production::WindowsHcsContainerSpec;
#[cfg(windows)]
use crate::production::verify_windows_hcs_assets;
use crate::sandbox::WINDOWS_HCS_PROCESSOR_MAXIMUM;

/// Stable ownership marker for operator-created HCS systems.
///
/// Worker reconciliation must not infer ownership from a system name alone.
pub const HIVEMIND_HCS_OWNER: &str = "hivemind";
pub const HIVEMIND_HCS_SYSTEM_ID_PREFIX: &str = "hivemind-";

#[cfg(any(windows, test))]
use crate::supervisor::RunStatus;
use crate::supervisor::{Cancellation, RunResult};
#[cfg(any(windows, test))]
use std::io::Read;
use std::time::Duration;

/// Lifecycle facts emitted by the native HCS launcher.
///
/// The event stream contains no task input, source bytes, credentials, or
/// result payload. It is suitable for an operator-owned recovery journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HcsLifecycleEvent {
    Created { system_id: String },
    Started,
    Waiting,
    GuestExited { exit_code: Option<i32> },
    Cancelled,
    TimedOut,
    StartFailed { error: String },
    WaitFailed { error: String },
    ShutdownStarted,
    ShutdownCompleted { status: i32, exit_type: String },
    ShutdownFailed { error: String },
    TerminateStarted,
    Terminated { status: i32, exit_type: String },
    TerminateFailed { error: String },
    ResultRead { sha256: String, size: usize },
    ResultReadFailed { error: String },
    Closed,
}

/// Receives authoritative lifecycle facts from [`WindowsHcsLauncher`].
///
/// Implementations may reject an event. A rejection is fatal to the
/// execution; the launcher still attempts HCS cleanup before returning.
pub trait HcsLifecycleObserver {
    fn on_event(&mut self, event: HcsLifecycleEvent) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcsSystemSummary {
    pub id: String,
    pub owner: Option<String>,
    pub state: Option<String>,
}

/// Enumerate existing HCS systems for crash/restart reconciliation.
///
/// Non-Windows builds deliberately return `UnsupportedPlatform`; they must not
/// emulate HCS with Docker, WSL, or a direct host process.
pub fn enumerate_systems(timeout: Duration) -> Result<Vec<HcsSystemSummary>, WindowsHcsError> {
    #[cfg(not(windows))]
    {
        let _ = timeout;
        Err(WindowsHcsError::UnsupportedPlatform)
    }
    #[cfg(windows)]
    {
        hcs::enumerate_systems(timeout)
    }
}

/// Terminate one existing HCS system by its exact operator-recorded identity
/// and retain the authoritative compute-system exit status.
///
/// This is used only for fail-closed recovery. It never opens an arbitrary host
/// process or substitutes a non-HCS runtime.
pub fn terminate_system_with_status(
    system_id: &str,
    timeout: Duration,
) -> Result<HcsSystemExitStatus, WindowsHcsError> {
    if system_id.trim().is_empty() {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS system identity must not be empty".into(),
        ));
    }
    #[cfg(not(windows))]
    {
        let _ = timeout;
        Err(WindowsHcsError::UnsupportedPlatform)
    }
    #[cfg(windows)]
    {
        hcs::terminate_system(system_id, timeout)
    }
}

/// Terminate one existing HCS system by its exact operator-recorded identity.
///
/// The status-bearing variant is used by recovery journaling; this compatibility
/// wrapper keeps callers that only need success/failure from discarding that API.
pub fn terminate_system(system_id: &str, timeout: Duration) -> Result<(), WindowsHcsError> {
    terminate_system_with_status(system_id, timeout).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsHcsError {
    UnsupportedPlatform,
    InvalidSpec(String),
    ProviderUnavailable(String),
    OperationFailed(String),
    CleanupFailed(String),
    ResultUnavailable(String),
    ResultTooLarge { limit: usize, actual: usize },
    Cancelled,
    TimedOut,
}

impl std::fmt::Display for WindowsHcsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Windows HCS execution unavailable: {self:?}")
    }
}

impl std::error::Error for WindowsHcsError {}

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsHcsLauncher {
    timeout: Duration,
}

impl WindowsHcsLauncher {
    #[must_use]
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn run(
        &self,
        spec: &WindowsHcsContainerSpec,
        cancellation: &Cancellation,
    ) -> Result<RunResult, WindowsHcsError> {
        self.run_with_observer(spec, cancellation, None)
    }

    pub fn run_with_observer(
        &self,
        spec: &WindowsHcsContainerSpec,
        cancellation: &Cancellation,
        observer: Option<&mut dyn HcsLifecycleObserver>,
    ) -> Result<RunResult, WindowsHcsError> {
        validate_spec(spec)?;
        if cancellation.is_cancelled() {
            return Err(WindowsHcsError::Cancelled);
        }
        self.run_platform(spec, cancellation, observer)
    }

    #[cfg(not(windows))]
    #[expect(clippy::unused_self)]
    fn run_platform(
        &self,
        _spec: &WindowsHcsContainerSpec,
        _cancellation: &Cancellation,
        _observer: Option<&mut dyn HcsLifecycleObserver>,
    ) -> Result<RunResult, WindowsHcsError> {
        Err(WindowsHcsError::UnsupportedPlatform)
    }

    #[cfg(windows)]
    fn run_platform(
        &self,
        spec: &WindowsHcsContainerSpec,
        cancellation: &Cancellation,
        observer: Option<&mut dyn HcsLifecycleObserver>,
    ) -> Result<RunResult, WindowsHcsError> {
        hcs::run(spec, self.timeout, cancellation, observer)
    }
}

fn validate_spec(spec: &WindowsHcsContainerSpec) -> Result<(), WindowsHcsError> {
    if spec.container_id.trim().is_empty()
        || spec.container_id.len() > 128
        || !spec
            .container_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(WindowsHcsError::InvalidSpec("invalid container id".into()));
    }
    if spec.backend_id.trim().is_empty()
        || spec.image_root.as_os_str().is_empty()
        || spec.runner_executable.as_os_str().is_empty()
        || spec.runner_sha256.trim().is_empty()
        || spec.image_material_digest.trim().is_empty()
        || spec.guest_image_digest.trim().is_empty()
        || spec.policy_digest.trim().is_empty()
        || spec.runner_container_path.trim().is_empty()
        || spec.entrypoint.is_empty()
        || spec.entrypoint.iter().any(|part| part.trim().is_empty())
    {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS asset identities and non-empty entrypoint are required".into(),
        ));
    }
    if !spec.runner_container_path.starts_with("C:\\")
        || spec
            .runner_container_path
            .split(['\\', '/'])
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS runner container path is invalid".into(),
        ));
    }
    let runner_name = spec
        .runner_container_path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default();
    if !spec
        .entrypoint
        .first()
        .is_some_and(|entrypoint| entrypoint.eq_ignore_ascii_case(runner_name))
    {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS entrypoint is not bound to the pinned runner".into(),
        ));
    }
    if !is_sha256_digest(&spec.runner_sha256)
        || !is_sha256_digest(&spec.guest_image_digest)
        || spec.image_material_digest != spec.guest_image_digest
        || !is_sha256_digest(&spec.policy_digest)
    {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS asset or policy digest is invalid".into(),
        ));
    }
    #[cfg(windows)]
    verify_windows_hcs_assets(
        &spec.image_root,
        &spec.runner_executable,
        &spec.guest_image_digest,
        &spec.runner_sha256,
    )
    .map_err(|error| WindowsHcsError::InvalidSpec(error.to_string()))?;

    if spec.storage_path.as_os_str().is_empty()
        || spec.result_path.as_os_str().is_empty()
        || spec.result_container_path.is_empty()
        || spec.max_output_bytes == 0
    {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS storage, result transport, and output limit are required".into(),
        ));
    }
    if spec.resource_limits.memory_size_mb == 0
        || spec.resource_limits.processor_maximum == 0
        || spec.resource_limits.processor_maximum > WINDOWS_HCS_PROCESSOR_MAXIMUM
    {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS memory and processor limits are invalid".into(),
        ));
    }
    let storage_path = normalize_windows_path(&spec.storage_path.to_string_lossy());
    let storage_mount = spec.mounts.iter().any(|mount| {
        !mount.read_only
            && normalize_windows_path(&mount.host_path.to_string_lossy()) == storage_path
    });
    if !storage_mount {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS storage path must use an explicit writable mount".into(),
        ));
    }
    let result_parent = windows_path_parent(&spec.result_path);
    let result_container_path = normalize_windows_path(&spec.result_container_path);
    let result_mount = spec.mounts.iter().any(|mount| {
        let mount_host_path = normalize_windows_path(&mount.host_path.to_string_lossy());
        let mount_container_path = normalize_windows_path(&mount.container_path);
        !mount.read_only
            && result_parent.as_deref() == Some(mount_host_path.as_str())
            && result_container_path.starts_with(&format!(
                "{}\\",
                mount_container_path.trim_end_matches('\\')
            ))
    });
    if !result_mount {
        return Err(WindowsHcsError::InvalidSpec(
            "result transport must use an explicit writable mount".into(),
        ));
    }
    if !spec.network_isolated || !spec.root_read_only {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS spec must deny networking and use a read-only root".into(),
        ));
    }
    if spec.mounts.is_empty()
        || spec
            .mounts
            .iter()
            .any(|mount| mount.host_path.as_os_str().is_empty() || mount.container_path.is_empty())
    {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS mounts must have operator paths and destinations".into(),
        ));
    }
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalize_windows_path(value: &str) -> String {
    value.replace('/', "\\")
}

fn windows_path_parent(path: &std::path::Path) -> Option<String> {
    let normalized = normalize_windows_path(&path.to_string_lossy());
    normalized
        .rsplit_once('\\')
        .map(|(parent, _)| parent.to_owned())
}

#[cfg(any(windows, test))]
fn configuration_json(spec: &WindowsHcsContainerSpec) -> Result<String, WindowsHcsError> {
    // HCS's Container schema has no arbitrary annotation field. Identity is
    // retained in the operator journal/spec and passed through the documented
    // process environment instead of adding an unsupported schema property.
    serde_json::to_string(&serde_json::json!({
        "Owner": HIVEMIND_HCS_OWNER,
        "SchemaVersion": {"Major": 2, "Minor": 1},
        "ShouldTerminateOnLastHandleClosed": true,
        "Container": {
            "Storage": {
                "Layers": [{"Path": spec.image_root}],
                "Path": spec.storage_path,
            },
            "MappedDirectories": spec.mounts.iter().map(|mount| serde_json::json!({
                "HostPath": mount.host_path,
                "HostPathType": "Directory",
                "ContainerPath": mount.container_path,
                "ReadOnly": mount.read_only,
            })).collect::<Vec<_>>(),
            "Memory": {
                "SizeInMB": spec.resource_limits.memory_size_mb,
            },
            "Processor": {
                "Maximum": spec.resource_limits.processor_maximum,
            },
            "Networking": {
                "NetworkAdapters": [],
            },
        }
    }))
    .map_err(|error| WindowsHcsError::InvalidSpec(error.to_string()))
}

#[cfg(any(windows, test))]
fn read_result_file(
    path: &std::path::Path,
    max_output_bytes: usize,
) -> Result<Vec<u8>, WindowsHcsError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| WindowsHcsError::ResultUnavailable(error.to_string()))?;
    if !metadata.is_file() {
        return Err(WindowsHcsError::ResultUnavailable(
            "result path is not a regular file".into(),
        ));
    }
    let actual = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if actual > max_output_bytes {
        return Err(WindowsHcsError::ResultTooLarge {
            limit: max_output_bytes,
            actual,
        });
    }
    let mut file = std::fs::File::open(path)
        .map_err(|error| WindowsHcsError::ResultUnavailable(error.to_string()))?;
    let mut bytes = Vec::with_capacity(actual.min(max_output_bytes));
    let read_limit = u64::try_from(max_output_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    file.by_ref()
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| WindowsHcsError::ResultUnavailable(error.to_string()))?;
    if bytes.len() > max_output_bytes {
        return Err(WindowsHcsError::ResultTooLarge {
            limit: max_output_bytes,
            actual: bytes.len(),
        });
    }
    Ok(bytes)
}

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HcsWaitOutcome {
    Exited { exit_code: Option<i32> },
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcsSystemExitStatus {
    pub status: i32,
    pub exit_type: String,
}

#[cfg(any(windows, test))]
trait HcsLifecycleProvider {
    fn start(&mut self, timeout: Duration) -> Result<(), WindowsHcsError>;
    fn wait_for_exit(&mut self, timeout: Duration) -> Result<HcsWaitOutcome, WindowsHcsError>;
    fn terminate(&mut self, timeout: Duration) -> Result<HcsSystemExitStatus, WindowsHcsError>;
    fn shutdown(&mut self, timeout: Duration) -> Result<HcsSystemExitStatus, WindowsHcsError>;
}

#[cfg(any(windows, test))]
fn validate_system_exit_status(status: &HcsSystemExitStatus) -> Result<(), WindowsHcsError> {
    if status.status != 0 {
        return Err(WindowsHcsError::OperationFailed(format!(
            "HCS system exit returned HRESULT 0x{:08x}",
            status.status
        )));
    }
    if !matches!(status.exit_type.as_str(), "GracefulExit" | "ForcedExit") {
        return Err(WindowsHcsError::OperationFailed(format!(
            "HCS system exit type is not authoritative: {}",
            status.exit_type
        )));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn terminal_result(status: RunStatus) -> RunResult {
    RunResult {
        status,
        exit_code: None,
        reaped: true,
        stdout: Vec::new(),
        stderr: Vec::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

#[cfg(any(windows, test))]
fn observe_event(
    observer: &mut Option<&mut dyn HcsLifecycleObserver>,
    event: HcsLifecycleEvent,
) -> Result<(), WindowsHcsError> {
    let Some(observer) = observer.as_deref_mut() else {
        return Ok(());
    };
    observer.on_event(event).map_err(|error| {
        WindowsHcsError::OperationFailed(format!("HCS lifecycle journal rejected event: {error}"))
    })
}

#[cfg(any(windows, test))]
fn cleanup_after_interrupt<P: HcsLifecycleProvider>(
    provider: &mut P,
    timeout: Duration,
    status: RunStatus,
    observer: &mut Option<&mut dyn HcsLifecycleObserver>,
) -> Result<RunResult, WindowsHcsError> {
    let observer_error = observe_event(observer, HcsLifecycleEvent::TerminateStarted).err();
    let cleanup_result = provider.terminate(timeout);
    match cleanup_result {
        Ok(system_status) => {
            let termination_event_error = observe_event(
                observer,
                HcsLifecycleEvent::Terminated {
                    status: system_status.status,
                    exit_type: system_status.exit_type.clone(),
                },
            )
            .err();
            if let Err(cleanup_error) = validate_system_exit_status(&system_status) {
                return Err(WindowsHcsError::CleanupFailed(format!(
                    "HCS {status:?} cleanup was not authoritative: {cleanup_error}"
                )));
            }
            if let Some(error) = observer_error.or(termination_event_error) {
                return Err(WindowsHcsError::CleanupFailed(format!(
                    "HCS {status:?} cleanup was not journaled: {error}"
                )));
            }
            Ok(terminal_result(status))
        }
        Err(cleanup_error) => {
            let _ = observe_event(
                observer,
                HcsLifecycleEvent::TerminateFailed {
                    error: cleanup_error.to_string(),
                },
            );
            Err(WindowsHcsError::CleanupFailed(format!(
                "HCS {status:?} cleanup failed: {cleanup_error}"
            )))
        }
    }
}

#[cfg(any(windows, test))]
fn cleanup_after_lifecycle_error<P: HcsLifecycleProvider>(
    provider: &mut P,
    timeout: Duration,
    lifecycle_error: WindowsHcsError,
    observer: &mut Option<&mut dyn HcsLifecycleObserver>,
) -> Result<RunResult, WindowsHcsError> {
    let observer_error = observe_event(observer, HcsLifecycleEvent::TerminateStarted).err();
    let cleanup_result = provider.terminate(timeout);
    match cleanup_result {
        Ok(system_status) => {
            let termination_event_error = observe_event(
                observer,
                HcsLifecycleEvent::Terminated {
                    status: system_status.status,
                    exit_type: system_status.exit_type.clone(),
                },
            )
            .err();
            if let Err(cleanup_error) = validate_system_exit_status(&system_status) {
                return Err(WindowsHcsError::CleanupFailed(format!(
                    "HCS lifecycle failed: {lifecycle_error}; cleanup was not authoritative: {cleanup_error}"
                )));
            }
            if let Some(error) = observer_error.or(termination_event_error) {
                return Err(WindowsHcsError::CleanupFailed(format!(
                    "HCS lifecycle failed: {lifecycle_error}; cleanup was not journaled: {error}"
                )));
            }
            Err(lifecycle_error)
        }
        Err(cleanup_error) => {
            let _ = observe_event(
                observer,
                HcsLifecycleEvent::TerminateFailed {
                    error: cleanup_error.to_string(),
                },
            );
            Err(WindowsHcsError::CleanupFailed(format!(
                "HCS lifecycle failed: {lifecycle_error}; cleanup failed: {cleanup_error}"
            )))
        }
    }
}

#[cfg(test)]
fn run_lifecycle<P: HcsLifecycleProvider>(
    provider: &mut P,
    timeout: Duration,
    cancellation: &Cancellation,
) -> Result<RunResult, WindowsHcsError> {
    let mut observer = None;
    run_lifecycle_with_observer(provider, timeout, cancellation, &mut observer)
}

#[cfg(any(windows, test))]
fn run_lifecycle_with_observer<P: HcsLifecycleProvider>(
    provider: &mut P,
    timeout: Duration,
    cancellation: &Cancellation,
    observer: &mut Option<&mut dyn HcsLifecycleObserver>,
) -> Result<RunResult, WindowsHcsError> {
    if let Err(error) = provider.start(timeout) {
        let _ = observe_event(
            observer,
            HcsLifecycleEvent::StartFailed {
                error: error.to_string(),
            },
        );
        return cleanup_after_lifecycle_error(provider, timeout, error, observer);
    }
    if let Err(error) = observe_event(observer, HcsLifecycleEvent::Started) {
        return cleanup_after_lifecycle_error(provider, timeout, error, observer);
    }
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cancellation.is_cancelled() {
            if let Err(error) = observe_event(observer, HcsLifecycleEvent::Cancelled) {
                return cleanup_after_lifecycle_error(provider, timeout, error, observer);
            }
            return cleanup_after_interrupt(provider, timeout, RunStatus::Cancelled, observer);
        }
        if std::time::Instant::now() >= deadline {
            if let Err(error) = observe_event(observer, HcsLifecycleEvent::TimedOut) {
                return cleanup_after_lifecycle_error(provider, timeout, error, observer);
            }
            return cleanup_after_interrupt(provider, timeout, RunStatus::TimedOut, observer);
        }
        if let Err(error) = observe_event(observer, HcsLifecycleEvent::Waiting) {
            return cleanup_after_lifecycle_error(provider, timeout, error, observer);
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let wait = match provider.wait_for_exit(remaining.min(Duration::from_secs(1))) {
            Ok(wait) => wait,
            Err(error) => {
                let _ = observe_event(
                    observer,
                    HcsLifecycleEvent::WaitFailed {
                        error: error.to_string(),
                    },
                );
                return cleanup_after_lifecycle_error(provider, timeout, error, observer);
            }
        };
        match wait {
            HcsWaitOutcome::TimedOut => {}
            HcsWaitOutcome::Exited { exit_code } => {
                if let Err(error) =
                    observe_event(observer, HcsLifecycleEvent::GuestExited { exit_code })
                {
                    return cleanup_after_lifecycle_error(provider, timeout, error, observer);
                }
                if let Err(error) = observe_event(observer, HcsLifecycleEvent::ShutdownStarted) {
                    return cleanup_after_lifecycle_error(provider, timeout, error, observer);
                }
                let system_status = match provider.shutdown(timeout) {
                    Ok(status) => status,
                    Err(error) => {
                        let _ = observe_event(
                            observer,
                            HcsLifecycleEvent::ShutdownFailed {
                                error: error.to_string(),
                            },
                        );
                        return cleanup_after_lifecycle_error(provider, timeout, error, observer);
                    }
                };
                if let Err(error) = validate_system_exit_status(&system_status) {
                    let _ = observe_event(
                        observer,
                        HcsLifecycleEvent::ShutdownFailed {
                            error: error.to_string(),
                        },
                    );
                    return cleanup_after_lifecycle_error(provider, timeout, error, observer);
                }
                if let Err(error) = observe_event(
                    observer,
                    HcsLifecycleEvent::ShutdownCompleted {
                        status: system_status.status,
                        exit_type: system_status.exit_type,
                    },
                ) {
                    return cleanup_after_lifecycle_error(provider, timeout, error, observer);
                }
                let Some(exit_code) = exit_code else {
                    return Err(WindowsHcsError::OperationFailed(
                        "guest process exit code was not available".into(),
                    ));
                };
                return Ok(RunResult {
                    status: if exit_code == 0 {
                        RunStatus::Completed
                    } else {
                        RunStatus::Failed
                    },
                    exit_code: Some(exit_code),
                    reaped: true,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                });
            }
        }
    }
}

#[cfg(windows)]
mod hcs {
    use super::{
        Cancellation, HcsLifecycleEvent, HcsLifecycleObserver, HcsLifecycleProvider,
        HcsSystemSummary, HcsWaitOutcome, RunResult, RunStatus, WindowsHcsError,
        configuration_json, run_lifecycle_with_observer,
    };
    use crate::production::WindowsHcsContainerSpec;
    use serde::Deserialize;
    use serde::de::DeserializeOwned;
    use serde_json::json;
    use std::collections::HashSet;
    use std::ffi::{OsStr, c_void};
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
    use windows_sys::Win32::System::HostComputeSystem::{
        HCS_OPERATION, HCS_PROCESS, HCS_PROCESS_INFORMATION, HCS_SYSTEM, HcsCloseComputeSystem,
        HcsCloseOperation, HcsCloseProcess, HcsCreateComputeSystem, HcsCreateOperation,
        HcsCreateProcess, HcsEnumerateComputeSystems, HcsOpenComputeSystem,
        HcsShutDownComputeSystem, HcsStartComputeSystem, HcsTerminateComputeSystem,
        HcsWaitForComputeSystemExit, HcsWaitForOperationResult,
        HcsWaitForOperationResultAndProcessInfo, HcsWaitForProcessExit,
    };

    type HcsOperation = HCS_OPERATION;
    type HcsProcess = HCS_PROCESS;
    type HcsSystem = HCS_SYSTEM;

    const MAX_ENUMERATED_SYSTEMS: usize = 4096;
    const MAX_SUMMARY_FIELD_BYTES: usize = 4096;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    pub fn run(
        spec: &WindowsHcsContainerSpec,
        timeout: Duration,
        cancellation: &Cancellation,
        observer: Option<&mut dyn HcsLifecycleObserver>,
    ) -> Result<RunResult, WindowsHcsError> {
        let configuration = configuration_json(spec)?;
        let process_parameters = process_parameters_json(spec)?;
        let id = wide(&spec.container_id);
        let configuration = wide(&configuration);
        let process_parameters = wide(&process_parameters);
        let system = create_system(&id, &configuration, timeout)?;
        let mut observer = observer;
        if let Err(error) = super::observe_event(
            &mut observer,
            HcsLifecycleEvent::Created {
                system_id: spec.container_id.clone(),
            },
        ) {
            let _ = terminate(system, timeout);
            // SAFETY: system is the live handle returned by HcsCreateComputeSystem
            // and is closed exactly once after the lifecycle completes.
            unsafe { HcsCloseComputeSystem(system) };
            return Err(error);
        }
        let result = run_system(
            system,
            &process_parameters,
            timeout,
            cancellation,
            &mut observer,
        );
        // SAFETY: system is the live handle returned by HcsCreateComputeSystem
        // and is closed exactly once after the lifecycle completes.
        unsafe { HcsCloseComputeSystem(system) };
        super::observe_event(&mut observer, HcsLifecycleEvent::Closed)?;
        let mut result = result?;
        if result.status == RunStatus::Completed {
            match super::read_result_file(&spec.result_path, spec.max_output_bytes) {
                Ok(bytes) => {
                    let size = bytes.len();
                    let digest = crate::sha256_digest(&bytes);
                    super::observe_event(
                        &mut observer,
                        HcsLifecycleEvent::ResultRead {
                            sha256: digest,
                            size,
                        },
                    )?;
                    result.stdout = bytes;
                }
                Err(error) => {
                    let _ = super::observe_event(
                        &mut observer,
                        HcsLifecycleEvent::ResultReadFailed {
                            error: error.to_string(),
                        },
                    );
                    return Err(error);
                }
            }
        }
        Ok(result)
    }

    pub fn enumerate_systems(timeout: Duration) -> Result<Vec<HcsSystemSummary>, WindowsHcsError> {
        // SAFETY: null callback/context arguments request a standalone HCS
        // operation, and the returned handle is checked before use.
        let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
        if operation.is_null() {
            return Err(WindowsHcsError::ProviderUnavailable(
                "HcsCreateOperation returned null".into(),
            ));
        }
        let query = wide("{}");
        // SAFETY: query is a NUL-terminated UTF-16 buffer alive for the call;
        // operation is a live HCS operation handle and is closed below.
        let hr = unsafe { HcsEnumerateComputeSystems(query.as_ptr(), operation) };
        let document = if hr < 0 {
            Err(WindowsHcsError::OperationFailed(format!(
                "HcsEnumerateComputeSystems HRESULT 0x{hr:08x}"
            )))
        } else {
            wait_operation_document(operation, timeout)
        };
        // SAFETY: operation is the valid enumeration handle and is closed once
        // after the result document has been copied.
        unsafe { HcsCloseOperation(operation) };
        let document = document?.ok_or_else(|| {
            WindowsHcsError::OperationFailed("HCS enumeration returned no document".into())
        })?;
        parse_system_summaries(&document)
    }

    pub fn terminate_system(
        system_id: &str,
        timeout: Duration,
    ) -> Result<super::HcsSystemExitStatus, WindowsHcsError> {
        let id = wide(system_id);
        let mut system = ptr::null_mut();
        // SAFETY: id is a NUL-terminated UTF-16 buffer alive for the call and
        // the output pointer is valid. GENERIC_ALL is the access requested by
        // the HCS API for an opened compute-system handle.
        let hr = unsafe { HcsOpenComputeSystem(id.as_ptr(), GENERIC_ALL, &raw mut system) };
        if hr < 0 {
            return Err(WindowsHcsError::OperationFailed(format!(
                "HcsOpenComputeSystem HRESULT 0x{hr:08x}"
            )));
        }
        if system.is_null() {
            return Err(WindowsHcsError::OperationFailed(
                "HcsOpenComputeSystem returned a null handle".into(),
            ));
        }
        let result = terminate(system, timeout)
            .and_then(|status| super::validate_system_exit_status(&status).map(|()| status));
        // SAFETY: system is the live handle returned by HcsOpenComputeSystem
        // and is closed exactly once after termination completes.
        unsafe { HcsCloseComputeSystem(system) };
        result
    }

    fn parse_system_summaries(document: &str) -> Result<Vec<HcsSystemSummary>, WindowsHcsError> {
        let value: serde_json::Value = serde_json::from_str(document).map_err(|error| {
            WindowsHcsError::OperationFailed(format!("invalid HCS enumeration document: {error}"))
        })?;
        let systems = match value {
            serde_json::Value::Array(systems) => systems,
            serde_json::Value::Object(mut object) => {
                let systems = object
                    .remove("ComputeSystems")
                    .or_else(|| object.remove("Systems"))
                    .ok_or_else(|| {
                        WindowsHcsError::OperationFailed(
                            "HCS enumeration document has no compute-system array".into(),
                        )
                    })?;
                systems.as_array().cloned().ok_or_else(|| {
                    WindowsHcsError::OperationFailed(
                        "HCS enumeration compute-system field is not an array".into(),
                    )
                })?
            }
            _ => {
                return Err(WindowsHcsError::OperationFailed(
                    "HCS enumeration document must be an array or object".into(),
                ));
            }
        };
        if systems.len() > MAX_ENUMERATED_SYSTEMS {
            return Err(WindowsHcsError::OperationFailed(
                "HCS enumeration returned too many compute systems".into(),
            ));
        }
        let mut system_ids = HashSet::with_capacity(systems.len());
        systems
            .into_iter()
            .map(|system| {
                let object = system.as_object().ok_or_else(|| {
                    WindowsHcsError::OperationFailed(
                        "HCS enumeration entry is not an object".into(),
                    )
                })?;
                let id = object
                    .get("Id")
                    .and_then(serde_json::Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| {
                        WindowsHcsError::OperationFailed(
                            "HCS enumeration entry has no stable system id".into(),
                        )
                    })?
                    .to_owned();
                validate_summary_field("Id", &id)?;
                if !system_ids.insert(id.clone()) {
                    return Err(WindowsHcsError::OperationFailed(
                        "HCS enumeration returned duplicate system identities".into(),
                    ));
                }
                let owner = match object.get("Owner") {
                    None => None,
                    Some(serde_json::Value::String(owner)) => {
                        validate_summary_field("Owner", owner)?;
                        Some(owner.clone())
                    }
                    Some(_) => {
                        return Err(WindowsHcsError::OperationFailed(
                            "HCS enumeration owner is not a string".into(),
                        ));
                    }
                };
                let state = match object.get("State") {
                    None => None,
                    Some(serde_json::Value::String(state)) => {
                        validate_summary_field("State", state)?;
                        Some(state.clone())
                    }
                    Some(_) => {
                        return Err(WindowsHcsError::OperationFailed(
                            "HCS enumeration state is not a string".into(),
                        ));
                    }
                };
                Ok(HcsSystemSummary { id, owner, state })
            })
            .collect()
    }

    fn validate_summary_field(name: &str, value: &str) -> Result<(), WindowsHcsError> {
        if value.is_empty()
            || value.len() > MAX_SUMMARY_FIELD_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(WindowsHcsError::OperationFailed(format!(
                "HCS enumeration {name} field is invalid"
            )));
        }
        Ok(())
    }

    fn create_system(
        id: &[u16],
        configuration: &[u16],
        timeout: Duration,
    ) -> Result<HcsSystem, WindowsHcsError> {
        // SAFETY: null callback/context arguments request a standalone HCS
        // operation, and the returned handle is checked before use.
        let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
        if operation.is_null() {
            return Err(WindowsHcsError::ProviderUnavailable(
                "HcsCreateOperation returned null".into(),
            ));
        }
        let mut system = ptr::null_mut();
        // SAFETY: id and configuration are NUL-terminated UTF-16 buffers
        // alive for the call; operation and output pointers are valid.
        let hr = unsafe {
            HcsCreateComputeSystem(
                id.as_ptr(),
                configuration.as_ptr(),
                operation,
                ptr::null(),
                &raw mut system,
            )
        };
        let wait = wait_operation(operation, timeout);
        // SAFETY: operation is the valid handle created above and is closed
        // exactly once after the asynchronous result has been observed.
        unsafe { HcsCloseOperation(operation) };
        if hr < 0 {
            return Err(WindowsHcsError::OperationFailed(format!(
                "HcsCreateComputeSystem HRESULT 0x{hr:08x}"
            )));
        }
        wait?;
        if system.is_null() {
            return Err(WindowsHcsError::OperationFailed(
                "HCS returned a null compute-system handle".into(),
            ));
        }
        Ok(system)
    }

    struct NativeHcsProvider {
        system: HcsSystem,
        process: HcsProcess,
        process_id: Option<u32>,
        process_parameters: Vec<u16>,
    }

    impl Drop for NativeHcsProvider {
        fn drop(&mut self) {
            if !self.process.is_null() {
                // SAFETY: process is the live handle returned by HcsCreateProcess
                // and is closed at most once by this provider.
                unsafe { HcsCloseProcess(self.process) };
                self.process = ptr::null_mut();
            }
        }
    }

    impl HcsLifecycleProvider for NativeHcsProvider {
        fn start(&mut self, timeout: Duration) -> Result<(), WindowsHcsError> {
            // SAFETY: null callback/context arguments request a standalone HCS
            // operation, and the returned handle is checked before use.
            let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
            if operation.is_null() {
                return Err(WindowsHcsError::ProviderUnavailable(
                    "HcsCreateOperation returned null".into(),
                ));
            }
            // SAFETY: self.system and operation are live HCS handles, and the
            // optional settings pointer is intentionally null.
            let hr = unsafe { HcsStartComputeSystem(self.system, operation, ptr::null()) };
            let start_wait = wait_operation(operation, timeout);
            // SAFETY: operation is the valid start operation handle and is
            // closed exactly once after waiting for its result.
            unsafe { HcsCloseOperation(operation) };
            if hr < 0 {
                return Err(WindowsHcsError::OperationFailed(format!(
                    "HcsStartComputeSystem HRESULT 0x{hr:08x}"
                )));
            }
            start_wait?;
            self.create_process(timeout)
        }

        fn wait_for_exit(&mut self, timeout: Duration) -> Result<HcsWaitOutcome, WindowsHcsError> {
            if self.process.is_null() {
                return Err(WindowsHcsError::OperationFailed(
                    "HCS guest process handle is unavailable".into(),
                ));
            }
            let mut document = ptr::null_mut();
            let wait_ms = timeout_millis(timeout, "HCS process wait timeout overflow")?;
            // SAFETY: self.process is a live HCS process handle and document
            // points to initialized storage owned by this stack frame.
            let hr = unsafe { HcsWaitForProcessExit(self.process, wait_ms, &raw mut document) };
            if is_timeout(hr) {
                free_document(document);
                return Ok(HcsWaitOutcome::TimedOut);
            }
            if hr < 0 {
                free_document(document);
                return Err(WindowsHcsError::OperationFailed(format!(
                    "HcsWaitForProcessExit HRESULT 0x{hr:08x}"
                )));
            }
            let status: ProcessStatusDocument = parse_document(document)?;
            if !status.exited && is_timeout(status.last_wait_result) {
                return Ok(HcsWaitOutcome::TimedOut);
            }
            if status.last_wait_result != 0 {
                return Err(WindowsHcsError::OperationFailed(format!(
                    "HCS process wait returned HRESULT 0x{:08x}",
                    status.last_wait_result
                )));
            }
            if !status.exited {
                return Err(WindowsHcsError::OperationFailed(
                    "HCS process wait completed without an exited process".into(),
                ));
            }
            if self.process_id != Some(status.process_id) {
                return Err(WindowsHcsError::OperationFailed(
                    "HCS process status identity does not match the created process".into(),
                ));
            }
            let exit_code = i32::try_from(status.exit_code).map_err(|_| {
                WindowsHcsError::OperationFailed(
                    "HCS guest process exit code exceeds the supported range".into(),
                )
            })?;
            Ok(HcsWaitOutcome::Exited {
                exit_code: Some(exit_code),
            })
        }

        fn terminate(
            &mut self,
            timeout: Duration,
        ) -> Result<super::HcsSystemExitStatus, WindowsHcsError> {
            terminate(self.system, timeout)
        }

        fn shutdown(
            &mut self,
            timeout: Duration,
        ) -> Result<super::HcsSystemExitStatus, WindowsHcsError> {
            shutdown(self.system, timeout)
        }
    }

    impl NativeHcsProvider {
        fn create_process(&mut self, timeout: Duration) -> Result<(), WindowsHcsError> {
            // SAFETY: null callback/context arguments request a standalone HCS
            // operation, and the returned handle is checked before use.
            let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
            if operation.is_null() {
                return Err(WindowsHcsError::ProviderUnavailable(
                    "HcsCreateOperation returned null".into(),
                ));
            }
            let mut process = ptr::null_mut();
            // SAFETY: self.system and operation are live HCS handles;
            // process_parameters is a NUL-terminated UTF-16 buffer owned by self.
            let hr = unsafe {
                HcsCreateProcess(
                    self.system,
                    self.process_parameters.as_ptr(),
                    operation,
                    ptr::null(),
                    &raw mut process,
                )
            };
            let mut process_information = HCS_PROCESS_INFORMATION::default();
            let wait = if hr >= 0 {
                wait_process_creation(operation, timeout, &mut process_information)
            } else {
                Err(WindowsHcsError::OperationFailed(format!(
                    "HcsCreateProcess HRESULT 0x{hr:08x}"
                )))
            };
            // SAFETY: operation is the valid process-creation operation handle
            // and is closed exactly once after its result has been observed.
            unsafe { HcsCloseOperation(operation) };
            close_standard_handles(&mut process_information);
            if hr < 0 {
                close_process_handle(process);
                return Err(WindowsHcsError::OperationFailed(format!(
                    "HcsCreateProcess HRESULT 0x{hr:08x}"
                )));
            }
            if let Err(error) = wait {
                close_process_handle(process);
                return Err(error);
            }
            if process.is_null() || process_information.ProcessId == 0 {
                close_process_handle(process);
                return Err(WindowsHcsError::OperationFailed(
                    "HCS returned incomplete guest process information".into(),
                ));
            }
            self.process = process;
            self.process_id = Some(process_information.ProcessId);
            Ok(())
        }
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct ProcessStatusDocument {
        process_id: u32,
        exited: bool,
        exit_code: u32,
        last_wait_result: i32,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct SystemExitStatusDocument {
        status: i32,
        exit_type: String,
    }

    fn timeout_millis(timeout: Duration, message: &str) -> Result<u32, WindowsHcsError> {
        u32::try_from(timeout.as_millis().min(u128::from(u32::MAX)))
            .map_err(|_| WindowsHcsError::OperationFailed(message.into()))
    }

    fn wait_process_creation(
        operation: HcsOperation,
        timeout: Duration,
        process_information: &mut HCS_PROCESS_INFORMATION,
    ) -> Result<(), WindowsHcsError> {
        let mut document = ptr::null_mut();
        let timeout_ms = timeout_millis(timeout, "HCS process creation timeout overflow")?;
        // SAFETY: operation is a live HCS operation and both output pointers
        // refer to initialized storage owned by this stack frame.
        let hr = unsafe {
            HcsWaitForOperationResultAndProcessInfo(
                operation,
                timeout_ms,
                process_information,
                &raw mut document,
            )
        };
        free_document(document);
        if is_timeout(hr) {
            return Err(WindowsHcsError::OperationFailed(
                "HCS process creation timed out".into(),
            ));
        }
        if hr < 0 {
            return Err(WindowsHcsError::OperationFailed(format!(
                "HCS process creation operation HRESULT 0x{hr:08x}"
            )));
        }
        Ok(())
    }

    fn close_process_handle(process: HcsProcess) {
        if !process.is_null() {
            // SAFETY: process is an HCS process handle returned by the API and
            // is closed only on an error path before ownership is transferred.
            unsafe { HcsCloseProcess(process) };
        }
    }

    fn close_standard_handles(process_information: &mut HCS_PROCESS_INFORMATION) {
        for handle in [
            &mut process_information.StdInput,
            &mut process_information.StdOutput,
            &mut process_information.StdError,
        ] {
            if !handle.is_null() && *handle != (-1isize as HANDLE) {
                // SAFETY: these handles are returned by HCS as process stdio
                // handles and are owned by this caller after process creation.
                unsafe { CloseHandle(*handle) };
                *handle = ptr::null_mut();
            }
        }
    }

    fn parse_document<T: DeserializeOwned>(document: *mut u16) -> Result<T, WindowsHcsError> {
        let text = take_document(document)?.ok_or_else(|| {
            WindowsHcsError::OperationFailed("HCS returned no status document".into())
        })?;
        serde_json::from_str(&text).map_err(|error| {
            WindowsHcsError::OperationFailed(format!("invalid HCS status document: {error}"))
        })
    }

    fn take_document(document: *mut u16) -> Result<Option<String>, WindowsHcsError> {
        const MAX_DOCUMENT_CODE_UNITS: usize = 1024 * 1024;
        if document.is_null() {
            return Ok(None);
        }
        // SAFETY: HCS returns a NUL-terminated UTF-16 allocation owned by the
        // caller until LocalFree. The bounded scan prevents an invalid status
        // document from causing an unbounded read.
        let result = unsafe {
            let mut length = 0usize;
            while length < MAX_DOCUMENT_CODE_UNITS && *document.add(length) != 0 {
                length += 1;
            }
            if length == MAX_DOCUMENT_CODE_UNITS {
                Err(WindowsHcsError::OperationFailed(
                    "HCS status document exceeds the size limit".into(),
                ))
            } else {
                let slice = std::slice::from_raw_parts(document, length);
                String::from_utf16(slice).map_err(|error| {
                    WindowsHcsError::OperationFailed(format!(
                        "HCS status document is not UTF-16: {error}"
                    ))
                })
            }
        };
        // SAFETY: document is the allocation returned by the HCS API and is
        // freed exactly once after its contents have been copied.
        unsafe { LocalFree(document.cast()) };
        result.map(Some)
    }

    fn wait_for_system_exit(
        system: HcsSystem,
        timeout: Duration,
    ) -> Result<super::HcsSystemExitStatus, WindowsHcsError> {
        let mut document = ptr::null_mut();
        let timeout_ms = timeout_millis(timeout, "HCS system-exit timeout overflow")?;
        // SAFETY: system is a live HCS handle and document points to
        // initialized storage owned by this stack frame.
        let hr = unsafe { HcsWaitForComputeSystemExit(system, timeout_ms, &raw mut document) };
        if is_timeout(hr) {
            free_document(document);
            return Err(WindowsHcsError::OperationFailed(
                "HCS compute system did not exit during cleanup".into(),
            ));
        }
        if hr < 0 {
            free_document(document);
            return Err(WindowsHcsError::OperationFailed(format!(
                "HcsWaitForComputeSystemExit HRESULT 0x{hr:08x}"
            )));
        }
        let status: SystemExitStatusDocument = parse_document(document)?;
        Ok(super::HcsSystemExitStatus {
            status: status.status,
            exit_type: status.exit_type,
        })
    }

    fn run_system(
        system: HcsSystem,
        process_parameters: &[u16],
        timeout: Duration,
        cancellation: &Cancellation,
        observer: &mut Option<&mut dyn HcsLifecycleObserver>,
    ) -> Result<RunResult, WindowsHcsError> {
        run_lifecycle_with_observer(
            &mut NativeHcsProvider {
                system,
                process: ptr::null_mut(),
                process_id: None,
                process_parameters: process_parameters.to_vec(),
            },
            timeout,
            cancellation,
            observer,
        )
    }

    fn is_timeout(hr: i32) -> bool {
        let value = hr.cast_unsigned();
        value == 258 || value == 0x8007_05b4
    }

    fn shutdown(
        system: HcsSystem,
        timeout: Duration,
    ) -> Result<super::HcsSystemExitStatus, WindowsHcsError> {
        // SAFETY: null callback/context arguments request a standalone HCS
        // operation, and the returned handle is checked before use.
        let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
        if operation.is_null() {
            return Err(WindowsHcsError::ProviderUnavailable(
                "HcsCreateOperation returned null".into(),
            ));
        }
        // SAFETY: system and operation are live HCS handles, and the optional
        // settings pointer is intentionally null.
        let hr = unsafe { HcsShutDownComputeSystem(system, operation, ptr::null()) };
        let wait = if hr >= 0 {
            wait_operation(operation, timeout)
        } else {
            Err(WindowsHcsError::OperationFailed(format!(
                "HcsShutDownComputeSystem HRESULT 0x{hr:08x}"
            )))
        };
        // SAFETY: operation is the valid shutdown operation handle and is
        // closed exactly once after the result has been observed.
        unsafe { HcsCloseOperation(operation) };
        wait?;
        wait_for_system_exit(system, timeout)
    }

    fn terminate(
        system: HcsSystem,
        timeout: Duration,
    ) -> Result<super::HcsSystemExitStatus, WindowsHcsError> {
        // SAFETY: null callback/context arguments request a standalone HCS
        // operation, and the returned handle is checked before use.
        let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
        if operation.is_null() {
            return Err(WindowsHcsError::ProviderUnavailable(
                "HcsCreateOperation returned null".into(),
            ));
        }
        // SAFETY: system and operation are live HCS handles, and the optional
        // settings pointer is intentionally null.
        let hr = unsafe { HcsTerminateComputeSystem(system, operation, ptr::null()) };
        let wait = if hr >= 0 {
            wait_operation(operation, timeout)
        } else {
            Err(WindowsHcsError::OperationFailed(format!(
                "HcsTerminateComputeSystem HRESULT 0x{hr:08x}"
            )))
        };
        // SAFETY: operation is the valid terminate operation handle and is
        // closed exactly once after the result has been observed.
        unsafe { HcsCloseOperation(operation) };
        wait?;
        wait_for_system_exit(system, timeout)
    }

    fn wait_operation(operation: HcsOperation, timeout: Duration) -> Result<(), WindowsHcsError> {
        let _ = wait_operation_document(operation, timeout)?;
        Ok(())
    }

    fn wait_operation_document(
        operation: HcsOperation,
        timeout: Duration,
    ) -> Result<Option<String>, WindowsHcsError> {
        let mut document = ptr::null_mut();
        let timeout_ms =
            u32::try_from(timeout.as_millis().min(u128::from(u32::MAX))).map_err(|_| {
                WindowsHcsError::OperationFailed("HCS operation timeout overflow".into())
            })?;
        // SAFETY: operation is a live HCS handle and document points to
        // initialized storage owned by this stack frame.
        let hr = unsafe { HcsWaitForOperationResult(operation, timeout_ms, &raw mut document) };
        if hr < 0 {
            free_document(document);
            return Err(WindowsHcsError::OperationFailed(format!(
                "HCS operation HRESULT 0x{hr:08x}"
            )));
        }
        take_document(document)
    }

    fn free_document(document: *mut u16) {
        if !document.is_null() {
            // SAFETY: document is the allocation returned by the HCS API and
            // is freed only when the API returned a non-null pointer.
            unsafe { LocalFree(document.cast()) };
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub(super) fn process_parameters_json(
        spec: &WindowsHcsContainerSpec,
    ) -> Result<String, WindowsHcsError> {
        let mut command = spec.entrypoint.clone();
        let application_name = command
            .first_mut()
            .ok_or_else(|| WindowsHcsError::InvalidSpec("HCS process has no application".into()))?;
        application_name.clone_from(&spec.runner_container_path);
        let command_line = command
            .iter()
            .map(|argument| quote_windows_argument(argument))
            .collect::<Vec<_>>()
            .join(" ");
        serde_json::to_string(&json!({
            "ApplicationName": spec.runner_container_path,
            "CommandLine": command_line,
            "CreateStdInPipe": false,
            "CreateStdOutPipe": false,
            "CreateStdErrPipe": false,
            "Environment": {
                "HIVEMIND_RESULT_PATH": spec.result_container_path,
                "HIVEMIND_BACKEND_ID": spec.backend_id,
                "HIVEMIND_GUEST_IMAGE_DIGEST": spec.guest_image_digest,
                "HIVEMIND_IMAGE_MATERIAL_SHA256": spec.image_material_digest,
                "HIVEMIND_RUNNER_SHA256": spec.runner_sha256,
                "HIVEMIND_POLICY_SHA256": spec.policy_digest,
            },
        }))
        .map_err(|error| WindowsHcsError::InvalidSpec(error.to_string()))
    }

    fn quote_windows_argument(argument: &str) -> String {
        if !argument.is_empty()
            && !argument
                .chars()
                .any(|character| character.is_whitespace() || character == '"')
        {
            return argument.to_owned();
        }
        let mut quoted = String::with_capacity(argument.len() + 2);
        quoted.push('"');
        let mut backslashes = 0usize;
        for character in argument.chars() {
            match character {
                '\\' => backslashes = backslashes.saturating_add(1),
                '"' => {
                    quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                    quoted.push('"');
                    backslashes = 0;
                }
                _ => {
                    quoted.extend(std::iter::repeat_n('\\', backslashes));
                    quoted.push(character);
                    backslashes = 0;
                }
            }
        }
        quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
        quoted.push('"');
        quoted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production::{WindowsHcsContainerSpec, WindowsHcsMountSpec};
    use std::path::PathBuf;

    struct MockHcsProvider {
        events: Vec<&'static str>,
        wait_outcome: HcsWaitOutcome,
        start_error: bool,
        terminate_error: Option<WindowsHcsError>,
        shutdown_error: Option<WindowsHcsError>,
        terminate_status: HcsSystemExitStatus,
        shutdown_status: HcsSystemExitStatus,
    }

    impl MockHcsProvider {
        fn new(wait_outcome: HcsWaitOutcome) -> Self {
            Self {
                events: Vec::new(),
                wait_outcome,
                start_error: false,
                terminate_error: None,
                shutdown_error: None,
                terminate_status: HcsSystemExitStatus {
                    status: 0,
                    exit_type: "ForcedExit".into(),
                },
                shutdown_status: HcsSystemExitStatus {
                    status: 0,
                    exit_type: "GracefulExit".into(),
                },
            }
        }
    }

    impl HcsLifecycleProvider for MockHcsProvider {
        fn start(&mut self, _timeout: Duration) -> Result<(), WindowsHcsError> {
            self.events.push("start");
            if self.start_error {
                return Err(WindowsHcsError::OperationFailed("mock start".into()));
            }
            Ok(())
        }

        fn wait_for_exit(&mut self, _timeout: Duration) -> Result<HcsWaitOutcome, WindowsHcsError> {
            self.events.push("wait");
            Ok(self.wait_outcome)
        }

        fn terminate(
            &mut self,
            _timeout: Duration,
        ) -> Result<HcsSystemExitStatus, WindowsHcsError> {
            self.events.push("terminate");
            self.terminate_error
                .take()
                .map_or_else(|| Ok(self.terminate_status.clone()), Err)
        }

        fn shutdown(&mut self, _timeout: Duration) -> Result<HcsSystemExitStatus, WindowsHcsError> {
            self.events.push("shutdown");
            self.shutdown_error
                .take()
                .map_or_else(|| Ok(self.shutdown_status.clone()), Err)
        }
    }

    fn spec() -> WindowsHcsContainerSpec {
        WindowsHcsContainerSpec {
            container_id: "hivemind-test".into(),
            backend_id: "windows-test".into(),
            guest_image_digest: format!("sha256:{}", "a".repeat(64)),
            image_material_digest: format!("sha256:{}", "a".repeat(64)),
            image_root: PathBuf::from("C:\\hivemind\\image"),
            runner_executable: PathBuf::from("C:\\hivemind\\image\\runner.exe"),
            runner_sha256: format!("sha256:{}", "b".repeat(64)),
            runner_container_path: "C:\\runner.exe".into(),
            policy_digest: format!("sha256:{}", "c".repeat(64)),
            entrypoint: vec!["runner.exe".into()],
            mounts: vec![
                WindowsHcsMountSpec {
                    host_path: PathBuf::from("C:\\hivemind\\artifact"),
                    container_path: "C:\\work\\source".into(),
                    read_only: true,
                },
                WindowsHcsMountSpec {
                    host_path: PathBuf::from("C:\\hivemind\\scratch"),
                    container_path: "C:\\work\\output".into(),
                    read_only: false,
                },
            ],
            storage_path: PathBuf::from("C:\\hivemind\\scratch"),
            result_path: PathBuf::from("C:\\hivemind\\scratch\\result.json"),
            result_container_path: "C:\\work\\output\\result.json".into(),
            max_output_bytes: 4096,
            network_isolated: true,
            root_read_only: true,
            resource_limits: crate::sandbox::WindowsHcsResourceLimits {
                memory_size_mb: 1,
                processor_maximum: 10_000,
            },
        }
    }

    #[test]
    fn hcs_configuration_uses_schema_two_resource_and_network_fields() {
        let value: serde_json::Value = serde_json::from_str(
            &configuration_json(&spec()).expect("HCS configuration should serialize"),
        )
        .expect("HCS configuration should be valid JSON");
        let container = &value["Container"];
        let storage = &container["Storage"];

        assert_eq!(value["Owner"], serde_json::json!("hivemind"));
        assert_eq!(
            value["SchemaVersion"],
            serde_json::json!({"Major": 2, "Minor": 1})
        );
        assert!(container.get("Annotations").is_none());
        assert_eq!(
            storage["Layers"][0]["Path"],
            serde_json::json!("C:\\hivemind\\image")
        );
        assert_eq!(storage["Path"], serde_json::json!("C:\\hivemind\\scratch"));
        assert!(storage.get("SandboxPath").is_none());
        assert_eq!(container["Memory"]["SizeInMB"], serde_json::json!(1));
        assert_eq!(container["Processor"]["Maximum"], serde_json::json!(10_000));
        assert_eq!(
            container["Networking"]["NetworkAdapters"],
            serde_json::json!([])
        );
        assert!(container.get("NetworkEndpoints").is_none());

        let mounts = container["MappedDirectories"]
            .as_array()
            .expect("HCS mapped directories should be an array");
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0]["HostPathType"], serde_json::json!("Directory"));
        assert_eq!(mounts[0]["ReadOnly"], serde_json::json!(true));
        assert_eq!(mounts[1]["ReadOnly"], serde_json::json!(false));
    }

    #[cfg(windows)]
    #[test]
    fn hcs_process_parameters_bind_verified_runner_and_identity() {
        let value: serde_json::Value = serde_json::from_str(
            &hcs::process_parameters_json(&spec())
                .expect("HCS process parameters should serialize"),
        )
        .expect("HCS process parameters should be valid JSON");

        assert_eq!(
            value["ApplicationName"],
            serde_json::json!("C:\\runner.exe")
        );
        assert_eq!(value["CommandLine"], serde_json::json!("C:\\runner.exe"));
        assert_eq!(
            value["Environment"]["HIVEMIND_BACKEND_ID"],
            serde_json::json!("windows-test")
        );
        assert_eq!(
            value["Environment"]["HIVEMIND_GUEST_IMAGE_DIGEST"],
            serde_json::json!(format!("sha256:{}", "a".repeat(64)))
        );
        assert_eq!(
            value["Environment"]["HIVEMIND_IMAGE_MATERIAL_SHA256"],
            serde_json::json!(format!("sha256:{}", "a".repeat(64)))
        );
        assert_eq!(
            value["Environment"]["HIVEMIND_RUNNER_SHA256"],
            serde_json::json!(format!("sha256:{}", "b".repeat(64)))
        );
        assert_eq!(
            value["Environment"]["HIVEMIND_POLICY_SHA256"],
            serde_json::json!(format!("sha256:{}", "c".repeat(64)))
        );
    }

    #[test]
    fn mock_hcs_lifecycle_propagates_exit_code_and_shuts_down() {
        let mut provider = MockHcsProvider::new(HcsWaitOutcome::Exited { exit_code: Some(0) });
        let result = run_lifecycle(&mut provider, Duration::from_secs(1), &Cancellation::new())
            .expect("mock HCS completion should succeed");
        assert_eq!(result.status, RunStatus::Completed);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.reaped);
        assert_eq!(provider.events, ["start", "wait", "shutdown"]);
    }

    #[test]
    fn mock_hcs_lifecycle_reports_nonzero_guest_exit() {
        let mut provider = MockHcsProvider::new(HcsWaitOutcome::Exited {
            exit_code: Some(17),
        });
        let result = run_lifecycle(&mut provider, Duration::from_secs(1), &Cancellation::new())
            .expect("nonzero guest exit is a terminal result");
        assert_eq!(result.status, RunStatus::Failed);
        assert_eq!(result.exit_code, Some(17));
        assert_eq!(provider.events, ["start", "wait", "shutdown"]);
    }

    #[test]
    fn mock_hcs_lifecycle_rejects_unknown_guest_exit() {
        let mut provider = MockHcsProvider::new(HcsWaitOutcome::Exited { exit_code: None });
        let error = run_lifecycle(&mut provider, Duration::from_secs(1), &Cancellation::new())
            .expect_err("unknown guest exit must fail closed");
        assert!(matches!(error, WindowsHcsError::OperationFailed(_)));
        assert_eq!(provider.events, ["start", "wait", "shutdown"]);
    }

    #[test]
    fn mock_hcs_lifecycle_terminates_on_timeout_and_cancellation() {
        let mut timed_out = MockHcsProvider::new(HcsWaitOutcome::TimedOut);
        let result = run_lifecycle(&mut timed_out, Duration::ZERO, &Cancellation::new())
            .expect("timeout cleanup should return a terminal result");
        assert_eq!(result.status, RunStatus::TimedOut);
        assert_eq!(timed_out.events, ["start", "terminate"]);

        let cancellation = Cancellation::new();
        cancellation.cancel();
        let mut cancelled = MockHcsProvider::new(HcsWaitOutcome::Exited { exit_code: Some(0) });
        let result = run_lifecycle(&mut cancelled, Duration::from_secs(1), &cancellation)
            .expect("cancellation cleanup should return a terminal result");
        assert_eq!(result.status, RunStatus::Cancelled);
        assert_eq!(cancelled.events, ["start", "terminate"]);
    }

    #[test]
    fn mock_hcs_lifecycle_cleans_up_after_start_failure() {
        let mut provider = MockHcsProvider::new(HcsWaitOutcome::Exited { exit_code: Some(0) });
        provider.start_error = true;
        let error = run_lifecycle(&mut provider, Duration::from_secs(1), &Cancellation::new())
            .expect_err("start failure must be returned");
        assert!(matches!(error, WindowsHcsError::OperationFailed(_)));
        assert_eq!(provider.events, ["start", "terminate"]);
    }

    #[test]
    fn mock_hcs_lifecycle_rejects_non_authoritative_shutdown_status() {
        let mut nonzero = MockHcsProvider::new(HcsWaitOutcome::Exited { exit_code: Some(0) });
        nonzero.shutdown_status.status = -1;
        let error = run_lifecycle(&mut nonzero, Duration::from_secs(1), &Cancellation::new())
            .expect_err("nonzero HCS shutdown status must fail closed");
        assert!(matches!(error, WindowsHcsError::OperationFailed(_)));
        assert_eq!(nonzero.events, ["start", "wait", "shutdown", "terminate"]);

        let mut unexpected_type =
            MockHcsProvider::new(HcsWaitOutcome::Exited { exit_code: Some(0) });
        unexpected_type.shutdown_status.exit_type = "UnknownExit".into();
        let error = run_lifecycle(
            &mut unexpected_type,
            Duration::from_secs(1),
            &Cancellation::new(),
        )
        .expect_err("unexpected HCS shutdown type must fail closed");
        assert!(matches!(error, WindowsHcsError::OperationFailed(_)));
        assert_eq!(
            unexpected_type.events,
            ["start", "wait", "shutdown", "terminate"]
        );
    }

    #[test]
    fn mock_hcs_lifecycle_rejects_non_authoritative_termination_status() {
        let mut provider = MockHcsProvider::new(HcsWaitOutcome::TimedOut);
        provider.terminate_status.status = -1;
        let error = run_lifecycle(&mut provider, Duration::ZERO, &Cancellation::new())
            .expect_err("nonzero HCS termination status must fail closed");
        assert!(matches!(error, WindowsHcsError::CleanupFailed(_)));
        assert_eq!(provider.events, ["start", "terminate"]);

        let mut provider = MockHcsProvider::new(HcsWaitOutcome::TimedOut);
        provider.terminate_status.exit_type = "UnknownExit".into();
        let error = run_lifecycle(&mut provider, Duration::ZERO, &Cancellation::new())
            .expect_err("unexpected HCS termination type must fail closed");
        assert!(matches!(error, WindowsHcsError::CleanupFailed(_)));
        assert_eq!(provider.events, ["start", "terminate"]);
    }

    #[test]
    fn mock_hcs_lifecycle_surfaces_cleanup_failure() {
        let mut provider = MockHcsProvider::new(HcsWaitOutcome::TimedOut);
        provider.terminate_error = Some(WindowsHcsError::OperationFailed("terminate".into()));
        let error = run_lifecycle(&mut provider, Duration::ZERO, &Cancellation::new())
            .expect_err("failed timeout cleanup must not look terminally successful");
        assert!(matches!(error, WindowsHcsError::CleanupFailed(_)));

        let mut provider = MockHcsProvider::new(HcsWaitOutcome::Exited { exit_code: Some(0) });
        provider.shutdown_error = Some(WindowsHcsError::OperationFailed("shutdown".into()));
        let error = run_lifecycle(&mut provider, Duration::from_secs(1), &Cancellation::new())
            .expect_err("failed normal cleanup must not complete");
        assert!(matches!(error, WindowsHcsError::OperationFailed(_)));
        assert_eq!(provider.events, ["start", "wait", "shutdown", "terminate"]);
    }

    #[test]
    fn result_transport_requires_a_writable_explicit_mount() {
        let mut invalid = spec();
        invalid.result_path = PathBuf::from("C:\\hivemind\\artifact\\result.json");
        let error = WindowsHcsLauncher::new()
            .run(&invalid, &Cancellation::new())
            .expect_err("result files must not be written through read-only mounts");
        assert!(matches!(error, WindowsHcsError::InvalidSpec(_)));
    }

    #[test]
    fn result_transport_rejects_missing_and_oversized_files() {
        let root = std::env::temp_dir().join(format!("hivemind-hcs-result-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let missing = root.join("missing.json");
        assert!(matches!(
            read_result_file(&missing, 32),
            Err(WindowsHcsError::ResultUnavailable(_))
        ));
        let oversized = root.join("oversized.json");
        std::fs::write(&oversized, b"0123456789").unwrap();
        assert_eq!(
            read_result_file(&oversized, 4).unwrap_err(),
            WindowsHcsError::ResultTooLarge {
                limit: 4,
                actual: 10,
            }
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_build_fails_closed_without_hcs() {
        let error = WindowsHcsLauncher::new()
            .run(&spec(), &Cancellation::new())
            .expect_err("Linux must not emulate Windows HCS");
        assert_eq!(error, WindowsHcsError::UnsupportedPlatform);
    }

    #[test]
    fn invalid_hcs_spec_fails_before_provider_access() {
        let mut invalid = spec();
        invalid.network_isolated = false;
        let error = WindowsHcsLauncher::new()
            .run(&invalid, &Cancellation::new())
            .expect_err("unsafe HCS policy must fail closed");
        assert!(matches!(error, WindowsHcsError::InvalidSpec(_)));
    }
}

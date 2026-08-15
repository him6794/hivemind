//! Native Windows Host Compute System integration.
//!
//! This module is deliberately separate from the Linux OCI launcher. It never
//! invokes Docker, WSL, a shell, or a direct host process. Windows builds use
//! ComputeCore.dll; non-Windows builds fail closed with `UnsupportedPlatform`.

use crate::production::WindowsHcsContainerSpec;
use crate::supervisor::{Cancellation, RunResult, RunStatus};
use std::io::Read;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsHcsError {
    UnsupportedPlatform,
    InvalidSpec(String),
    ProviderUnavailable(String),
    OperationFailed(String),
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
        validate_spec(spec)?;
        if cancellation.is_cancelled() {
            return Err(WindowsHcsError::Cancelled);
        }
        self.run_platform(spec, cancellation)
    }

    #[cfg(not(windows))]
    fn run_platform(
        &self,
        _spec: &WindowsHcsContainerSpec,
        _cancellation: &Cancellation,
    ) -> Result<RunResult, WindowsHcsError> {
        Err(WindowsHcsError::UnsupportedPlatform)
    }

    #[cfg(windows)]
    fn run_platform(
        &self,
        spec: &WindowsHcsContainerSpec,
        cancellation: &Cancellation,
    ) -> Result<RunResult, WindowsHcsError> {
        hcs::run(spec, self.timeout, cancellation)
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
    if spec.image_root.as_os_str().is_empty() || spec.entrypoint.is_empty() {
        return Err(WindowsHcsError::InvalidSpec(
            "image root and entrypoint are required".into(),
        ));
    }
    if spec.result_path.as_os_str().is_empty()
        || spec.result_container_path.is_empty()
        || spec.max_output_bytes == 0
    {
        return Err(WindowsHcsError::InvalidSpec(
            "result transport and output limit are required".into(),
        ));
    }
    let result_parent = spec.result_path.parent();
    let result_mount = spec.mounts.iter().any(|mount| {
        !mount.read_only
            && result_parent == Some(mount.host_path.as_path())
            && spec.result_container_path.starts_with(&format!(
                "{}\\",
                mount.container_path.trim_end_matches('\\')
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
    if spec.memory_bytes == 0
        || spec.cpu_millis == 0
        || spec.process_limit == 0
        || spec.thread_limit == 0
        || spec.scratch_bytes == 0
    {
        return Err(WindowsHcsError::InvalidSpec(
            "HCS resource limits must be nonzero".into(),
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
    file.by_ref().take(read_limit)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HcsWaitOutcome {
    Exited,
    TimedOut,
}

trait HcsLifecycleProvider {
    fn start(&mut self, timeout: Duration) -> Result<(), WindowsHcsError>;
    fn wait_for_exit(
        &mut self,
        timeout: Duration,
    ) -> Result<HcsWaitOutcome, WindowsHcsError>;
    fn terminate(&mut self, timeout: Duration);
    fn shutdown(&mut self, timeout: Duration);
}

fn run_lifecycle<P: HcsLifecycleProvider>(
    provider: &mut P,
    timeout: Duration,
    cancellation: &Cancellation,
) -> Result<RunResult, WindowsHcsError> {
    provider.start(timeout)?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cancellation.is_cancelled() {
            provider.terminate(timeout);
            return Err(WindowsHcsError::Cancelled);
        }
        if std::time::Instant::now() >= deadline {
            provider.terminate(timeout);
            return Err(WindowsHcsError::TimedOut);
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match provider.wait_for_exit(remaining.min(Duration::from_secs(1)))? {
            HcsWaitOutcome::TimedOut => continue,
            HcsWaitOutcome::Exited => {
                provider.shutdown(timeout);
                return Ok(RunResult {
                    status: RunStatus::Completed,
                    exit_code: Some(0),
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
        run_lifecycle, Cancellation, HcsLifecycleProvider, HcsWaitOutcome, RunResult,
        WindowsHcsError,
    };
    use crate::production::WindowsHcsContainerSpec;
    use serde_json::json;
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::time::Duration;
    use windows_sys::Win32::System::HostComputeSystem::{
        HcsCloseComputeSystem, HcsCloseOperation, HcsCreateComputeSystem, HcsCreateOperation,
        HcsShutDownComputeSystem, HcsStartComputeSystem, HcsTerminateComputeSystem,
        HcsWaitForComputeSystemExit, HcsWaitForOperationResult, HCS_OPERATION, HCS_SYSTEM,
    };

    type HcsOperation = HCS_OPERATION;
    type HcsSystem = HCS_SYSTEM;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    pub fn run(
        spec: &WindowsHcsContainerSpec,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<RunResult, WindowsHcsError> {
        let configuration = configuration_json(spec)?;
        let id = wide(&spec.container_id);
        let configuration = wide(&configuration);
        let system = create_system(&id, &configuration, timeout)?;
        let result = run_system(system, timeout, cancellation);
        unsafe { HcsCloseComputeSystem(system) };
        let mut result = result?;
        result.stdout = super::read_result_file(&spec.result_path, spec.max_output_bytes)?;
        Ok(result)
    }

    fn create_system(
        id: &[u16],
        configuration: &[u16],
        timeout: Duration,
    ) -> Result<HcsSystem, WindowsHcsError> {
        let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
        if operation.is_null() {
            return Err(WindowsHcsError::ProviderUnavailable(
                "HcsCreateOperation returned null".into(),
            ));
        }
        let mut system = ptr::null_mut();
        let hr = unsafe {
            HcsCreateComputeSystem(
                id.as_ptr(),
                configuration.as_ptr(),
                operation,
                ptr::null(),
                &mut system,
            )
        };
        let wait = wait_operation(operation, timeout);
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
    }

    impl HcsLifecycleProvider for NativeHcsProvider {
        fn start(&mut self, timeout: Duration) -> Result<(), WindowsHcsError> {
            let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
            if operation.is_null() {
                return Err(WindowsHcsError::ProviderUnavailable(
                    "HcsCreateOperation returned null".into(),
                ));
            }
            let hr = unsafe { HcsStartComputeSystem(self.system, operation, ptr::null()) };
            let start_wait = wait_operation(operation, timeout);
            unsafe { HcsCloseOperation(operation) };
            if hr < 0 {
                return Err(WindowsHcsError::OperationFailed(format!(
                    "HcsStartComputeSystem HRESULT 0x{hr:08x}"
                )));
            }
            start_wait
        }

        fn wait_for_exit(
            &mut self,
            timeout: Duration,
        ) -> Result<HcsWaitOutcome, WindowsHcsError> {
            let mut document = ptr::null_mut();
            let wait_ms = timeout.as_millis().max(1).min(u32::MAX as u128) as u32;
            let hr = unsafe { HcsWaitForComputeSystemExit(self.system, wait_ms, &mut document) };
            free_document(document);
            if is_timeout(hr) {
                return Ok(HcsWaitOutcome::TimedOut);
            }
            if hr >= 0 {
                return Ok(HcsWaitOutcome::Exited);
            }
            Err(WindowsHcsError::OperationFailed(format!(
                "HcsWaitForComputeSystemExit HRESULT 0x{hr:08x}"
            )))
        }

        fn terminate(&mut self, timeout: Duration) {
            terminate(self.system, timeout);
        }

        fn shutdown(&mut self, timeout: Duration) {
            shutdown(self.system, timeout);
        }
    }

    fn run_system(
        system: HcsSystem,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<RunResult, WindowsHcsError> {
        run_lifecycle(&mut NativeHcsProvider { system }, timeout, cancellation)
    }

    fn is_timeout(hr: i32) -> bool {
        let value = hr as u32;
        value == 258 || value == 0x8007_05b4
    }

    fn shutdown(system: HcsSystem, timeout: Duration) {
        let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
        if operation.is_null() {
            return;
        }
        let hr = unsafe { HcsShutDownComputeSystem(system, operation, ptr::null()) };
        if hr >= 0 {
            let _ = wait_operation(operation, timeout);
        }
        unsafe { HcsCloseOperation(operation) };
    }

    fn terminate(system: HcsSystem, timeout: Duration) {
        let operation = unsafe { HcsCreateOperation(ptr::null(), None) };
        if operation.is_null() {
            return;
        }
        let hr = unsafe { HcsTerminateComputeSystem(system, operation, ptr::null()) };
        if hr >= 0 {
            let _ = wait_operation(operation, timeout);
        }
        unsafe { HcsCloseOperation(operation) };
    }

    fn wait_operation(operation: HcsOperation, timeout: Duration) -> Result<(), WindowsHcsError> {
        let mut document = ptr::null_mut();
        let hr = unsafe {
            HcsWaitForOperationResult(
                operation,
                timeout.as_millis().min(u32::MAX as u128) as u32,
                &mut document,
            )
        };
        free_document(document);
        if hr < 0 {
            return Err(WindowsHcsError::OperationFailed(format!(
                "HCS operation HRESULT 0x{hr:08x}"
            )));
        }
        Ok(())
    }

    fn free_document(document: *mut u16) {
        if !document.is_null() {
            unsafe { LocalFree(document.cast()) };
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn configuration_json(spec: &WindowsHcsContainerSpec) -> Result<String, WindowsHcsError> {
        serde_json::to_string(&json!({
            "Owner": "hivemind",
            "SchemaVersion": {"Major": 2, "Minor": 1},
            "ShouldTerminateOnLastHandleClosed": true,
            "Container": {
                "Storage": {
                    "Layers": [{"Path": spec.image_root}],
                    "SandboxPath": spec.image_root,
                },
                "MappedDirectories": spec.mounts.iter().map(|mount| json!({
                    "HostPath": mount.host_path,
                    "ContainerPath": mount.container_path,
                    "ReadOnly": mount.read_only,
                })).collect::<Vec<_>>(),
                "NetworkEndpoints": [],
                "Process": {
                    "CommandLine": spec.entrypoint.join(" "),
                    "Environment": {
                        "HIVEMIND_RESULT_PATH": spec.result_container_path,
                    },
                },
            }
        }))
        .map_err(|error| WindowsHcsError::InvalidSpec(error.to_string()))
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
    }

    impl HcsLifecycleProvider for MockHcsProvider {
        fn start(&mut self, _timeout: Duration) -> Result<(), WindowsHcsError> {
            self.events.push("start");
            if self.start_error {
                return Err(WindowsHcsError::OperationFailed("mock start".into()));
            }
            Ok(())
        }

        fn wait_for_exit(
            &mut self,
            _timeout: Duration,
        ) -> Result<HcsWaitOutcome, WindowsHcsError> {
            self.events.push("wait");
            Ok(self.wait_outcome)
        }

        fn terminate(&mut self, _timeout: Duration) {
            self.events.push("terminate");
        }

        fn shutdown(&mut self, _timeout: Duration) {
            self.events.push("shutdown");
        }
    }

    fn spec() -> WindowsHcsContainerSpec {
        WindowsHcsContainerSpec {
            container_id: "hivemind-test".into(),
            image_root: PathBuf::from("C:\\hivemind\\image"),
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
            result_path: PathBuf::from("C:\\hivemind\\scratch\\result.json"),
            result_container_path: "C:\\work\\output\\result.json".into(),
            max_output_bytes: 4096,
            network_isolated: true,
            root_read_only: true,
            memory_bytes: 1024,
            cpu_millis: 1000,
            process_limit: 4,
            thread_limit: 4,
            scratch_bytes: 1024,
        }
    }

    #[test]
    fn mock_hcs_lifecycle_shuts_down_after_normal_exit() {
        let mut provider = MockHcsProvider {
            events: Vec::new(),
            wait_outcome: HcsWaitOutcome::Exited,
            start_error: false,
        };
        let result = run_lifecycle(
            &mut provider,
            Duration::from_secs(1),
            &Cancellation::new(),
        )
        .expect("mock HCS completion should succeed");
        assert_eq!(result.status, RunStatus::Completed);
        assert_eq!(provider.events, ["start", "wait", "shutdown"]);
    }

    #[test]
    fn mock_hcs_lifecycle_terminates_on_timeout_and_cancellation() {
        let mut timed_out = MockHcsProvider {
            events: Vec::new(),
            wait_outcome: HcsWaitOutcome::TimedOut,
            start_error: false,
        };
        let error = run_lifecycle(
            &mut timed_out,
            Duration::ZERO,
            &Cancellation::new(),
        )
        .expect_err("zero timeout must fail closed");
        assert_eq!(error, WindowsHcsError::TimedOut);
        assert_eq!(timed_out.events, ["start", "terminate"]);

        let cancellation = Cancellation::new();
        cancellation.cancel();
        let mut cancelled = MockHcsProvider {
            events: Vec::new(),
            wait_outcome: HcsWaitOutcome::Exited,
            start_error: false,
        };
        let error = run_lifecycle(&mut cancelled, Duration::from_secs(1), &cancellation)
            .expect_err("cancelled HCS task must fail closed");
        assert_eq!(error, WindowsHcsError::Cancelled);
        assert_eq!(cancelled.events, ["start", "terminate"]);
    }

    #[test]
    fn mock_hcs_lifecycle_does_not_cleanup_after_start_failure() {
        let mut provider = MockHcsProvider {
            events: Vec::new(),
            wait_outcome: HcsWaitOutcome::Exited,
            start_error: true,
        };
        let error = run_lifecycle(
            &mut provider,
            Duration::from_secs(1),
            &Cancellation::new(),
        )
        .expect_err("start failure must be returned");
        assert!(matches!(error, WindowsHcsError::OperationFailed(_)));
        assert_eq!(provider.events, ["start"]);
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

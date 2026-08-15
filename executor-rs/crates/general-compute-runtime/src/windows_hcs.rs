//! Native Windows Host Compute System integration.
//!
//! This module is deliberately separate from the Linux OCI launcher. It never
//! invokes Docker, WSL, a shell, or a direct host process. Windows builds use
//! ComputeCore.dll; non-Windows builds fail closed with `UnsupportedPlatform`.

use crate::production::WindowsHcsContainerSpec;
use crate::supervisor::{Cancellation, RunResult, RunStatus};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsHcsError {
    UnsupportedPlatform,
    InvalidSpec(String),
    ProviderUnavailable(String),
    OperationFailed(String),
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

#[cfg(windows)]
mod hcs {
    use super::{Cancellation, RunResult, RunStatus, WindowsHcsError};
    use crate::production::WindowsHcsContainerSpec;
    use serde_json::json;
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::time::{Duration, Instant};

    type HcsOperation = *mut c_void;
    type HcsSystem = *mut c_void;

    #[link(name = "computecore")]
    unsafe extern "system" {
        fn HcsCreateOperation(context: *mut c_void, callback: *mut c_void) -> HcsOperation;
        fn HcsCloseOperation(operation: HcsOperation);
        fn HcsCreateComputeSystem(
            id: *const u16,
            configuration: *const u16,
            operation: HcsOperation,
            security_descriptor: *const c_void,
            compute_system: *mut HcsSystem,
        ) -> i32;
        fn HcsStartComputeSystem(
            compute_system: HcsSystem,
            operation: HcsOperation,
            options: *const u16,
        ) -> i32;
        fn HcsWaitForOperationResult(
            operation: HcsOperation,
            timeout_ms: u32,
            result_document: *mut *mut u16,
        ) -> i32;
        fn HcsWaitForComputeSystemExit(
            compute_system: HcsSystem,
            timeout_ms: u32,
            result_document: *mut *mut u16,
        ) -> i32;
        fn HcsShutDownComputeSystem(
            compute_system: HcsSystem,
            operation: HcsOperation,
            options: *const u16,
        ) -> i32;
        fn HcsTerminateComputeSystem(
            compute_system: HcsSystem,
            operation: HcsOperation,
            options: *const u16,
        ) -> i32;
        fn HcsCloseComputeSystem(compute_system: HcsSystem);
    }

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
        result
    }

    fn create_system(
        id: &[u16],
        configuration: &[u16],
        timeout: Duration,
    ) -> Result<HcsSystem, WindowsHcsError> {
        let operation = unsafe { HcsCreateOperation(ptr::null_mut(), ptr::null_mut()) };
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

    fn run_system(
        system: HcsSystem,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<RunResult, WindowsHcsError> {
        let operation = unsafe { HcsCreateOperation(ptr::null_mut(), ptr::null_mut()) };
        if operation.is_null() {
            return Err(WindowsHcsError::ProviderUnavailable(
                "HcsCreateOperation returned null".into(),
            ));
        }
        let hr = unsafe { HcsStartComputeSystem(system, operation, ptr::null()) };
        let start_wait = wait_operation(operation, timeout);
        unsafe { HcsCloseOperation(operation) };
        if hr < 0 {
            return Err(WindowsHcsError::OperationFailed(format!(
                "HcsStartComputeSystem HRESULT 0x{hr:08x}"
            )));
        }
        start_wait?;

        let deadline = Instant::now() + timeout;
        loop {
            if cancellation.is_cancelled() {
                terminate(system, timeout);
                return Err(WindowsHcsError::Cancelled);
            }
            if Instant::now() >= deadline {
                terminate(system, timeout);
                return Err(WindowsHcsError::TimedOut);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let mut document = ptr::null_mut();
            let hr = unsafe {
                HcsWaitForComputeSystemExit(system, remaining.as_millis().min(u32::MAX as u128) as u32, &mut document)
            };
            free_document(document);
            if hr >= 0 {
                shutdown(system, timeout);
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
            return Err(WindowsHcsError::OperationFailed(format!(
                "HcsWaitForComputeSystemExit HRESULT 0x{hr:08x}"
            )));
        }
    }

    fn shutdown(system: HcsSystem, timeout: Duration) {
        let operation = unsafe { HcsCreateOperation(ptr::null_mut(), ptr::null_mut()) };
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
        let operation = unsafe { HcsCreateOperation(ptr::null_mut(), ptr::null_mut()) };
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
                "Process": {"CommandLine": spec.entrypoint.join(" ")},
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

    fn spec() -> WindowsHcsContainerSpec {
        WindowsHcsContainerSpec {
            container_id: "hivemind-test".into(),
            image_root: PathBuf::from("C:\\hivemind\\image"),
            entrypoint: vec!["runner.exe".into()],
            mounts: vec![WindowsHcsMountSpec {
                host_path: PathBuf::from("C:\\hivemind\\artifact"),
                container_path: "C:\\work\\source".into(),
                read_only: true,
            }],
            network_isolated: true,
            root_read_only: true,
            memory_bytes: 1024,
            cpu_millis: 1000,
            process_limit: 4,
            thread_limit: 4,
            scratch_bytes: 1024,
        }
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

//! Direct-process execution is an internal reference-oracle implementation.
//! External callers must use the production sandbox boundary and cannot build
//! arbitrary host commands:
//!
//! ```compile_fail
//! use general_compute_runtime::supervisor::{
//!     ReferenceCommandSpec, ReferenceProcessSupervisor,
//! };
//!
//! let _command = ReferenceCommandSpec::new("python3", [] as [&str; 0]);
//! let _supervisor = ReferenceProcessSupervisor::new();
//! ```

use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(5);
const DEFAULT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const DEFAULT_COMBINED_OUTPUT_LIMIT: usize = DEFAULT_OUTPUT_LIMIT * 2;
const DEFAULT_INPUT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}

/// Direct-process command used only by reference and lifecycle paths.
/// Production backends use [`crate::sandbox::ProductionSandboxLauncher`].
#[derive(Debug, Clone)]
pub(crate) struct ReferenceCommandSpec {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) timeout: Duration,
    pub(crate) output_limit: usize,
    pub(crate) combined_output_limit: usize,
    pub(crate) input_limit: usize,
}

impl ReferenceCommandSpec {
    pub(crate) fn new<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
            combined_output_limit: DEFAULT_COMBINED_OUTPUT_LIMIT,
            input_limit: DEFAULT_INPUT_LIMIT,
        }
    }

    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub(crate) fn with_output_limit(mut self, output_limit: usize) -> Self {
        self.output_limit = output_limit;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_combined_output_limit(mut self, combined_output_limit: usize) -> Self {
        self.combined_output_limit = combined_output_limit;
        self
    }

    pub(crate) fn with_input_limit(mut self, input_limit: usize) -> Self {
        self.input_limit = input_limit;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
    OutputLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub reaped: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug)]
pub enum SupervisorError {
    EmptyProgram,
    InputTooLarge,
    Spawn(io::Error),
    Wait(io::Error),
    Kill(io::Error),
    Input(io::Error),
    InputThread,
    Capture(io::Error),
    CaptureThread,
}

impl std::fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("supervisor program must not be empty"),
            Self::InputTooLarge => formatter.write_str("supervisor stdin exceeds configured limit"),
            Self::Spawn(_) => formatter.write_str("supervisor failed to spawn child"),
            Self::Wait(_) => formatter.write_str("supervisor failed to wait for child"),
            Self::Kill(_) => formatter.write_str("supervisor failed to kill child process tree"),
            Self::Input(_) => formatter.write_str("supervisor failed to write child stdin"),
            Self::InputThread => formatter.write_str("supervisor stdin writer thread panicked"),
            Self::Capture(_) => formatter.write_str("supervisor failed to capture child output"),
            Self::CaptureThread => formatter.write_str("supervisor output capture thread panicked"),
        }
    }
}

impl std::error::Error for SupervisorError {}

/// Direct-process supervisor for reference-oracle and lifecycle fixtures.
/// It is deliberately named so it cannot be confused with the production OCI
/// launch boundary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReferenceProcessSupervisor {
    poll_interval: Duration,
}

impl ReferenceProcessSupervisor {
    pub(crate) fn new() -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    #[cfg(test)]
    pub(crate) fn run(
        &self,
        command: ReferenceCommandSpec,
        cancellation: &Cancellation,
    ) -> Result<RunResult, SupervisorError> {
        self.run_with_stdin(command, &[], cancellation)
    }

    pub(crate) fn run_with_stdin(
        &self,
        command: ReferenceCommandSpec,
        input: &[u8],
        cancellation: &Cancellation,
    ) -> Result<RunResult, SupervisorError> {
        if command.program.trim().is_empty() {
            return Err(SupervisorError::EmptyProgram);
        }
        if input.len() > command.input_limit {
            return Err(SupervisorError::InputTooLarge);
        }

        let deadline = Instant::now() + command.timeout;
        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_group(&mut process);
        let mut child = process.spawn().map_err(SupervisorError::Spawn)?;
        let stdin = child.stdin.take().ok_or_else(|| {
            SupervisorError::Input(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stdin pipe missing",
            ))
        })?;
        let stdin_writer = spawn_stdin_writer(stdin, input.to_vec());
        let stdout = child.stdout.take().ok_or_else(|| {
            SupervisorError::Capture(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stdout pipe missing",
            ))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            SupervisorError::Capture(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stderr pipe missing",
            ))
        })?;
        let output_budget = Arc::new(OutputBudget::new(command.combined_output_limit));
        let stdout_reader =
            spawn_capture_reader(stdout, command.output_limit, Arc::clone(&output_budget));
        let stderr_reader =
            spawn_capture_reader(stderr, command.output_limit, Arc::clone(&output_budget));

        loop {
            if output_budget.exceeded() {
                return self.terminate_and_reap(
                    child,
                    stdin_writer,
                    stdout_reader,
                    stderr_reader,
                    RunStatus::OutputLimitExceeded,
                    output_budget,
                );
            }
            if let Some(status) = child.try_wait().map_err(SupervisorError::Wait)? {
                return self.result_after_reap(
                    if status.success() {
                        RunStatus::Completed
                    } else {
                        RunStatus::Failed
                    },
                    status.code(),
                    stdin_writer,
                    stdout_reader,
                    stderr_reader,
                    output_budget,
                );
            }

            if cancellation.is_cancelled() {
                return self.terminate_and_reap(
                    child,
                    stdin_writer,
                    stdout_reader,
                    stderr_reader,
                    RunStatus::Cancelled,
                    output_budget,
                );
            }
            if Instant::now() >= deadline {
                return self.terminate_and_reap(
                    child,
                    stdin_writer,
                    stdout_reader,
                    stderr_reader,
                    RunStatus::TimedOut,
                    output_budget,
                );
            }

            thread::sleep(self.poll_interval);
        }
    }

    fn terminate_and_reap(
        &self,
        mut child: Child,
        stdin_writer: JoinHandle<io::Result<()>>,
        stdout_reader: JoinHandle<io::Result<CapturedOutput>>,
        stderr_reader: JoinHandle<io::Result<CapturedOutput>>,
        status: RunStatus,
        output_budget: Arc<OutputBudget>,
    ) -> Result<RunResult, SupervisorError> {
        kill_process_tree(&mut child).map_err(SupervisorError::Kill)?;
        let exit_status = child.wait().map_err(SupervisorError::Wait)?;
        self.result_after_reap(
            status,
            exit_status.code(),
            stdin_writer,
            stdout_reader,
            stderr_reader,
            output_budget,
        )
    }

    fn result_after_reap(
        &self,
        status: RunStatus,
        exit_code: Option<i32>,
        stdin_writer: JoinHandle<io::Result<()>>,
        stdout_reader: JoinHandle<io::Result<CapturedOutput>>,
        stderr_reader: JoinHandle<io::Result<CapturedOutput>>,
        output_budget: Arc<OutputBudget>,
    ) -> Result<RunResult, SupervisorError> {
        join_stdin(stdin_writer)?;
        let stdout = join_capture(stdout_reader)?;
        let stderr = join_capture(stderr_reader)?;
        Ok(RunResult {
            status: if status == RunStatus::Completed && output_budget.exceeded() {
                RunStatus::OutputLimitExceeded
            } else {
                status
            },
            exit_code,
            reaped: true,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        })
    }
}

impl Default for ReferenceProcessSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

#[derive(Debug)]
struct OutputBudget {
    limit: usize,
    captured: AtomicUsize,
    exceeded: AtomicBool,
}

impl OutputBudget {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            captured: AtomicUsize::new(0),
            exceeded: AtomicBool::new(false),
        }
    }

    fn reserve(&self, requested: usize) -> usize {
        let mut current = self.captured.load(Ordering::Acquire);
        loop {
            let remaining = self.limit.saturating_sub(current);
            let granted = remaining.min(requested);
            let next = current.saturating_add(granted);
            match self.captured.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    if granted < requested {
                        self.exceeded.store(true, Ordering::Release);
                    }
                    return granted;
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn exceeded(&self) -> bool {
        self.exceeded.load(Ordering::Acquire)
    }
}

fn spawn_capture_reader<R>(
    mut reader: R,
    output_limit: usize,
    output_budget: Arc<OutputBudget>,
) -> JoinHandle<io::Result<CapturedOutput>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut captured = CapturedOutput {
            bytes: Vec::with_capacity(output_limit.min(8 * 1024)),
            truncated: false,
        };
        let mut buffer = [0u8; 8 * 1024];
        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            let remaining = output_limit.saturating_sub(captured.bytes.len());
            let globally_allowed = output_budget.reserve(bytes_read);
            let bytes_to_keep = remaining.min(globally_allowed);
            captured.bytes.extend_from_slice(&buffer[..bytes_to_keep]);
            if bytes_to_keep < bytes_read || globally_allowed < bytes_read || remaining < bytes_read
            {
                captured.truncated = true;
            }
        }
        Ok(captured)
    })
}

fn spawn_stdin_writer<W>(mut writer: W, input: Vec<u8>) -> JoinHandle<io::Result<()>>
where
    W: Write + Send + 'static,
{
    thread::spawn(move || {
        writer.write_all(&input)?;
        writer.flush()
    })
}

fn join_stdin(handle: JoinHandle<io::Result<()>>) -> Result<(), SupervisorError> {
    match handle.join() {
        Ok(result) => result.map_err(SupervisorError::Input),
        Err(_) => Err(SupervisorError::InputThread),
    }
}

fn join_capture(
    handle: JoinHandle<io::Result<CapturedOutput>>,
) -> Result<CapturedOutput, SupervisorError> {
    match handle.join() {
        Ok(result) => result.map_err(SupervisorError::Capture),
        Err(_) => Err(SupervisorError::CaptureThread),
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // SAFETY: the hook only changes the child process's own process group before
    // it executes the requested program. It does not access Rust-managed state.
    unsafe {
        command.pre_exec(|| {
            if setpgid(0, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_process_tree(child: &mut Child) -> io::Result<()> {
    let process_group = -(child.id() as i32);
    // SAFETY: process_group is the negative PID of the group created in the
    // pre-exec hook, so the signal is scoped to this runtime invocation.
    let result = unsafe { kill(process_group, SIGKILL) };
    if result == -1 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::NotFound {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn kill_process_tree(child: &mut Child) -> io::Result<()> {
    let pid = child.id().to_string();
    let status = Command::new("taskkill.exe")
        .args(["/PID", &pid, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() || child.try_wait()?.is_some() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "taskkill.exe exited with {status}"
        )))
    }
}

#[cfg(unix)]
const SIGKILL: i32 = 9;

#[cfg(unix)]
unsafe extern "C" {
    fn setpgid(pid: i32, process_group: i32) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
}

#[cfg(test)]
mod tests;

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(5);
const DEFAULT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

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

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub output_limit: usize,
}

impl CommandSpec {
    pub fn new<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_output_limit(mut self, output_limit: usize) -> Self {
        self.output_limit = output_limit;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
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
    Spawn(io::Error),
    Wait(io::Error),
    Capture(io::Error),
    CaptureThread,
}

impl std::fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("supervisor program must not be empty"),
            Self::Spawn(_) => formatter.write_str("supervisor failed to spawn child"),
            Self::Wait(_) => formatter.write_str("supervisor failed to wait for child"),
            Self::Capture(_) => formatter.write_str("supervisor failed to capture child output"),
            Self::CaptureThread => formatter.write_str("supervisor output capture thread panicked"),
        }
    }
}

impl std::error::Error for SupervisorError {}

#[derive(Debug, Clone, Copy)]
pub struct Supervisor {
    poll_interval: Duration,
}

impl Supervisor {
    pub fn new() -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    pub fn run(&self, command: CommandSpec, cancellation: &Cancellation) -> Result<RunResult, SupervisorError> {
        if command.program.trim().is_empty() {
            return Err(SupervisorError::EmptyProgram);
        }

        let deadline = Instant::now() + command.timeout;
        let mut child = Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(SupervisorError::Spawn)?;
        let stdout = child.stdout.take().ok_or_else(|| {
            SupervisorError::Capture(io::Error::new(io::ErrorKind::BrokenPipe, "stdout pipe missing"))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            SupervisorError::Capture(io::Error::new(io::ErrorKind::BrokenPipe, "stderr pipe missing"))
        })?;
        let stdout_reader = spawn_capture_reader(stdout, command.output_limit);
        let stderr_reader = spawn_capture_reader(stderr, command.output_limit);

        loop {
            if let Some(status) = child.try_wait().map_err(SupervisorError::Wait)? {
                return self.result_after_reap(
                    if status.success() {
                        RunStatus::Completed
                    } else {
                        RunStatus::Failed
                    },
                    status.code(),
                    stdout_reader,
                    stderr_reader,
                );
            }

            if cancellation.is_cancelled() {
                return self.terminate_and_reap(child, stdout_reader, stderr_reader, RunStatus::Cancelled);
            }
            if Instant::now() >= deadline {
                return self.terminate_and_reap(child, stdout_reader, stderr_reader, RunStatus::TimedOut);
            }

            thread::sleep(self.poll_interval);
        }
    }

    fn terminate_and_reap(
        &self,
        mut child: Child,
        stdout_reader: JoinHandle<io::Result<CapturedOutput>>,
        stderr_reader: JoinHandle<io::Result<CapturedOutput>>,
        status: RunStatus,
    ) -> Result<RunResult, SupervisorError> {
        let _ = child.kill();
        let exit_status = child.wait().map_err(SupervisorError::Wait)?;
        self.result_after_reap(status, exit_status.code(), stdout_reader, stderr_reader)
    }

    fn result_after_reap(
        &self,
        status: RunStatus,
        exit_code: Option<i32>,
        stdout_reader: JoinHandle<io::Result<CapturedOutput>>,
        stderr_reader: JoinHandle<io::Result<CapturedOutput>>,
    ) -> Result<RunResult, SupervisorError> {
        let stdout = join_capture(stdout_reader)?;
        let stderr = join_capture(stderr_reader)?;
        Ok(RunResult {
            status,
            exit_code,
            reaped: true,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        })
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn spawn_capture_reader<R>(mut reader: R, output_limit: usize) -> JoinHandle<io::Result<CapturedOutput>>
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
            let bytes_to_keep = remaining.min(bytes_read);
            captured.bytes.extend_from_slice(&buffer[..bytes_to_keep]);
            if bytes_to_keep < bytes_read {
                captured.truncated = true;
            }
        }
        Ok(captured)
    })
}

fn join_capture(handle: JoinHandle<io::Result<CapturedOutput>>) -> Result<CapturedOutput, SupervisorError> {
    match handle.join() {
        Ok(result) => result.map_err(SupervisorError::Capture),
        Err(_) => Err(SupervisorError::CaptureThread),
    }
}

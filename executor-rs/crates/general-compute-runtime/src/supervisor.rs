use std::io;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(5);

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
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
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
}

#[derive(Debug)]
pub enum SupervisorError {
    EmptyProgram,
    Spawn(io::Error),
    Wait(io::Error),
}

impl std::fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("supervisor program must not be empty"),
            Self::Spawn(_) => formatter.write_str("supervisor failed to spawn child"),
            Self::Wait(_) => formatter.write_str("supervisor failed to wait for child"),
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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(SupervisorError::Spawn)?;

        loop {
            if let Some(status) = child.try_wait().map_err(SupervisorError::Wait)? {
                return Ok(RunResult {
                    status: if status.success() {
                        RunStatus::Completed
                    } else {
                        RunStatus::Failed
                    },
                    exit_code: status.code(),
                    reaped: true,
                });
            }

            if cancellation.is_cancelled() {
                return self.terminate_and_reap(child, RunStatus::Cancelled);
            }
            if Instant::now() >= deadline {
                return self.terminate_and_reap(child, RunStatus::TimedOut);
            }

            thread::sleep(self.poll_interval);
        }
    }

    fn terminate_and_reap(&self, mut child: Child, status: RunStatus) -> Result<RunResult, SupervisorError> {
        let _ = child.kill();
        let exit_status = child.wait().map_err(SupervisorError::Wait)?;
        Ok(RunResult {
            status,
            exit_code: exit_status.code(),
            reaped: true,
        })
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

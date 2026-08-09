use hivemind_managed_proof::ExecutionClaim;
use hivemind_proto::ManagedProofEnvelope;
use prost::Message;
#[cfg(test)]
use std::ffi::OsStr;
use std::ffi::OsString;
#[cfg(test)]
use std::path::PathBuf;
use std::process::Stdio;
#[cfg(test)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::Semaphore;
use tokio::time::{timeout_at, Instant};

const MAX_ENVELOPE_BYTES: usize = hivemind_proto::MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES;
const MAX_STDOUT_BYTES: usize = 4 * 1024;
const MEMORY_LIMIT_BYTES: usize = 128 * 1024 * 1024;
const MAX_WAITERS: usize = 8;
const VERIFICATION_DEADLINE: Duration = Duration::from_secs(1);
const VERIFIER_ARGUMENT: &str = "--verify-managed-proof";

static GLOBAL_VERIFIER: LazyLock<ManagedProofVerifier> =
    LazyLock::new(ManagedProofVerifier::production);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum ManagedProofVerifierError {
    #[error("managed proof verifier input exceeds the size limit")]
    InputTooLarge,
    #[error("managed proof verifier queue is full")]
    QueueFull,
    #[error("managed proof verifier queue deadline exceeded")]
    QueueDeadlineExceeded,
    #[error("managed proof verifier deadline exceeded")]
    DeadlineExceeded,
    #[error("managed proof verifier could not be started")]
    StartFailed,
    #[error("managed proof verifier resource limit could not be applied")]
    ResourceLimitFailed,
    #[error("managed proof verifier communication failed")]
    CommunicationFailed,
    #[error("managed proof verifier process failed")]
    VerifierFailed,
    #[error("managed proof verifier output exceeds the size limit")]
    OutputTooLarge,
    #[error("managed proof verifier returned an invalid claim")]
    InvalidClaim,
}

#[derive(Clone)]
enum Executable {
    Current,
    #[cfg(test)]
    Path(PathBuf),
}

#[derive(Clone)]
struct CommandSpec {
    executable: Executable,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    #[cfg(test)]
    spawned_pid: Option<Arc<AtomicU32>>,
}

impl CommandSpec {
    fn managed_proof_verifier() -> Self {
        Self {
            executable: Executable::Current,
            arguments: vec![OsString::from(VERIFIER_ARGUMENT)],
            environment: Vec::new(),
            #[cfg(test)]
            spawned_pid: None,
        }
    }

    #[cfg(test)]
    fn new(executable: PathBuf) -> Self {
        Self {
            executable: Executable::Path(executable),
            arguments: Vec::new(),
            environment: Vec::new(),
            spawned_pid: None,
        }
    }

    #[cfg(test)]
    fn arg(mut self, argument: impl AsRef<OsStr>) -> Self {
        self.arguments.push(argument.as_ref().to_owned());
        self
    }

    #[cfg(test)]
    fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.environment
            .push((key.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    #[cfg(test)]
    fn observe_spawned_pid(mut self, spawned_pid: Arc<AtomicU32>) -> Self {
        self.spawned_pid = Some(spawned_pid);
        self
    }

    fn command(&self) -> Result<Command, ManagedProofVerifierError> {
        let executable = match &self.executable {
            Executable::Current => {
                std::env::current_exe().map_err(|_| ManagedProofVerifierError::StartFailed)?
            }
            #[cfg(test)]
            Executable::Path(path) => path.clone(),
        };
        let mut command = Command::new(executable);
        command
            .args(&self.arguments)
            .envs(self.environment.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        configure_process_limit(&mut command);
        Ok(command)
    }
}

pub(crate) struct ManagedProofVerifier {
    command: CommandSpec,
    deadline: Duration,
    admission: Arc<Semaphore>,
    execution: Arc<Semaphore>,
}

impl ManagedProofVerifier {
    fn production() -> Self {
        Self::with_limits(
            CommandSpec::managed_proof_verifier(),
            VERIFICATION_DEADLINE,
            MAX_WAITERS,
        )
    }

    fn with_limits(command: CommandSpec, deadline: Duration, max_waiters: usize) -> Self {
        let admission_limit = max_waiters
            .checked_add(1)
            .expect("managed proof verifier admission limit fits usize");
        Self {
            command,
            deadline,
            admission: Arc::new(Semaphore::new(admission_limit)),
            execution: Arc::new(Semaphore::new(1)),
        }
    }

    async fn verify(
        &self,
        envelope: &ManagedProofEnvelope,
    ) -> Result<ExecutionClaim, ManagedProofVerifierError> {
        let deadline = Instant::now() + self.deadline;
        if envelope.encoded_len() > MAX_ENVELOPE_BYTES {
            return Err(ManagedProofVerifierError::InputTooLarge);
        }
        let input = envelope.encode_to_vec();

        let _admission = self
            .admission
            .try_acquire()
            .map_err(|_| ManagedProofVerifierError::QueueFull)?;
        let _execution = timeout_at(deadline, self.execution.acquire())
            .await
            .map_err(|_| ManagedProofVerifierError::QueueDeadlineExceeded)?
            .map_err(|_| ManagedProofVerifierError::QueueFull)?;
        if Instant::now() >= deadline {
            return Err(ManagedProofVerifierError::QueueDeadlineExceeded);
        }

        let mut command = self.command.command()?;
        let mut child = command
            .spawn()
            .map_err(|_| ManagedProofVerifierError::StartFailed)?;
        #[cfg(test)]
        if let (Some(spawned_pid), Some(pid)) = (&self.command.spawned_pid, child.id()) {
            spawned_pid.store(pid, Ordering::SeqCst);
        }
        let _process_limit = match apply_process_limit(&child) {
            Ok(limit) => limit,
            Err(()) => {
                kill_and_reap(&mut child).await;
                return Err(ManagedProofVerifierError::ResourceLimitFailed);
            }
        };

        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                kill_and_reap(&mut child).await;
                return Err(ManagedProofVerifierError::CommunicationFailed);
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                kill_and_reap(&mut child).await;
                return Err(ManagedProofVerifierError::CommunicationFailed);
            }
        };

        let communication = timeout_at(
            deadline,
            communicate_with_child(&mut child, stdin, stdout, &input),
        )
        .await;
        let (status, output) = match communication {
            Err(_) => {
                kill_and_reap(&mut child).await;
                return Err(ManagedProofVerifierError::DeadlineExceeded);
            }
            Ok(Err(error)) => {
                kill_and_reap(&mut child).await;
                return Err(error);
            }
            Ok(Ok(result)) => result,
        };
        if !status.success() {
            return Err(ManagedProofVerifierError::VerifierFailed);
        }
        let claim =
            serde_json::from_slice(&output).map_err(|_| ManagedProofVerifierError::InvalidClaim)?;
        if Instant::now() > deadline {
            return Err(ManagedProofVerifierError::DeadlineExceeded);
        }
        Ok(claim)
    }
}

pub(crate) async fn verify_managed_proof(
    envelope: &ManagedProofEnvelope,
) -> Result<ExecutionClaim, ManagedProofVerifierError> {
    GLOBAL_VERIFIER.verify(envelope).await
}

async fn communicate_with_child(
    child: &mut Child,
    mut stdin: tokio::process::ChildStdin,
    stdout: ChildStdout,
    input: &[u8],
) -> Result<(std::process::ExitStatus, Vec<u8>), ManagedProofVerifierError> {
    let write_input = async move {
        stdin
            .write_all(input)
            .await
            .map_err(|_| ManagedProofVerifierError::CommunicationFailed)?;
        stdin
            .shutdown()
            .await
            .map_err(|_| ManagedProofVerifierError::CommunicationFailed)
    };
    let read_output = read_limited_stdout(stdout);
    let wait = async {
        child
            .wait()
            .await
            .map_err(|_| ManagedProofVerifierError::CommunicationFailed)
    };
    let (_, output, status) = tokio::try_join!(write_input, read_output, wait)?;
    Ok((status, output))
}

async fn read_limited_stdout(stdout: ChildStdout) -> Result<Vec<u8>, ManagedProofVerifierError> {
    let mut output = Vec::with_capacity(MAX_STDOUT_BYTES + 1);
    stdout
        .take(u64::try_from(MAX_STDOUT_BYTES + 1).expect("stdout limit fits u64"))
        .read_to_end(&mut output)
        .await
        .map_err(|_| ManagedProofVerifierError::CommunicationFailed)?;
    if output.len() > MAX_STDOUT_BYTES {
        return Err(ManagedProofVerifierError::OutputTooLarge);
    }
    Ok(output)
}

async fn kill_and_reap(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

#[cfg(unix)]
fn configure_process_limit(command: &mut Command) {
    // SAFETY: the closure performs only getrlimit/setrlimit and constructs an OS error before exec.
    unsafe {
        command.pre_exec(|| {
            let mut inherited = std::mem::zeroed::<libc::rlimit>();
            if libc::getrlimit(libc::RLIMIT_AS, &mut inherited) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let requested = MEMORY_LIMIT_BYTES as libc::rlim_t;
            let hard_limit = requested.min(inherited.rlim_max);
            let limit = libc::rlimit {
                rlim_cur: hard_limit,
                rlim_max: hard_limit,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(unix)]
struct ProcessLimitGuard;

#[cfg(unix)]
fn apply_process_limit(_: &Child) -> Result<ProcessLimitGuard, ()> {
    Ok(ProcessLimitGuard)
}

#[cfg(windows)]
fn configure_process_limit(command: &mut Command) {
    use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

    command.creation_flags(CREATE_SUSPENDED);
}

#[cfg(windows)]
struct ProcessLimitGuard {
    _job: WindowsHandle,
}

#[cfg(windows)]
fn apply_process_limit(child: &Child) -> Result<ProcessLimitGuard, ()> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };

    // SAFETY: each Win32 handle is validated and owned by an RAII guard; all struct sizes match
    // the API contract, and the suspended child is resumed only after assignment succeeds.
    unsafe {
        let job = WindowsHandle::new(CreateJobObjectW(std::ptr::null(), std::ptr::null()))?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        limits.ProcessMemoryLimit = MEMORY_LIMIT_BYTES;
        let limits_size =
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).map_err(|_| ())?;
        if SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast::<c_void>(),
            limits_size,
        ) == 0
        {
            return Err(());
        }
        let process = child.raw_handle().ok_or(())?.cast::<c_void>();
        if AssignProcessToJobObject(job.0, process) == 0 {
            return Err(());
        }
        resume_suspended_child(child.id().ok_or(())?)?;
        Ok(ProcessLimitGuard { _job: job })
    }
}

#[cfg(windows)]
struct WindowsHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl WindowsHandle {
    fn new(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<Self, ()> {
        if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            Err(())
        } else {
            Ok(Self(handle))
        }
    }
}

#[cfg(windows)]
// SAFETY: this type uniquely owns a kernel handle, which may be closed from any thread.
unsafe impl Send for WindowsHandle {}

#[cfg(windows)]
impl Drop for WindowsHandle {
    fn drop(&mut self) {
        // SAFETY: this guard owns a valid handle and closes it exactly once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn resume_suspended_child(pid: u32) -> Result<(), ()> {
    use std::mem::size_of;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // SAFETY: the snapshot and thread handles are guarded and all API buffers have the required
    // size. Only the initial thread belonging to the suspended child is resumed.
    unsafe {
        let snapshot = WindowsHandle::new(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0))?;
        let mut entry = THREADENTRY32 {
            dwSize: u32::try_from(size_of::<THREADENTRY32>()).map_err(|_| ())?,
            ..THREADENTRY32::default()
        };
        if Thread32First(snapshot.0, &mut entry) == 0 {
            return Err(());
        }
        loop {
            if entry.th32OwnerProcessID == pid {
                let thread =
                    WindowsHandle::new(OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID))?;
                if ResumeThread(thread.0) == u32::MAX {
                    return Err(());
                }
                return Ok(());
            }
            if Thread32Next(snapshot.0, &mut entry) == 0 {
                return Err(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandSpec, ManagedProofVerifier, ManagedProofVerifierError, MAX_ENVELOPE_BYTES,
        MAX_WAITERS,
    };
    use hivemind_managed_proof::{ExecutionClaim, ExecutionMetrics};
    use hivemind_proto::ManagedProofEnvelope;
    use prost::Message;
    use serde::Deserialize;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::{timeout, Instant};

    const FAKE_CHILD_SOURCE: &str = r#"
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process;
use std::thread;
use std::time::Duration;

fn write_claim() {
    let claim = env::var("HIVEMIND_FAKE_CLAIM").expect("fake claim");
    std::io::stdout().write_all(claim.as_bytes()).expect("write claim");
}

fn main() {
    let mode = env::args().nth(1).expect("fake mode");
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).expect("read stdin");
    if let Some(path) = env::var_os("HIVEMIND_FAKE_STDIN_PATH") {
        fs::write(path, &input).expect("record stdin");
    }
    match mode.as_str() {
        "success" => write_claim(),
        "nonzero" => process::exit(23),
        "malformed" => std::io::stdout().write_all(b"not-json").expect("write malformed"),
        "oversized" => {
            let output = vec![b'x'; 4097];
            std::io::stdout().write_all(&output).expect("write oversized");
        }
        "gate" => {
            let starts = env::var_os("HIVEMIND_FAKE_STARTS_PATH").expect("starts path");
            writeln!(
                OpenOptions::new().create(true).append(true).open(starts).expect("open starts"),
                "{}",
                process::id(),
            )
            .expect("record start");
            let release = env::var_os("HIVEMIND_FAKE_RELEASE_PATH").expect("release path");
            while !Path::new(&release).exists() {
                thread::sleep(Duration::from_millis(5));
            }
            write_claim();
        }
        "timeout" => {
            thread::sleep(Duration::from_secs(60));
        }
        _ => process::exit(24),
    }
}
"#;

    struct FakeProgram {
        directory: PathBuf,
        executable: PathBuf,
    }

    impl FakeProgram {
        fn compile() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "hivemind-managed-proof-verifier-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            fs::create_dir(&directory).expect("create fake child directory");
            let source = directory.join("fake_verifier.rs");
            fs::write(&source, FAKE_CHILD_SOURCE).expect("write fake child source");
            let executable =
                directory.join(format!("fake_verifier{}", std::env::consts::EXE_SUFFIX));
            let output =
                Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc")))
                    .arg(&source)
                    .arg("-o")
                    .arg(&executable)
                    .output()
                    .expect("run rustc for fake child");
            assert!(
                output.status.success(),
                "fake child compilation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            Self {
                directory,
                executable,
            }
        }

        fn command(&self, mode: &str, claim: &ExecutionClaim) -> CommandSpec {
            CommandSpec::new(self.executable.clone()).arg(mode).env(
                "HIVEMIND_FAKE_CLAIM",
                serde_json::to_string(claim).expect("serialize fake claim"),
            )
        }
    }

    impl Drop for FakeProgram {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn envelope() -> ManagedProofEnvelope {
        ManagedProofEnvelope {
            proof_scheme: "test".to_owned(),
            image_id: vec![1, 2, 3],
            journal: b"journal".to_vec(),
            receipt_json: b"receipt".to_vec(),
        }
    }

    fn claim() -> ExecutionClaim {
        ExecutionClaim::new(
            "task-a",
            b"source",
            b"input",
            b"output",
            100,
            ExecutionMetrics {
                usage_units: 42,
                executed_ops: 30,
                function_calls: 2,
                loop_iterations: 3,
                max_call_depth: 1,
            },
        )
        .expect("valid claim")
    }

    #[derive(Deserialize)]
    struct ProofFixture {
        proof_scheme: String,
        image_id: [u32; 8],
        journal: Vec<u8>,
        receipt: serde_json::Value,
    }

    fn real_envelope_fixture() -> ManagedProofEnvelope {
        let fixture: ProofFixture = serde_json::from_slice(include_bytes!(
            "../../managed-proof/tests/fixtures/risc0-managed-proof-v1.json"
        ))
        .expect("pinned proof fixture parses");
        ManagedProofEnvelope {
            proof_scheme: fixture.proof_scheme,
            image_id: fixture.image_id.to_vec(),
            journal: fixture.journal,
            receipt_json: serde_json::to_vec(&fixture.receipt).expect("receipt serializes"),
        }
    }

    fn verifier(command: CommandSpec) -> ManagedProofVerifier {
        ManagedProofVerifier::with_limits(command, Duration::from_secs(5), MAX_WAITERS)
    }

    async fn wait_until(path: &Path, predicate: impl Fn(&str) -> bool) {
        timeout(Duration::from_secs(2), async {
            loop {
                let contents = fs::read_to_string(path).unwrap_or_default();
                if predicate(&contents) {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("condition became true");
    }

    #[tokio::test]
    async fn rejects_oversized_envelope_before_spawning() {
        let command = CommandSpec::new(PathBuf::from("executable-that-must-not-be-spawned"));
        let verifier = verifier(command);
        let mut oversized = envelope();
        oversized.receipt_json = vec![0; MAX_ENVELOPE_BYTES];

        let error = verifier.verify(&oversized).await.expect_err("must reject");

        assert_eq!(error, ManagedProofVerifierError::InputTooLarge);
        assert_eq!(
            error.to_string(),
            "managed proof verifier input exceeds the size limit"
        );
    }

    #[tokio::test]
    async fn sends_protobuf_and_parses_claim_json() {
        let fake = FakeProgram::compile();
        let stdin_path = fake.directory.join("stdin.pb");
        let expected_claim = claim();
        let command = fake
            .command("success", &expected_claim)
            .env("HIVEMIND_FAKE_STDIN_PATH", stdin_path.as_os_str());
        let input = envelope();

        let actual_claim = verifier(command).verify(&input).await.expect("valid claim");

        assert_eq!(actual_claim, expected_claim);
        let wire = fs::read(stdin_path).expect("recorded protobuf input");
        assert_eq!(
            ManagedProofEnvelope::decode(wire.as_slice()).expect("decode protobuf"),
            input
        );
    }

    #[tokio::test]
    async fn rejects_nonzero_and_malformed_and_oversized_output() {
        let fake = FakeProgram::compile();
        let expected_claim = claim();
        for (mode, expected_error) in [
            ("nonzero", ManagedProofVerifierError::VerifierFailed),
            ("malformed", ManagedProofVerifierError::InvalidClaim),
            ("oversized", ManagedProofVerifierError::OutputTooLarge),
        ] {
            let error = verifier(fake.command(mode, &expected_claim))
                .verify(&envelope())
                .await
                .expect_err("must fail closed");
            assert_eq!(error, expected_error, "mode {mode}");
        }
    }

    #[tokio::test]
    async fn queue_wait_deadline_is_distinct_from_child_deadline() {
        let verifier = ManagedProofVerifier::with_limits(
            CommandSpec::new(PathBuf::from("executable-that-must-not-be-spawned")),
            Duration::from_millis(20),
            MAX_WAITERS,
        );
        let _running = verifier
            .execution
            .acquire()
            .await
            .expect("execution semaphore is open");

        let error = verifier
            .verify(&envelope())
            .await
            .expect_err("queue wait must time out");

        assert_eq!(error, ManagedProofVerifierError::QueueDeadlineExceeded);
    }

    #[tokio::test]
    async fn enforces_one_running_child_and_rejects_the_tenth_admission() {
        let fake = FakeProgram::compile();
        let starts_path = fake.directory.join("starts");
        let release_path = fake.directory.join("release");
        let command = fake
            .command("gate", &claim())
            .env("HIVEMIND_FAKE_STARTS_PATH", starts_path.as_os_str())
            .env("HIVEMIND_FAKE_RELEASE_PATH", release_path.as_os_str());
        let verifier = Arc::new(verifier(command));
        let input = Arc::new(envelope());
        let mut calls = Vec::new();
        for _ in 0..=MAX_WAITERS {
            let verifier = Arc::clone(&verifier);
            let input = Arc::clone(&input);
            calls.push(tokio::spawn(async move { verifier.verify(&input).await }));
        }
        timeout(Duration::from_secs(2), async {
            while verifier.admission.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("all admission slots filled");
        wait_until(&starts_path, |contents| !contents.is_empty()).await;
        assert_eq!(
            fs::read_to_string(&starts_path)
                .expect("read starts")
                .lines()
                .count(),
            1,
            "only one child may run"
        );

        let overflow_started = Instant::now();
        let overflow = verifier
            .verify(&input)
            .await
            .expect_err("tenth admission must fail");
        assert_eq!(overflow, ManagedProofVerifierError::QueueFull);
        assert!(overflow_started.elapsed() < Duration::from_millis(100));

        fs::write(&release_path, b"release").expect("release children");
        for call in calls {
            timeout(Duration::from_secs(3), call)
                .await
                .expect("queued call completed")
                .expect("task joined")
                .expect("claim parsed");
        }
    }

    #[tokio::test]
    async fn timeout_kills_and_reaps_child() {
        let fake = FakeProgram::compile();
        let spawned_pid = Arc::new(AtomicU32::new(0));
        let command = fake
            .command("timeout", &claim())
            .observe_spawned_pid(Arc::clone(&spawned_pid));
        let verifier =
            ManagedProofVerifier::with_limits(command, Duration::from_millis(750), MAX_WAITERS);

        let error = verifier
            .verify(&envelope())
            .await
            .expect_err("must time out");

        assert_eq!(error, ManagedProofVerifierError::DeadlineExceeded);
        let pid = spawned_pid.load(Ordering::SeqCst);
        assert_ne!(pid, 0, "parent recorded spawned child pid");
        assert!(!process_is_alive(pid), "timed-out child {pid} survived");
    }

    #[tokio::test]
    #[ignore = "requires HIVEMIND_REAL_VERIFIER_EXE pointing at a nodepool-capable binary"]
    async fn production_binary_verifies_real_fixture_under_process_limits() {
        let executable = std::env::var_os("HIVEMIND_REAL_VERIFIER_EXE")
            .expect("HIVEMIND_REAL_VERIFIER_EXE is required for this acceptance test");
        let verifier = ManagedProofVerifier::with_limits(
            CommandSpec::new(PathBuf::from(executable)).arg("--verify-managed-proof"),
            super::VERIFICATION_DEADLINE,
            MAX_WAITERS,
        );

        let verified = verifier
            .verify(&real_envelope_fixture())
            .await
            .expect("real proof verifies in the isolated production process");

        assert_eq!(verified.task_id, "task-zk-golden");
        assert_eq!(verified.usage_units, 29);
    }

    #[cfg(unix)]
    fn process_is_alive(pid: u32) -> bool {
        let pid = i32::try_from(pid).expect("pid fits i32");
        // SAFETY: kill with signal zero only probes the kernel process table.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(windows)]
    fn process_is_alive(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: the returned process handle is checked, queried, and closed exactly once.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut exit_code = 0;
            let alive = GetExitCodeProcess(process, &mut exit_code) != 0
                && exit_code == STILL_ACTIVE as u32;
            CloseHandle(process);
            alive
        }
    }
}

use anyhow::{bail, Context, Result};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: u64 = 64 * 1024;

pub(crate) struct CancellationGuard(pub Arc<AtomicBool>);
impl Drop for CancellationGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) fn run(args: &[String], deadline: Duration, cancelled: &AtomicBool) -> Result<Vec<u8>> {
    let mut command = Command::new(std::env::current_exe().context("locate benchmark executable")?);
    command
        .arg(super::HELPER_MODE)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "HOME",
        "XDG_RUNTIME_DIR",
        "LD_LIBRARY_PATH",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }
    let started = Instant::now();
    let mut child = command.spawn().context("start fixed benchmark helper")?;
    let group = match ProcessGroup::new(&child) {
        Ok(group) => group,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let stdout = child
        .stdout
        .take()
        .context("benchmark stdout pipe unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("benchmark stderr pipe unavailable")?;
    let out = std::thread::spawn(move || read_bounded(stdout, OUTPUT_LIMIT));
    let err = std::thread::spawn(move || read_bounded(stderr, 4096));
    let outcome = wait(&mut child, deadline, started, cancelled);
    drop(group);
    if outcome.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let bytes = out
        .join()
        .map_err(|_| anyhow::anyhow!("benchmark output reader panicked"))??;
    let _ = err.join();
    let status = outcome?;
    if !status.success() {
        bail!("benchmark helper failed ({status})");
    }
    Ok(bytes)
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("benchmark output exceeded bound");
    }
    Ok(bytes)
}

fn wait(
    child: &mut Child,
    deadline: Duration,
    started: Instant,
    cancelled: &AtomicBool,
) -> Result<std::process::ExitStatus> {
    loop {
        if cancelled.load(Ordering::Acquire) {
            bail!("benchmark cancelled");
        }
        if started.elapsed() >= deadline {
            bail!("benchmark timed out");
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
struct ProcessGroup {
    pid: i32,
}
#[cfg(unix)]
impl ProcessGroup {
    fn new(child: &Child) -> Result<Self> {
        Ok(Self {
            pid: i32::try_from(child.id())?,
        })
    }
    fn terminate(&self) {
        unsafe {
            libc::kill(-self.pid, libc::SIGKILL);
        }
    }
}
#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(windows)]
struct ProcessGroup {
    job: std::os::windows::io::OwnedHandle,
}
#[cfg(windows)]
impl ProcessGroup {
    fn new(child: &Child) -> Result<Self> {
        use std::mem::{size_of, zeroed};
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        };
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows_sys::Win32::System::Threading::{
            OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
        };
        unsafe {
            let raw_job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if raw_job.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let job = OwnedHandle::from_raw_handle(raw_job);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                raw_job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
                || AssignProcessToJobObject(raw_job, child.as_raw_handle()) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let group = Self { job };
            let raw_snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if raw_snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error().into());
            }
            let snapshot = OwnedHandle::from_raw_handle(raw_snapshot);
            let mut entry: THREADENTRY32 = zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut found = Thread32First(snapshot.as_raw_handle(), &mut entry);
            while found != 0 {
                if entry.th32OwnerProcessID == child.id() {
                    let raw_thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if raw_thread.is_null() {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    let thread = OwnedHandle::from_raw_handle(raw_thread);
                    if ResumeThread(thread.as_raw_handle()) == u32::MAX {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    return Ok(group);
                }
                found = Thread32Next(snapshot.as_raw_handle(), &mut entry);
            }
            bail!("suspended benchmark primary thread unavailable");
        }
    }
    fn terminate(&self) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job.as_raw_handle(), 1);
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(not(any(unix, windows)))]
struct ProcessGroup;
#[cfg(not(any(unix, windows)))]
impl ProcessGroup {
    fn new(_child: &Child) -> Result<Self> {
        bail!("benchmark process isolation unsupported")
    }
    fn terminate(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_is_bounded_and_preserves_payload() {
        assert_eq!(read_bounded(&b"abc"[..], 3).unwrap(), b"abc");
        assert!(read_bounded(&b"abcd"[..], 3).is_err());
    }
    #[test]
    fn dropping_guard_requests_cancellation() {
        let signal = Arc::new(AtomicBool::new(false));
        drop(CancellationGuard(signal.clone()));
        assert!(signal.load(Ordering::Acquire));
    }
}

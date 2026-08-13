use super::{Cancellation, ReferenceCommandSpec, ReferenceProcessSupervisor, RunStatus};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn command_that_finishes() -> ReferenceCommandSpec {
    if cfg!(windows) {
        ReferenceCommandSpec::new("cmd.exe", ["/C", "exit 0"])
    } else {
        ReferenceCommandSpec::new("sh", ["-c", "exit 0"])
    }
}

fn command_that_sleeps() -> ReferenceCommandSpec {
    if cfg!(windows) {
        ReferenceCommandSpec::new(
            "powershell.exe",
            ["-NoProfile", "-Command", "Start-Sleep -Seconds 5"],
        )
    } else {
        ReferenceCommandSpec::new("sh", ["-c", "sleep 5"])
    }
}

fn command_that_writes_large_output() -> ReferenceCommandSpec {
    if cfg!(windows) {
        ReferenceCommandSpec::new(
            "powershell.exe",
            [
                "-NoProfile",
                "-Command",
                "$s='x'*65536; [Console]::Out.Write($s); [Console]::Error.Write($s)",
            ],
        )
    } else {
        ReferenceCommandSpec::new(
            "sh",
            [
                "-c",
                "head -c 65536 /dev/zero | tr '\\0' x; head -c 65536 /dev/zero | tr '\\0' e >&2",
            ],
        )
    }
}

fn command_that_writes_stdout_only() -> ReferenceCommandSpec {
    if cfg!(windows) {
        ReferenceCommandSpec::new(
            "powershell.exe",
            [
                "-NoProfile",
                "-Command",
                "$s='x'*65536; [Console]::Out.Write($s)",
            ],
        )
    } else {
        ReferenceCommandSpec::new("sh", ["-c", "head -c 65536 /dev/zero | tr '\\0' x"])
    }
}

fn descendant_marker_paths() -> (PathBuf, PathBuf) {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos()
    );
    (
        std::env::temp_dir().join(format!(
            "hivemind-supervisor-descendant-start-{suffix}.marker"
        )),
        std::env::temp_dir().join(format!(
            "hivemind-supervisor-descendant-final-{suffix}.marker"
        )),
    )
}

fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    path.exists()
}

#[cfg(unix)]
fn command_that_spawns_descendant(start: &Path, final_marker: &Path) -> ReferenceCommandSpec {
    let start = start.to_string_lossy();
    let final_marker = final_marker.to_string_lossy();
    ReferenceCommandSpec::new(
        "sh",
        [
            "-c",
            &format!(
                "(printf started > '{}'; sleep 1; printf survived > '{}') & wait",
                start.replace('\'', "'\\''"),
                final_marker.replace('\'', "'\\''")
            ),
        ],
    )
}

#[cfg(windows)]
fn command_that_spawns_descendant(start: &Path, final_marker: &Path) -> ReferenceCommandSpec {
    let start = start.to_string_lossy().replace('\'', "''");
    let final_marker = final_marker.to_string_lossy().replace('\'', "''");
    let child_script = format!(
        "Set-Content -LiteralPath '{start}' -Value started; Start-Sleep -Milliseconds 1500; Set-Content -LiteralPath '{final_marker}' -Value survived"
    );
    let parent_script = format!(
        "Set-Content -LiteralPath '{start}' -Value started; $childScript='{child}'; $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childScript)); $child=Start-Process powershell.exe -ArgumentList @('-NoProfile','-EncodedCommand',$encoded) -PassThru; Start-Sleep -Seconds 5",
        start = start,
        child = child_script.replace('\'', "''")
    );
    ReferenceCommandSpec::new("powershell.exe", ["-NoProfile", "-Command", &parent_script])
}

#[test]
fn supervisor_reports_completed_child_after_waiting_for_reap() {
    let cancellation = Cancellation::new();
    let result = ReferenceProcessSupervisor::new()
        .run(command_that_finishes(), &cancellation)
        .expect("child should execute");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.exit_code, Some(0));
    assert!(
        result.reaped,
        "completed child must be waited before returning"
    );
}

#[test]
fn supervisor_timeout_kills_and_reaps_child() {
    let mut command = command_that_sleeps();
    command.timeout = Duration::from_millis(100);
    let started = std::time::Instant::now();

    let result = ReferenceProcessSupervisor::new()
        .run(command, &Cancellation::new())
        .expect("spawn should succeed");

    assert_eq!(result.status, RunStatus::TimedOut);
    assert!(result.reaped, "timed-out child must be waited after kill");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn supervisor_cancellation_kills_and_reaps_child() {
    let cancellation = Arc::new(Cancellation::new());
    let trigger = Arc::clone(&cancellation);
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        trigger.cancel();
    });

    let result = ReferenceProcessSupervisor::new()
        .run(command_that_sleeps(), &cancellation)
        .expect("spawn should succeed");
    thread.join().expect("cancellation trigger should finish");

    assert_eq!(result.status, RunStatus::Cancelled);
    assert!(result.reaped, "cancelled child must be waited after kill");
}

#[test]
fn supervisor_rejects_empty_program_without_spawning() {
    let result = ReferenceProcessSupervisor::new().run(
        ReferenceCommandSpec::new("", [] as [&str; 0]),
        &Cancellation::new(),
    );

    assert!(result.is_err());
}

#[test]
fn supervisor_drains_and_bounds_stdout_and_stderr_capture() {
    let started = std::time::Instant::now();
    let result = ReferenceProcessSupervisor::new()
        .run(
            command_that_writes_large_output()
                .with_output_limit(64)
                .with_combined_output_limit(200_000),
            &Cancellation::new(),
        )
        .expect("output-producing child should execute");

    assert_eq!(result.status, RunStatus::Completed);
    assert!(
        result.stdout.len() <= 64,
        "stdout must respect the configured cap"
    );
    assert!(
        result.stderr.len() <= 64,
        "stderr must respect the configured cap"
    );
    assert!(result.stdout_truncated, "stdout cap must be observable");
    assert!(result.stderr_truncated, "stderr cap must be observable");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "draining must not deadlock on a full pipe"
    );
}

#[test]
fn supervisor_kills_and_reaps_when_combined_output_limit_is_exceeded() {
    let result = ReferenceProcessSupervisor::new()
        .run(
            command_that_writes_large_output()
                .with_output_limit(64)
                .with_combined_output_limit(64),
            &Cancellation::new(),
        )
        .expect("output limit termination should be reported as a run result");

    assert_eq!(result.status, RunStatus::OutputLimitExceeded);
    assert!(
        result.reaped,
        "output-limit termination must reap the child"
    );
    assert!(result.stdout.len() + result.stderr.len() <= 64);
    assert!(result.stdout_truncated || result.stderr_truncated);
}

#[test]
fn supervisor_counts_discarded_single_stream_output_against_combined_limit() {
    let result = ReferenceProcessSupervisor::new()
        .run(
            command_that_writes_stdout_only()
                .with_output_limit(64)
                .with_combined_output_limit(128),
            &Cancellation::new(),
        )
        .expect("output limit termination should be reported as a run result");

    assert_eq!(result.status, RunStatus::OutputLimitExceeded);
    assert!(
        result.reaped,
        "output-limit termination must reap the child"
    );
}

#[test]
fn supervisor_passes_bounded_stdin_without_putting_payload_in_arguments() {
    let command = if cfg!(windows) {
        ReferenceCommandSpec::new(
            "powershell.exe",
            [
                "-NoProfile",
                "-Command",
                "[Console]::OpenStandardInput().CopyTo([Console]::OpenStandardOutput())",
            ],
        )
    } else {
        ReferenceCommandSpec::new("sh", ["-c", "cat"])
    };
    let result = ReferenceProcessSupervisor::new()
        .run_with_stdin(command, b"framed-input", &Cancellation::new())
        .expect("stdin child should execute");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.stdout, b"framed-input");
}

#[test]
fn supervisor_timeout_kills_descendants_before_returning() {
    let (start_marker, final_marker) = descendant_marker_paths();
    let result = ReferenceProcessSupervisor::new()
        .run(
            command_that_spawns_descendant(&start_marker, &final_marker)
                .with_timeout(Duration::from_millis(600)),
            &Cancellation::new(),
        )
        .expect("descendant-producing child should execute");

    assert_eq!(result.status, RunStatus::TimedOut);
    assert!(result.reaped, "timed-out process tree must be reaped");
    assert!(
        wait_for_path(&start_marker, Duration::from_secs(1)),
        "descendant fixture must prove that the child was launched"
    );
    thread::sleep(Duration::from_secs(2));
    assert!(
        !final_marker.exists(),
        "descendant must not outlive a timed-out supervisor process"
    );
    let _ = fs::remove_file(start_marker);
    let _ = fs::remove_file(final_marker);
}

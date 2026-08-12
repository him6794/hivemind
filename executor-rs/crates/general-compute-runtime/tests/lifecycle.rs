use general_compute_runtime::supervisor::{Cancellation, CommandSpec, RunStatus, Supervisor};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn command_that_finishes() -> CommandSpec {
    if cfg!(windows) {
        CommandSpec::new("cmd.exe", ["/C", "exit 0"])
    } else {
        CommandSpec::new("sh", ["-c", "exit 0"])
    }
}

fn command_that_sleeps() -> CommandSpec {
    if cfg!(windows) {
        CommandSpec::new("powershell.exe", ["-NoProfile", "-Command", "Start-Sleep -Seconds 5"])
    } else {
        CommandSpec::new("sh", ["-c", "sleep 5"])
    }
}

#[test]
fn supervisor_reports_completed_child_after_waiting_for_reap() {
    let cancellation = Cancellation::new();
    let result = Supervisor::new()
        .run(command_that_finishes(), &cancellation)
        .expect("child should execute");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.exit_code, Some(0));
    assert!(result.reaped, "completed child must be waited before returning");
}

#[test]
fn supervisor_timeout_kills_and_reaps_child() {
    let mut command = command_that_sleeps();
    command.timeout = Duration::from_millis(100);
    let started = std::time::Instant::now();

    let result = Supervisor::new()
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

    let result = Supervisor::new()
        .run(command_that_sleeps(), &cancellation)
        .expect("spawn should succeed");
    thread.join().expect("cancellation trigger should finish");

    assert_eq!(result.status, RunStatus::Cancelled);
    assert!(result.reaped, "cancelled child must be waited after kill");
}

#[test]
fn supervisor_rejects_empty_program_without_spawning() {
    let result = Supervisor::new().run(CommandSpec::new("", [] as [&str; 0]), &Cancellation::new());

    assert!(result.is_err());
}

use general_compute_runtime::cp_python::{
    PinnedPythonAdapter, PythonAdapterError, PythonBackendRegistration, PythonBackendRegistry,
};
use general_compute_runtime::supervisor::Cancellation;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn registration() -> PythonBackendRegistration {
    PythonBackendRegistration {
        backend_id: "python-cpython-312".into(),
        executable: "python".into(),
        runtime_version: "CPython 3.12.9".into(),
        guest_image_digest: format!("sha256:{}", "a".repeat(64)),
        protocol_version: "general-compute-wire-v1".into(),
        max_output_bytes: 1024,
    }
}

#[test]
fn cp_python_adapter_requires_a_registry_approved_backend() {
    let registry = PythonBackendRegistry::new(Vec::new()).expect("empty registry is valid");

    let error = PinnedPythonAdapter::from_registry(&registry, "python-cpython-312")
        .expect_err("unregistered CPython backend must fail closed");

    assert!(matches!(error, PythonAdapterError::BackendUnavailable { .. }));
}

#[test]
fn python_registry_rejects_shell_interpreters_as_backend_executables() {
    for executable in ["sh", "bash", "cmd.exe", "powershell.exe", "pwsh"] {
        let mut spec = registration();
        spec.executable = executable.into();
        assert!(
            PythonBackendRegistry::new(vec![spec]).is_err(),
            "shell executable {executable} must not be registry-approved"
        );
    }
}

#[test]
fn python_registry_rejects_executable_argument_injection() {
    let mut spec = registration();
    spec.executable = "python -c malicious".into();
    assert!(
        PythonBackendRegistry::new(vec![spec]).is_err(),
        "executable field must not contain an argument string"
    );

    let mut spec = registration();
    spec.executable = "python; touch /tmp/escape".into();
    assert!(
        PythonBackendRegistry::new(vec![spec]).is_err(),
        "shell metacharacters must not enter a backend executable"
    );
}

#[test]
fn cp_python_adapter_rejects_malformed_observation_fields_and_status() {
    let registry = PythonBackendRegistry::new(vec![registration()]).expect("registration is valid");
    let adapter =
        PinnedPythonAdapter::from_registry(&registry, "python-cpython-312").expect("registered backend should resolve");

    let unknown_field = br#"{"status":"halted","steps":1,"output":"1","secret":"leak"}"#;
    assert!(matches!(
        adapter.parse_observation(unknown_field),
        Err(PythonAdapterError::MalformedObservation(_))
    ));

    let unknown_status = br#"{"status":"success","steps":1,"output":"1"}"#;
    assert!(matches!(
        adapter.parse_observation(unknown_status),
        Err(PythonAdapterError::MalformedObservation(_))
    ));
}

#[test]
fn cp_python_adapter_enforces_registered_output_cap() {
    let mut spec = registration();
    spec.max_output_bytes = 3;
    let registry = PythonBackendRegistry::new(vec![spec]).expect("registration is valid");
    let adapter =
        PinnedPythonAdapter::from_registry(&registry, "python-cpython-312").expect("registered backend should resolve");

    let oversized = br#"{"status":"halted","steps":1,"output":"1234"}"#;
    assert!(matches!(
        adapter.parse_observation(oversized),
        Err(PythonAdapterError::MalformedObservation(_))
    ));
}

#[test]
fn cp_python_adapter_executes_source_over_framed_stdin() {
    let registry = PythonBackendRegistry::new(vec![registration()]).expect("registration is valid");
    let adapter =
        PinnedPythonAdapter::from_registry(&registry, "python-cpython-312").expect("registered backend should resolve");

    let observation = adapter
        .execute(
            "result = input['value'] + 1",
            r#"{"value": 4}"#,
            7,
            &Cancellation::new(),
        )
        .expect("pinned CPython should execute the fixture");

    assert_eq!(observation.status, "halted");
    assert_eq!(observation.steps, 1);
    assert_eq!(observation.output, "5");
}

#[test]
fn cp_python_adapter_maps_timeout_to_a_typed_supervisor_failure() {
    let registry = PythonBackendRegistry::new(vec![registration()]).expect("registration is valid");
    let adapter =
        PinnedPythonAdapter::from_registry(&registry, "python-cpython-312").expect("registered backend should resolve");

    let error = adapter
        .execute_with_timeout(
            "while True: pass",
            r#"{"value": 4}"#,
            7,
            Duration::from_millis(100),
            &Cancellation::new(),
        )
        .expect_err("infinite Python loop must hit the deadline");

    assert!(matches!(error, PythonAdapterError::Supervisor(message) if message.contains("timed out")));
}

#[test]
fn cp_python_adapter_maps_cooperative_cancellation_to_a_typed_failure() {
    let registry = PythonBackendRegistry::new(vec![registration()]).expect("registration is valid");
    let adapter =
        PinnedPythonAdapter::from_registry(&registry, "python-cpython-312").expect("registered backend should resolve");
    let cancellation = Arc::new(Cancellation::new());
    let trigger = Arc::clone(&cancellation);
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        trigger.cancel();
    });

    let error = adapter
        .execute_with_timeout(
            "while True: pass",
            r#"{"value": 4}"#,
            7,
            Duration::from_secs(5),
            &cancellation,
        )
        .expect_err("cancelled Python loop must stop");
    thread.join().expect("cancellation trigger should finish");

    assert!(matches!(error, PythonAdapterError::Supervisor(message) if message.contains("cancelled")));
}

#[test]
fn cp_python_adapter_maps_source_exception_to_bounded_observation() {
    let registry = PythonBackendRegistry::new(vec![registration()]).expect("registration is valid");
    let adapter =
        PinnedPythonAdapter::from_registry(&registry, "python-cpython-312").expect("registered backend should resolve");

    let observation = adapter
        .execute(
            "raise ValueError('bad input')",
            r#"{"value": 4}"#,
            7,
            &Cancellation::new(),
        )
        .expect("source exceptions should become observations");

    assert_eq!(observation.status, "exception");
    assert_eq!(observation.steps, 1);
    assert!(observation.output.contains("ValueError"));
}

#[test]
fn cp_python_adapter_rejects_runner_output_with_trailing_frame_bytes() {
    let registry = PythonBackendRegistry::new(vec![registration()]).expect("registration is valid");
    let adapter =
        PinnedPythonAdapter::from_registry(&registry, "python-cpython-312").expect("registered backend should resolve");

    let frame =
        general_compute_runtime::encode_frame(&serde_json::json!({"status":"halted","steps":1,"output":"5"}), 1024)
            .expect("observation frame should encode");
    let mut trailing = frame;
    trailing.extend_from_slice(b"trailing");
    let error = adapter
        .parse_framed_observation(&trailing)
        .expect_err("trailing response bytes must fail closed");
    assert!(matches!(error, PythonAdapterError::Protocol(message) if message.contains("trailing")));
}

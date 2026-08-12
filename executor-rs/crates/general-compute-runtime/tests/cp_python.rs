use general_compute_runtime::cp_python::{
    PinnedPythonAdapter, PythonAdapterError, PythonBackendRegistration, PythonBackendRegistry,
};

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

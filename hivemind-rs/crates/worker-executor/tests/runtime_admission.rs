use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, BackendRegistration, CapabilityMatrix, DeterminismPolicy,
    ExecutionPolicy, GeneralComputeRequest, WorkerCapabilities, GENERAL_COMPUTE_RUNTIME_VERSION,
};
use hivemind_worker_executor::runtime_admission::{
    RuntimeAdmissionError, RuntimeRoute, WorkerRuntimeAdmission,
};

#[test]
fn general_compute_v1alpha1_requires_an_explicit_request_manifest() {
    let error = WorkerRuntimeAdmission::default()
        .admit("general-compute-v1alpha1", &[])
        .expect_err("v1alpha1 must not be admitted without its manifest");

    assert!(matches!(error, RuntimeAdmissionError::ManifestRequired));
}

#[test]
fn general_compute_v1alpha1_rejects_malformed_request_manifest() {
    let error = WorkerRuntimeAdmission::default()
        .admit("general-compute-v1alpha1", br#"not-json"#)
        .expect_err("malformed v1alpha1 manifests must fail closed");

    assert!(matches!(error, RuntimeAdmissionError::ManifestMalformed(_)));
}

#[test]
fn a_manifest_cannot_be_smuggled_through_the_legacy_route() {
    let error = WorkerRuntimeAdmission::default()
        .admit("", br#"{}"#)
        .expect_err("manifests must not change the meaning of the legacy route");

    assert!(matches!(
        error,
        RuntimeAdmissionError::ManifestRuntimeMismatch
    ));
}

#[test]
fn production_sandboxed_dsl_is_admitted_without_general_compute_manifest() {
    let route = WorkerRuntimeAdmission::default()
        .admit("production_sandboxed_dsl", &[])
        .expect("closed DSL route should not require a general-compute manifest");

    assert_eq!(route, RuntimeRoute::ProductionSandboxedDsl);
}

#[test]
fn managed_function_v0_keeps_its_existing_typed_route() {
    let route = WorkerRuntimeAdmission::default()
        .admit("managed-function-v0", &[])
        .expect("v0 admission is handled by the legacy contract");

    assert_eq!(route, RuntimeRoute::ManagedFunctionV0);
}

#[test]
fn general_compute_v1alpha1_rejects_an_unregistered_backend() {
    let request =
        request_manifest("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let error = WorkerRuntimeAdmission::new(CapabilityMatrix::default(), worker_capabilities())
        .admit(
            GENERAL_COMPUTE_RUNTIME_VERSION,
            &serde_json::to_vec(&request).unwrap(),
        )
        .expect_err("unregistered backend must fail closed");

    assert!(matches!(
        error,
        RuntimeAdmissionError::ManifestRejected {
            code: general_compute_runtime::ValidationErrorCode::BackendUnavailable,
            ..
        }
    ));
}

#[test]
fn general_compute_v1alpha1_rejects_an_image_missing_from_worker_capabilities() {
    let request =
        request_manifest("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let error = WorkerRuntimeAdmission::new(
        registry_for(request.guest_image_digest.clone()),
        WorkerCapabilities {
            guest_image_digests: vec![],
            ..worker_capabilities()
        },
    )
    .admit(
        GENERAL_COMPUTE_RUNTIME_VERSION,
        &serde_json::to_vec(&request).unwrap(),
    )
    .expect_err("worker image capability must be required");

    assert!(matches!(
        error,
        RuntimeAdmissionError::ManifestRejected {
            code: general_compute_runtime::ValidationErrorCode::GuestImageMismatch,
            ..
        }
    ));
}

#[test]
fn general_compute_v1alpha1_routes_a_registered_manifest() {
    let request =
        request_manifest("sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc");
    let route = WorkerRuntimeAdmission::new(
        registry_for(request.guest_image_digest.clone()),
        worker_capabilities_with_image(request.guest_image_digest.clone()),
    )
    .admit(
        GENERAL_COMPUTE_RUNTIME_VERSION,
        &serde_json::to_vec(&request).unwrap(),
    )
    .expect("registered backend and image should route");

    assert!(
        matches!(route, RuntimeRoute::GeneralComputeV1Alpha1(r) if r.backend_id == "python-cpython-312")
    );
}

fn request_manifest(image: &str) -> GeneralComputeRequest {
    let mut request = GeneralComputeRequest {
        execution_id: "execution-1".into(),
        attempt_id: "attempt-1".into(),
        idempotency_key: "idempotency-1".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: image.into(),
        backend_id: "python-cpython-312".into(),
        entrypoint: "main".into(),
        source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, b"source"),
        input_artifacts: vec![],
        execution_policy: ExecutionPolicy::default(),
        determinism: DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn registry_for(image: String) -> CapabilityMatrix {
    CapabilityMatrix::new(vec![BackendRegistration {
        backend_id: "python-cpython-312".into(),
        execution_mode: general_compute_runtime::sandbox::BackendExecutionMode::ReferenceDirect,
        guest_image_digest: image,
        capabilities: vec!["cpu".into()],
        max_threads: 2,
        network_allowed: false,
        filesystem_read_only: true,
        gpu_allowed: false,
    }])
}

fn worker_capabilities() -> WorkerCapabilities {
    worker_capabilities_with_image(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
    )
}

fn worker_capabilities_with_image(image: String) -> WorkerCapabilities {
    WorkerCapabilities {
        guest_image_digests: vec![image],
        capabilities: vec!["cpu".into()],
        max_threads: 2,
        gpu_available: false,
    }
}

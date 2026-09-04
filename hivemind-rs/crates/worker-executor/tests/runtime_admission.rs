use general_compute_runtime::managed_gpu::{
    ManagedGpuBackendRegistration, ManagedGpuCapability, ManagedGpuLimits, ManagedGpuProofPolicy,
    ManagedGpuRequest, ManagedGpuRequirement, MANAGED_GPU_BILLING_VERSION,
    MANAGED_GPU_COST_MODEL_VERSION, MANAGED_GPU_OPERATION_REGISTRY_VERSION,
    MANAGED_GPU_REQUEST_PROTOCOL_VERSION, MANAGED_GPU_RUNTIME_VERSION,
    MANAGED_GPU_SEMANTICS_MANIFEST_SHA256, MANAGED_GPU_SETTLEMENT_BASIS,
};
use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, BackendRegistration, CapabilityMatrix, DeterminismPolicy,
    ExecutionPolicy, GeneralComputeRequest, TrustedWorkerCapabilityRegistration,
    WorkerCapabilities, GENERAL_COMPUTE_RUNTIME_VERSION,
};
use hivemind_worker_executor::runtime_admission::{
    RuntimeAdmissionError, RuntimeRoute, WorkerRuntimeAdmission,
};

#[test]
fn managed_function_gpu_v1_requires_its_dedicated_manifest() {
    let error = WorkerRuntimeAdmission::default()
        .admit_with_manifests(MANAGED_GPU_RUNTIME_VERSION, &[], &[])
        .expect_err("GPU-v1 must not be admitted without its dedicated manifest");

    assert!(matches!(error, RuntimeAdmissionError::ManifestRequired));
}

#[test]
fn managed_function_gpu_v1_rejects_cross_route_manifest_channels() {
    let request = managed_gpu_request();
    let managed_gpu_manifest = serde_json::to_vec(&request).unwrap();
    let error = WorkerRuntimeAdmission::default()
        .admit_with_manifests(MANAGED_GPU_RUNTIME_VERSION, br#"{}"#, &managed_gpu_manifest)
        .expect_err("GPU-v1 must not accept a general-compute manifest channel");

    assert!(matches!(
        error,
        RuntimeAdmissionError::ManifestRuntimeMismatch
    ));
}

#[test]
fn managed_function_gpu_v1_routes_a_valid_operator_registered_manifest() {
    let request = managed_gpu_request();
    let capability = managed_gpu_capability(&request, 0, "GPU-0123456789abcdef");
    let admission = WorkerRuntimeAdmission::new_with_trusted_registration(
        managed_gpu_registration(&request, vec![capability.clone()]),
    );

    let route = admission
        .admit_with_manifests(
            MANAGED_GPU_RUNTIME_VERSION,
            &[],
            &serde_json::to_vec(&request).unwrap(),
        )
        .expect("operator-approved GPU registration should admit the route");

    assert!(matches!(
        route,
        RuntimeRoute::ManagedFunctionGpuV1(admitted)
            if admitted == request
    ));
}

#[test]
fn managed_function_gpu_v1_rejects_missing_or_incompatible_trusted_device() {
    let request = managed_gpu_request();
    let empty_registration = TrustedWorkerCapabilityRegistration {
        worker: managed_gpu_worker_capabilities(),
        gpu_capabilities: vec![],
        managed_gpu_backends: vec![],
        backends: vec![],
    };
    let error = WorkerRuntimeAdmission::new_with_trusted_registration(empty_registration)
        .admit_with_manifests(
            MANAGED_GPU_RUNTIME_VERSION,
            &[],
            &serde_json::to_vec(&request).unwrap(),
        )
        .expect_err("a GPU boolean without a typed device must fail closed");
    assert!(matches!(
        error,
        RuntimeAdmissionError::ManifestRejected {
            code: general_compute_runtime::ValidationErrorCode::BackendUnavailable,
            ..
        }
    ));

    let incompatible_device = ManagedGpuCapability::new(
        "gpu-incompatible",
        "7.5",
        request.gpu_requirement.runtime_version.clone(),
        request.gpu_requirement.driver_abi.clone(),
        16 * 1024 * 1024 * 1024,
        32,
        request.guest_image_digest.clone(),
        0,
        "GPU-bbbbbbbbbbbbbbbb",
    )
    .unwrap();
    let error = WorkerRuntimeAdmission::new_with_trusted_registration(managed_gpu_registration(
        &request,
        vec![incompatible_device],
    ))
    .admit_with_manifests(
        MANAGED_GPU_RUNTIME_VERSION,
        &[],
        &serde_json::to_vec(&request).unwrap(),
    )
    .expect_err("a trusted device with incompatible compute capability must fail closed");
    assert!(matches!(
        error,
        RuntimeAdmissionError::ManifestRejected {
            code: general_compute_runtime::ValidationErrorCode::GpuUnavailable,
            ..
        }
    ));
}

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

const MANAGED_GPU_IMAGE: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn managed_gpu_request() -> ManagedGpuRequest {
    let requirement = ManagedGpuRequirement::new(
        "8.9",
        "12.4",
        "550",
        8 * 1024 * 1024 * 1024,
        1,
        MANAGED_GPU_IMAGE,
    )
    .unwrap();
    let mut request = ManagedGpuRequest {
        protocol_version: MANAGED_GPU_REQUEST_PROTOCOL_VERSION.into(),
        execution_id: "runtime-admission-execution".into(),
        attempt_id: "runtime-admission-attempt".into(),
        idempotency_key: "runtime-admission-idempotency".into(),
        request_digest: String::new(),
        runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
        semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
        operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
        backend_id: "managed-cuda-test".into(),
        guest_image_digest: MANAGED_GPU_IMAGE.into(),
        source: "gpu_add_f32([1.0], [2.0])".into(),
        input_json: "{}".into(),
        gpu_requirement: requirement,
        limits: ManagedGpuLimits::default(),
        reservation_cpt: 10,
        billing_version: MANAGED_GPU_BILLING_VERSION.into(),
        cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
        settlement_basis: MANAGED_GPU_SETTLEMENT_BASIS.into(),
        proof_policy: ManagedGpuProofPolicy::None,
    };
    request.request_digest = request.canonical_request_digest();
    request
}

fn managed_gpu_worker_capabilities() -> WorkerCapabilities {
    WorkerCapabilities {
        guest_image_digests: vec![MANAGED_GPU_IMAGE.into()],
        capabilities: vec![MANAGED_GPU_RUNTIME_VERSION.into()],
        max_threads: 4,
        gpu_available: true,
    }
}

fn managed_gpu_capability(
    request: &ManagedGpuRequest,
    ordinal: i32,
    uuid: &str,
) -> ManagedGpuCapability {
    ManagedGpuCapability::new(
        "cuda-runtime-admission-0",
        request.gpu_requirement.compute_capability.clone(),
        request.gpu_requirement.runtime_version.clone(),
        request.gpu_requirement.driver_abi.clone(),
        16 * 1024 * 1024 * 1024,
        32,
        request.guest_image_digest.clone(),
        ordinal,
        uuid,
    )
    .unwrap()
}

fn managed_gpu_registration(
    request: &ManagedGpuRequest,
    capabilities: Vec<ManagedGpuCapability>,
) -> TrustedWorkerCapabilityRegistration {
    TrustedWorkerCapabilityRegistration {
        worker: managed_gpu_worker_capabilities(),
        gpu_capabilities: vec![],
        managed_gpu_backends: vec![ManagedGpuBackendRegistration {
            backend_id: request.backend_id.clone(),
            runtime_version: MANAGED_GPU_RUNTIME_VERSION.into(),
            semantics_manifest_sha256: MANAGED_GPU_SEMANTICS_MANIFEST_SHA256.into(),
            operation_registry_version: MANAGED_GPU_OPERATION_REGISTRY_VERSION.into(),
            guest_image_digest: request.guest_image_digest.clone(),
            billing_version: MANAGED_GPU_BILLING_VERSION.into(),
            cost_model_version: MANAGED_GPU_COST_MODEL_VERSION.into(),
            reservation_cpt: request.reservation_cpt,
            max_source_bytes: 256 * 1024,
            max_input_bytes: 16 * 1024 * 1024,
            max_output_bytes: 16 * 1024 * 1024,
            max_operations: 1_000_000,
            max_gpu_time_ms: 120_000,
            capabilities,
        }],
        backends: vec![],
    }
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

use general_compute_runtime::gpu::{GpuCapability, GpuRuntime, GpuSelection, GpuVendor};
use general_compute_runtime::onnx::{OnnxBackendConfig, OnnxExecutionProvider};
use general_compute_runtime::production::{
    GpuDeviceMapping, ManagedDslBackendRegistration, ManagedDslBackendRegistry,
    ProductionBackendConfig, ProductionBackendRegistry, ProductionBackendRegistryError,
    WindowsProductionBackendConfig, WindowsProductionBackendRegistry,
};
use general_compute_runtime::sandbox::{
    CgroupPolicy, LinuxNamespace, LinuxSandboxPolicy, OciPrivilegeMode, PrivilegeEscalationPolicy,
    RootFilesystemPolicy, SandboxDevice, SandboxDeviceType, SandboxMount, SandboxNetworkPolicy,
    SeccompPolicy, WindowsIsolationMode, WindowsRootFilesystemPolicy, WindowsSandboxNetworkPolicy,
    WindowsSandboxPolicy,
};
use general_compute_runtime::{
    ArtifactManifest, ArtifactRole, DeterminismPolicy, ExecutionPolicy,
    GENERAL_COMPUTE_RUNTIME_VERSION, GeneralComputeRequest,
};
use std::path::PathBuf;

fn dsl_registration(backend_id: &str) -> ManagedDslBackendRegistration {
    ManagedDslBackendRegistration {
        backend_id: backend_id.into(),
        runtime_version: general_compute_runtime::MANAGED_DSL_RUNTIME_VERSION.into(),
        semantics_manifest_sha256: general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256
            .into(),
        max_usage_units: 10_000,
        max_output_bytes: 4096,
    }
}

fn operator_path(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\hivemind").join(name)
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/hivemind").join(name)
    }
}

#[test]
fn managed_dsl_registry_round_trips_and_has_no_host_execution_fields() {
    let registration = dsl_registration("managed-default");
    let encoded = serde_json::to_value(&registration).expect("DSL registration serializes");
    assert_eq!(
        registration.execution_mode(),
        general_compute_runtime::sandbox::BackendExecutionMode::ProductionSandboxedDsl
    );
    assert!(encoded.get("runner_executable").is_none());
    assert!(encoded.get("image_root").is_none());
    assert!(encoded.get("network").is_none());
    let decoded: ManagedDslBackendRegistration =
        serde_json::from_value(encoded).expect("DSL registration deserializes");
    assert_eq!(decoded, registration);
    assert_eq!(
        ManagedDslBackendRegistry::new(vec![decoded]).unwrap().len(),
        1
    );
}

#[test]
fn managed_dsl_registry_rejects_wrong_identity_or_limits() {
    let mut registration = dsl_registration("managed-invalid");
    registration.runtime_version = "managed-function-v1".into();
    assert_eq!(
        ManagedDslBackendRegistry::new(vec![registration]).unwrap_err(),
        ProductionBackendRegistryError::ManagedDslRuntimeMismatch
    );

    let mut registration = dsl_registration("managed-invalid-digest");
    registration.semantics_manifest_sha256 = "sha256:bad".into();
    assert_eq!(
        ManagedDslBackendRegistry::new(vec![registration]).unwrap_err(),
        ProductionBackendRegistryError::ManagedDslSemanticsMismatch
    );

    let mut registration = dsl_registration("managed-invalid-limit");
    registration.max_usage_units = 0;
    assert_eq!(
        ManagedDslBackendRegistry::new(vec![registration]).unwrap_err(),
        ProductionBackendRegistryError::ManagedDslUsageLimitRequired
    );
}

#[test]
fn managed_dsl_registry_rejects_duplicate_ids_and_unknown_fields() {
    let error = ManagedDslBackendRegistry::new(vec![
        dsl_registration("managed-duplicate"),
        dsl_registration("managed-duplicate"),
    ])
    .unwrap_err();
    assert_eq!(
        error,
        ProductionBackendRegistryError::DuplicateBackend("managed-duplicate".into())
    );

    let mut value = serde_json::to_value(dsl_registration("managed-extra")).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("runner_executable".into(), serde_json::json!("runner.exe"));
    assert!(serde_json::from_value::<ManagedDslBackendRegistration>(value).is_err());
}

fn policy() -> LinuxSandboxPolicy {
    LinuxSandboxPolicy {
        oci_privilege: OciPrivilegeMode::Rootless,
        namespaces: vec![
            LinuxNamespace::User,
            LinuxNamespace::Pid,
            LinuxNamespace::Mount,
            LinuxNamespace::Network,
        ],
        cgroup: CgroupPolicy::V2,
        seccomp: SeccompPolicy::DefaultDeny {
            profile_sha256: general_compute_runtime::sha256_digest(seccomp_profile_bytes()),
        },
        privilege_escalation: PrivilegeEscalationPolicy::NoNewPrivileges,
        root_filesystem: RootFilesystemPolicy::ReadOnly,
        network: SandboxNetworkPolicy::DenyAll,
        mounts: vec![SandboxMount::ReadOnlyArtifact {
            artifact_id: "source".into(),
            destination: "/work/source".into(),
        }],
        devices: Vec::new(),
    }
}

fn seccomp_profile_bytes() -> &'static [u8] {
    br#"{"defaultAction":"SCMP_ACT_ERRNO","syscalls":[{"action":"SCMP_ACT_ALLOW","names":["exit","exit_group"]}]}"#
}

fn config() -> ProductionBackendConfig {
    ProductionBackendConfig {
        backend_id: "python-cpython-312".into(),
        guest_image_digest: format!("sha256:{}", "a".repeat(64)),
        bundle_root: PathBuf::from("relative-bundle"),
        artifact_root: PathBuf::from("relative-artifacts"),
        runner_executable: PathBuf::from("relative-runc"),
        runner_state_root: PathBuf::from("relative-runner-state"),
        seccomp_profile_path: PathBuf::from("relative-seccomp.json"),
        runner_prefix_args: Vec::new(),
        runner_sha256: format!("sha256:{}", "c".repeat(64)),
        entrypoint: vec!["python".into(), "/runtime/runner.py".into()],
        policy: policy(),
        gpu_device_mappings: Vec::new(),
        onnx: None,
        max_output_bytes: 1024,
    }
}

#[test]
fn gpu_selection_resolves_only_operator_device_mapping() {
    let mut registration = config();
    registration.gpu_device_mappings = vec![GpuDeviceMapping {
        device_id: "gpu-0".into(),
        devices: vec![SandboxDevice {
            path: "/dev/nvidia0".into(),
            device_type: SandboxDeviceType::Char,
            major: 195,
            minor: 0,
            access: "rw".into(),
        }],
    }];
    registration.onnx =
        Some(OnnxBackendConfig::new("source", Vec::new(), OnnxExecutionProvider::Cuda).unwrap());
    assert_eq!(
        registration.launch_for_gpu_selection(None).unwrap_err(),
        ProductionBackendRegistryError::OnnxGpuSelectionRequired
    );
    let capability = GpuCapability::new(
        GpuVendor::Nvidia,
        "gpu-0",
        "sm_80",
        GpuRuntime::Cuda,
        "12.4",
        "550.54",
        16 * 1024 * 1024 * 1024,
        8,
        registration.guest_image_digest.clone(),
    )
    .unwrap();
    let launch = registration
        .launch_for_gpu_selection(Some(&GpuSelection::Gpu(capability)))
        .unwrap();
    assert_eq!(launch.policy.devices[0].path, "/dev/nvidia0");

    let unknown = GpuCapability::new(
        GpuVendor::Nvidia,
        "gpu-1",
        "sm_80",
        GpuRuntime::Cuda,
        "12.4",
        "550.54",
        16 * 1024 * 1024 * 1024,
        8,
        registration.guest_image_digest.clone(),
    )
    .unwrap();
    assert_eq!(
        registration
            .launch_for_gpu_selection(Some(&GpuSelection::Gpu(unknown)))
            .unwrap_err(),
        ProductionBackendRegistryError::GpuDeviceMappingMissing("gpu-1".into())
    );
}

fn windows_policy() -> WindowsSandboxPolicy {
    WindowsSandboxPolicy {
        isolation: WindowsIsolationMode::Process,
        network: WindowsSandboxNetworkPolicy::DenyAll,
        root_filesystem: WindowsRootFilesystemPolicy::ReadOnly,
        mounts: vec![
            SandboxMount::ReadOnlyArtifact {
                artifact_id: "source".into(),
                destination: "/work/source".into(),
            },
            SandboxMount::EphemeralScratch {
                destination: "/work/output".into(),
                max_bytes: 4096,
            },
        ],
        memory_bytes: 1024 * 1024,
        cpu_millis: 1000,
        process_limit: 8,
        thread_limit: 16,
        scratch_bytes: 4096,
    }
}

fn windows_config(backend_id: &str) -> WindowsProductionBackendConfig {
    WindowsProductionBackendConfig {
        backend_id: backend_id.into(),
        guest_image_digest: format!("sha256:{}", "a".repeat(64)),
        image_root: PathBuf::from("C:\\hivemind\\windows\\images\\python"),
        artifact_root: PathBuf::from("C:\\hivemind\\windows\\artifacts"),
        runner_executable: PathBuf::from("C:\\hivemind\\windows\\hcs-helper.exe"),
        runner_sha256: format!("sha256:{}", "b".repeat(64)),
        entrypoint: vec!["hivemind-runner.exe".into()],
        policy: windows_policy(),
        max_output_bytes: 1024 * 1024,
        timeout_ms: 30_000,
    }
}

#[test]
fn windows_registry_round_trips_a_distinct_native_registration() {
    let registration = windows_config("windows-python");
    let encoded = serde_json::to_vec(&registration).expect("Windows registration serializes");
    let decoded: WindowsProductionBackendConfig =
        serde_json::from_slice(&encoded).expect("Windows registration deserializes");
    assert_eq!(decoded, registration);
    assert_eq!(
        decoded.execution_mode(),
        general_compute_runtime::sandbox::BackendExecutionMode::ProductionSandboxedWindows
    );
    assert_eq!(
        WindowsProductionBackendRegistry::new(vec![decoded])
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn windows_registry_rejects_unknown_fields_and_duplicate_backend_ids() {
    let mut value = serde_json::to_value(windows_config("windows-python")).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("bundle_root".into(), serde_json::json!("C:\\\\wrong"));
    assert!(serde_json::from_value::<WindowsProductionBackendConfig>(value).is_err());

    let error = WindowsProductionBackendRegistry::new(vec![
        windows_config("windows-python"),
        windows_config("windows-python"),
    ])
    .expect_err("duplicate Windows backend ids must fail closed");
    assert_eq!(
        error,
        ProductionBackendRegistryError::DuplicateBackend("windows-python".into())
    );
}

#[test]
fn windows_registry_rejects_relative_or_traversing_operator_paths() {
    let mut registration = windows_config("windows-paths");
    registration.image_root = PathBuf::from("windows\\images");
    assert_eq!(
        WindowsProductionBackendRegistry::new(vec![registration]).unwrap_err(),
        ProductionBackendRegistryError::WindowsPathMustBeAbsolute
    );

    let mut registration = windows_config("windows-traversal");
    registration.artifact_root = PathBuf::from("C:\\hivemind\\windows\\..\\artifacts");
    assert_eq!(
        WindowsProductionBackendRegistry::new(vec![registration]).unwrap_err(),
        ProductionBackendRegistryError::WindowsPathTraversal
    );
}

#[test]
fn windows_registry_rejects_empty_registry() {
    assert_eq!(
        WindowsProductionBackendRegistry::new(Vec::new()).unwrap_err(),
        ProductionBackendRegistryError::WindowsRegistryEmpty
    );
}

#[test]
fn windows_hcs_spec_uses_only_operator_roots_and_enforces_isolation_flags() {
    let registration = windows_config("windows-spec");
    let spec = registration
        .hcs_spec("task-123")
        .expect("validated registration should produce an HCS spec");
    assert_eq!(spec.container_id, "hivemind-task-123");
    assert!(spec.network_isolated);
    assert!(spec.root_read_only);
    assert_eq!(spec.entrypoint, vec!["hivemind-runner.exe"]);
    assert_eq!(spec.mounts.len(), 2);
    assert!(spec.mounts[0].read_only);
    assert!(
        spec.mounts[0]
            .host_path
            .ends_with("artifacts\\task-123\\source")
    );
    assert_eq!(spec.mounts[0].container_path, "C:\\work\\source");
    assert!(!spec.mounts[1].read_only);
    assert!(
        spec.mounts[1]
            .host_path
            .ends_with("artifacts\\task-123\\scratch")
    );
    assert_eq!(spec.mounts[1].container_path, "C:\\work\\output");
    assert!(
        spec.result_path
            .ends_with("artifacts\\task-123\\scratch\\result.json")
    );
    assert_eq!(spec.result_container_path, "C:\\work\\output\\result.json");
    assert_eq!(spec.max_output_bytes, registration.max_output_bytes);
}

#[test]
fn windows_hcs_spec_rejects_task_id_traversal_before_path_construction() {
    let registration = windows_config("windows-spec-traversal");
    assert_eq!(
        registration.hcs_spec("..\\escape").unwrap_err(),
        ProductionBackendRegistryError::UnsafeTaskId
    );
}

#[test]
fn production_registry_rejects_unpinned_or_relative_operator_paths() {
    let error = ProductionBackendRegistry::new(vec![config()])
        .expect_err("production backend must not accept relative operator paths");
    assert_eq!(error, ProductionBackendRegistryError::PathMustBeAbsolute);
}

#[test]
fn production_registry_requires_a_dedicated_runner_state_root() {
    let mut registration = config();
    registration.bundle_root = operator_path("bundle");
    registration.artifact_root = operator_path("artifacts");
    registration.runner_executable = operator_path("runc.exe");
    registration.runner_state_root = PathBuf::from("relative-runner-state");

    let error = ProductionBackendRegistry::new(vec![registration])
        .expect_err("rootless OCI runners must use an absolute operator state root");
    assert_eq!(error, ProductionBackendRegistryError::PathMustBeAbsolute);
}

#[test]
fn production_materializer_requires_an_operator_seccomp_profile() {
    let mut registration = config();
    let root = std::env::temp_dir().join(format!(
        "hivemind-production-seccomp-profile-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    registration.bundle_root = root.join("bundles");
    registration.artifact_root = root.join("artifacts");
    registration.runner_executable = root.join("runc");
    registration.runner_state_root = root.join("runner-state");
    registration.seccomp_profile_path = root.join("missing-seccomp.json");
    std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();

    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    let request = request_for_mount_test(&registration, "execution-seccomp-profile");
    let error = registry
        .get(&registration.backend_id)
        .unwrap()
        .materialize_bundle(&request, "task-seccomp-profile")
        .expect_err("production materialization must require an operator seccomp profile");

    assert!(matches!(
        error,
        ProductionBackendRegistryError::SeccompProfileUnavailable(_)
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn production_registry_rejects_backend_mounts_that_do_not_match_the_entrypoint() {
    let mut registration = config();
    registration.bundle_root = operator_path("bundle");
    registration.artifact_root = operator_path("artifacts");
    registration.runner_executable = operator_path("runc.exe");
    registration.runner_state_root = operator_path("runner-state");
    registration.seccomp_profile_path = operator_path("seccomp.json");
    registration.policy.mounts[0] = SandboxMount::ReadOnlyArtifact {
        artifact_id: "different-source".into(),
        destination: "/work/source".into(),
    };

    let error = ProductionBackendRegistry::new(vec![registration])
        .expect_err("production registration must bind the source artifact mount");
    assert_eq!(
        error,
        ProductionBackendRegistryError::SourceArtifactMountRequired
    );
}

#[test]
fn production_task_root_rejects_path_traversal_and_materializes_bound_bundle() {
    let mut registration = config();
    registration.bundle_root = std::env::temp_dir().join("hivemind-production-bundles");
    registration.artifact_root = std::env::temp_dir().join("hivemind-production-artifacts");
    registration.runner_executable = operator_path("runc.exe");
    registration.runner_state_root = std::env::temp_dir().join("hivemind-production-runner-state");
    registration.seccomp_profile_path = registration.bundle_root.join("seccomp.json");
    let _ = std::fs::remove_dir_all(&registration.bundle_root);
    let _ = std::fs::remove_dir_all(&registration.artifact_root);
    std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();
    write_seccomp_profile(&registration);
    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    assert!(matches!(
        registry
            .get(&registration.backend_id)
            .unwrap()
            .task_root("../escape"),
        Err(ProductionBackendRegistryError::UnsafeTaskId)
    ));

    let source = ArtifactManifest::inline_json("source", ArtifactRole::Source, b"result = 1");
    let mut request = GeneralComputeRequest {
        execution_id: "execution-production-bundle".into(),
        attempt_id: "attempt-production-bundle".into(),
        idempotency_key: "idempotency-production-bundle".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: registration.guest_image_digest.clone(),
        backend_id: registration.backend_id.clone(),
        entrypoint: "main".into(),
        source_artifact: source,
        input_artifacts: Vec::new(),
        execution_policy: ExecutionPolicy::default(),
        determinism: DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    let (bundle, artifacts) = registry
        .get(&registration.backend_id)
        .unwrap()
        .materialize_bundle(&request, "task-production-bundle")
        .unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(bundle.join("config.json")).unwrap()).unwrap();
    let canonical_artifacts = std::fs::canonicalize(&artifacts).unwrap();
    assert_eq!(
        config["mounts"][3]["source"],
        canonical_artifacts
            .join("source")
            .to_string_lossy()
            .to_string()
    );
    let _ = std::fs::remove_dir_all(bundle);
    let _ = std::fs::remove_dir_all(artifacts);
}

#[test]
fn materialize_bundle_for_gpu_launch_emits_exact_device_and_cgroup_entries() {
    let root = std::env::temp_dir().join(format!(
        "hivemind-production-gpu-materialized-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let mut registration = config();
    registration.bundle_root = root.join("bundles");
    registration.artifact_root = root.join("artifacts");
    registration.runner_executable = root.join("runc");
    registration.runner_state_root = root.join("runner-state");
    registration.seccomp_profile_path = root.join("seccomp.json");
    registration.gpu_device_mappings = vec![GpuDeviceMapping {
        device_id: "gpu-0".into(),
        devices: vec![
            SandboxDevice {
                path: "/dev/nvidia0".into(),
                device_type: SandboxDeviceType::Char,
                major: 195,
                minor: 0,
                access: "rw".into(),
            },
            SandboxDevice {
                path: "/dev/nvidiactl".into(),
                device_type: SandboxDeviceType::Char,
                major: 195,
                minor: 255,
                access: "rwm".into(),
            },
        ],
    }];
    std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();
    write_seccomp_profile(&registration);

    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    let request = request_for_mount_test(&registration, "execution-gpu-materialized");
    let capability = GpuCapability::new(
        GpuVendor::Nvidia,
        "gpu-0",
        "sm_89",
        GpuRuntime::Cuda,
        "12.4",
        "550.54",
        16 * 1024 * 1024 * 1024,
        8,
        registration.guest_image_digest.clone(),
    )
    .unwrap();
    let launch = registry
        .get(&registration.backend_id)
        .unwrap()
        .launch_for_gpu_selection(Some(&GpuSelection::Gpu(capability)))
        .unwrap();
    let (bundle, artifacts) = registry
        .get(&registration.backend_id)
        .unwrap()
        .materialize_bundle_for_launch(&request, "task-gpu-materialized", &launch)
        .unwrap();

    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(bundle.join("config.json")).unwrap()).unwrap();
    assert_eq!(
        config["linux"]["devices"],
        serde_json::json!([
            {"path": "/dev/null", "type": "c", "major": 1, "minor": 3},
            {"path": "/dev/zero", "type": "c", "major": 1, "minor": 5},
            {"path": "/dev/full", "type": "c", "major": 1, "minor": 7},
            {"path": "/dev/random", "type": "c", "major": 1, "minor": 8},
            {"path": "/dev/urandom", "type": "c", "major": 1, "minor": 9},
            {"path": "/dev/tty", "type": "c", "major": 5, "minor": 0},
            {"path": "/dev/nvidia0", "type": "c", "major": 195, "minor": 0},
            {"path": "/dev/nvidiactl", "type": "c", "major": 195, "minor": 255}
        ])
    );
    assert_eq!(
        config["linux"]["resources"]["devices"],
        serde_json::json!([
            {"allow": true, "type": "c", "major": 1, "minor": 3, "access": "rwm"},
            {"allow": true, "type": "c", "major": 1, "minor": 5, "access": "rwm"},
            {"allow": true, "type": "c", "major": 1, "minor": 7, "access": "rwm"},
            {"allow": true, "type": "c", "major": 1, "minor": 8, "access": "rwm"},
            {"allow": true, "type": "c", "major": 1, "minor": 9, "access": "rwm"},
            {"allow": true, "type": "c", "major": 5, "minor": 0, "access": "rwm"},
            {"allow": true, "type": "c", "major": 195, "minor": 0, "access": "rw"},
            {"allow": true, "type": "c", "major": 195, "minor": 255, "access": "rwm"}
        ])
    );

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(bundle);
    let _ = std::fs::remove_dir_all(artifacts);
}

#[test]
fn onnx_backend_requires_fixed_source_and_numbered_input_mounts() {
    let mut registration = config();
    registration.bundle_root = operator_path("bundle");
    registration.artifact_root = operator_path("artifacts");
    registration.runner_executable = operator_path("runc.exe");
    registration.runner_state_root = operator_path("runner-state");
    registration.seccomp_profile_path = operator_path("seccomp.json");
    registration.onnx = Some(
        OnnxBackendConfig::new(
            "source",
            vec!["tensor-0".into(), "tensor-1".into()],
            OnnxExecutionProvider::Cpu,
        )
        .unwrap(),
    );
    registration.policy.mounts.extend([
        SandboxMount::ReadOnlyArtifact {
            artifact_id: "tensor-0".into(),
            destination: "/work/input-0".into(),
        },
        SandboxMount::ReadOnlyArtifact {
            artifact_id: "tensor-1".into(),
            destination: "/work/input-1".into(),
        },
    ]);
    ProductionBackendRegistry::new(vec![registration.clone()])
        .expect("numbered ONNX mounts should be accepted");

    registration.policy.mounts[1] = SandboxMount::ReadOnlyArtifact {
        artifact_id: "tensor-0".into(),
        destination: "/work/input-2".into(),
    };
    assert_eq!(
        ProductionBackendRegistry::new(vec![registration]).unwrap_err(),
        ProductionBackendRegistryError::OnnxInputMountInvalid(0)
    );
}

#[test]
fn onnx_backend_binds_verified_artifacts_and_runner_annotations() {
    let root = std::env::temp_dir().join(format!(
        "hivemind-production-onnx-materialized-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let mut registration = config();
    registration.bundle_root = root.join("bundles");
    registration.artifact_root = root.join("artifacts");
    registration.runner_executable = root.join("runc");
    registration.runner_state_root = root.join("runner-state");
    registration.seccomp_profile_path = root.join("seccomp.json");
    registration.onnx =
        Some(OnnxBackendConfig::new("source", Vec::new(), OnnxExecutionProvider::Cpu).unwrap());
    std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();
    write_seccomp_profile(&registration);

    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    let request = request_for_mount_test(&registration, "execution-onnx-materialized");
    let launch = registry
        .get(&registration.backend_id)
        .unwrap()
        .launch_for_gpu_selection(None)
        .unwrap();
    let (bundle, artifacts) = registry
        .get(&registration.backend_id)
        .unwrap()
        .materialize_bundle_for_launch(&request, "task-onnx-materialized", &launch)
        .unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(bundle.join("config.json")).unwrap()).unwrap();

    assert_eq!(config["annotations"]["org.hivemind.workload"], "onnx");
    assert_eq!(
        config["annotations"]["org.hivemind.onnx.protocol"],
        "general-compute-onnx-v1"
    );
    assert_eq!(
        config["annotations"]["org.hivemind.onnx.execution-provider"],
        "cpu"
    );
    assert_eq!(
        config["annotations"]["org.hivemind.onnx.model-artifact-id"],
        "source"
    );
    assert_eq!(
        config["linux"]["devices"],
        serde_json::json!([
            {"path": "/dev/null", "type": "c", "major": 1, "minor": 3},
            {"path": "/dev/zero", "type": "c", "major": 1, "minor": 5},
            {"path": "/dev/full", "type": "c", "major": 1, "minor": 7},
            {"path": "/dev/random", "type": "c", "major": 1, "minor": 8},
            {"path": "/dev/urandom", "type": "c", "major": 1, "minor": 9},
            {"path": "/dev/tty", "type": "c", "major": 5, "minor": 0}
        ])
    );
    assert!(config["linux"]["resources"]["devices"].is_array());

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(bundle);
    let _ = std::fs::remove_dir_all(artifacts);
}

#[test]
fn production_registry_requires_mounts_for_every_request_artifact() {
    let mut registration = config();
    registration.bundle_root = operator_path("bundle");
    registration.artifact_root = operator_path("artifacts");
    registration.runner_executable = operator_path("runc.exe");
    registration.runner_state_root = operator_path("runner-state");
    registration.seccomp_profile_path = operator_path("seccomp.json");
    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    let mut request = GeneralComputeRequest {
        execution_id: "execution-production-input-mount".into(),
        attempt_id: "attempt-production-input-mount".into(),
        idempotency_key: "idempotency-production-input-mount".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: registration.guest_image_digest.clone(),
        backend_id: registration.backend_id.clone(),
        entrypoint: "main".into(),
        source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, b"source"),
        input_artifacts: vec![ArtifactManifest::inline_json(
            "input",
            ArtifactRole::Input,
            b"input",
        )],
        execution_policy: ExecutionPolicy::default(),
        determinism: DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    assert!(matches!(
        registry.get(&registration.backend_id).unwrap().validate_request_mounts(&request),
        Err(ProductionBackendRegistryError::ArtifactMountRequired(id)) if id == "input"
    ));
}

#[test]
fn production_registry_rejects_mounts_for_unrequested_artifacts() {
    let mut registration = config();
    registration.bundle_root = operator_path("bundle");
    registration.artifact_root = operator_path("artifacts");
    registration.runner_executable = operator_path("runc.exe");
    registration.runner_state_root = operator_path("runner-state");
    registration.seccomp_profile_path = operator_path("seccomp.json");
    registration
        .policy
        .mounts
        .push(SandboxMount::ReadOnlyArtifact {
            artifact_id: "unrequested".into(),
            destination: "/work/unrequested".into(),
        });
    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    let request = request_for_mount_test(&registration, "execution-production-extra-mount");

    assert!(matches!(
        registry.get(&registration.backend_id).unwrap().validate_request_mounts(&request),
        Err(ProductionBackendRegistryError::ArtifactMountNotRequested(id)) if id == "unrequested"
    ));
}

fn request_for_mount_test(
    registration: &ProductionBackendConfig,
    execution_id: &str,
) -> GeneralComputeRequest {
    let mut request = GeneralComputeRequest {
        execution_id: execution_id.into(),
        attempt_id: "attempt-production-extra-mount".into(),
        idempotency_key: "idempotency-production-extra-mount".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: registration.guest_image_digest.clone(),
        backend_id: registration.backend_id.clone(),
        entrypoint: "main".into(),
        source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, b"source"),
        input_artifacts: Vec::new(),
        execution_policy: ExecutionPolicy::default(),
        determinism: DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

#[cfg(unix)]
#[test]
fn production_materializer_rejects_a_symlinked_task_bundle_root() {
    use std::os::unix::fs::symlink;

    let mut registration = config();
    let root = test_root("task-bundle");
    registration.bundle_root = root.join("bundles");
    registration.artifact_root = root.join("artifacts");
    registration.runner_executable = PathBuf::from("/hivemind/runc");
    registration.runner_state_root = root.join("runner-state");
    registration.seccomp_profile_path = root.join("seccomp.json");
    std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();
    write_seccomp_profile(&registration);
    let redirected = root.join("redirected");
    std::fs::create_dir_all(&redirected).unwrap();
    std::fs::create_dir_all(&registration.bundle_root).unwrap();
    symlink(&redirected, registration.bundle_root.join("task-symlink")).unwrap();

    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    let request = request_for(&registration, "execution-task-symlink");
    let error = registry
        .get(&registration.backend_id)
        .unwrap()
        .materialize_bundle(&request, "task-symlink")
        .expect_err("a task bundle symlink must not be followed");

    assert!(matches!(
        error,
        ProductionBackendRegistryError::RootUnavailable(_)
    ));
    assert!(!redirected.join("config.json").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn production_materializer_rejects_a_symlinked_task_rootfs() {
    use std::os::unix::fs::symlink;

    let mut registration = config();
    let root = test_root("task-rootfs");
    registration.bundle_root = root.join("bundles");
    registration.artifact_root = root.join("artifacts");
    registration.runner_executable = PathBuf::from("/hivemind/runc");
    registration.runner_state_root = root.join("runner-state");
    registration.seccomp_profile_path = root.join("seccomp.json");
    std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();
    write_seccomp_profile(&registration);
    let redirected = root.join("redirected-rootfs");
    std::fs::create_dir_all(&redirected).unwrap();
    std::fs::create_dir_all(registration.bundle_root.join("task-rootfs")).unwrap();
    symlink(
        &redirected,
        registration.bundle_root.join("task-rootfs/rootfs"),
    )
    .unwrap();

    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    let request = request_for(&registration, "execution-task-rootfs");
    let error = registry
        .get(&registration.backend_id)
        .unwrap()
        .materialize_bundle(&request, "task-rootfs")
        .expect_err("a task rootfs symlink must not be followed");

    assert!(matches!(
        error,
        ProductionBackendRegistryError::RootUnavailable(_)
    ));
    assert!(!redirected.join("config.json").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn production_materializer_rejects_a_symlinked_task_config_before_writing() {
    use std::os::unix::fs::symlink;

    let mut registration = config();
    let root = test_root("task-config");
    registration.bundle_root = root.join("bundles");
    registration.artifact_root = root.join("artifacts");
    registration.runner_executable = PathBuf::from("/hivemind/runc");
    registration.runner_state_root = root.join("runner-state");
    registration.seccomp_profile_path = root.join("seccomp.json");
    std::fs::create_dir_all(registration.bundle_root.join("rootfs")).unwrap();
    write_seccomp_profile(&registration);
    let redirected = root.join("redirected-config.json");
    std::fs::write(&redirected, b"sentinel").unwrap();
    let task_bundle = registration.bundle_root.join("task-config");
    std::fs::create_dir_all(&task_bundle).unwrap();
    symlink(&redirected, task_bundle.join("config.json")).unwrap();

    let registry = ProductionBackendRegistry::new(vec![registration.clone()]).unwrap();
    let request = request_for(&registration, "execution-task-config");
    let error = registry
        .get(&registration.backend_id)
        .unwrap()
        .materialize_bundle(&request, "task-config")
        .expect_err("a task config symlink must not be followed");

    assert!(matches!(
        error,
        ProductionBackendRegistryError::RootUnavailable(_)
    ));
    assert_eq!(std::fs::read(&redirected).unwrap(), b"sentinel");
    let _ = std::fs::remove_dir_all(root);
}

fn write_seccomp_profile(registration: &ProductionBackendConfig) {
    if let Some(parent) = registration.seccomp_profile_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&registration.seccomp_profile_path, seccomp_profile_bytes()).unwrap();
}

#[cfg(unix)]
fn test_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "hivemind-production-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[cfg(unix)]
fn request_for(
    registration: &ProductionBackendConfig,
    execution_id: &str,
) -> GeneralComputeRequest {
    let mut request = GeneralComputeRequest {
        execution_id: execution_id.into(),
        attempt_id: "attempt-production-symlink".into(),
        idempotency_key: "idempotency-production-symlink".into(),
        request_digest: String::new(),
        runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
        guest_image_digest: registration.guest_image_digest.clone(),
        backend_id: registration.backend_id.clone(),
        entrypoint: "main".into(),
        source_artifact: ArtifactManifest::inline_json("source", ArtifactRole::Source, b"source"),
        input_artifacts: Vec::new(),
        execution_policy: ExecutionPolicy::default(),
        determinism: DeterminismPolicy::default(),
        billing_version: "billing-v1".into(),
        cost_model_version: "cost-v1".into(),
    };
    request.request_digest = request.canonical_request_digest();
    request
}

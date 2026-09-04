use general_compute_runtime::onnx::{OnnxBackendConfig, OnnxExecutionProvider};
use general_compute_runtime::sandbox::{
    CgroupPolicy, LinuxNamespace, LinuxSandboxPolicy, OciPrivilegeMode, PrivilegeEscalationPolicy,
    ProductionSandboxError, ProductionSandboxLaunch, ProductionSandboxLauncher,
    RootFilesystemPolicy, SandboxDevice, SandboxDeviceType, SandboxMount, SandboxNetworkPolicy,
    SandboxPolicyError, SeccompPolicy, WindowsIsolationMode, WindowsNativeSandboxLaunch,
    WindowsRootFilesystemPolicy, WindowsSandboxNetworkPolicy, WindowsSandboxPolicy,
    rootless_id_mappings,
};
use general_compute_runtime::sha256_digest;
use general_compute_runtime::supervisor::Cancellation;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn valid_policy() -> LinuxSandboxPolicy {
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
            profile_sha256: format!("sha256:{}", "b".repeat(64)),
        },
        privilege_escalation: PrivilegeEscalationPolicy::NoNewPrivileges,
        root_filesystem: RootFilesystemPolicy::ReadOnly,
        network: SandboxNetworkPolicy::DenyAll,
        mounts: vec![
            SandboxMount::ReadOnlyArtifact {
                artifact_id: "source".into(),
                destination: "/work/source".into(),
            },
            SandboxMount::EphemeralScratch {
                destination: "/work/output".into(),
                max_bytes: 1024,
            },
        ],
        devices: Vec::new(),
    }
}

fn valid_windows_policy() -> WindowsSandboxPolicy {
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
                max_bytes: 1024,
            },
        ],
        memory_bytes: 1024 * 1024,
        cpu_millis: 1000,
        process_limit: 4,
        thread_limit: 8,
        scratch_bytes: 1024,
    }
}

#[test]
fn windows_native_policy_requires_process_isolation_and_deny_all_network() {
    let mut policy = valid_windows_policy();
    assert!(policy.validate().is_ok());

    policy.network = WindowsSandboxNetworkPolicy::AllowAll;
    assert!(policy.validate().is_err());

    let mut policy = valid_windows_policy();
    policy.mounts.pop();
    assert!(policy.validate().is_err());

    let mut policy = valid_windows_policy();
    policy.mounts[0] = SandboxMount::ReadOnlyArtifact {
        artifact_id: "C:\\outside".into(),
        destination: "/work/source".into(),
    };
    assert!(policy.validate().is_err());
}

#[test]
fn windows_native_launch_is_distinct_from_linux_oci_launch() {
    let launch = WindowsNativeSandboxLaunch {
        backend_id: "windows-python".into(),
        guest_image_digest: format!("sha256:{}", "a".repeat(64)),
        entrypoint: vec!["/runtime/runner.exe".into()],
        policy: valid_windows_policy(),
    };
    assert!(launch.validate().is_ok());
}

fn valid_launch() -> ProductionSandboxLaunch {
    ProductionSandboxLaunch {
        backend_id: "python-numpy-scipy".into(),
        guest_image_digest: format!("sha256:{}", "a".repeat(64)),
        entrypoint: vec!["python".into(), "/runtime/runner.py".into()],
        policy: valid_policy(),
        onnx: None,
    }
}

fn temporary_bundle_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "hivemind-oci-runner-{}-{suffix}",
        std::process::id()
    ))
}

fn write_valid_bundle(root: &Path, launch: &ProductionSandboxLaunch) {
    fs::create_dir_all(root.join("rootfs")).expect("bundle rootfs should be created");
    let mut mounts = vec![
        serde_json::json!({
            "destination": "/proc",
            "type": "proc",
            "source": "proc",
            "options": ["nosuid", "nodev", "noexec"]
        }),
        serde_json::json!({
            "destination": "/dev",
            "type": "tmpfs",
            "source": "tmpfs",
            "options": ["nosuid", "strictatime", "mode=755", "size=65536k"]
        }),
        serde_json::json!({
            "destination": "/dev/pts",
            "type": "devpts",
            "source": "devpts",
            "options": ["nosuid", "noexec", "newinstance", "ptmxmode=0666", "mode=0620", "gid=5"]
        }),
    ];
    mounts.extend(
        launch
            .policy
            .mounts
            .iter()
            .map(|mount| match mount {
                SandboxMount::ReadOnlyArtifact {
                    artifact_id,
                    destination,
                } => serde_json::json!({
                    "destination": destination,
                    "type": "bind",
                    "source": format!("/hivemind/artifacts/{artifact_id}"),
                    "options": ["bind", "ro", "nodev", "nosuid", "noexec"]
                }),
                SandboxMount::EphemeralScratch {
                    destination,
                    max_bytes,
                } => serde_json::json!({
                    "destination": destination,
                    "type": "tmpfs",
                    "source": "tmpfs",
                    "options": [
                        "rw",
                        "nodev",
                        "nosuid",
                        "noexec",
                        format!("size={max_bytes}")
                    ]
                }),
            })
            .collect::<Vec<_>>(),
    );
    let (uid_mappings, gid_mappings) =
        rootless_id_mappings().expect("the test host must provide rootless id mappings");
    let mut config = serde_json::json!({
        "ociVersion": "1.0.2",
        "process": {
            "args": launch.entrypoint,
            "cwd": "/",
            "noNewPrivileges": true,
            "user": {"uid": 65532, "gid": 65532}
        },
        "root": {"path": "rootfs", "readonly": true},
        "mounts": mounts,
        "linux": {
            "namespaces": [
                {"type": "user"},
                {"type": "pid"},
                {"type": "mount"},
                {"type": "network"}
            ],
            "uidMappings": uid_mappings
                .iter()
                .map(|mapping| serde_json::json!({
                    "containerID": mapping.container_id,
                    "hostID": mapping.host_id,
                    "size": mapping.size,
                }))
                .collect::<Vec<_>>(),
            "gidMappings": gid_mappings
                .iter()
                .map(|mapping| serde_json::json!({
                    "containerID": mapping.container_id,
                    "hostID": mapping.host_id,
                    "size": mapping.size,
                }))
                .collect::<Vec<_>>(),
            "devices": [
                {"path": "/dev/null", "type": "c", "major": 1, "minor": 3},
                {"path": "/dev/zero", "type": "c", "major": 1, "minor": 5},
                {"path": "/dev/full", "type": "c", "major": 1, "minor": 7},
                {"path": "/dev/random", "type": "c", "major": 1, "minor": 8},
                {"path": "/dev/urandom", "type": "c", "major": 1, "minor": 9},
                {"path": "/dev/tty", "type": "c", "major": 5, "minor": 0}
            ],
            "resources": {"devices": [
                {"allow": true, "type": "c", "major": 1, "minor": 3, "access": "rwm"},
                {"allow": true, "type": "c", "major": 1, "minor": 5, "access": "rwm"},
                {"allow": true, "type": "c", "major": 1, "minor": 7, "access": "rwm"},
                {"allow": true, "type": "c", "major": 1, "minor": 8, "access": "rwm"},
                {"allow": true, "type": "c", "major": 1, "minor": 9, "access": "rwm"},
                {"allow": true, "type": "c", "major": 5, "minor": 0, "access": "rwm"}
            ]},
            "seccomp": {"defaultAction": "SCMP_ACT_ERRNO"}
        },
        "annotations": {
            "org.hivemind.guest-image-digest": launch.guest_image_digest,
            "org.hivemind.backend-id": launch.backend_id,
            "org.hivemind.cgroup-version": "v2",
            "org.hivemind.network-policy": "deny_all",
            "org.hivemind.seccomp-profile-sha256": launch.policy.seccomp_profile_sha256()
        }
    });
    if let Some(onnx) = &launch.onnx {
        config["annotations"]["org.hivemind.workload"] = serde_json::json!("onnx");
        config["annotations"]["org.hivemind.onnx.protocol"] =
            serde_json::json!(onnx.protocol_version);
        config["annotations"]["org.hivemind.onnx.execution-provider"] =
            serde_json::json!(onnx.execution_provider.as_str());
        config["annotations"]["org.hivemind.onnx.model-artifact-id"] =
            serde_json::json!(onnx.model_artifact_id);
        config["annotations"]["org.hivemind.onnx.input-artifact-ids"] = serde_json::json!(
            serde_json::to_string(&onnx.input_artifact_ids)
                .expect("ONNX input artifact IDs serialize infallibly")
        );
    }
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(&config).expect("bundle config should serialize"),
    )
    .expect("bundle config should be written");
}

trait SeccompProfileDigest {
    fn seccomp_profile_sha256(&self) -> String;
}

impl SeccompProfileDigest for LinuxSandboxPolicy {
    fn seccomp_profile_sha256(&self) -> String {
        match &self.seccomp {
            SeccompPolicy::DefaultDeny { profile_sha256 } => profile_sha256.clone(),
            SeccompPolicy::Disabled => panic!("valid test policy must have seccomp profile"),
        }
    }
}

fn runner_digest(executable: &Path) -> String {
    sha256_digest(&fs::read(executable).expect("runner executable should be readable"))
}

#[cfg(unix)]
fn fake_runner(root: &Path) -> (PathBuf, PathBuf, Vec<String>) {
    use std::os::unix::fs::PermissionsExt;

    let executable = root.join("fake-runc.sh");
    let marker = root.join("runner-args.txt");
    let marker_literal = marker.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s' \"$*\" > '{marker_literal}'\nexit 0\n"),
    )
    .expect("fake OCI runner should be written");
    let mut permissions = fs::metadata(&executable)
        .expect("fake runner metadata should be readable")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&executable, permissions).expect("fake runner should be executable");
    (executable, marker, Vec::new())
}

#[cfg(windows)]
fn fake_runner(root: &Path) -> (PathBuf, PathBuf, Vec<String>) {
    let script = root.join("fake-runc.cmd");
    let marker = root.join("runner-args.txt");
    fs::write(
        &script,
        format!(
            "@echo off\r\necho %* > \"{}\"\r\nexit /b 0\r\n",
            marker.display()
        ),
    )
    .expect("fake OCI runner should be written");
    let executable = PathBuf::from(std::env::var("ComSpec").expect("ComSpec should exist"));
    (
        executable,
        marker,
        vec!["/C".into(), script.to_string_lossy().into_owned()],
    )
}

fn remove_bundle(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

#[test]
fn production_policy_requires_rootless_oci_and_separate_linux_namespaces() {
    let mut policy = valid_policy();
    policy.oci_privilege = OciPrivilegeMode::Rootful;
    assert_eq!(
        policy.validate(),
        Err(SandboxPolicyError::RootlessOciRequired)
    );

    for namespace in [
        LinuxNamespace::User,
        LinuxNamespace::Pid,
        LinuxNamespace::Mount,
        LinuxNamespace::Network,
    ] {
        let mut policy = valid_policy();
        policy
            .namespaces
            .retain(|candidate| *candidate != namespace);
        assert_eq!(
            policy.validate(),
            Err(SandboxPolicyError::MissingNamespace(namespace))
        );
    }
}

#[test]
fn production_policy_requires_cgroup_seccomp_and_no_new_privileges() {
    let mut policy = valid_policy();
    policy.cgroup = CgroupPolicy::LegacyOrMissing;
    assert_eq!(policy.validate(), Err(SandboxPolicyError::CgroupV2Required));

    let mut policy = valid_policy();
    policy.seccomp = SeccompPolicy::Disabled;
    assert_eq!(
        policy.validate(),
        Err(SandboxPolicyError::SeccompProfileRequired)
    );

    let mut policy = valid_policy();
    policy.privilege_escalation = PrivilegeEscalationPolicy::Allowed;
    assert_eq!(
        policy.validate(),
        Err(SandboxPolicyError::NoNewPrivilegesRequired)
    );
}

#[test]
fn device_policy_validates_paths_types_and_access() {
    fn nvidia_device(path: &str) -> SandboxDevice {
        SandboxDevice {
            path: path.into(),
            device_type: SandboxDeviceType::Char,
            major: 195,
            minor: 0,
            access: "rw".into(),
        }
    }

    let mut policy = valid_policy();
    policy.devices = vec![nvidia_device("/dev/nvidia0")];
    assert!(policy.validate().is_ok());

    // Only absolute /dev/ paths without traversal or colons are allowed.
    for hostile in ["/etc/passwd", "/dev/../..", "/dev/", "dev/nvidia0"] {
        let mut policy = valid_policy();
        policy.devices = vec![nvidia_device(hostile)];
        assert_eq!(
            policy.validate(),
            Err(SandboxPolicyError::InvalidDevicePath),
            "hostile device path {hostile}"
        );
    }

    let mut policy = valid_policy();
    policy.devices = vec![SandboxDevice {
        access: "rwx".into(),
        ..nvidia_device("/dev/nvidia0")
    }];
    assert_eq!(
        policy.validate(),
        Err(SandboxPolicyError::InvalidDeviceAccess)
    );

    let mut policy = valid_policy();
    policy.devices = vec![SandboxDevice {
        major: -1,
        ..nvidia_device("/dev/nvidia0")
    }];
    assert_eq!(
        policy.validate(),
        Err(SandboxPolicyError::InvalidDeviceSpec)
    );

    // Duplicate paths fail closed.
    let mut policy = valid_policy();
    policy.devices = vec![nvidia_device("/dev/nvidia0"), nvidia_device("/dev/nvidia0")];
    assert_eq!(
        policy.validate(),
        Err(SandboxPolicyError::DuplicateDevicePath)
    );
}

#[test]
fn hostile_filesystem_and_network_policy_fail_before_launch() {
    let mut launch = valid_launch();
    launch.policy.root_filesystem = RootFilesystemPolicy::Writable;
    let error = ProductionSandboxLauncher::new()
        .run(&launch, &Cancellation::new())
        .expect_err("writable root filesystem must fail closed");
    assert_eq!(
        error,
        ProductionSandboxError::Policy(SandboxPolicyError::ReadOnlyRootRequired)
    );

    let mut launch = valid_launch();
    launch.policy.network = SandboxNetworkPolicy::AllowAll;
    let error = ProductionSandboxLauncher::new()
        .run(&launch, &Cancellation::new())
        .expect_err("host networking must fail closed");
    assert_eq!(
        error,
        ProductionSandboxError::Policy(SandboxPolicyError::NetworkDenyRequired)
    );

    let mut launch = valid_launch();
    launch.policy.mounts.push(SandboxMount::ReadOnlyArtifact {
        artifact_id: "host-root".into(),
        destination: "/".into(),
    });
    let error = ProductionSandboxLauncher::new()
        .run(&launch, &Cancellation::new())
        .expect_err("mounting over the sandbox root must fail closed");
    assert_eq!(
        error,
        ProductionSandboxError::Policy(SandboxPolicyError::InvalidMountDestination)
    );
}

#[test]
fn production_launcher_never_falls_back_to_direct_process_spawn() {
    let error = ProductionSandboxLauncher::new()
        .run(&valid_launch(), &Cancellation::new())
        .expect_err("production launch must require a real sandbox runner");

    #[cfg(target_os = "linux")]
    assert_eq!(error, ProductionSandboxError::RunnerUnavailable);
    #[cfg(not(target_os = "linux"))]
    assert_eq!(error, ProductionSandboxError::UnsupportedPlatform);
}

#[test]
fn materialized_production_launch_requires_a_runner_state_root() {
    let error = ProductionSandboxLauncher::new()
        .run_materialized_bundle(
            &valid_launch(),
            Path::new("/operator/bundle"),
            Path::new("/operator/artifacts"),
            "hivemind-state-root-test",
            &Cancellation::new(),
        )
        .expect_err("materialized production launches must bind runner state");
    assert_eq!(error, ProductionSandboxError::RunnerStateRootUnavailable);
}

#[test]
fn sandbox_policy_json_rejects_unknown_fields_and_invalid_tags() {
    let mut value = serde_json::to_value(valid_policy()).expect("policy should serialize");
    value
        .as_object_mut()
        .expect("policy must be an object")
        .insert(
            "host_socket".into(),
            serde_json::json!("/var/run/docker.sock"),
        );
    assert!(
        serde_json::from_value::<LinuxSandboxPolicy>(value).is_err(),
        "unknown policy fields must fail closed"
    );

    let mut value = serde_json::to_value(valid_policy()).expect("policy should serialize");
    value["seccomp"]["default_action"] = serde_json::json!("allow");
    assert!(
        serde_json::from_value::<LinuxSandboxPolicy>(value).is_err(),
        "unknown seccomp actions must fail closed"
    );

    let mut value = serde_json::to_value(valid_policy()).expect("policy should serialize");
    value["mounts"][0]["kind"] = serde_json::json!("host_bind");
    assert!(
        serde_json::from_value::<LinuxSandboxPolicy>(value).is_err(),
        "unknown mount kinds must fail closed"
    );
}

#[test]
fn production_launch_rejects_whitespace_only_entrypoint_parts() {
    let mut launch = valid_launch();
    launch.entrypoint[0] = "   ".into();

    assert_eq!(
        launch.validate(),
        Err(ProductionSandboxError::InvalidEntrypoint)
    );
}

#[test]
fn production_runner_executes_only_a_bound_and_validated_oci_bundle() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let runner_state_root = root.join("runner-state");
    fs::create_dir_all(&runner_state_root).expect("runner state root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, marker, prefix_args) = fake_runner(&root);
    let runner_sha256 = runner_digest(&executable);

    let result = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
        .with_runner_sha256(runner_sha256)
        .with_runner_state_root(&runner_state_root)
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect("validated bundle should reach the pinned runner");

    assert_eq!(
        result.status,
        general_compute_runtime::supervisor::RunStatus::Completed
    );
    let args = fs::read_to_string(&marker).expect("runner should receive an argument trace");
    assert!(args.contains("run"));
    assert!(args.contains("--root"));
    assert!(args.contains(runner_state_root.to_string_lossy().as_ref()));
    assert!(args.contains("--bundle"));
    assert!(args.contains("hivemind-test-container"));
    remove_bundle(&root);
}

#[test]
fn production_runner_accepts_only_matching_onnx_annotations() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let runner_state_root = root.join("runner-state");
    fs::create_dir_all(&runner_state_root).expect("runner state root should be created");
    let mut launch = valid_launch();
    launch.onnx =
        Some(OnnxBackendConfig::new("source", Vec::new(), OnnxExecutionProvider::Cpu).unwrap());
    write_valid_bundle(&root, &launch);
    let (executable, _marker, prefix_args) = fake_runner(&root);

    let result =
        ProductionSandboxLauncher::with_oci_runner_command(executable.clone(), prefix_args)
            .with_runner_sha256(runner_digest(&executable))
            .with_runner_state_root(&runner_state_root)
            .run_bundle(
                &launch,
                &root,
                "hivemind-onnx-container",
                &Cancellation::new(),
            )
            .expect("matching ONNX annotations should reach the pinned runner");
    assert_eq!(
        result.status,
        general_compute_runtime::supervisor::RunStatus::Completed
    );

    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("config.json")).unwrap()).unwrap();
    config["annotations"]["org.hivemind.onnx.execution-provider"] = serde_json::json!("cuda");
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let (executable, _marker, prefix_args) = fake_runner(&root);
    let error = ProductionSandboxLauncher::with_oci_runner_command(executable.clone(), prefix_args)
        .with_runner_sha256(runner_digest(&executable))
        .with_runner_state_root(&runner_state_root)
        .run_bundle(
            &launch,
            &root,
            "hivemind-onnx-container-tampered",
            &Cancellation::new(),
        )
        .expect_err("provider annotation drift must fail before runner spawn");
    assert_eq!(error, ProductionSandboxError::BundleMetadataMismatch);
    remove_bundle(&root);
}

#[test]
fn production_runner_rejects_bundle_identity_or_process_mismatch_before_spawn() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, marker, prefix_args) = fake_runner(&root);
    let mut config: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("config.json")).expect("bundle config should be readable"),
    )
    .expect("bundle config should be JSON");
    config["annotations"]["org.hivemind.guest-image-digest"] = serde_json::json!(
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
    );
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(&config).expect("tampered config should serialize"),
    )
    .expect("tampered config should be written");

    let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
        .with_runner_sha256(runner_sha256_for_platform(&root))
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect_err("identity mismatch must fail closed before runner spawn");

    assert_eq!(error, ProductionSandboxError::BundleMetadataMismatch);
    assert!(
        !marker.exists(),
        "runner must not execute a mismatched bundle"
    );
    remove_bundle(&root);
}

#[test]
fn production_runner_requires_an_absolute_operator_pinned_executable() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, marker, prefix_args) = fake_runner(&root);

    let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect_err("an unpinned runner must fail closed");

    assert_eq!(error, ProductionSandboxError::RunnerNotPinned);
    assert!(!marker.exists(), "unpinned runner must not execute");
    remove_bundle(&root);
}

#[test]
fn production_runner_rejects_runner_digest_mismatch_before_spawn() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, marker, prefix_args) = fake_runner(&root);
    let wrong_digest = format!("sha256:{}", "f".repeat(64));

    let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
        .with_runner_sha256(wrong_digest)
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect_err("runner digest mismatch must fail closed before spawn");

    assert_eq!(error, ProductionSandboxError::RunnerDigestMismatch);
    assert!(!marker.exists(), "mismatched runner must not execute");
    remove_bundle(&root);
}

#[test]
fn production_runner_enforces_timeout_and_reaps_runner_tree() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, started, survived, prefix_args) = slow_fake_runner(&root);
    let runner_sha256 = runner_digest(&executable);

    let result = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
        .with_runner_sha256(runner_sha256)
        .with_timeout(Duration::from_millis(600))
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect("runner timeout should be represented as a result");

    assert_eq!(
        result.status,
        general_compute_runtime::supervisor::RunStatus::TimedOut
    );
    assert!(
        started.exists(),
        "runner should have started before timing out"
    );
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        !survived.exists(),
        "runner descendants must be killed and reaped"
    );
    remove_bundle(&root);
}

#[test]
fn production_runner_cancellation_kills_and_reaps_runner_tree() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, started, survived, prefix_args) = slow_fake_runner(&root);
    let runner_sha256 = runner_digest(&executable);
    let cancellation = Cancellation::new();
    let worker_cancellation = cancellation.clone();
    let worker_root = root.clone();
    let worker_launch = launch;
    let handle = std::thread::spawn(move || {
        ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
            .with_runner_sha256(runner_sha256)
            .with_timeout(Duration::from_secs(10))
            .run_bundle(
                &worker_launch,
                &worker_root,
                "hivemind-test-container",
                &worker_cancellation,
            )
            .expect("runner cancellation should be represented as a result")
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !started.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    cancellation.cancel();
    let result = handle.join().expect("runner thread should join");

    assert_eq!(
        result.status,
        general_compute_runtime::supervisor::RunStatus::Cancelled
    );
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        !survived.exists(),
        "cancelled runner descendants must not survive"
    );
    remove_bundle(&root);
}

#[test]
fn production_runner_rejects_unknown_or_duplicate_oci_namespaces_before_spawn() {
    for tamper in [
        serde_json::json!([{"type": "user"}, {"type": "pid"}, {"type": "mount"}, {"type": "network"}, {"type": "ipc"}]),
        serde_json::json!([{"type": "user"}, {"type": "pid"}, {"type": "mount"}, {"type": "network"}, {"type": "user"}]),
    ] {
        let root = temporary_bundle_root();
        fs::create_dir_all(&root).expect("temporary runner root should be created");
        let launch = valid_launch();
        write_valid_bundle(&root, &launch);
        let (executable, marker, prefix_args) = fake_runner(&root);
        let mut config: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("config.json")).expect("bundle config should be readable"),
        )
        .expect("bundle config should be JSON");
        config["linux"]["namespaces"] = tamper;
        fs::write(
            root.join("config.json"),
            serde_json::to_vec(&config).expect("tampered config should serialize"),
        )
        .expect("tampered config should be written");

        let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
            .with_runner_sha256(runner_sha256_for_platform(&root))
            .run_bundle(
                &launch,
                &root,
                "hivemind-test-container",
                &Cancellation::new(),
            )
            .expect_err("unknown and duplicate namespaces must fail closed");

        assert_eq!(error, ProductionSandboxError::BundleMetadataMismatch);
        assert!(
            !marker.exists(),
            "invalid namespace config must not execute"
        );
        remove_bundle(&root);
    }
}

#[test]
fn production_runner_requires_exact_mounts_and_isolation_annotations() {
    for tamper in [
        ("mounts", serde_json::Value::Null),
        ("org.hivemind.cgroup-version", serde_json::json!("v1")),
        (
            "org.hivemind.network-policy",
            serde_json::json!("allow_all"),
        ),
        (
            "org.hivemind.seccomp-profile-sha256",
            serde_json::json!(
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            ),
        ),
    ] {
        let root = temporary_bundle_root();
        fs::create_dir_all(&root).expect("temporary runner root should be created");
        let launch = valid_launch();
        write_valid_bundle(&root, &launch);
        let (executable, marker, prefix_args) = fake_runner(&root);
        let mut config: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("config.json")).expect("bundle config should be readable"),
        )
        .expect("bundle config should be JSON");
        if tamper.0 == "mounts" {
            config
                .as_object_mut()
                .expect("bundle config should be an object")
                .remove("mounts");
        } else {
            config["annotations"][tamper.0] = tamper.1;
        }
        fs::write(
            root.join("config.json"),
            serde_json::to_vec(&config).expect("tampered config should serialize"),
        )
        .expect("tampered config should be written");

        let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
            .with_runner_sha256(runner_sha256_for_platform(&root))
            .run_bundle(
                &launch,
                &root,
                "hivemind-test-container",
                &Cancellation::new(),
            )
            .expect_err("OCI isolation metadata must fail closed");

        assert_eq!(error, ProductionSandboxError::BundleMetadataMismatch);
        assert!(
            !marker.exists(),
            "invalid isolation metadata must not execute"
        );
        remove_bundle(&root);
    }
}

#[test]
fn production_runner_rejects_unknown_oci_config_fields_before_spawn() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, marker, prefix_args) = fake_runner(&root);
    let mut config: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("config.json")).expect("bundle config should be readable"),
    )
    .expect("bundle config should be JSON");
    config["process"]["hooks"] = serde_json::json!([]);
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(&config).expect("tampered config should serialize"),
    )
    .expect("tampered config should be written");

    let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
        .with_runner_sha256(runner_sha256_for_platform(&root))
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect_err("unknown OCI fields must fail closed");

    assert_eq!(error, ProductionSandboxError::InvalidBundle);
    assert!(!marker.exists(), "unknown OCI fields must not execute");
    remove_bundle(&root);
}

#[test]
fn production_runner_rejects_unknown_nested_oci_fields_before_spawn() {
    for (section, key) in [
        ("process_user", "additionalGids"),
        ("namespace", "path"),
        ("seccomp", "architectures"),
    ] {
        let root = temporary_bundle_root();
        fs::create_dir_all(&root).expect("temporary runner root should be created");
        let launch = valid_launch();
        write_valid_bundle(&root, &launch);
        let (executable, marker, prefix_args) = fake_runner(&root);
        let mut config: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("config.json")).expect("bundle config should be readable"),
        )
        .expect("bundle config should be JSON");
        match section {
            "process_user" => config["process"]["user"][key] = serde_json::json!([1]),
            "namespace" => config["linux"]["namespaces"][0][key] = serde_json::json!("/"),
            "seccomp" => config["linux"]["seccomp"][key] = serde_json::json!(["SCMP_ARCH_X86_64"]),
            _ => unreachable!("test section is exhaustive"),
        }
        fs::write(
            root.join("config.json"),
            serde_json::to_vec(&config).expect("tampered config should serialize"),
        )
        .expect("tampered config should be written");

        let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
            .with_runner_sha256(runner_sha256_for_platform(&root))
            .run_bundle(
                &launch,
                &root,
                "hivemind-test-container",
                &Cancellation::new(),
            )
            .expect_err("unknown nested OCI fields must fail closed");

        assert_eq!(error, ProductionSandboxError::InvalidBundle);
        assert!(
            !marker.exists(),
            "unknown nested OCI fields must not execute"
        );
        remove_bundle(&root);
    }
}

#[test]
fn production_runner_rejects_unknown_annotations_and_relative_bundle_paths() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, marker, prefix_args) = fake_runner(&root);
    let executable_for_unknown_annotation = executable.clone();
    let mut config: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("config.json")).expect("bundle config should be readable"),
    )
    .expect("bundle config should be JSON");
    config["annotations"]["org.hivemind.untrusted"] = serde_json::json!("unexpected");
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(&config).expect("tampered config should serialize"),
    )
    .expect("tampered config should be written");

    let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args.clone())
        .with_runner_sha256(runner_sha256_for_platform(&root))
        .run_bundle(
            &launch,
            Path::new("relative-bundle-path"),
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect_err("relative bundle paths must fail closed");

    assert_eq!(error, ProductionSandboxError::InvalidBundle);
    assert!(!marker.exists(), "relative bundle path must not execute");

    let error = ProductionSandboxLauncher::with_oci_runner_command(
        executable_for_unknown_annotation,
        prefix_args,
    )
    .with_runner_sha256(runner_sha256_for_platform(&root))
    .run_bundle(
        &launch,
        &root,
        "hivemind-test-container",
        &Cancellation::new(),
    )
    .expect_err("unknown annotations must fail closed");
    assert_eq!(error, ProductionSandboxError::InvalidBundle);
    assert!(!marker.exists(), "unknown annotations must not execute");
    remove_bundle(&root);
}

#[test]
fn production_runner_rejects_symlinked_runner_executable() {
    let root = temporary_bundle_root();
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    let launch = valid_launch();
    write_valid_bundle(&root, &launch);
    let (executable, marker, prefix_args) = fake_runner(&root);
    let linked_runner = root.join("linked-runc");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&executable, &linked_runner)
        .expect("runner symlink should be created");
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&executable, &linked_runner) {
        if error.raw_os_error() == Some(1314) {
            remove_bundle(&root);
            return;
        }
        panic!("runner symlink should be created: {error}");
    }

    let error = ProductionSandboxLauncher::with_oci_runner_command(linked_runner, prefix_args)
        .with_runner_sha256(runner_digest(&executable))
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect_err("symlinked runner executable must fail closed");

    assert_eq!(error, ProductionSandboxError::RunnerNotPinned);
    assert!(!marker.exists(), "symlinked runner must not execute");
    remove_bundle(&root);
}

#[cfg(unix)]
#[test]
fn production_runner_rejects_symlinked_bundle_directory() {
    use std::os::unix::fs::symlink;

    let root = temporary_bundle_root();
    let real_bundle = temporary_bundle_root();
    fs::create_dir_all(&real_bundle).expect("real bundle should be created");
    let launch = valid_launch();
    write_valid_bundle(&real_bundle, &launch);
    symlink(&real_bundle, &root).expect("bundle symlink should be created");
    let (executable, marker, prefix_args) = fake_runner(&real_bundle);

    let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
        .with_runner_sha256(runner_digest(&real_bundle.join("fake-runc.sh")))
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect_err("symlinked bundle directory must fail closed");

    assert_eq!(error, ProductionSandboxError::InvalidBundle);
    assert!(!marker.exists(), "symlinked bundle must not execute");
    remove_bundle(&root);
    remove_bundle(&real_bundle);
}

#[cfg(unix)]
#[test]
fn production_runner_rejects_symlinked_bundle_rootfs() {
    use std::os::unix::fs::symlink;

    let root = temporary_bundle_root();
    let real_rootfs = root.join("real-rootfs");
    fs::create_dir_all(&real_rootfs).expect("real rootfs should be created");
    fs::create_dir_all(&root).expect("temporary runner root should be created");
    symlink(&real_rootfs, root.join("rootfs")).expect("rootfs symlink should be created");
    let launch = valid_launch();
    let config = serde_json::json!({
        "ociVersion": "1.0.2",
        "process": {
            "args": launch.entrypoint,
            "cwd": "/",
            "noNewPrivileges": true,
            "user": {"uid": 65532, "gid": 65532}
        },
        "root": {"path": "rootfs", "readonly": true},
        "mounts": [],
        "linux": {
            "namespaces": [
                {"type": "user"},
                {"type": "pid"},
                {"type": "mount"},
                {"type": "network"}
            ],
            "seccomp": {"defaultAction": "SCMP_ACT_ERRNO"}
        },
        "annotations": {}
    });
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(&config).expect("bundle config should serialize"),
    )
    .expect("bundle config should be written");
    let (executable, marker, prefix_args) = fake_runner(&root);

    let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
        .with_runner_sha256(runner_sha256_for_platform(&root))
        .run_bundle(
            &launch,
            &root,
            "hivemind-test-container",
            &Cancellation::new(),
        )
        .expect_err("symlinked rootfs must fail closed");

    assert_eq!(error, ProductionSandboxError::InvalidBundle);
    assert!(!marker.exists(), "symlinked rootfs must not execute");
    remove_bundle(&root);
}

#[test]
fn production_runner_requires_non_root_process_identity_and_pinned_oci_version() {
    for (tamper, expected_error) in [
        ("root_user", ProductionSandboxError::BundleMetadataMismatch),
        ("oci_version", ProductionSandboxError::InvalidBundle),
    ] {
        let root = temporary_bundle_root();
        fs::create_dir_all(&root).expect("temporary runner root should be created");
        let launch = valid_launch();
        write_valid_bundle(&root, &launch);
        let (executable, marker, prefix_args) = fake_runner(&root);
        let mut config: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("config.json")).expect("bundle config should be readable"),
        )
        .expect("bundle config should be JSON");
        if tamper == "root_user" {
            config["process"]["user"]["uid"] = serde_json::json!(0);
            config["process"]["user"]["gid"] = serde_json::json!(0);
        } else {
            config["ociVersion"] = serde_json::json!("1.0.0");
        }
        fs::write(
            root.join("config.json"),
            serde_json::to_vec(&config).expect("tampered config should serialize"),
        )
        .expect("tampered config should be written");

        let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
            .with_runner_sha256(runner_sha256_for_platform(&root))
            .run_bundle(
                &launch,
                &root,
                "hivemind-test-container",
                &Cancellation::new(),
            )
            .expect_err("unsafe OCI identity/version must fail closed");

        assert_eq!(error, expected_error);
        assert!(!marker.exists(), "unsafe OCI config must not execute");
        remove_bundle(&root);
    }
}

#[test]
fn production_runner_rejects_path_like_container_ids_before_spawn() {
    for container_id in [".", "..", "./escaped", "nested/id"] {
        let root = temporary_bundle_root();
        fs::create_dir_all(&root).expect("temporary runner root should be created");
        let launch = valid_launch();
        write_valid_bundle(&root, &launch);
        let (executable, marker, prefix_args) = fake_runner(&root);

        let error = ProductionSandboxLauncher::with_oci_runner_command(executable, prefix_args)
            .with_runner_sha256(runner_sha256_for_platform(&root))
            .run_bundle(&launch, &root, container_id, &Cancellation::new())
            .expect_err("path-like container IDs must fail closed");

        assert_eq!(error, ProductionSandboxError::InvalidContainerId);
        assert!(!marker.exists(), "invalid container ID must not execute");
        remove_bundle(&root);
    }
}

#[test]
fn production_policy_rejects_artifact_mount_source_traversal() {
    let mut launch = valid_launch();
    launch.policy.mounts[0] = SandboxMount::ReadOnlyArtifact {
        artifact_id: "../../host-root".into(),
        destination: "/work/source".into(),
    };

    assert_eq!(
        launch.validate(),
        Err(ProductionSandboxError::Policy(
            SandboxPolicyError::InvalidMountSource
        ))
    );
}

#[cfg(unix)]
fn runner_sha256_for_platform(root: &Path) -> String {
    runner_digest(&root.join("fake-runc.sh"))
}

#[cfg(windows)]
fn runner_sha256_for_platform(_root: &Path) -> String {
    runner_digest(&PathBuf::from(
        std::env::var("ComSpec").expect("ComSpec should exist"),
    ))
}

#[cfg(unix)]
fn slow_fake_runner(root: &Path) -> (PathBuf, PathBuf, PathBuf, Vec<String>) {
    use std::os::unix::fs::PermissionsExt;

    let executable = root.join("slow-fake-runc.sh");
    let started = root.join("runner-started.txt");
    let survived = root.join("runner-survived.txt");
    let started_literal = started.to_string_lossy().replace('\'', "'\\''");
    let survived_literal = survived.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\ntouch '{started_literal}'\n(sleep 1; touch '{survived_literal}') &\nwait\n"
        ),
    )
    .expect("slow fake runner should be written");
    let mut permissions = fs::metadata(&executable)
        .expect("slow runner metadata should be readable")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&executable, permissions).expect("slow runner should be executable");
    (executable, started, survived, Vec::new())
}

#[cfg(windows)]
fn slow_fake_runner(root: &Path) -> (PathBuf, PathBuf, PathBuf, Vec<String>) {
    let script = root.join("slow-fake-runc.cmd");
    let started = root.join("runner-started.txt");
    let survived = root.join("runner-survived.txt");
    let started_literal = started.to_string_lossy().replace('"', "\\\"");
    let survived_literal = survived.to_string_lossy().replace('"', "\\\"");
    fs::write(
        &script,
        format!(
            "@echo off\r\necho started > \"{started_literal}\"\r\nstart \"\" /b powershell -NoProfile -Command \"Start-Sleep -Milliseconds 1000; Set-Content -Path '{survived_literal}' -Value survived\"\r\nping 127.0.0.1 -n 3 >nul\r\n"
        ),
    )
    .expect("slow fake runner should be written");
    let executable = PathBuf::from(std::env::var("ComSpec").expect("ComSpec should exist"));
    (
        executable,
        started,
        survived,
        vec!["/C".into(), script.to_string_lossy().into_owned()],
    )
}

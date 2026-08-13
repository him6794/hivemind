use general_compute_runtime::sandbox::{
    CgroupPolicy, LinuxNamespace, LinuxSandboxPolicy, OciPrivilegeMode, PrivilegeEscalationPolicy,
    ProductionSandboxError, ProductionSandboxLaunch, ProductionSandboxLauncher,
    RootFilesystemPolicy, SandboxMount, SandboxNetworkPolicy, SandboxPolicyError, SeccompPolicy,
};
use general_compute_runtime::supervisor::Cancellation;

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
    }
}

fn valid_launch() -> ProductionSandboxLaunch {
    ProductionSandboxLaunch {
        backend_id: "python-numpy-scipy".into(),
        guest_image_digest: format!("sha256:{}", "a".repeat(64)),
        entrypoint: vec!["python".into(), "/runtime/runner.py".into()],
        policy: valid_policy(),
    }
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

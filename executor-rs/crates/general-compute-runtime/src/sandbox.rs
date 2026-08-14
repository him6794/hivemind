use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::sha256_digest;
use crate::supervisor::{
    Cancellation, ReferenceCommandSpec, ReferenceProcessSupervisor, RunResult,
};

/// Distinguishes the local reference oracle from a production OCI backend.
///
/// A production registration must never be executed by the direct-process
/// adapter. It is routed through [`ProductionSandboxLaunch`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendExecutionMode {
    ReferenceDirect,
    ProductionSandboxedOci,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OciPrivilegeMode {
    Rootless,
    Rootful,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CgroupPolicy {
    V2,
    LegacyOrMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "default_action", rename_all = "snake_case", deny_unknown_fields)]
pub enum SeccompPolicy {
    DefaultDeny { profile_sha256: String },
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegeEscalationPolicy {
    NoNewPrivileges,
    Allowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootFilesystemPolicy {
    ReadOnly,
    Writable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxNamespace {
    User,
    Pid,
    Mount,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxNetworkPolicy {
    DenyAll,
    AllowAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SandboxMount {
    ReadOnlyArtifact {
        artifact_id: String,
        destination: String,
    },
    EphemeralScratch {
        destination: String,
        max_bytes: u64,
    },
}

impl SandboxMount {
    fn destination(&self) -> &str {
        match self {
            Self::ReadOnlyArtifact { destination, .. }
            | Self::EphemeralScratch { destination, .. } => destination,
        }
    }
}

/// Required policy envelope for a Linux production OCI sandbox.
///
/// Validation is deliberately stricter than an OCI runtime's defaults: an
/// omitted namespace would otherwise share the host namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxSandboxPolicy {
    pub oci_privilege: OciPrivilegeMode,
    pub namespaces: Vec<LinuxNamespace>,
    pub cgroup: CgroupPolicy,
    pub seccomp: SeccompPolicy,
    pub privilege_escalation: PrivilegeEscalationPolicy,
    pub root_filesystem: RootFilesystemPolicy,
    pub network: SandboxNetworkPolicy,
    pub mounts: Vec<SandboxMount>,
}

impl LinuxSandboxPolicy {
    pub fn validate(&self) -> Result<(), SandboxPolicyError> {
        if self.oci_privilege != OciPrivilegeMode::Rootless {
            return Err(SandboxPolicyError::RootlessOciRequired);
        }

        let namespaces = self.namespaces.iter().copied().collect::<BTreeSet<_>>();
        for required in [
            LinuxNamespace::User,
            LinuxNamespace::Pid,
            LinuxNamespace::Mount,
            LinuxNamespace::Network,
        ] {
            if !namespaces.contains(&required) {
                return Err(SandboxPolicyError::MissingNamespace(required));
            }
        }
        if namespaces.len() != self.namespaces.len() {
            return Err(SandboxPolicyError::DuplicateNamespace);
        }
        if self.cgroup != CgroupPolicy::V2 {
            return Err(SandboxPolicyError::CgroupV2Required);
        }
        if !matches!(
            &self.seccomp,
            SeccompPolicy::DefaultDeny { profile_sha256 }
                if is_sha256_digest(profile_sha256)
        ) {
            return Err(SandboxPolicyError::SeccompProfileRequired);
        }
        if self.privilege_escalation != PrivilegeEscalationPolicy::NoNewPrivileges {
            return Err(SandboxPolicyError::NoNewPrivilegesRequired);
        }
        if self.root_filesystem != RootFilesystemPolicy::ReadOnly {
            return Err(SandboxPolicyError::ReadOnlyRootRequired);
        }
        if self.network != SandboxNetworkPolicy::DenyAll {
            return Err(SandboxPolicyError::NetworkDenyRequired);
        }
        if self.mounts.is_empty() {
            return Err(SandboxPolicyError::ExplicitMountsRequired);
        }

        let mut destinations = BTreeSet::new();
        for mount in &self.mounts {
            let destination = mount.destination();
            if !valid_mount_destination(destination) {
                return Err(SandboxPolicyError::InvalidMountDestination);
            }
            if !destinations.insert(destination) {
                return Err(SandboxPolicyError::DuplicateMountDestination);
            }
            match mount {
                SandboxMount::ReadOnlyArtifact { artifact_id, .. }
                    if artifact_id.trim().is_empty()
                        || artifact_id.split('/').any(|component| component == "..")
                        || artifact_id.split('\\').any(|component| component == "..")
                        || artifact_id.contains(':') =>
                {
                    return Err(SandboxPolicyError::InvalidMountSource);
                }
                SandboxMount::EphemeralScratch { max_bytes: 0, .. } => {
                    return Err(SandboxPolicyError::InvalidMountSource);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxPolicyError {
    RootlessOciRequired,
    MissingNamespace(LinuxNamespace),
    DuplicateNamespace,
    CgroupV2Required,
    SeccompProfileRequired,
    NoNewPrivilegesRequired,
    ReadOnlyRootRequired,
    NetworkDenyRequired,
    ExplicitMountsRequired,
    InvalidMountDestination,
    DuplicateMountDestination,
    InvalidMountSource,
}

impl std::fmt::Display for SandboxPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid production sandbox policy: {self:?}")
    }
}

impl std::error::Error for SandboxPolicyError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionSandboxLaunch {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub entrypoint: Vec<String>,
    pub policy: LinuxSandboxPolicy,
}

impl ProductionSandboxLaunch {
    pub fn validate(&self) -> Result<(), ProductionSandboxError> {
        self.policy
            .validate()
            .map_err(ProductionSandboxError::Policy)?;
        if self.backend_id.trim().is_empty() {
            return Err(ProductionSandboxError::InvalidBackendId);
        }
        if !is_sha256_digest(&self.guest_image_digest) {
            return Err(ProductionSandboxError::InvalidImageDigest);
        }
        if self.entrypoint.is_empty() || self.entrypoint.iter().any(|part| part.trim().is_empty()) {
            return Err(ProductionSandboxError::InvalidEntrypoint);
        }
        Ok(())
    }
}

/// A production launch can only report policy/platform/runner state here; it
/// never falls back to direct process execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionSandboxError {
    Policy(SandboxPolicyError),
    InvalidBackendId,
    InvalidImageDigest,
    InvalidEntrypoint,
    InvalidContainerId,
    InvalidBundle,
    BundleMetadataMismatch,
    RunnerNotPinned,
    RunnerDigestMismatch,
    UnsupportedPlatform,
    RunnerUnavailable,
    RunnerSpawn,
}

impl std::fmt::Display for ProductionSandboxError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "production sandbox unavailable: {self:?}")
    }
}

impl std::error::Error for ProductionSandboxError {}

/// The only public entry point for a production backend launch.
///
/// It validates the complete isolation envelope and fails closed until a real
/// rootless OCI runner is installed by a later milestone.
#[derive(Debug, Clone, Default)]
pub struct ProductionSandboxLauncher {
    runner_executable: Option<PathBuf>,
    runner_prefix_args: Vec<String>,
    runner_sha256: Option<String>,
    timeout: Duration,
    output_limit: usize,
    combined_output_limit: usize,
}

impl ProductionSandboxLauncher {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure an operator-pinned OCI runner executable and fixed argument
    /// prefix. Arguments are passed directly to `Command`; no shell is used.
    #[must_use]
    pub fn with_oci_runner_command<I, S>(executable: impl Into<PathBuf>, prefix_args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            runner_executable: Some(executable.into()),
            runner_prefix_args: prefix_args.into_iter().map(Into::into).collect(),
            runner_sha256: None,
            timeout: Duration::from_secs(30),
            output_limit: 16 * 1024 * 1024,
            combined_output_limit: 32 * 1024 * 1024,
        }
    }

    /// Pin the exact bytes of the operator-installed runner executable.
    #[must_use]
    pub fn with_runner_sha256(mut self, digest: impl Into<String>) -> Self {
        self.runner_sha256 = Some(digest.into());
        self
    }

    /// Bound the complete runner invocation, including its descendants.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Bound retained and combined runner diagnostics.
    #[must_use]
    pub fn with_output_limits(mut self, per_stream: usize, combined: usize) -> Self {
        self.output_limit = per_stream;
        self.combined_output_limit = combined;
        self
    }

    pub fn run(
        &self,
        launch: &ProductionSandboxLaunch,
        _cancellation: &Cancellation,
    ) -> Result<RunResult, ProductionSandboxError> {
        launch.validate()?;
        if cfg!(target_os = "linux") {
            Err(ProductionSandboxError::RunnerUnavailable)
        } else {
            Err(ProductionSandboxError::UnsupportedPlatform)
        }
    }

    /// Validate and execute an OCI bundle through the operator-pinned runner.
    ///
    /// This path intentionally accepts an already materialized bundle rather
    /// than arbitrary host commands. The OCI config is checked against the
    /// versioned launch envelope before the runner is spawned, and all runner
    /// arguments are passed without shell interpolation.
    pub fn run_bundle(
        &self,
        launch: &ProductionSandboxLaunch,
        bundle_root: &Path,
        container_id: &str,
        cancellation: &Cancellation,
    ) -> Result<RunResult, ProductionSandboxError> {
        launch.validate()?;
        if !valid_container_id(container_id) {
            return Err(ProductionSandboxError::InvalidContainerId);
        }
        let runner = self
            .runner_executable
            .as_ref()
            .ok_or(ProductionSandboxError::RunnerUnavailable)?;
        let runner_metadata =
            fs::symlink_metadata(runner).map_err(|_| ProductionSandboxError::RunnerNotPinned)?;
        if !runner.is_absolute()
            || !runner_metadata.file_type().is_file()
            || self.runner_sha256.is_none()
        {
            return Err(ProductionSandboxError::RunnerNotPinned);
        }
        let expected_runner_sha256 = self
            .runner_sha256
            .as_deref()
            .ok_or(ProductionSandboxError::RunnerNotPinned)?;
        if !is_sha256_digest(expected_runner_sha256) {
            return Err(ProductionSandboxError::RunnerNotPinned);
        }
        let actual_runner_sha256 =
            sha256_digest(&fs::read(runner).map_err(|_| ProductionSandboxError::RunnerSpawn)?);
        if actual_runner_sha256 != expected_runner_sha256 {
            return Err(ProductionSandboxError::RunnerDigestMismatch);
        }
        validate_oci_bundle(bundle_root, launch)?;

        self.run_validated_bundle(launch, bundle_root, container_id, cancellation)
    }

    /// Execute a task-specific bundle whose artifact bind sources were
    /// materialized below the operator-owned `artifact_root`.
    pub fn run_materialized_bundle(
        &self,
        launch: &ProductionSandboxLaunch,
        bundle_root: &Path,
        artifact_root: &Path,
        container_id: &str,
        cancellation: &Cancellation,
    ) -> Result<RunResult, ProductionSandboxError> {
        launch.validate()?;
        if !valid_container_id(container_id) {
            return Err(ProductionSandboxError::InvalidContainerId);
        }
        validate_oci_bundle_with_artifact_root(bundle_root, launch, artifact_root)?;
        self.run_validated_bundle(launch, bundle_root, container_id, cancellation)
    }

    fn run_validated_bundle(
        &self,
        _launch: &ProductionSandboxLaunch,
        bundle_root: &Path,
        container_id: &str,
        cancellation: &Cancellation,
    ) -> Result<RunResult, ProductionSandboxError> {
        let runner = self
            .runner_executable
            .as_ref()
            .ok_or(ProductionSandboxError::RunnerUnavailable)?;
        let runner_metadata =
            fs::symlink_metadata(runner).map_err(|_| ProductionSandboxError::RunnerNotPinned)?;
        if !runner.is_absolute()
            || !runner_metadata.file_type().is_file()
            || self.runner_sha256.is_none()
        {
            return Err(ProductionSandboxError::RunnerNotPinned);
        }
        let expected_runner_sha256 = self
            .runner_sha256
            .as_deref()
            .ok_or(ProductionSandboxError::RunnerNotPinned)?;
        if !is_sha256_digest(expected_runner_sha256) {
            return Err(ProductionSandboxError::RunnerNotPinned);
        }
        let actual_runner_sha256 =
            sha256_digest(&fs::read(runner).map_err(|_| ProductionSandboxError::RunnerSpawn)?);
        if actual_runner_sha256 != expected_runner_sha256 {
            return Err(ProductionSandboxError::RunnerDigestMismatch);
        }

        let command = ReferenceCommandSpec::new(runner.to_string_lossy(), {
            let mut args = self.runner_prefix_args.clone();
            args.extend([
                "run".to_owned(),
                "--bundle".to_owned(),
                bundle_root.to_string_lossy().into_owned(),
                container_id.to_owned(),
            ]);
            args
        })
        .with_timeout(self.timeout)
        .with_output_limit(self.output_limit)
        .with_combined_output_limit(self.combined_output_limit);
        // Keep the command construction in one place and make the no-shell
        // boundary explicit. The supervisor passes every argument directly to
        // Command and owns process-group/job cleanup.
        let result = ReferenceProcessSupervisor::new()
            .run_with_stdin(command, &[], cancellation)
            .map_err(|_| ProductionSandboxError::RunnerSpawn)?;
        Ok(result)
    }
}

fn valid_container_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_oci_bundle(
    bundle_root: &Path,
    launch: &ProductionSandboxLaunch,
) -> Result<(), ProductionSandboxError> {
    validate_oci_bundle_inner(bundle_root, launch, None)
}

fn validate_oci_bundle_with_artifact_root(
    bundle_root: &Path,
    launch: &ProductionSandboxLaunch,
    artifact_root: &Path,
) -> Result<(), ProductionSandboxError> {
    if !artifact_root.is_absolute()
        || fs::symlink_metadata(artifact_root)
            .map_err(|_| ProductionSandboxError::InvalidBundle)?
            .file_type()
            .is_symlink()
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let artifact_root =
        fs::canonicalize(artifact_root).map_err(|_| ProductionSandboxError::InvalidBundle)?;
    validate_oci_bundle_inner(bundle_root, launch, Some(&artifact_root))
}

fn validate_oci_bundle_inner(
    bundle_root: &Path,
    launch: &ProductionSandboxLaunch,
    artifact_root: Option<&Path>,
) -> Result<(), ProductionSandboxError> {
    if !bundle_root.is_absolute()
        || fs::symlink_metadata(bundle_root)
            .map_err(|_| ProductionSandboxError::InvalidBundle)?
            .file_type()
            .is_symlink()
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let bundle_root =
        fs::canonicalize(bundle_root).map_err(|_| ProductionSandboxError::InvalidBundle)?;
    let metadata =
        fs::symlink_metadata(&bundle_root).map_err(|_| ProductionSandboxError::InvalidBundle)?;
    if !metadata.is_dir() {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let rootfs = bundle_root.join("rootfs");
    if !fs::symlink_metadata(&rootfs)
        .map_err(|_| ProductionSandboxError::InvalidBundle)?
        .is_dir()
        || fs::canonicalize(&rootfs).map_err(|_| ProductionSandboxError::InvalidBundle)? != rootfs
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let config_path = bundle_root.join("config.json");
    let bytes = fs::read(config_path).map_err(|_| ProductionSandboxError::InvalidBundle)?;
    let config: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ProductionSandboxError::InvalidBundle)?;
    let Some(object) = config.as_object() else {
        return Err(ProductionSandboxError::InvalidBundle);
    };
    const ALLOWED_ROOT_KEYS: &[&str] = &[
        "ociVersion",
        "process",
        "root",
        "mounts",
        "linux",
        "annotations",
    ];
    if object
        .keys()
        .any(|key| !ALLOWED_ROOT_KEYS.contains(&key.as_str()))
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    if object.get("ociVersion").and_then(serde_json::Value::as_str) != Some("1.0.2") {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let process = config
        .get("process")
        .and_then(serde_json::Value::as_object)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    const ALLOWED_PROCESS_KEYS: &[&str] = &["args", "cwd", "noNewPrivileges", "user"];
    if process
        .keys()
        .any(|key| !ALLOWED_PROCESS_KEYS.contains(&key.as_str()))
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let args = process
        .get("args")
        .and_then(serde_json::Value::as_array)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    let user = process
        .get("user")
        .and_then(serde_json::Value::as_object)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    if user
        .keys()
        .any(|key| !["uid", "gid"].contains(&key.as_str()))
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let expected_args = launch
        .entrypoint
        .iter()
        .map(|part| serde_json::Value::String(part.clone()))
        .collect::<Vec<_>>();
    if args != &expected_args
        || process
            .get("noNewPrivileges")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        || process
            .get("user")
            .and_then(serde_json::Value::as_object)
            .and_then(|user| user.get("uid"))
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|uid| uid == 0)
        || process
            .get("user")
            .and_then(serde_json::Value::as_object)
            .and_then(|user| user.get("gid"))
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|gid| gid == 0)
    {
        return Err(ProductionSandboxError::BundleMetadataMismatch);
    }
    let root = config
        .get("root")
        .and_then(serde_json::Value::as_object)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    if root
        .keys()
        .any(|key| !["path", "readonly"].contains(&key.as_str()))
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    if root.get("path").and_then(serde_json::Value::as_str) != Some("rootfs")
        || root.get("readonly").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Err(ProductionSandboxError::BundleMetadataMismatch);
    }
    let linux = config
        .get("linux")
        .and_then(serde_json::Value::as_object)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    if linux
        .keys()
        .any(|key| !["namespaces", "seccomp"].contains(&key.as_str()))
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let namespaces = linux
        .get("namespaces")
        .and_then(serde_json::Value::as_array)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    let namespace_types = namespaces
        .iter()
        .map(|namespace| {
            let namespace = namespace
                .as_object()
                .ok_or(ProductionSandboxError::BundleMetadataMismatch)?;
            if namespace.keys().any(|key| key != "type") {
                return Err(ProductionSandboxError::InvalidBundle);
            }
            namespace
                .get("type")
                .and_then(serde_json::Value::as_str)
                .ok_or(ProductionSandboxError::BundleMetadataMismatch)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if namespace_types
        != ["mount", "network", "pid", "user"]
            .into_iter()
            .collect::<BTreeSet<_>>()
        || namespaces.len() != namespace_types.len()
    {
        return Err(ProductionSandboxError::BundleMetadataMismatch);
    }
    let seccomp = linux
        .get("seccomp")
        .and_then(serde_json::Value::as_object)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    if seccomp.keys().any(|key| key != "defaultAction")
        || seccomp
            .get("defaultAction")
            .and_then(serde_json::Value::as_str)
            != Some("SCMP_ACT_ERRNO")
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let mounts = config
        .get("mounts")
        .and_then(serde_json::Value::as_array)
        .ok_or(ProductionSandboxError::BundleMetadataMismatch)?;
    let expected_mounts = launch
        .policy
        .mounts
        .iter()
        .map(|mount| match mount {
            crate::sandbox::SandboxMount::ReadOnlyArtifact {
                artifact_id,
                destination,
            } => {
                let source = artifact_root
                    .map(|root| root.join(artifact_id).to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("/hivemind/artifacts/{artifact_id}"));
                serde_json::json!({
                    "destination": destination,
                    "type": "bind",
                    "source": source,
                    "options": ["ro", "nodev", "nosuid", "noexec"]
                })
            }
            crate::sandbox::SandboxMount::EphemeralScratch {
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
        .collect::<Vec<_>>();
    if mounts != expected_mounts.as_slice() {
        return Err(ProductionSandboxError::BundleMetadataMismatch);
    }
    let annotations = config
        .get("annotations")
        .and_then(serde_json::Value::as_object)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    const ALLOWED_ANNOTATIONS: &[&str] = &[
        "org.hivemind.guest-image-digest",
        "org.hivemind.backend-id",
        "org.hivemind.cgroup-version",
        "org.hivemind.network-policy",
        "org.hivemind.seccomp-profile-sha256",
    ];
    if annotations
        .keys()
        .any(|key| !ALLOWED_ANNOTATIONS.contains(&key.as_str()))
    {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    if annotations
        .get("org.hivemind.guest-image-digest")
        .and_then(serde_json::Value::as_str)
        != Some(launch.guest_image_digest.as_str())
        || annotations
            .get("org.hivemind.backend-id")
            .and_then(serde_json::Value::as_str)
            != Some(launch.backend_id.as_str())
        || annotations
            .get("org.hivemind.cgroup-version")
            .and_then(serde_json::Value::as_str)
            != Some("v2")
        || annotations
            .get("org.hivemind.network-policy")
            .and_then(serde_json::Value::as_str)
            != Some("deny_all")
        || annotations
            .get("org.hivemind.seccomp-profile-sha256")
            .and_then(serde_json::Value::as_str)
            != Some(expected_seccomp_profile(&launch.policy).as_str())
    {
        return Err(ProductionSandboxError::BundleMetadataMismatch);
    }
    Ok(())
}

fn expected_seccomp_profile(policy: &LinuxSandboxPolicy) -> String {
    match &policy.seccomp {
        SeccompPolicy::DefaultDeny { profile_sha256 } => profile_sha256.clone(),
        SeccompPolicy::Disabled => String::new(),
    }
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_mount_destination(destination: &str) -> bool {
    destination.starts_with('/')
        && destination != "/"
        && !destination.ends_with('/')
        && !destination.split('/').any(|component| component == "..")
}

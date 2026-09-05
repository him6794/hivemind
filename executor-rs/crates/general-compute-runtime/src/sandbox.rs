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
    ProductionSandboxedWindows,
    ProductionSandboxedDsl,
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

/// One host device node exposed to the sandbox through the OCI
/// `linux.devices` array plus its cgroup device rule.
///
/// Every field is explicit: a wildcard or "let runc decide" entry would let a
/// bundle widen its own hardware access, so the operator registration must
/// enumerate exact nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxDevice {
    pub path: String,
    pub device_type: SandboxDeviceType,
    pub major: i64,
    pub minor: i64,
    /// OCI access string, e.g. "rwm". Only read/write/mknod are expressible.
    pub access: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxDeviceType {
    Block,
    Char,
}

impl SandboxDevice {
    pub fn validate(&self) -> Result<(), SandboxPolicyError> {
        if !self.path.starts_with("/dev/")
            || self.path.len() <= "/dev/".len()
            || self.path.split('/').any(|component| component == "..")
            || self.path.contains(':')
        {
            return Err(SandboxPolicyError::InvalidDevicePath);
        }
        if !matches!(
            self.device_type,
            SandboxDeviceType::Char | SandboxDeviceType::Block
        ) {
            return Err(SandboxPolicyError::InvalidDeviceSpec);
        }
        if self.major < 0
            || self.minor < 0
            || self.major > i64::MAX / 2
            || self.minor > i64::MAX / 2
        {
            return Err(SandboxPolicyError::InvalidDeviceSpec);
        }
        if self.access.is_empty()
            || self.access.len() > 3
            || !self
                .access
                .bytes()
                .all(|byte| matches!(byte, b'r' | b'w' | b'm'))
            || {
                let bytes = self.access.as_bytes();
                bytes.iter().collect::<BTreeSet<_>>().len() != bytes.len()
            }
        {
            return Err(SandboxPolicyError::InvalidDeviceAccess);
        }
        Ok(())
    }

    pub(crate) fn cgroup_rule(&self) -> serde_json::Value {
        let kind = match self.device_type {
            SandboxDeviceType::Char => "c",
            SandboxDeviceType::Block => "b",
        };
        serde_json::json!({
            "allow": true,
            "type": kind,
            "major": self.major,
            "minor": self.minor,
            "access": self.access,
        })
    }

    /// Render the OCI device object. The access mask belongs to the cgroup
    /// rule, not to an OCI `linux.devices` entry.
    pub(crate) fn oci_spec(&self) -> serde_json::Value {
        let kind = match self.device_type {
            SandboxDeviceType::Char => "c",
            SandboxDeviceType::Block => "b",
        };
        serde_json::json!({
            "path": self.path,
            "type": kind,
            "major": self.major,
            "minor": self.minor,
        })
    }
}

/// The default character devices required by the OCI runtime for a
/// non-interactive process. `/dev/ptmx` is supplied by the fixed devpts mount
/// and is intentionally not listed as a host device.
pub(crate) fn standard_linux_devices() -> Vec<SandboxDevice> {
    [
        ("/dev/null", 1, 3),
        ("/dev/zero", 1, 5),
        ("/dev/full", 1, 7),
        ("/dev/random", 1, 8),
        ("/dev/urandom", 1, 9),
        ("/dev/tty", 5, 0),
    ]
    .into_iter()
    .map(|(path, major, minor)| SandboxDevice {
        path: path.into(),
        device_type: SandboxDeviceType::Char,
        major,
        minor,
        access: "rwm".into(),
    })
    .collect()
}

/// Fixed mounts needed by the OCI runtime itself. They are not part of the
/// operator artifact policy and cannot be supplied or overridden by a task.
pub(crate) fn standard_linux_mounts() -> Vec<serde_json::Value> {
    vec![
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
    ]
}

/// One segment of the rootless OCI user/group mapping.
///
/// The first segment maps the invoking operator identity to container root;
/// the second segment maps the operator's subordinate range. A mapping that
/// starts at the subordinate range without the invoking identity makes runc
/// attempt to chown its synchronization pipe to an unmapped host id and fails
/// before the container starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxIdMapping {
    pub container_id: u32,
    pub host_id: u32,
    pub size: u32,
}

impl LinuxIdMapping {
    pub(crate) fn oci_spec(&self) -> serde_json::Value {
        serde_json::json!({
            "containerID": self.container_id,
            "hostID": self.host_id,
            "size": self.size,
        })
    }
}

/// Resolve the host-specific rootless mapping used by both bundle materializer
/// and preflight validator. The result is operator/host state, never task
/// input, and is deliberately unavailable when subordinate ids are missing.
pub fn rootless_id_mappings() -> Result<(Vec<LinuxIdMapping>, Vec<LinuxIdMapping>), String> {
    #[cfg(unix)]
    {
        let uid = current_linux_id("Uid")?;
        let gid = current_linux_id("Gid")?;
        let uid_range = subordinate_id_range("/etc/subuid", uid)?;
        let gid_range = subordinate_id_range("/etc/subgid", gid)?;
        let uid_mappings = rootless_mapping(uid, uid_range)?;
        let gid_mappings = rootless_mapping(gid, gid_range)?;
        Ok((uid_mappings, gid_mappings))
    }
    #[cfg(not(unix))]
    {
        let mapping = vec![LinuxIdMapping {
            container_id: 0,
            host_id: 100_000,
            size: 65_536,
        }];
        Ok((mapping.clone(), mapping))
    }
}

#[cfg(unix)]
fn current_linux_id(field: &str) -> Result<u32, String> {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|error| format!("cannot read current process identity: {error}"))?;
    let value = status
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{field}:")))
        .and_then(|line| line.split_whitespace().next())
        .ok_or_else(|| format!("current process identity field {field} is unavailable"))?;
    value
        .parse::<u32>()
        .map_err(|_| format!("current process identity field {field} is invalid"))
}

#[cfg(unix)]
fn subordinate_id_range(path: &str, id: u32) -> Result<(u32, u32), String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("subordinate id database {path} is unavailable: {error}"))?;
    let numeric_id = id.to_string();
    let username = fs::read_to_string("/etc/passwd").ok().and_then(|passwd| {
        passwd.lines().find_map(|line| {
            let fields = line.split(':').collect::<Vec<_>>();
            (fields.len() > 3 && fields[2] == numeric_id).then(|| fields[0].to_owned())
        })
    });
    for line in contents.lines() {
        let fields = line.split(':').collect::<Vec<_>>();
        if fields.len() != 3 || (fields[0] != numeric_id && username.as_deref() != Some(fields[0]))
        {
            continue;
        }
        let start = fields[1]
            .parse::<u32>()
            .map_err(|_| format!("subordinate id start in {path} is invalid"))?;
        let size = fields[2]
            .parse::<u32>()
            .map_err(|_| format!("subordinate id size in {path} is invalid"))?;
        if size < 65_535 || start == id || start.checked_add(size).is_none() {
            return Err(format!(
                "subordinate id range in {path} cannot map the sandbox"
            ));
        }
        return Ok((start, size));
    }
    Err(format!(
        "no subordinate id range for the current identity in {path}"
    ))
}

#[cfg(unix)]
fn rootless_mapping(id: u32, range: (u32, u32)) -> Result<Vec<LinuxIdMapping>, String> {
    let (start, size) = range;
    if start == id || start.checked_add(size).is_none() {
        return Err("rootless id mappings overlap or overflow".into());
    }
    Ok(vec![
        LinuxIdMapping {
            container_id: 0,
            host_id: id,
            size: 1,
        },
        LinuxIdMapping {
            container_id: 1,
            host_id: start,
            size: 65_535.min(size),
        },
    ])
}

/// Runtime-owned mount points cannot be shadowed by an artifact or scratch
/// mount. This includes descendants because mount ordering would otherwise
/// let the policy alter the runtime's pseudo-filesystems.
pub(crate) fn conflicts_with_standard_linux_mount(destination: &str) -> bool {
    ["/proc", "/dev", "/dev/pts"]
        .into_iter()
        .any(|reserved| destination == reserved || destination.starts_with(&format!("{reserved}/")))
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
    #[serde(default)]
    pub devices: Vec<SandboxDevice>,
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
            if conflicts_with_standard_linux_mount(destination) {
                return Err(SandboxPolicyError::ReservedMountDestination);
            }
            if !destinations.insert(destination) {
                return Err(SandboxPolicyError::DuplicateMountDestination);
            }
            match mount {
                SandboxMount::ReadOnlyArtifact { artifact_id, .. }
                    if crate::validate_artifact_id(artifact_id).is_err() =>
                {
                    return Err(SandboxPolicyError::InvalidMountSource);
                }
                SandboxMount::EphemeralScratch { max_bytes: 0, .. } => {
                    return Err(SandboxPolicyError::InvalidMountSource);
                }
                _ => {}
            }
        }

        let mut device_paths = BTreeSet::new();
        for device in &self.devices {
            device.validate()?;
            if standard_linux_devices()
                .iter()
                .any(|standard| standard.path == device.path)
            {
                return Err(SandboxPolicyError::ReservedDevicePath);
            }
            if !device_paths.insert(device.path.as_str()) {
                return Err(SandboxPolicyError::DuplicateDevicePath);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsIsolationMode {
    Process,
    HyperV,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsSandboxNetworkPolicy {
    DenyAll,
    AllowAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsRootFilesystemPolicy {
    ReadOnly,
    Writable,
}

/// Operator-enforced policy for native Windows process-isolated containers.
///
/// This is deliberately separate from [`LinuxSandboxPolicy`]. A Windows
/// worker must never reinterpret Linux namespaces or seccomp fields as a
/// Windows security boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsSandboxPolicy {
    pub isolation: WindowsIsolationMode,
    pub network: WindowsSandboxNetworkPolicy,
    pub root_filesystem: WindowsRootFilesystemPolicy,
    pub mounts: Vec<SandboxMount>,
    pub memory_bytes: u64,
    pub cpu_millis: u64,
    pub process_limit: u32,
    pub thread_limit: u32,
    pub scratch_bytes: u64,
}

impl WindowsSandboxPolicy {
    pub fn validate(&self) -> Result<(), WindowsSandboxPolicyError> {
        if self.isolation != WindowsIsolationMode::Process {
            return Err(WindowsSandboxPolicyError::ProcessIsolationRequired);
        }
        if self.network != WindowsSandboxNetworkPolicy::DenyAll {
            return Err(WindowsSandboxPolicyError::NetworkDenyRequired);
        }
        if self.root_filesystem != WindowsRootFilesystemPolicy::ReadOnly {
            return Err(WindowsSandboxPolicyError::ReadOnlyRootRequired);
        }
        if self.mounts.is_empty() {
            return Err(WindowsSandboxPolicyError::ExplicitMountsRequired);
        }
        if self.memory_bytes == 0
            || self.cpu_millis == 0
            || self.process_limit == 0
            || self.thread_limit == 0
            || self.scratch_bytes == 0
        {
            return Err(WindowsSandboxPolicyError::ResourceLimitsRequired);
        }

        let mut destinations = BTreeSet::new();
        let mut has_artifact = false;
        let mut has_scratch = false;
        for mount in &self.mounts {
            let destination = mount.destination();
            if !valid_mount_destination(destination) {
                return Err(WindowsSandboxPolicyError::InvalidMountDestination);
            }
            if !destinations.insert(destination) {
                return Err(WindowsSandboxPolicyError::DuplicateMountDestination);
            }
            match mount {
                SandboxMount::ReadOnlyArtifact { artifact_id, .. }
                    if crate::validate_artifact_id(artifact_id).is_err() =>
                {
                    return Err(WindowsSandboxPolicyError::InvalidMountSource);
                }
                SandboxMount::ReadOnlyArtifact { .. } => has_artifact = true,
                SandboxMount::EphemeralScratch { max_bytes: 0, .. } => {
                    return Err(WindowsSandboxPolicyError::InvalidMountSource);
                }
                SandboxMount::EphemeralScratch { max_bytes, .. } => {
                    if *max_bytes > self.scratch_bytes {
                        return Err(WindowsSandboxPolicyError::ScratchLimitExceeded);
                    }
                    has_scratch = true;
                }
            }
        }
        if !has_artifact || !has_scratch {
            return Err(WindowsSandboxPolicyError::ExplicitArtifactAndScratchMountsRequired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsSandboxPolicyError {
    ProcessIsolationRequired,
    NetworkDenyRequired,
    ReadOnlyRootRequired,
    ExplicitMountsRequired,
    ExplicitArtifactAndScratchMountsRequired,
    ResourceLimitsRequired,
    ScratchLimitExceeded,
    InvalidMountDestination,
    DuplicateMountDestination,
    InvalidMountSource,
}

impl std::fmt::Display for WindowsSandboxPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid Windows production sandbox policy: {self:?}"
        )
    }
}

impl std::error::Error for WindowsSandboxPolicyError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsNativeSandboxLaunch {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub entrypoint: Vec<String>,
    pub policy: WindowsSandboxPolicy,
}

impl WindowsNativeSandboxLaunch {
    pub fn validate(&self) -> Result<(), ProductionSandboxError> {
        self.policy
            .validate()
            .map_err(ProductionSandboxError::WindowsPolicy)?;
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
    ReservedMountDestination,
    InvalidDevicePath,
    InvalidDeviceSpec,
    InvalidDeviceAccess,
    ReservedDevicePath,
    DuplicateDevicePath,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onnx: Option<crate::onnx::OnnxBackendConfig>,
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
        if let Some(onnx) = &self.onnx {
            onnx.validate()
                .map_err(|_| ProductionSandboxError::BundleMetadataMismatch)?;
            if onnx.model_artifact_id != "source" {
                return Err(ProductionSandboxError::BundleMetadataMismatch);
            }
        }
        Ok(())
    }
}

/// A production launch can only report policy/platform/runner state here; it
/// never falls back to direct process execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionSandboxError {
    Policy(SandboxPolicyError),
    WindowsPolicy(WindowsSandboxPolicyError),
    InvalidBackendId,
    InvalidImageDigest,
    InvalidEntrypoint,
    InvalidContainerId,
    InvalidBundle,
    BundleMetadataMismatch,
    RunnerNotPinned,
    RunnerDigestMismatch,
    RunnerStateRootUnavailable,
    UnsupportedPlatform,
    RunnerUnavailable,
    RunnerSpawn,
    RunnerSpawnDetail(String),
    DeviceUnavailable(String),
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
    runner_state_root: Option<PathBuf>,
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
            runner_state_root: None,
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

    /// Configure the operator-owned writable state directory used by the OCI
    /// runner (runc's `--root` path). The directory is validated again at
    /// execution time so a replaced or symlinked volume fails closed.
    #[must_use]
    pub fn with_runner_state_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.runner_state_root = Some(root.into());
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
        #[cfg(unix)]
        validate_host_device_sources(launch)?;

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
        self.runner_state_root
            .as_deref()
            .ok_or(ProductionSandboxError::RunnerStateRootUnavailable)
            .and_then(validate_runner_state_root)?;
        validate_oci_bundle_with_artifact_root(bundle_root, launch, artifact_root)?;
        #[cfg(unix)]
        validate_host_device_sources(launch)?;
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

        let runner_state_root = self
            .runner_state_root
            .as_deref()
            .map(validate_runner_state_root)
            .transpose()?;

        let command = ReferenceCommandSpec::new(runner.to_string_lossy(), {
            let mut args = self.runner_prefix_args.clone();
            if let Some(root) = runner_state_root {
                args.extend(["--root".to_owned(), root.to_string_lossy().into_owned()]);
            }
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
            .run_with_stdin(&command, &[], cancellation)
            .map_err(|error| ProductionSandboxError::RunnerSpawnDetail(format!("{error:?}")))?;
        Ok(result)
    }
}

fn validate_runner_state_root(root: &Path) -> Result<&Path, ProductionSandboxError> {
    if !root.is_absolute()
        || root.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(ProductionSandboxError::RunnerStateRootUnavailable);
    }
    let metadata = fs::symlink_metadata(root)
        .map_err(|_| ProductionSandboxError::RunnerStateRootUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ProductionSandboxError::RunnerStateRootUnavailable);
    }
    Ok(root)
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

#[cfg(unix)]
fn validate_host_device_sources(
    launch: &ProductionSandboxLaunch,
) -> Result<(), ProductionSandboxError> {
    let mut devices = standard_linux_devices();
    devices.extend(launch.policy.devices.iter().cloned());
    validate_host_device_sources_inner(&devices)
}

#[cfg(unix)]
fn validate_host_device_sources_inner(
    devices: &[SandboxDevice],
) -> Result<(), ProductionSandboxError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    for device in devices {
        let metadata = fs::symlink_metadata(&device.path).map_err(|error| {
            ProductionSandboxError::DeviceUnavailable(format!(
                "device source {} is unavailable: {error}",
                device.path
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ProductionSandboxError::DeviceUnavailable(format!(
                "device source {} is a symlink",
                device.path
            )));
        }
        let type_matches = match device.device_type {
            SandboxDeviceType::Char => metadata.file_type().is_char_device(),
            SandboxDeviceType::Block => metadata.file_type().is_block_device(),
        };
        let raw_device = metadata.rdev();
        if !type_matches
            || linux_device_major(raw_device).cast_signed() != device.major
            || linux_device_minor(raw_device).cast_signed() != device.minor
        {
            return Err(ProductionSandboxError::DeviceUnavailable(format!(
                "device source {} does not match the pinned type or identity",
                device.path
            )));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn linux_device_major(device: u64) -> u64 {
    ((device >> 8) & 0x0fff) | ((device >> 32) & 0xffff_f000)
}

#[cfg(unix)]
fn linux_device_minor(device: u64) -> u64 {
    (device & 0x00ff) | ((device >> 12) & 0xffff_ff00)
}

const ALLOWED_OCI_ROOT_KEYS: &[&str] = &[
    "ociVersion",
    "process",
    "root",
    "mounts",
    "linux",
    "annotations",
];
const ALLOWED_OCI_PROCESS_KEYS: &[&str] = &["args", "cwd", "noNewPrivileges", "user"];
const ALLOWED_OCI_ANNOTATIONS: &[&str] = &[
    "org.hivemind.guest-image-digest",
    "org.hivemind.backend-id",
    "org.hivemind.cgroup-version",
    "org.hivemind.network-policy",
    "org.hivemind.seccomp-profile-sha256",
    "org.hivemind.workload",
    "org.hivemind.onnx.protocol",
    "org.hivemind.onnx.execution-provider",
    "org.hivemind.onnx.model-artifact-id",
    "org.hivemind.onnx.input-artifact-ids",
];

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
    if object
        .keys()
        .any(|key| !ALLOWED_OCI_ROOT_KEYS.contains(&key.as_str()))
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
    if process
        .keys()
        .any(|key| !ALLOWED_OCI_PROCESS_KEYS.contains(&key.as_str()))
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
    if linux.keys().any(|key| {
        ![
            "namespaces",
            "uidMappings",
            "gidMappings",
            "seccomp",
            "devices",
            "resources",
        ]
        .contains(&key.as_str())
    }) {
        return Err(ProductionSandboxError::InvalidBundle);
    }
    let (expected_uid_mappings, expected_gid_mappings) =
        rootless_id_mappings().map_err(|_| ProductionSandboxError::BundleMetadataMismatch)?;
    let expected_uid_mapping = serde_json::Value::Array(
        expected_uid_mappings
            .iter()
            .map(LinuxIdMapping::oci_spec)
            .collect(),
    );
    let expected_gid_mapping = serde_json::Value::Array(
        expected_gid_mappings
            .iter()
            .map(LinuxIdMapping::oci_spec)
            .collect(),
    );
    if linux.get("uidMappings") != Some(&expected_uid_mapping)
        || linux.get("gidMappings") != Some(&expected_gid_mapping)
    {
        return Err(ProductionSandboxError::BundleMetadataMismatch);
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
    if artifact_root.is_some() {
        if !valid_materialized_seccomp_profile(seccomp) {
            return Err(ProductionSandboxError::InvalidBundle);
        }
    } else if seccomp.keys().any(|key| key != "defaultAction")
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
    let mut expected_mounts = standard_linux_mounts();
    expected_mounts.extend(
        launch
            .policy
            .mounts
            .iter()
            .map(|mount| match mount {
                crate::sandbox::SandboxMount::ReadOnlyArtifact {
                    artifact_id,
                    destination,
                } => {
                    let source = artifact_root.map_or_else(
                        || format!("/hivemind/artifacts/{artifact_id}"),
                        |root| root.join(artifact_id).to_string_lossy().into_owned(),
                    );
                    serde_json::json!({
                        "destination": destination,
                        "type": "bind",
                        "source": source,
                        "options": ["bind", "ro", "nodev", "nosuid", "noexec"]
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
            .collect::<Vec<_>>(),
    );
    if mounts != expected_mounts.as_slice() {
        return Err(ProductionSandboxError::BundleMetadataMismatch);
    }
    // Runtime-owned character devices are always present. Operator devices
    // are appended only after trusted GPU selection and remain exact.
    let mut expected_device_specs = standard_linux_devices();
    expected_device_specs.extend(launch.policy.devices.iter().cloned());
    let expected_devices: Vec<serde_json::Value> = expected_device_specs
        .iter()
        .map(SandboxDevice::oci_spec)
        .collect();
    let expected_cgroup_rules: Vec<serde_json::Value> = expected_device_specs
        .iter()
        .map(SandboxDevice::cgroup_rule)
        .collect();
    let bundle_devices = linux.get("devices").and_then(serde_json::Value::as_array);
    let bundle_resources_devices = linux
        .get("resources")
        .and_then(serde_json::Value::as_object)
        .and_then(|resources| resources.get("devices"))
        .and_then(serde_json::Value::as_array);
    match (bundle_devices, bundle_resources_devices) {
        (Some(bundle_devices), Some(bundle_resources_devices)) => {
            if bundle_devices != &expected_devices
                || bundle_resources_devices != &expected_cgroup_rules
            {
                return Err(ProductionSandboxError::BundleMetadataMismatch);
            }
        }
        _ => return Err(ProductionSandboxError::BundleMetadataMismatch),
    }
    let annotations = config
        .get("annotations")
        .and_then(serde_json::Value::as_object)
        .ok_or(ProductionSandboxError::InvalidBundle)?;
    if annotations
        .keys()
        .any(|key| !ALLOWED_OCI_ANNOTATIONS.contains(&key.as_str()))
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
    match &launch.onnx {
        None => {
            if [
                "org.hivemind.workload",
                "org.hivemind.onnx.protocol",
                "org.hivemind.onnx.execution-provider",
                "org.hivemind.onnx.model-artifact-id",
                "org.hivemind.onnx.input-artifact-ids",
            ]
            .iter()
            .any(|key| annotations.contains_key(*key))
            {
                return Err(ProductionSandboxError::BundleMetadataMismatch);
            }
        }
        Some(onnx) => {
            let expected_input_ids = serde_json::json!(
                serde_json::to_string(&onnx.input_artifact_ids)
                    .expect("ONNX input artifact IDs serialize infallibly")
            );
            if annotations
                .get("org.hivemind.workload")
                .and_then(serde_json::Value::as_str)
                != Some("onnx")
                || annotations
                    .get("org.hivemind.onnx.protocol")
                    .and_then(serde_json::Value::as_str)
                    != Some(onnx.protocol_version.as_str())
                || annotations
                    .get("org.hivemind.onnx.execution-provider")
                    .and_then(serde_json::Value::as_str)
                    != Some(onnx.execution_provider.as_str())
                || annotations
                    .get("org.hivemind.onnx.model-artifact-id")
                    .and_then(serde_json::Value::as_str)
                    != Some(onnx.model_artifact_id.as_str())
                || annotations.get("org.hivemind.onnx.input-artifact-ids")
                    != Some(&expected_input_ids)
            {
                return Err(ProductionSandboxError::BundleMetadataMismatch);
            }
        }
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

fn valid_materialized_seccomp_profile(
    profile: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    if profile
        .keys()
        .any(|key| !matches!(key.as_str(), "defaultAction" | "architectures" | "syscalls"))
        || profile
            .get("defaultAction")
            .and_then(serde_json::Value::as_str)
            != Some("SCMP_ACT_ERRNO")
    {
        return false;
    }
    if let Some(architectures) = profile.get("architectures") {
        let Some(architectures) = architectures.as_array() else {
            return false;
        };
        if architectures.is_empty()
            || architectures
                .iter()
                .any(|architecture| architecture.as_str().is_none())
        {
            return false;
        }
    }
    let Some(syscalls) = profile
        .get("syscalls")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    if syscalls.is_empty() {
        return false;
    }
    let mut names = BTreeSet::new();
    for group in syscalls {
        let Some(group) = group.as_object() else {
            return false;
        };
        if group
            .keys()
            .any(|key| !matches!(key.as_str(), "names" | "action"))
            || group.get("action").and_then(serde_json::Value::as_str) != Some("SCMP_ACT_ALLOW")
        {
            return false;
        }
        let Some(syscall_names) = group.get("names").and_then(serde_json::Value::as_array) else {
            return false;
        };
        if syscall_names.is_empty() {
            return false;
        }
        for name in syscall_names {
            let Some(name) = name.as_str() else {
                return false;
            };
            if name.trim().is_empty() || !names.insert(name) {
                return false;
            }
        }
    }
    true
}

fn valid_mount_destination(destination: &str) -> bool {
    destination.starts_with('/')
        && destination != "/"
        && !destination.ends_with('/')
        && !destination.split('/').any(|component| component == "..")
}

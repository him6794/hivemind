use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::supervisor::{Cancellation, RunResult};

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
                    if artifact_id.trim().is_empty() =>
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
    UnsupportedPlatform,
    RunnerUnavailable,
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
#[derive(Debug, Clone, Copy, Default)]
pub struct ProductionSandboxLauncher;

impl ProductionSandboxLauncher {
    #[must_use]
    pub const fn new() -> Self {
        Self
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

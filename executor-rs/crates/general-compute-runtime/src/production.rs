//! Operator-owned configuration and routing for production OCI backends.
//!
//! This module deliberately contains no URL, command-line, or Worker-provided
//! path interpretation. Configuration is loaded by the Worker from an
//! operator-controlled file and every path is validated before it can reach
//! the OCI launcher.

use crate::GeneralComputeRequest;
use crate::sandbox::{BackendExecutionMode, ProductionSandboxLaunch, SandboxMount};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionBackendConfig {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub bundle_root: PathBuf,
    pub artifact_root: PathBuf,
    pub runner_executable: PathBuf,
    pub runner_prefix_args: Vec<String>,
    pub runner_sha256: String,
    pub entrypoint: Vec<String>,
    pub policy: crate::sandbox::LinuxSandboxPolicy,
    pub max_output_bytes: usize,
}

impl ProductionBackendConfig {
    pub fn execution_mode(&self) -> BackendExecutionMode {
        BackendExecutionMode::ProductionSandboxedOci
    }

    pub fn launch(&self) -> ProductionSandboxLaunch {
        ProductionSandboxLaunch {
            backend_id: self.backend_id.clone(),
            guest_image_digest: self.guest_image_digest.clone(),
            entrypoint: self.entrypoint.clone(),
            policy: self.policy.clone(),
        }
    }

    /// Return the operator-owned task directory after validating that the
    /// task id cannot escape this backend's roots. The directory is created by
    /// the Worker materializer, never by a caller-provided path.
    pub fn task_root(
        &self,
        task_id: &str,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        if !is_safe_task_id(task_id) {
            return Err(ProductionBackendRegistryError::UnsafeTaskId);
        }
        ensure_no_symlink_ancestors(&self.bundle_root)?;
        ensure_no_symlink_ancestors(&self.artifact_root)?;
        let bundle_root = self.bundle_root.join(task_id);
        let artifact_root = self.artifact_root.join(task_id);
        ensure_contained(&self.bundle_root, &bundle_root)?;
        ensure_contained(&self.artifact_root, &artifact_root)?;
        // The task id is untrusted input.  Check the exact task directories as
        // well as their configured ancestors before any create/open operation;
        // otherwise a pre-existing task symlink could redirect bundle or
        // artifact writes outside the operator roots.
        ensure_no_symlink_ancestors(&bundle_root)?;
        ensure_no_symlink_ancestors(&artifact_root)?;
        Ok((bundle_root, artifact_root))
    }

    /// Validate that every request artifact has a mount declared by the
    /// operator policy. The source and all inputs are materialized beneath the
    /// task-specific artifact root, so the OCI config never receives a
    /// Worker-provided filesystem path.
    pub fn validate_request_mounts(
        &self,
        request: &GeneralComputeRequest,
    ) -> Result<(), ProductionBackendRegistryError> {
        request
            .validate()
            .map_err(|error| ProductionBackendRegistryError::RequestInvalid(error.message))?;
        let declared = self
            .policy
            .mounts
            .iter()
            .filter_map(|mount| match mount {
                SandboxMount::ReadOnlyArtifact { artifact_id, .. } => Some(artifact_id.as_str()),
                SandboxMount::EphemeralScratch { .. } => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        let requested = std::iter::once(&request.source_artifact)
            .chain(request.input_artifacts.iter())
            .map(|artifact| artifact.artifact_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for artifact in
            std::iter::once(&request.source_artifact).chain(request.input_artifacts.iter())
        {
            if !declared.contains(artifact.artifact_id.as_str()) {
                return Err(ProductionBackendRegistryError::ArtifactMountRequired(
                    artifact.artifact_id.clone(),
                ));
            }
        }
        for artifact_id in declared {
            if !requested.contains(artifact_id) {
                return Err(ProductionBackendRegistryError::ArtifactMountNotRequested(
                    artifact_id.to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ProductionBackendRegistryError> {
        if self.backend_id.trim().is_empty() {
            return Err(ProductionBackendRegistryError::EmptyBackendId);
        }
        if self.max_output_bytes == 0 {
            return Err(ProductionBackendRegistryError::ZeroOutputLimit);
        }
        for path in [
            &self.bundle_root,
            &self.artifact_root,
            &self.runner_executable,
        ] {
            if !path.is_absolute() {
                return Err(ProductionBackendRegistryError::PathMustBeAbsolute);
            }
            if path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            }) {
                return Err(ProductionBackendRegistryError::PathTraversal);
            }
        }
        if !is_sha256_digest(&self.runner_sha256) {
            return Err(ProductionBackendRegistryError::RunnerDigestInvalid);
        }
        self.launch()
            .validate()
            .map_err(ProductionBackendRegistryError::LaunchInvalid)?;
        let source_mount = self.policy.mounts.iter().find_map(|mount| match mount {
            SandboxMount::ReadOnlyArtifact {
                artifact_id,
                destination,
            } if artifact_id == "source" && destination == "/work/source" => Some(()),
            _ => None,
        });
        if source_mount.is_none() {
            return Err(ProductionBackendRegistryError::SourceArtifactMountRequired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionBackendRegistryError {
    EmptyBackendId,
    DuplicateBackend(String),
    ZeroOutputLimit,
    PathMustBeAbsolute,
    PathTraversal,
    RunnerDigestInvalid,
    LaunchInvalid(crate::sandbox::ProductionSandboxError),
    SourceArtifactMountRequired,
    UnsafeTaskId,
    RequestInvalid(String),
    ArtifactMountRequired(String),
    ArtifactMountNotRequested(String),
    RootUnavailable(String),
}

fn ensure_contained(
    root: &std::path::Path,
    child: &std::path::Path,
) -> Result<(), ProductionBackendRegistryError> {
    if !child.starts_with(root) {
        return Err(ProductionBackendRegistryError::RootUnavailable(
            "task path escapes configured production root".into(),
        ));
    }
    Ok(())
}

fn ensure_no_symlink_ancestors(
    path: &std::path::Path,
) -> Result<(), ProductionBackendRegistryError> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "configured production root contains a symlink boundary".into(),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "configured production root is not a directory".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    error.to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn is_safe_task_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

impl std::fmt::Display for ProductionBackendRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid production backend registration: {self:?}"
        )
    }
}

impl std::error::Error for ProductionBackendRegistryError {}

#[derive(Debug, Clone, Default)]
pub struct ProductionBackendRegistry {
    backends: BTreeMap<String, ProductionBackendConfig>,
}

impl ProductionBackendConfig {
    /// Build a minimal task-specific OCI bundle envelope. The rootfs itself
    /// belongs to the operator's pinned backend installation; only the
    /// verified artifact bind sources are selected per task.
    pub fn materialize_bundle(
        &self,
        request: &GeneralComputeRequest,
        task_id: &str,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        self.validate_request_mounts(request)?;
        let (bundle_root, artifact_root) = self.task_root(task_id)?;
        let template_rootfs = self.bundle_root.join("rootfs");
        let template_metadata = std::fs::symlink_metadata(&template_rootfs).map_err(|error| {
            ProductionBackendRegistryError::RootUnavailable(format!(
                "operator bundle template rootfs is unavailable: {error}"
            ))
        })?;
        if !template_metadata.is_dir() || template_metadata.file_type().is_symlink() {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "operator bundle template rootfs must be a real directory".into(),
            ));
        }
        std::fs::create_dir_all(&bundle_root)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        std::fs::create_dir_all(&artifact_root)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        // Re-check after directory creation to close the normal
        // check-then-create path and reject a task root replaced by a symlink.
        ensure_no_symlink_ancestors(&bundle_root)?;
        ensure_no_symlink_ancestors(&artifact_root)?;
        // The validator canonicalizes the operator-owned artifact root before
        // comparing bind sources. Emit that same spelling into config.json so
        // Windows extended-path prefixes (and any safe normalization on Unix)
        // cannot make a valid materialized bundle fail its own validation.
        let canonical_artifact_root = std::fs::canonicalize(&artifact_root)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        let rootfs = bundle_root.join("rootfs");
        match std::fs::symlink_metadata(&rootfs) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "task bundle rootfs must not be a symlink".into(),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "task bundle rootfs must be a directory".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                copy_directory_no_symlinks(&template_rootfs, &rootfs)?;
            }
            Err(error) => {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    error.to_string(),
                ));
            }
        }
        let mounts = self
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
                    "source": canonical_artifact_root.join(artifact_id).to_string_lossy(),
                    "options": ["ro", "nodev", "nosuid", "noexec"]
                }),
                SandboxMount::EphemeralScratch {
                    destination,
                    max_bytes,
                } => serde_json::json!({
                    "destination": destination,
                    "type": "tmpfs",
                    "source": "tmpfs",
                    "options": ["rw", "nodev", "nosuid", "noexec", format!("size={max_bytes}")]
                }),
            })
            .collect::<Vec<_>>();
        let config = serde_json::json!({
            "ociVersion": "1.0.2",
            "process": {
                "args": self.entrypoint,
                "cwd": "/",
                "noNewPrivileges": true,
                "user": {"uid": 65532, "gid": 65532}
            },
            "root": {"path": "rootfs", "readonly": true},
            "mounts": mounts,
            "linux": {
                "namespaces": [
                    {"type": "user"}, {"type": "pid"},
                    {"type": "mount"}, {"type": "network"}
                ],
                "seccomp": {"defaultAction": "SCMP_ACT_ERRNO"}
            },
            "annotations": {
                "org.hivemind.guest-image-digest": self.guest_image_digest,
                "org.hivemind.backend-id": self.backend_id,
                "org.hivemind.cgroup-version": "v2",
                "org.hivemind.network-policy": "deny_all",
                "org.hivemind.seccomp-profile-sha256": match &self.policy.seccomp {
                    crate::sandbox::SeccompPolicy::DefaultDeny { profile_sha256 } => profile_sha256,
                    crate::sandbox::SeccompPolicy::Disabled => "",
                }
            }
        });
        let config_path = bundle_root.join("config.json");
        if let Ok(metadata) = std::fs::symlink_metadata(&config_path) {
            if metadata.file_type().is_symlink() {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "task bundle config must not be a symlink".into(),
                ));
            }
            if !metadata.is_file() {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "task bundle config must be a regular file".into(),
                ));
            }
        }
        let bytes = serde_json::to_vec(&config)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        std::fs::write(&config_path, bytes)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        Ok((bundle_root, artifact_root))
    }
}

fn copy_directory_no_symlinks(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), ProductionBackendRegistryError> {
    ensure_no_symlink_ancestors(destination)?;
    std::fs::create_dir_all(destination)
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    ensure_no_symlink_ancestors(destination)?;
    for entry in std::fs::read_dir(source)
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?
    {
        let entry = entry
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        let target = destination.join(entry.file_name());
        if let Ok(target_metadata) = std::fs::symlink_metadata(&target) {
            if target_metadata.file_type().is_symlink() {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "task bundle destination contains a symlink".into(),
                ));
            }
        }
        if metadata.file_type().is_symlink() {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "operator bundle template contains a symlink".into(),
            ));
        }
        if metadata.is_dir() {
            copy_directory_no_symlinks(&entry.path(), &target)?;
        } else if metadata.is_file() {
            std::fs::copy(entry.path(), target).map_err(|error| {
                ProductionBackendRegistryError::RootUnavailable(error.to_string())
            })?;
        } else {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "operator bundle template contains an unsupported filesystem entry".into(),
            ));
        }
    }
    Ok(())
}

impl ProductionBackendRegistry {
    pub fn new(
        registrations: Vec<ProductionBackendConfig>,
    ) -> Result<Self, ProductionBackendRegistryError> {
        let mut backends = BTreeMap::new();
        for registration in registrations {
            registration.validate()?;
            if backends
                .insert(registration.backend_id.clone(), registration)
                .is_some()
            {
                let id = backends.keys().next_back().cloned().unwrap_or_default();
                return Err(ProductionBackendRegistryError::DuplicateBackend(id));
            }
        }
        Ok(Self { backends })
    }

    pub fn get(&self, backend_id: &str) -> Option<&ProductionBackendConfig> {
        self.backends.get(backend_id)
    }

    pub fn registrations(&self) -> impl Iterator<Item = &ProductionBackendConfig> {
        self.backends.values()
    }

    pub fn len(&self) -> usize {
        self.backends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

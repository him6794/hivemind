//! Operator-owned configuration and routing for production OCI backends.
//!
//! This module deliberately contains no URL, command-line, or Worker-provided
//! path interpretation. Configuration is loaded by the Worker from an
//! operator-controlled file and every path is validated before it can reach
//! the OCI launcher.

use crate::onnx::OnnxBackendConfig;
use crate::sandbox::{
    BackendExecutionMode, ProductionSandboxLaunch, SandboxDevice, SandboxMount,
    WindowsHcsResourceLimits, WindowsNativeSandboxLaunch, WindowsSandboxPolicy,
    WindowsSandboxPolicyError,
};
use crate::{
    GeneralComputeRequest, MANAGED_DSL_RUNTIME_VERSION, MANAGED_DSL_SEMANTICS_MANIFEST_SHA256,
    gpu::GpuSelection, managed_gpu::ManagedGpuCapability, sha256_digest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Operator-owned registration for the cross-platform closed managed DSL.
///
/// Unlike OCI/HCS registrations this contains no executable, image, or host
/// path. The interpreter is the backend and its semantics digest is the trust
/// binding used by Worker admission and consensus settlement validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedDslBackendRegistration {
    pub backend_id: String,
    pub runtime_version: String,
    pub semantics_manifest_sha256: String,
    pub max_usage_units: u64,
    pub max_output_bytes: usize,
}

impl ManagedDslBackendRegistration {
    #[must_use]
    pub fn execution_mode(&self) -> BackendExecutionMode {
        BackendExecutionMode::ProductionSandboxedDsl
    }

    pub fn validate(&self) -> Result<(), ProductionBackendRegistryError> {
        if self.backend_id.trim().is_empty() {
            return Err(ProductionBackendRegistryError::ManagedDslBackendIdEmpty);
        }
        if self.runtime_version != MANAGED_DSL_RUNTIME_VERSION {
            return Err(ProductionBackendRegistryError::ManagedDslRuntimeMismatch);
        }
        if self.semantics_manifest_sha256 != MANAGED_DSL_SEMANTICS_MANIFEST_SHA256 {
            return Err(ProductionBackendRegistryError::ManagedDslSemanticsMismatch);
        }
        if self.max_usage_units == 0 {
            return Err(ProductionBackendRegistryError::ManagedDslUsageLimitRequired);
        }
        if self.max_output_bytes == 0 {
            return Err(ProductionBackendRegistryError::ManagedDslOutputLimitRequired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ManagedDslBackendRegistry {
    backends: BTreeMap<String, ManagedDslBackendRegistration>,
}

impl ManagedDslBackendRegistry {
    pub fn new(
        registrations: Vec<ManagedDslBackendRegistration>,
    ) -> Result<Self, ProductionBackendRegistryError> {
        let mut backends = BTreeMap::new();
        for registration in registrations {
            registration.validate()?;
            let backend_id = registration.backend_id.clone();
            if backends.insert(backend_id.clone(), registration).is_some() {
                return Err(ProductionBackendRegistryError::DuplicateBackend(backend_id));
            }
        }
        Ok(Self { backends })
    }

    #[must_use]
    pub fn get(&self, backend_id: &str) -> Option<&ManagedDslBackendRegistration> {
        self.backends.get(backend_id)
    }

    pub fn registrations(&self) -> impl Iterator<Item = &ManagedDslBackendRegistration> {
        self.backends.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.backends.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuDeviceMapping {
    /// Stable operator-owned device identity from `GpuCapability.device_id`.
    pub device_id: String,
    /// Exact host device nodes exposed for this GPU selection.
    pub devices: Vec<SandboxDevice>,
}

impl GpuDeviceMapping {
    fn validate(&self) -> Result<(), ProductionBackendRegistryError> {
        if self.device_id.trim().is_empty()
            || self.device_id.len() > 128
            || !self.device_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
            })
        {
            return Err(ProductionBackendRegistryError::GpuDeviceMappingInvalid);
        }
        if self.devices.is_empty() {
            return Err(ProductionBackendRegistryError::GpuDeviceMappingEmpty);
        }
        for device in &self.devices {
            device
                .validate()
                .map_err(|_| ProductionBackendRegistryError::GpuDeviceMappingInvalid)?;
        }
        Ok(())
    }
}

/// Operator-owned production registration for the independent managed GPU
/// runtime.  It intentionally has its own registry and environment boundary;
/// a general-compute backend must never be reinterpreted as a managed GPU
/// runner merely because the backend IDs happen to match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGpuProductionBackendConfig {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub bundle_root: PathBuf,
    pub artifact_root: PathBuf,
    pub runner_executable: PathBuf,
    pub runner_state_root: PathBuf,
    pub seccomp_profile_path: PathBuf,
    pub runner_prefix_args: Vec<String>,
    pub runner_sha256: String,
    pub entrypoint: Vec<String>,
    pub policy: crate::sandbox::LinuxSandboxPolicy,
    pub gpu_device_mappings: Vec<GpuDeviceMapping>,
    pub max_output_bytes: usize,
}

impl ManagedGpuProductionBackendConfig {
    /// The fixed file contract used by the Rust managed-GPU guest runner.
    /// These files are all operator-created beneath the task artifact root;
    /// the task cannot add, remove, or rename mounts.
    const REQUIRED_MOUNTS: [(&'static str, &'static str); 4] = [
        ("source", "/work/source"),
        ("input", "/work/input"),
        ("manifest", "/work/manifest"),
        ("selection", "/work/selection"),
    ];

    #[must_use]
    pub fn execution_mode(&self) -> BackendExecutionMode {
        BackendExecutionMode::ProductionSandboxedOci
    }

    pub fn launch_for_managed_gpu_capability(
        &self,
        capability: &ManagedGpuCapability,
    ) -> Result<ProductionSandboxLaunch, ProductionBackendRegistryError> {
        self.validate()?;
        capability
            .validate()
            .map_err(ProductionBackendRegistryError::ManagedGpuCapabilityInvalid)?;
        if capability.image_digest != self.guest_image_digest {
            return Err(ProductionBackendRegistryError::GuestImageMismatch);
        }
        let mapping = self
            .gpu_device_mappings
            .iter()
            .find(|mapping| mapping.device_id == capability.device_id)
            .ok_or_else(|| {
                ProductionBackendRegistryError::GpuDeviceMappingMissing(
                    capability.device_id.clone(),
                )
            })?;
        let mut policy = self.policy.clone();
        policy.devices.clone_from(&mapping.devices);
        let launch = ProductionSandboxLaunch {
            backend_id: self.backend_id.clone(),
            guest_image_digest: self.guest_image_digest.clone(),
            entrypoint: self.entrypoint.clone(),
            policy,
            onnx: None,
        };
        launch
            .validate()
            .map_err(ProductionBackendRegistryError::LaunchInvalid)?;
        Ok(launch)
    }

    /// Materialize through the already-reviewed rootless OCI implementation.
    /// The request is a synthetic general-compute envelope used only for the
    /// shared artifact/bundle writer; the managed-GPU manifest remains the
    /// authoritative protocol and is mounted separately as `/work/manifest`.
    pub fn materialize_bundle_for_launch(
        &self,
        request: &GeneralComputeRequest,
        task_id: &str,
        launch: &ProductionSandboxLaunch,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        self.materialize_bundle_for_execution(request, task_id, task_id, launch)
    }

    pub fn materialize_bundle_for_execution(
        &self,
        request: &GeneralComputeRequest,
        task_id: &str,
        execution_scope: &str,
        launch: &ProductionSandboxLaunch,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        self.validate_mount_contract()?;
        self.as_general_compute_config()
            .materialize_bundle_for_execution(request, task_id, execution_scope, launch)
    }

    pub fn validate(&self) -> Result<(), ProductionBackendRegistryError> {
        self.validate_mount_contract()?;
        if self.gpu_device_mappings.is_empty() {
            return Err(ProductionBackendRegistryError::ManagedGpuDeviceMappingRequired);
        }
        self.as_general_compute_config().validate()
    }

    fn validate_mount_contract(&self) -> Result<(), ProductionBackendRegistryError> {
        let mut actual = BTreeMap::new();
        for mount in &self.policy.mounts {
            let SandboxMount::ReadOnlyArtifact {
                artifact_id,
                destination,
            } = mount
            else {
                return Err(ProductionBackendRegistryError::ManagedGpuMountContractInvalid);
            };
            if actual
                .insert(artifact_id.as_str(), destination.as_str())
                .is_some()
            {
                return Err(ProductionBackendRegistryError::ManagedGpuMountContractInvalid);
            }
        }
        let expected = Self::REQUIRED_MOUNTS
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        if actual != expected {
            return Err(ProductionBackendRegistryError::ManagedGpuMountContractInvalid);
        }
        Ok(())
    }

    fn as_general_compute_config(&self) -> ProductionBackendConfig {
        ProductionBackendConfig {
            backend_id: self.backend_id.clone(),
            guest_image_digest: self.guest_image_digest.clone(),
            bundle_root: self.bundle_root.clone(),
            artifact_root: self.artifact_root.clone(),
            runner_executable: self.runner_executable.clone(),
            runner_state_root: self.runner_state_root.clone(),
            seccomp_profile_path: self.seccomp_profile_path.clone(),
            runner_prefix_args: self.runner_prefix_args.clone(),
            runner_sha256: self.runner_sha256.clone(),
            entrypoint: self.entrypoint.clone(),
            policy: self.policy.clone(),
            gpu_device_mappings: self.gpu_device_mappings.clone(),
            onnx: None,
            max_output_bytes: self.max_output_bytes,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ManagedGpuProductionBackendRegistry {
    backends: BTreeMap<String, ManagedGpuProductionBackendConfig>,
}

impl ManagedGpuProductionBackendRegistry {
    pub fn new(
        registrations: Vec<ManagedGpuProductionBackendConfig>,
    ) -> Result<Self, ProductionBackendRegistryError> {
        let mut backends = BTreeMap::new();
        for registration in registrations {
            registration.validate()?;
            let backend_id = registration.backend_id.clone();
            if backends.insert(backend_id.clone(), registration).is_some() {
                return Err(ProductionBackendRegistryError::DuplicateBackend(backend_id));
            }
        }
        Ok(Self { backends })
    }

    #[must_use]
    pub fn get(&self, backend_id: &str) -> Option<&ManagedGpuProductionBackendConfig> {
        self.backends.get(backend_id)
    }

    pub fn registrations(&self) -> impl Iterator<Item = &ManagedGpuProductionBackendConfig> {
        self.backends.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.backends.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionBackendConfig {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub bundle_root: PathBuf,
    pub artifact_root: PathBuf,
    pub runner_executable: PathBuf,
    /// Operator-owned writable state for the OCI runner itself (for example
    /// runc's `--root` directory). It must be separate from task bundles and
    /// never be derived from a task id or a Worker request.
    pub runner_state_root: PathBuf,
    /// Canonical operator-owned OCI seccomp profile bytes. The SHA-256 must
    /// equal the digest embedded in `policy.seccomp` before a task bundle is
    /// materialized.
    pub seccomp_profile_path: PathBuf,
    pub runner_prefix_args: Vec<String>,
    pub runner_sha256: String,
    pub entrypoint: Vec<String>,
    pub policy: crate::sandbox::LinuxSandboxPolicy,
    /// Optional per-device mapping. When present, GPU requests must resolve to
    /// one of these mappings before any device node reaches the OCI bundle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_device_mappings: Vec<GpuDeviceMapping>,
    /// Optional operator-pinned ONNX runner contract. The actual ONNX Runtime
    /// or TensorRT library remains inside the guest image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onnx: Option<OnnxBackendConfig>,
    pub max_output_bytes: usize,
}

impl ProductionBackendConfig {
    #[must_use]
    pub fn execution_mode(&self) -> BackendExecutionMode {
        BackendExecutionMode::ProductionSandboxedOci
    }

    #[must_use]
    pub fn launch(&self) -> ProductionSandboxLaunch {
        ProductionSandboxLaunch {
            backend_id: self.backend_id.clone(),
            guest_image_digest: self.guest_image_digest.clone(),
            entrypoint: self.entrypoint.clone(),
            policy: self.policy.clone(),
            onnx: self.onnx.clone(),
        }
    }

    /// Build the sandbox launch envelope for the scheduler's trusted GPU
    /// selection. Device nodes are selected only from this operator-owned
    /// mapping; a task cannot provide paths or widen the policy itself.
    pub fn launch_for_gpu_selection(
        &self,
        selection: Option<&GpuSelection>,
    ) -> Result<ProductionSandboxLaunch, ProductionBackendRegistryError> {
        let mut launch = self.launch();
        match selection {
            None | Some(GpuSelection::CpuFallback { .. }) => {
                // A missing selection or explicit CPU fallback must never carry
                // static device nodes into the OCI bundle. GPU access is valid
                // only after a typed selection resolves an operator mapping.
                launch.policy.devices.clear();
                if let Some(onnx) = &self.onnx
                    && onnx.execution_provider.requires_cuda_gpu()
                    && matches!(selection, None | Some(GpuSelection::CpuFallback { .. }))
                {
                    return Err(ProductionBackendRegistryError::OnnxGpuSelectionRequired);
                }
            }
            Some(GpuSelection::Gpu(capability)) => {
                if let Some(onnx) = &self.onnx {
                    if !onnx.execution_provider.requires_cuda_gpu() {
                        return Err(ProductionBackendRegistryError::OnnxCpuSelectionMismatch);
                    }
                    if !matches!(capability.runtime, crate::gpu::GpuRuntime::Cuda) {
                        return Err(ProductionBackendRegistryError::OnnxGpuRuntimeMismatch);
                    }
                }
                let mapping = self
                    .gpu_device_mappings
                    .iter()
                    .find(|mapping| mapping.device_id == capability.device_id)
                    .ok_or_else(|| {
                        ProductionBackendRegistryError::GpuDeviceMappingMissing(
                            capability.device_id.clone(),
                        )
                    })?;
                launch.policy.devices.clone_from(&mapping.devices);
            }
        }
        launch
            .validate()
            .map_err(ProductionBackendRegistryError::LaunchInvalid)?;
        Ok(launch)
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
        if let Some(onnx) = &self.onnx {
            onnx.validate_request_artifacts(
                &request.source_artifact.artifact_id,
                request
                    .input_artifacts
                    .iter()
                    .map(|artifact| artifact.artifact_id.as_str()),
            )
            .map_err(ProductionBackendRegistryError::OnnxConfigInvalid)?;
            self.validate_onnx_mount_contract(onnx)?;
        }
        Ok(())
    }

    fn validate_onnx_mount_contract(
        &self,
        onnx: &OnnxBackendConfig,
    ) -> Result<(), ProductionBackendRegistryError> {
        let mut model_mounts = 0usize;
        let mut input_mounts = vec![0usize; onnx.input_artifact_ids.len()];
        for mount in &self.policy.mounts {
            let SandboxMount::ReadOnlyArtifact {
                artifact_id,
                destination,
            } = mount
            else {
                continue;
            };
            if artifact_id == &onnx.model_artifact_id {
                model_mounts += 1;
                if destination != "/work/source" {
                    return Err(ProductionBackendRegistryError::OnnxModelMountInvalid);
                }
                continue;
            }
            if let Some(index) = onnx
                .input_artifact_ids
                .iter()
                .position(|id| id == artifact_id)
            {
                input_mounts[index] += 1;
                if destination != &format!("/work/input-{index}") {
                    return Err(ProductionBackendRegistryError::OnnxInputMountInvalid(index));
                }
            }
        }
        if model_mounts != 1 {
            return Err(ProductionBackendRegistryError::OnnxModelMountInvalid);
        }
        if let Some(index) = input_mounts.iter().position(|count| *count != 1) {
            return Err(ProductionBackendRegistryError::OnnxInputMountInvalid(index));
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
            &self.runner_state_root,
            &self.seccomp_profile_path,
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
        if self
            .runner_prefix_args
            .iter()
            .enumerate()
            .any(|(index, arg)| {
                (arg.starts_with("--rootless=") && arg != "--rootless=true")
                    || (arg == "--rootless"
                        && self
                            .runner_prefix_args
                            .get(index + 1)
                            .is_some_and(|value| value != "true"))
            })
        {
            return Err(ProductionBackendRegistryError::RunnerPrefixInvalid);
        }
        self.launch()
            .validate()
            .map_err(ProductionBackendRegistryError::LaunchInvalid)?;
        let mut mapping_ids = BTreeSet::new();
        for mapping in &self.gpu_device_mappings {
            mapping.validate()?;
            if !mapping_ids.insert(mapping.device_id.as_str()) {
                return Err(ProductionBackendRegistryError::GpuDeviceMappingDuplicate(
                    mapping.device_id.clone(),
                ));
            }
        }
        if !self.gpu_device_mappings.is_empty() && !self.policy.devices.is_empty() {
            return Err(ProductionBackendRegistryError::GpuDevicePolicyConflict);
        }
        if let Some(onnx) = &self.onnx {
            onnx.validate()
                .map_err(ProductionBackendRegistryError::OnnxConfigInvalid)?;
            if onnx.model_artifact_id != "source" {
                return Err(ProductionBackendRegistryError::OnnxConfigInvalid(
                    crate::onnx::OnnxBackendError::ModelMustBeSourceArtifact,
                ));
            }
            self.validate_onnx_mount_contract(onnx)?;
        }
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

/// Operator-owned registration for a native Windows HCS/container backend.
///
/// This schema is intentionally separate from [`ProductionBackendConfig`].
/// Linux OCI paths and policies must never be reinterpreted as Windows
/// isolation settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsProductionBackendConfig {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub image_root: PathBuf,
    pub artifact_root: PathBuf,
    pub runner_executable: PathBuf,
    pub runner_sha256: String,
    pub entrypoint: Vec<String>,
    pub policy: WindowsSandboxPolicy,
    pub max_output_bytes: usize,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsHcsMountSpec {
    pub host_path: PathBuf,
    pub container_path: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsHcsContainerSpec {
    pub container_id: String,
    pub backend_id: String,
    pub guest_image_digest: String,
    pub image_material_digest: String,
    pub image_root: PathBuf,
    pub runner_executable: PathBuf,
    pub runner_sha256: String,
    pub runner_container_path: String,
    pub policy_digest: String,
    pub entrypoint: Vec<String>,
    pub mounts: Vec<WindowsHcsMountSpec>,
    /// HCS `Container.Storage.Path`, owned by the operator and used for the
    /// container scratch layer.
    pub storage_path: PathBuf,
    pub result_path: PathBuf,
    pub result_container_path: String,
    pub max_output_bytes: usize,
    pub network_isolated: bool,
    pub root_read_only: bool,
    pub resource_limits: WindowsHcsResourceLimits,
}

/// Verified operator assets bound to one native HCS launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsHcsAssetBinding {
    pub image_material_digest: String,
    pub runner_sha256: String,
    pub runner_container_path: String,
}

impl WindowsProductionBackendConfig {
    #[must_use]
    pub fn execution_mode(&self) -> BackendExecutionMode {
        BackendExecutionMode::ProductionSandboxedWindows
    }

    #[must_use]
    pub fn launch(&self) -> WindowsNativeSandboxLaunch {
        WindowsNativeSandboxLaunch {
            backend_id: self.backend_id.clone(),
            guest_image_digest: self.guest_image_digest.clone(),
            entrypoint: self.entrypoint.clone(),
            policy: self.policy.clone(),
        }
    }

    pub fn task_root(
        &self,
        task_id: &str,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        if !is_safe_task_id(task_id) {
            return Err(ProductionBackendRegistryError::UnsafeTaskId);
        }
        ensure_no_symlink_ancestors(&self.image_root)?;
        ensure_no_symlink_ancestors(&self.artifact_root)?;
        let image_root = self.image_root.clone();
        let artifact_task_root = self.artifact_root.join(task_id);
        ensure_contained(&self.image_root, &image_root)?;
        ensure_contained(&self.artifact_root, &artifact_task_root)?;
        ensure_no_symlink_ancestors(&image_root)?;
        ensure_no_symlink_ancestors(&artifact_task_root)?;
        Ok((image_root, artifact_task_root))
    }

    /// Remove the per-execution artifact, scratch, and result tree without
    /// touching the shared guest image or runner.
    pub fn cleanup_task_root(
        &self,
        execution_scope: &str,
    ) -> Result<(), ProductionBackendRegistryError> {
        let (_, artifact_root) = self.task_root(execution_scope)?;
        remove_owned_task_directory(&artifact_root)
    }

    /// Verify the operator-owned Windows image and runner before HCS creation.
    ///
    /// The configured guest image digest is the digest of a deterministic
    /// directory walk of `image_root`; it is not accepted as a label for an
    /// arbitrary directory. The runner must be a regular, non-reparse file
    /// below that image root so the process created inside HCS is the exact
    /// pinned runner whose bytes were checked here.
    pub fn verify_operator_assets(
        &self,
    ) -> Result<WindowsHcsAssetBinding, ProductionBackendRegistryError> {
        verify_windows_hcs_assets(
            &self.image_root,
            &self.runner_executable,
            &self.guest_image_digest,
            &self.runner_sha256,
        )
    }

    /// Build the operator-owned HCS specification without invoking HCS.
    ///
    /// Every host path comes from this validated registration and the
    /// task-specific operator roots; no Worker-provided path is accepted.
    pub fn hcs_spec(
        &self,
        task_id: &str,
    ) -> Result<WindowsHcsContainerSpec, ProductionBackendRegistryError> {
        self.hcs_spec_for_execution(task_id, task_id)
    }

    pub fn hcs_spec_for_execution(
        &self,
        task_id: &str,
        execution_scope: &str,
    ) -> Result<WindowsHcsContainerSpec, ProductionBackendRegistryError> {
        if !is_safe_task_id(task_id) || !is_safe_task_id(execution_scope) {
            return Err(ProductionBackendRegistryError::UnsafeTaskId);
        }
        self.validate()?;
        let resource_limits = self
            .policy
            .hcs_enforced_resource_limits()
            .map_err(ProductionBackendRegistryError::WindowsPolicyUnenforceable)?;
        self.hcs_spec_with_limits(execution_scope, resource_limits)
    }

    fn hcs_spec_with_limits(
        &self,
        task_id: &str,
        resource_limits: WindowsHcsResourceLimits,
    ) -> Result<WindowsHcsContainerSpec, ProductionBackendRegistryError> {
        let assets = self.verify_operator_assets()?;
        let (image_root, artifact_task_root) = self.task_root(task_id)?;
        let container_id = format!("hivemind-{task_id}");
        let mounts = self
            .policy
            .mounts
            .iter()
            .map(|mount| match mount {
                SandboxMount::ReadOnlyArtifact {
                    artifact_id,
                    destination,
                } => WindowsHcsMountSpec {
                    host_path: artifact_task_root.join(artifact_id),
                    container_path: windows_container_path(destination),
                    read_only: true,
                },
                SandboxMount::EphemeralScratch { destination, .. } => WindowsHcsMountSpec {
                    host_path: artifact_task_root.join("scratch"),
                    container_path: windows_container_path(destination),
                    read_only: false,
                },
            })
            .collect();
        let storage_path = artifact_task_root.join("scratch");
        let policy_digest = sha256_digest(&serde_json::to_vec(&self.policy).map_err(|error| {
            ProductionBackendRegistryError::WindowsPolicyIdentityUnavailable(error.to_string())
        })?);
        Ok(WindowsHcsContainerSpec {
            container_id,
            backend_id: self.backend_id.clone(),
            guest_image_digest: self.guest_image_digest.clone(),
            image_material_digest: assets.image_material_digest,
            image_root,
            runner_executable: self.runner_executable.clone(),
            runner_sha256: assets.runner_sha256,
            runner_container_path: assets.runner_container_path,
            policy_digest,
            entrypoint: self.entrypoint.clone(),
            mounts,
            storage_path: storage_path.clone(),
            result_path: storage_path.join("result.json"),
            result_container_path: "C:\\work\\output\\result.json".into(),
            max_output_bytes: self.max_output_bytes,
            network_isolated: true,
            root_read_only: true,
            resource_limits,
        })
    }

    pub fn validate(&self) -> Result<(), ProductionBackendRegistryError> {
        if self.backend_id.trim().is_empty() {
            return Err(ProductionBackendRegistryError::EmptyBackendId);
        }
        if self.max_output_bytes == 0 || self.timeout_ms == 0 {
            return Err(ProductionBackendRegistryError::WindowsResourceLimitRequired);
        }
        for path in [
            &self.image_root,
            &self.artifact_root,
            &self.runner_executable,
        ] {
            if !is_absolute_windows_path(path) {
                return Err(ProductionBackendRegistryError::WindowsPathMustBeAbsolute);
            }
            if windows_path_has_traversal(path) {
                return Err(ProductionBackendRegistryError::WindowsPathTraversal);
            }
        }
        if !is_sha256_digest(&self.runner_sha256) {
            return Err(ProductionBackendRegistryError::WindowsRunnerDigestInvalid);
        }
        if !windows_path_is_contained(&self.image_root, &self.runner_executable) {
            return Err(ProductionBackendRegistryError::WindowsRunnerOutsideImage);
        }
        let runner_name = windows_path_file_name(&self.runner_executable)
            .ok_or(ProductionBackendRegistryError::WindowsRunnerOutsideImage)?;
        if self
            .entrypoint
            .first()
            .is_none_or(|entrypoint| !entrypoint.eq_ignore_ascii_case(&runner_name))
        {
            return Err(ProductionBackendRegistryError::WindowsEntrypointRunnerMismatch);
        }
        self.launch()
            .validate()
            .map_err(ProductionBackendRegistryError::WindowsLaunchInvalid)?;
        Ok(())
    }
}

fn windows_container_path(destination: &str) -> String {
    format!(
        "C:\\{}",
        destination.trim_start_matches('/').replace('/', "\\")
    )
}

fn is_absolute_windows_path(path: &std::path::Path) -> bool {
    let value = path.to_string_lossy();
    let bytes = value.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || value.starts_with("\\\\")
}

fn windows_path_has_traversal(path: &std::path::Path) -> bool {
    path.to_string_lossy()
        .split(['\\', '/'])
        .any(|component| matches!(component, ".." | "."))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionBackendRegistryError {
    EmptyBackendId,
    DuplicateBackend(String),
    ZeroOutputLimit,
    PathMustBeAbsolute,
    PathTraversal,
    RunnerDigestInvalid,
    RunnerPrefixInvalid,
    SeccompProfileUnavailable(String),
    LaunchInvalid(crate::sandbox::ProductionSandboxError),
    SourceArtifactMountRequired,
    UnsafeTaskId,
    RequestInvalid(String),
    ArtifactMountRequired(String),
    ArtifactMountNotRequested(String),
    RootUnavailable(String),
    WindowsPathMustBeAbsolute,
    WindowsPathTraversal,
    WindowsRunnerDigestInvalid,
    WindowsRunnerUnavailable(String),
    WindowsRunnerDigestMismatch,
    WindowsRunnerOutsideImage,
    WindowsEntrypointRunnerMismatch,
    WindowsImageUnavailable(String),
    WindowsImageDigestMismatch,
    WindowsPolicyIdentityUnavailable(String),
    WindowsLaunchInvalid(crate::sandbox::ProductionSandboxError),
    WindowsPolicyUnenforceable(WindowsSandboxPolicyError),
    WindowsResourceLimitRequired,
    WindowsRegistryEmpty,
    ManagedDslBackendIdEmpty,
    ManagedDslRuntimeMismatch,
    ManagedDslSemanticsMismatch,
    ManagedDslUsageLimitRequired,
    ManagedDslOutputLimitRequired,
    GpuDeviceMappingInvalid,
    GpuDeviceMappingEmpty,
    GpuDeviceMappingDuplicate(String),
    GpuDeviceMappingMissing(String),
    GpuDevicePolicyConflict,
    ManagedGpuCapabilityInvalid(String),
    GuestImageMismatch,
    ManagedGpuDeviceMappingRequired,
    ManagedGpuMountContractInvalid,
    OnnxConfigInvalid(crate::onnx::OnnxBackendError),
    OnnxModelMountInvalid,
    OnnxInputMountInvalid(usize),
    OnnxGpuSelectionRequired,
    OnnxGpuRuntimeMismatch,
    OnnxCpuSelectionMismatch,
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
            Ok(metadata) if is_reparse_point(&metadata) => {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "configured production root contains a reparse-point boundary".into(),
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

fn validate_seccomp_profile(profile: &serde_json::Value) -> Result<(), String> {
    let object = profile
        .as_object()
        .ok_or_else(|| "seccomp profile must be a JSON object".to_string())?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "defaultAction" | "architectures" | "syscalls"))
    {
        return Err("seccomp profile contains an unknown field".into());
    }
    if object
        .get("defaultAction")
        .and_then(serde_json::Value::as_str)
        != Some("SCMP_ACT_ERRNO")
    {
        return Err("seccomp profile defaultAction must be SCMP_ACT_ERRNO".into());
    }
    if let Some(architectures) = object.get("architectures") {
        let Some(architectures) = architectures.as_array() else {
            return Err("seccomp profile architectures must be an array".into());
        };
        if architectures.is_empty()
            || architectures
                .iter()
                .any(|architecture| architecture.as_str().is_none())
        {
            return Err("seccomp profile architectures must contain names".into());
        }
    }
    let syscalls = object
        .get("syscalls")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "seccomp profile must contain a syscall allowlist".to_string())?;
    if syscalls.is_empty() {
        return Err("seccomp profile syscall allowlist must not be empty".into());
    }
    let mut names = BTreeSet::new();
    for group in syscalls {
        let group = group
            .as_object()
            .ok_or_else(|| "seccomp syscall groups must be objects".to_string())?;
        if group
            .keys()
            .any(|key| !matches!(key.as_str(), "names" | "action"))
        {
            return Err("seccomp syscall group contains an unknown field".into());
        }
        if group.get("action").and_then(serde_json::Value::as_str) != Some("SCMP_ACT_ALLOW") {
            return Err("seccomp syscall groups must use SCMP_ACT_ALLOW".into());
        }
        let syscall_names = group
            .get("names")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "seccomp syscall group must contain names".to_string())?;
        if syscall_names.is_empty() {
            return Err("seccomp syscall group names must not be empty".into());
        }
        for name in syscall_names {
            let Some(name) = name.as_str() else {
                return Err("seccomp syscall names must be strings".into());
            };
            if name.trim().is_empty() || !names.insert(name) {
                return Err("seccomp syscall names must be unique and non-empty".into());
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct ProductionBackendRegistry {
    backends: BTreeMap<String, ProductionBackendConfig>,
}

impl ProductionBackendConfig {
    /// Build a minimal task-specific OCI bundle envelope. The rootfs itself
    /// belongs to the operator's pinned backend installation; only the
    /// verified artifact bind sources are selected per task.
    ///
    /// # Panics
    ///
    /// Panics only if the operator-owned ONNX input-artifact list cannot be
    /// serialized, which would indicate a programming error.
    pub fn materialize_bundle(
        &self,
        request: &GeneralComputeRequest,
        task_id: &str,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        self.materialize_bundle_with_devices(request, task_id, &self.policy.devices)
    }

    /// Remove only the per-execution roots owned by this backend. Shared
    /// runner state, templates, and CAS data are deliberately outside this
    /// boundary.
    pub fn cleanup_task_root(
        &self,
        execution_scope: &str,
    ) -> Result<(), ProductionBackendRegistryError> {
        let (bundle_root, artifact_root) = self.task_root(execution_scope)?;
        remove_owned_task_directory(&bundle_root)?;
        remove_owned_task_directory(&artifact_root)
    }

    fn materialize_bundle_with_devices(
        &self,
        request: &GeneralComputeRequest,
        task_id: &str,
        selected_devices: &[SandboxDevice],
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        self.validate_request_mounts(request)?;
        let seccomp_profile = self.load_seccomp_profile()?;
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
        let rootfs_snapshot = rootfs_snapshot_for_template(&template_rootfs, &self.bundle_root)?;
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
            Ok(_) => {
                validate_task_rootfs_marker(&rootfs, &rootfs_snapshot)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                clone_directory_reflink_or_copy(&rootfs_snapshot.root, &rootfs)?;
                write_task_rootfs_marker(&rootfs, &rootfs_snapshot)?;
            }
            Err(error) => {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    error.to_string(),
                ));
            }
        }
        let mut mounts = crate::sandbox::standard_linux_mounts();
        mounts.extend(
            self.policy
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
                        "options": ["bind", "ro", "nodev", "nosuid", "noexec"]
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
                .collect::<Vec<_>>(),
        );
        let mut devices = crate::sandbox::standard_linux_devices();
        devices.extend(selected_devices.iter().cloned());
        let (uid_mappings, gid_mappings) = crate::sandbox::rootless_id_mappings()
            .map_err(ProductionBackendRegistryError::RootUnavailable)?;
        let linux = serde_json::json!({
            "namespaces": [
                {"type": "user"}, {"type": "pid"},
                {"type": "mount"}, {"type": "network"}
            ],
            "uidMappings": uid_mappings
                .iter()
                .map(crate::sandbox::LinuxIdMapping::oci_spec)
                .collect::<Vec<_>>(),
            "gidMappings": gid_mappings
                .iter()
                .map(crate::sandbox::LinuxIdMapping::oci_spec)
                .collect::<Vec<_>>(),
            "seccomp": seccomp_profile,
            "devices": devices.iter().map(SandboxDevice::oci_spec).collect::<Vec<_>>(),
            "resources": {
                "devices": devices.iter().map(SandboxDevice::cgroup_rule).collect::<Vec<_>>()
            }
        });
        let mut config = serde_json::json!({
            "ociVersion": "1.0.2",
            "process": {
                "args": self.entrypoint,
                "cwd": "/",
                "noNewPrivileges": true,
                "user": {"uid": 65532, "gid": 65532}
            },
            "root": {"path": "rootfs", "readonly": true},
            "mounts": mounts,
            "linux": linux,
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
        if let Some(onnx) = &self.onnx {
            config["annotations"]["org.hivemind.workload"] = serde_json::json!("onnx");
            config["annotations"]["org.hivemind.onnx.protocol"] =
                serde_json::json!(onnx.protocol_version);
            config["annotations"]["org.hivemind.onnx.execution-provider"] =
                serde_json::json!(onnx.execution_provider.as_str());
            config["annotations"]["org.hivemind.onnx.model-artifact-id"] =
                serde_json::json!(onnx.model_artifact_id);
            // OCI annotations are string-valued; preserve the ordered artifact
            // IDs as canonical JSON inside the annotation value.
            config["annotations"]["org.hivemind.onnx.input-artifact-ids"] = serde_json::json!(
                serde_json::to_string(&onnx.input_artifact_ids)
                    .expect("ONNX input artifact IDs serialize infallibly")
            );
        }
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
        atomic_replace_config(&config_path, &bytes)?;
        Ok((bundle_root, artifact_root))
    }

    /// Materialize a bundle using the exact device set selected by trusted
    /// admission. The legacy method keeps task-id paths for existing callers.
    pub fn materialize_bundle_for_launch(
        &self,
        request: &GeneralComputeRequest,
        task_id: &str,
        launch: &ProductionSandboxLaunch,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        self.materialize_bundle_for_execution(request, task_id, task_id, launch)
    }

    /// Materialize an isolated bundle for one execution attempt. The scope is
    /// caller-derived from the immutable execution identity, never from a
    /// Worker-provided filesystem path.
    pub fn materialize_bundle_for_execution(
        &self,
        request: &GeneralComputeRequest,
        task_id: &str,
        execution_scope: &str,
        launch: &ProductionSandboxLaunch,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
        if !is_safe_task_id(task_id) {
            return Err(ProductionBackendRegistryError::UnsafeTaskId);
        }
        if launch.backend_id != self.backend_id
            || launch.guest_image_digest != self.guest_image_digest
        {
            return Err(ProductionBackendRegistryError::LaunchInvalid(
                crate::sandbox::ProductionSandboxError::BundleMetadataMismatch,
            ));
        }
        launch
            .validate()
            .map_err(ProductionBackendRegistryError::LaunchInvalid)?;
        let mut expected_policy = self.policy.clone();
        expected_policy.devices.clone_from(&launch.policy.devices);
        if launch.entrypoint != self.entrypoint
            || launch.onnx != self.onnx
            || launch.policy != expected_policy
        {
            return Err(ProductionBackendRegistryError::LaunchInvalid(
                crate::sandbox::ProductionSandboxError::BundleMetadataMismatch,
            ));
        }
        if launch.policy.devices != self.policy.devices
            && !self
                .gpu_device_mappings
                .iter()
                .any(|mapping| mapping.devices == launch.policy.devices)
        {
            return Err(ProductionBackendRegistryError::GpuDevicePolicyConflict);
        }
        self.materialize_bundle_with_devices(request, execution_scope, &launch.policy.devices)
    }

    fn load_seccomp_profile(&self) -> Result<serde_json::Value, ProductionBackendRegistryError> {
        let expected_digest = match &self.policy.seccomp {
            crate::sandbox::SeccompPolicy::DefaultDeny { profile_sha256 } => profile_sha256,
            crate::sandbox::SeccompPolicy::Disabled => {
                return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
                    "production seccomp policy cannot be disabled".into(),
                ));
            }
        };
        load_verified_seccomp_profile(&self.seccomp_profile_path, expected_digest)
    }
}

const SECCOMP_FULL_REVERIFY_INTERVAL: Duration = Duration::from_secs(15 * 60);
const SECCOMP_CACHE_LIMIT: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SeccompCacheKey {
    path: PathBuf,
    expected_digest: String,
}

#[derive(Debug, Clone)]
struct SeccompCacheEntry {
    size_bytes: u64,
    modified: Option<SystemTime>,
    profile: serde_json::Value,
    verified_at: Instant,
}

static SECCOMP_PROFILE_CACHE: OnceLock<Mutex<HashMap<SeccompCacheKey, SeccompCacheEntry>>> =
    OnceLock::new();

fn load_verified_seccomp_profile(
    path: &Path,
    expected_digest: &str,
) -> Result<serde_json::Value, ProductionBackendRegistryError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        ProductionBackendRegistryError::SeccompProfileUnavailable(error.to_string())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
            "seccomp profile must be a regular non-symlink file".into(),
        ));
    }
    if !is_sha256_digest(expected_digest) {
        return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
            "seccomp profile SHA-256 policy pin is invalid".into(),
        ));
    }
    let key = SeccompCacheKey {
        path: path.to_path_buf(),
        expected_digest: expected_digest.to_owned(),
    };
    let modified = metadata.modified().ok();
    let now = Instant::now();
    let cache = SECCOMP_PROFILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let entries = cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = entries.get(&key)
            && entry.size_bytes == metadata.len()
            && entry.modified == modified
            && now.duration_since(entry.verified_at) < SECCOMP_FULL_REVERIFY_INTERVAL
        {
            return Ok(entry.profile.clone());
        }
    }

    let bytes = std::fs::read(path).map_err(|error| {
        ProductionBackendRegistryError::SeccompProfileUnavailable(error.to_string())
    })?;
    if crate::sha256_digest(&bytes) != expected_digest {
        return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
            "seccomp profile SHA-256 does not match the policy pin".into(),
        ));
    }
    let profile: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        ProductionBackendRegistryError::SeccompProfileUnavailable(format!(
            "seccomp profile is not valid JSON: {error}"
        ))
    })?;
    validate_seccomp_profile(&profile)
        .map_err(ProductionBackendRegistryError::SeccompProfileUnavailable)?;
    let canonical = serde_json::to_vec(&profile).map_err(|error| {
        ProductionBackendRegistryError::SeccompProfileUnavailable(error.to_string())
    })?;
    if canonical != bytes {
        return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
            "seccomp profile must use canonical JSON bytes".into(),
        ));
    }
    let after = std::fs::symlink_metadata(path).map_err(|error| {
        ProductionBackendRegistryError::SeccompProfileUnavailable(error.to_string())
    })?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || after.len() != metadata.len()
        || after.modified().ok() != modified
    {
        return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
            "seccomp profile changed while it was verified".into(),
        ));
    }

    let mut entries = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if entries.len() >= SECCOMP_CACHE_LIMIT
        && !entries.contains_key(&key)
        && let Some(oldest) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.verified_at)
            .map(|(key, _)| key.clone())
    {
        entries.remove(&oldest);
    }
    entries.insert(
        key,
        SeccompCacheEntry {
            size_bytes: after.len(),
            modified: after.modified().ok(),
            profile: profile.clone(),
            verified_at: now,
        },
    );
    Ok(profile)
}

const ROOTFS_FULL_REVERIFY_INTERVAL: Duration = Duration::from_secs(15 * 60);
const ROOTFS_CACHE_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone)]
struct RootfsSnapshot {
    root: PathBuf,
    metadata_fingerprint: String,
    content_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootfsSnapshotRecord {
    metadata_fingerprint: String,
    content_digest: String,
    verified_at_ms: u128,
}

fn rootfs_snapshot_for_template(
    template_rootfs: &Path,
    bundle_root: &Path,
) -> Result<RootfsSnapshot, ProductionBackendRegistryError> {
    let metadata_fingerprint = hash_rootfs_tree(template_rootfs, false)?;
    let cache_root = bundle_root.join(".rootfs-cache");
    if let Ok(metadata) = fs::symlink_metadata(&cache_root) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "rootfs cache must be a real directory".into(),
            ));
        }
    } else {
        fs::create_dir_all(&cache_root)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    }
    ensure_no_symlink_ancestors(&cache_root)?;

    let now_ms = unix_time_ms();
    let mut stale_cache_dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(&cache_root) {
        for entry in entries {
            let entry = entry.map_err(|error| {
                ProductionBackendRegistryError::RootUnavailable(error.to_string())
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                ProductionBackendRegistryError::RootUnavailable(error.to_string())
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let record_path = path.join("record.json");
            let record_bytes = match fs::read(&record_path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(ProductionBackendRegistryError::RootUnavailable(
                        error.to_string(),
                    ));
                }
            };
            let record: RootfsSnapshotRecord = match serde_json::from_slice(&record_bytes) {
                Ok(record) => record,
                Err(_) => continue,
            };
            if record.metadata_fingerprint != metadata_fingerprint {
                continue;
            }
            let root = path.join("rootfs");
            let Ok(root_metadata) = fs::symlink_metadata(&root) else {
                continue;
            };
            if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
                continue;
            }
            let age_ms = now_ms.saturating_sub(record.verified_at_ms);
            if age_ms < ROOTFS_FULL_REVERIFY_INTERVAL.as_millis() {
                return Ok(RootfsSnapshot {
                    root,
                    metadata_fingerprint,
                    content_digest: record.content_digest,
                });
            }
            let content_digest = hash_rootfs_tree(template_rootfs, true)?;
            if content_digest == record.content_digest
                && hash_rootfs_tree(&root, true)? == record.content_digest
            {
                let refreshed = RootfsSnapshotRecord {
                    metadata_fingerprint: metadata_fingerprint.clone(),
                    content_digest: content_digest.clone(),
                    verified_at_ms: now_ms,
                };
                write_json_file_atomically(&record_path, &refreshed)?;
                return Ok(RootfsSnapshot {
                    root,
                    metadata_fingerprint,
                    content_digest,
                });
            }
            stale_cache_dirs.push(path);
        }
    }

    let content_digest = hash_rootfs_tree(template_rootfs, true)?;
    let content_hex = content_digest
        .strip_prefix("sha256:")
        .unwrap_or(&content_digest);
    let cache_dir = cache_root.join(format!(
        "{}-{}",
        metadata_fingerprint
            .strip_prefix("sha256:")
            .unwrap_or(&metadata_fingerprint),
        &content_hex[..16.min(content_hex.len())]
    ));
    let root = cache_dir.join("rootfs");
    if let Ok(metadata) = fs::symlink_metadata(&root) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "rootfs cache snapshot must be a real directory".into(),
            ));
        }
        let record_path = cache_dir.join("record.json");
        let record: RootfsSnapshotRecord =
            serde_json::from_slice(&fs::read(&record_path).map_err(|error| {
                ProductionBackendRegistryError::RootUnavailable(error.to_string())
            })?)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        if record.metadata_fingerprint == metadata_fingerprint
            && record.content_digest == content_digest
            && hash_rootfs_tree(&root, true)? == content_digest
        {
            return Ok(RootfsSnapshot {
                root,
                metadata_fingerprint,
                content_digest,
            });
        }
        return Err(ProductionBackendRegistryError::RootUnavailable(
            "rootfs cache snapshot content does not match its record".into(),
        ));
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = cache_root.join(format!(".building-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&temporary)
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    let temporary_root = temporary.join("rootfs");
    let result = (|| {
        clone_directory_reflink_or_copy(template_rootfs, &temporary_root)?;
        let record = RootfsSnapshotRecord {
            metadata_fingerprint: metadata_fingerprint.clone(),
            content_digest: content_digest.clone(),
            verified_at_ms: now_ms,
        };
        write_json_file_atomically(&temporary.join("record.json"), &record)?;
        fs::rename(&temporary, &cache_dir)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        Ok::<(), ProductionBackendRegistryError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result?;

    for stale in stale_cache_dirs {
        let _ = fs::remove_dir_all(stale);
    }
    gc_rootfs_cache(&cache_root, &cache_dir, now_ms);
    Ok(RootfsSnapshot {
        root,
        metadata_fingerprint,
        content_digest,
    })
}

fn gc_rootfs_cache(cache_root: &Path, current: &Path, now_ms: u128) {
    let Ok(entries) = fs::read_dir(cache_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current
            || !path.is_dir()
            || path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        let Ok(bytes) = fs::read(path.join("record.json")) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<RootfsSnapshotRecord>(&bytes) else {
            continue;
        };
        if now_ms.saturating_sub(record.verified_at_ms) >= ROOTFS_CACHE_RETENTION.as_millis() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn write_task_rootfs_marker(
    rootfs: &Path,
    snapshot: &RootfsSnapshot,
) -> Result<(), ProductionBackendRegistryError> {
    let marker = rootfs
        .parent()
        .ok_or_else(|| {
            ProductionBackendRegistryError::RootUnavailable("rootfs has no parent".into())
        })?
        .join(".hivemind-rootfs.json");
    let value = serde_json::json!({
        "metadata_fingerprint": snapshot.metadata_fingerprint,
        "content_digest": snapshot.content_digest,
    });
    write_json_file_atomically(&marker, &value)
}

fn validate_task_rootfs_marker(
    rootfs: &Path,
    snapshot: &RootfsSnapshot,
) -> Result<(), ProductionBackendRegistryError> {
    let marker = rootfs
        .parent()
        .ok_or_else(|| {
            ProductionBackendRegistryError::RootUnavailable("rootfs has no parent".into())
        })?
        .join(".hivemind-rootfs.json");
    let metadata = fs::symlink_metadata(&marker).map_err(|error| {
        ProductionBackendRegistryError::RootUnavailable(format!(
            "stale task root is missing its rootfs marker: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ProductionBackendRegistryError::RootUnavailable(
            "stale task root has an invalid rootfs marker".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(&marker)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?,
    )
    .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    if value
        .get("metadata_fingerprint")
        .and_then(serde_json::Value::as_str)
        != Some(snapshot.metadata_fingerprint.as_str())
        || value
            .get("content_digest")
            .and_then(serde_json::Value::as_str)
            != Some(snapshot.content_digest.as_str())
    {
        return Err(ProductionBackendRegistryError::RootUnavailable(
            "stale task root does not match the current rootfs template".into(),
        ));
    }
    Ok(())
}

fn write_json_file_atomically<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), ProductionBackendRegistryError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    atomic_replace_config(path, &bytes)
}

fn hash_rootfs_tree(
    root: &Path,
    include_content: bool,
) -> Result<String, ProductionBackendRegistryError> {
    let mut entries = Vec::new();
    collect_rootfs_entries(root, "", &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    hasher.update(if include_content {
        b"hivemind-rootfs-content-v1\\0".as_slice()
    } else {
        b"hivemind-rootfs-metadata-v1\\0".as_slice()
    });
    for (relative, path, is_directory) in entries {
        hasher.update(if is_directory { b"d" } else { b"f" });
        hasher.update((relative.len() as u64).to_be_bytes());
        hasher.update(relative.as_bytes());
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        if !is_directory {
            hasher.update(metadata.len().to_be_bytes());
            if include_content {
                let bytes = fs::read(&path).map_err(|error| {
                    ProductionBackendRegistryError::RootUnavailable(error.to_string())
                })?;
                hasher.update(&bytes);
                let after = fs::symlink_metadata(&path).map_err(|error| {
                    ProductionBackendRegistryError::RootUnavailable(error.to_string())
                })?;
                if after.len() != metadata.len()
                    || after.file_type().is_symlink()
                    || !after.is_file()
                {
                    return Err(ProductionBackendRegistryError::RootUnavailable(
                        "rootfs file changed while it was hashed".into(),
                    ));
                }
            }
        } else if !include_content {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos());
            hasher.update(modified.to_be_bytes());
        }
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn collect_rootfs_entries(
    root: &Path,
    relative_root: &str,
    entries: &mut Vec<(String, PathBuf, bool)>,
) -> Result<(), ProductionBackendRegistryError> {
    let mut children = fs::read_dir(root)
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    children.sort_by_key(|entry| entry.file_name().to_string_lossy().to_string());
    for child in children {
        let name = child
            .file_name()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                ProductionBackendRegistryError::RootUnavailable(
                    "rootfs entry name must be valid UTF-8".into(),
                )
            })?;
        if name.is_empty() || name == "." || name == ".." {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "rootfs entry name is invalid".into(),
            ));
        }
        let relative = if relative_root.is_empty() {
            name
        } else {
            format!("{relative_root}/{name}")
        };
        let path = child.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "rootfs contains a symlink".into(),
            ));
        }
        if metadata.is_dir() {
            entries.push((relative.clone(), path.clone(), true));
            collect_rootfs_entries(&path, &relative, entries)?;
        } else if metadata.is_file() {
            entries.push((relative, path, false));
        } else {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "rootfs contains an unsupported filesystem entry".into(),
            ));
        }
    }
    Ok(())
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn remove_owned_task_directory(path: &Path) -> Result<(), ProductionBackendRegistryError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "task cleanup encountered a symlink".into(),
                ));
            }
            if !metadata.is_dir() {
                return Err(ProductionBackendRegistryError::RootUnavailable(
                    "task cleanup encountered a non-directory root".into(),
                ));
            }
            fs::remove_dir_all(path)
                .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProductionBackendRegistryError::RootUnavailable(
            error.to_string(),
        )),
    }
}

fn atomic_replace_config(path: &Path, bytes: &[u8]) -> Result<(), ProductionBackendRegistryError> {
    let parent = path.parent().ok_or_else(|| {
        ProductionBackendRegistryError::RootUnavailable(
            "OCI config path has no parent directory".into(),
        )
    })?;
    ensure_no_symlink_ancestors(parent)?;
    if matches!(
        fs::symlink_metadata(path),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file()
    ) {
        return Err(ProductionBackendRegistryError::RootUnavailable(
            "OCI config path must be a regular non-symlink file".into(),
        ));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = parent.join(format!(".config-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        file.write_all(bytes)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        file.flush()
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        file.sync_all()
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        drop(file);
        atomic_replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn atomic_replace_file(
    source: &Path,
    destination: &Path,
) -> Result<(), ProductionBackendRegistryError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
        }
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))
    }
}

fn clone_directory_reflink_or_copy(
    source: &Path,
    destination: &Path,
) -> Result<(), ProductionBackendRegistryError> {
    let source_metadata = fs::symlink_metadata(source)
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err(ProductionBackendRegistryError::RootUnavailable(
            "rootfs source must be a real directory".into(),
        ));
    }
    ensure_no_symlink_ancestors(destination)?;
    fs::create_dir_all(destination)
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    ensure_no_symlink_ancestors(destination)?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_string());
    for entry in entries {
        let source_path = entry.path();
        let target = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "operator bundle template contains a symlink".into(),
            ));
        }
        if metadata.is_dir() {
            clone_directory_reflink_or_copy(&source_path, &target)?;
        } else if metadata.is_file() {
            clone_file_reflink_or_copy(&source_path, &target)?;
        } else {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "operator bundle template contains an unsupported filesystem entry".into(),
            ));
        }
    }
    Ok(())
}

fn clone_file_reflink_or_copy(
    source: &Path,
    destination: &Path,
) -> Result<(), ProductionBackendRegistryError> {
    if try_reflink_file(source, destination) {
        return Ok(());
    }
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))
}

#[cfg(target_os = "linux")]
fn try_reflink_file(source: &Path, destination: &Path) -> bool {
    use std::os::fd::AsRawFd;
    const FICLONE: u64 = 0x4004_9409;
    unsafe extern "C" {
        fn ioctl(file_descriptor: i32, request: u64, ...) -> i32;
    }
    let Ok(source) = File::open(source) else {
        return false;
    };
    let destination_path = destination.to_path_buf();
    let Ok(destination) = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination_path)
    else {
        return false;
    };
    // SAFETY: Both owned files remain open during the call. FICLONE takes
    // the destination descriptor and a source descriptor as its integer
    // variadic argument, matching the Linux ioctl ABI; no pointers are passed.
    let result = unsafe { ioctl(destination.as_raw_fd(), FICLONE, source.as_raw_fd()) } == 0;
    drop(destination);
    if !result {
        let _ = fs::remove_file(destination_path);
    }
    result
}

#[cfg(not(target_os = "linux"))]
fn try_reflink_file(_source: &Path, _destination: &Path) -> bool {
    false
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

    #[must_use]
    pub fn get(&self, backend_id: &str) -> Option<&ProductionBackendConfig> {
        self.backends.get(backend_id)
    }

    pub fn registrations(&self) -> impl Iterator<Item = &ProductionBackendConfig> {
        self.backends.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.backends.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct WindowsProductionBackendRegistry {
    backends: BTreeMap<String, WindowsProductionBackendConfig>,
}

impl WindowsProductionBackendRegistry {
    pub fn new(
        registrations: Vec<WindowsProductionBackendConfig>,
    ) -> Result<Self, ProductionBackendRegistryError> {
        if registrations.is_empty() {
            return Err(ProductionBackendRegistryError::WindowsRegistryEmpty);
        }
        let mut backends = BTreeMap::new();
        for registration in registrations {
            registration.validate()?;
            let backend_id = registration.backend_id.clone();
            if backends.insert(backend_id.clone(), registration).is_some() {
                return Err(ProductionBackendRegistryError::DuplicateBackend(backend_id));
            }
        }
        Ok(Self { backends })
    }

    #[must_use]
    pub fn get(&self, backend_id: &str) -> Option<&WindowsProductionBackendConfig> {
        self.backends.get(backend_id)
    }

    pub fn registrations(&self) -> impl Iterator<Item = &WindowsProductionBackendConfig> {
        self.backends.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.backends.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

const WINDOWS_ASSET_FULL_REVERIFY_INTERVAL: Duration = Duration::from_secs(15 * 60);
const WINDOWS_ASSET_CACHE_LIMIT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WindowsAssetCacheKey {
    image_root: PathBuf,
    runner_executable: PathBuf,
    guest_image_digest: String,
    runner_sha256: String,
}

#[derive(Debug, Clone)]
struct WindowsAssetCacheEntry {
    image_metadata_fingerprint: String,
    runner_size_bytes: u64,
    runner_modified: Option<SystemTime>,
    binding: WindowsHcsAssetBinding,
    verified_at: Instant,
}

static WINDOWS_ASSET_CACHE: OnceLock<Mutex<HashMap<WindowsAssetCacheKey, WindowsAssetCacheEntry>>> =
    OnceLock::new();

pub(crate) fn verify_windows_hcs_assets(
    image_root: &Path,
    runner_executable: &Path,
    guest_image_digest: &str,
    runner_sha256: &str,
) -> Result<WindowsHcsAssetBinding, ProductionBackendRegistryError> {
    if !is_sha256_digest(guest_image_digest) {
        return Err(ProductionBackendRegistryError::WindowsImageDigestMismatch);
    }
    if !is_sha256_digest(runner_sha256) {
        return Err(ProductionBackendRegistryError::WindowsRunnerDigestInvalid);
    }
    if !is_absolute_windows_path(image_root) || !is_absolute_windows_path(runner_executable) {
        return Err(ProductionBackendRegistryError::WindowsPathMustBeAbsolute);
    }
    if windows_path_has_traversal(image_root) || windows_path_has_traversal(runner_executable) {
        return Err(ProductionBackendRegistryError::WindowsPathTraversal);
    }
    if !windows_path_is_contained(image_root, runner_executable) {
        return Err(ProductionBackendRegistryError::WindowsRunnerOutsideImage);
    }
    let image_metadata = std::fs::symlink_metadata(image_root).map_err(|error| {
        ProductionBackendRegistryError::WindowsImageUnavailable(error.to_string())
    })?;
    if !image_metadata.is_dir() || is_reparse_point(&image_metadata) {
        return Err(ProductionBackendRegistryError::WindowsImageUnavailable(
            "Windows image root must be a real directory without reparse points".into(),
        ));
    }
    ensure_no_symlink_ancestors(image_root)?;

    let runner_metadata = std::fs::symlink_metadata(runner_executable).map_err(|error| {
        ProductionBackendRegistryError::WindowsRunnerUnavailable(error.to_string())
    })?;
    if !runner_metadata.is_file() || is_reparse_point(&runner_metadata) {
        return Err(ProductionBackendRegistryError::WindowsRunnerUnavailable(
            "Windows runner must be a regular file without a reparse point".into(),
        ));
    }
    let runner_modified = runner_metadata.modified().ok();
    let image_metadata_fingerprint = hash_windows_image_metadata(image_root)
        .map_err(ProductionBackendRegistryError::WindowsImageUnavailable)?;
    let key = WindowsAssetCacheKey {
        image_root: image_root.to_path_buf(),
        runner_executable: runner_executable.to_path_buf(),
        guest_image_digest: guest_image_digest.to_owned(),
        runner_sha256: runner_sha256.to_owned(),
    };
    let cache = WINDOWS_ASSET_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();
    {
        let entries = cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = entries.get(&key)
            && entry.image_metadata_fingerprint == image_metadata_fingerprint
            && entry.runner_size_bytes == runner_metadata.len()
            && entry.runner_modified == runner_modified
            && now.duration_since(entry.verified_at) < WINDOWS_ASSET_FULL_REVERIFY_INTERVAL
        {
            return Ok(entry.binding.clone());
        }
    }

    let actual_runner_sha256 = hash_windows_file(runner_executable).map_err(|error| {
        ProductionBackendRegistryError::WindowsRunnerUnavailable(error.to_string())
    })?;
    if actual_runner_sha256 != runner_sha256 {
        return Err(ProductionBackendRegistryError::WindowsRunnerDigestMismatch);
    }

    let image_material_digest = hash_windows_image_tree(image_root)
        .map_err(ProductionBackendRegistryError::WindowsImageUnavailable)?;
    if image_material_digest != guest_image_digest {
        return Err(ProductionBackendRegistryError::WindowsImageDigestMismatch);
    }
    let relative_runner = windows_relative_path(image_root, runner_executable)
        .ok_or(ProductionBackendRegistryError::WindowsRunnerOutsideImage)?;
    let runner_name = windows_path_file_name(runner_executable)
        .ok_or(ProductionBackendRegistryError::WindowsRunnerOutsideImage)?;
    let binding = WindowsHcsAssetBinding {
        image_material_digest,
        runner_sha256: runner_sha256.to_owned(),
        runner_container_path: format!("C:\\{relative_runner}"),
    };
    if !binding
        .runner_container_path
        .rsplit('\\')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(&runner_name))
    {
        return Err(ProductionBackendRegistryError::WindowsRunnerOutsideImage);
    }

    let mut entries = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if entries.len() >= WINDOWS_ASSET_CACHE_LIMIT
        && !entries.contains_key(&key)
        && let Some(oldest) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.verified_at)
            .map(|(key, _)| key.clone())
    {
        entries.remove(&oldest);
    }
    entries.insert(
        key,
        WindowsAssetCacheEntry {
            image_metadata_fingerprint,
            runner_size_bytes: runner_metadata.len(),
            runner_modified,
            binding: binding.clone(),
            verified_at: now,
        },
    );
    Ok(binding)
}

fn hash_windows_file(path: &Path) -> std::io::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_reparse_point(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "path is not a regular non-reparse file",
        ));
    }
    let expected_size = metadata.len();
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let after = std::fs::symlink_metadata(path)?;
    if after.len() != expected_size || !after.is_file() || is_reparse_point(&after) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed or became a reparse point while it was hashed",
        ));
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn hash_windows_image_tree(root: &Path) -> Result<String, String> {
    let mut entries = Vec::new();
    collect_windows_image_entries(root, "", &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for pair in entries.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err("Windows image contains duplicate case-insensitive paths".into());
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(b"hivemind-windows-image-material-v1\0");
    for (relative, path, is_directory) in entries {
        let relative_bytes = relative.as_bytes();
        hasher.update(if is_directory { b"d" } else { b"f" });
        hasher.update((relative_bytes.len() as u64).to_be_bytes());
        hasher.update(relative_bytes);
        if !is_directory {
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("image file metadata failed: {error}"))?;
            hasher.update(metadata.len().to_be_bytes());
            let mut file =
                File::open(&path).map_err(|error| format!("image file open failed: {error}"))?;
            let mut buffer = vec![0_u8; 128 * 1024];
            loop {
                let read = file
                    .read(&mut buffer)
                    .map_err(|error| format!("image file read failed: {error}"))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            let after = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("image file recheck failed: {error}"))?;
            if after.len() != metadata.len() || !after.is_file() || is_reparse_point(&after) {
                return Err(
                    "Windows image file changed or became a reparse point while hashed".into(),
                );
            }
        }
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn hash_windows_image_metadata(root: &Path) -> Result<String, String> {
    let mut entries = Vec::new();
    collect_windows_image_entries(root, "", &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for pair in entries.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err("Windows image contains duplicate case-insensitive paths".into());
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(b"hivemind-windows-image-metadata-v1\\0");
    for (relative, path, is_directory) in entries {
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("Windows image metadata failed: {error}"))?;
        hasher.update(if is_directory { b"d" } else { b"f" });
        hasher.update((relative.len() as u64).to_be_bytes());
        hasher.update(relative.as_bytes());
        hasher.update(metadata.len().to_be_bytes());
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        hasher.update(modified.to_be_bytes());
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn collect_windows_image_entries(
    root: &Path,
    relative_root: &str,
    entries: &mut Vec<(String, PathBuf, bool)>,
) -> Result<(), String> {
    let mut children = std::fs::read_dir(root)
        .map_err(|error| format!("Windows image directory read failed: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Windows image directory entry failed: {error}"))?;
    children.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());
    for child in children {
        let file_name = child.file_name();
        let name = file_name
            .to_str()
            .filter(|name| !name.is_empty() && !name.chars().any(char::is_control))
            .ok_or_else(|| "Windows image path is not valid UTF-8".to_owned())?;
        let normalized_name = name.to_ascii_lowercase();
        let relative = if relative_root.is_empty() {
            normalized_name
        } else {
            format!("{relative_root}/{normalized_name}")
        };
        let path = child.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("Windows image entry metadata failed: {error}"))?;
        if is_reparse_point(&metadata) {
            return Err(format!(
                "Windows image contains a reparse point: {relative}"
            ));
        }
        if metadata.is_dir() {
            entries.push((relative.clone(), path.clone(), true));
            collect_windows_image_entries(&path, &relative, entries)?;
        } else if metadata.is_file() {
            entries.push((relative, path, false));
        } else {
            return Err(format!(
                "Windows image contains an unsupported entry: {relative}"
            ));
        }
    }
    Ok(())
}

fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn windows_path_is_contained(root: &Path, child: &Path) -> bool {
    let root = normalize_windows_host_path(root);
    let child = normalize_windows_host_path(child);
    child.starts_with(&(root + "\\"))
}

fn windows_relative_path(root: &Path, child: &Path) -> Option<String> {
    let root = normalize_windows_host_path(root);
    let child = normalize_windows_host_path(child);
    child
        .strip_prefix(&(root + "\\"))
        .filter(|relative| !relative.is_empty())
        .map(str::to_owned)
}

fn windows_path_file_name(path: &Path) -> Option<String> {
    normalize_windows_host_path(path)
        .rsplit('\\')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn normalize_windows_host_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seccomp_cache_reuses_valid_profiles_and_invalidates_on_drift() {
        let root = std::env::temp_dir().join(format!(
            "hivemind-seccomp-cache-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("seccomp.json");
        let bytes = br#"{"defaultAction":"SCMP_ACT_ERRNO","syscalls":[{"action":"SCMP_ACT_ALLOW","names":["exit","exit_group"]}]}"#;
        fs::write(&path, bytes).unwrap();
        let digest = crate::sha256_digest(bytes);

        let first = load_verified_seccomp_profile(&path, &digest).unwrap();
        let second = load_verified_seccomp_profile(&path, &digest).unwrap();
        assert_eq!(
            first, second,
            "cached seccomp profile should preserve its value"
        );

        fs::write(&path, b"{}").unwrap();
        assert!(matches!(
            load_verified_seccomp_profile(&path, &digest),
            Err(ProductionBackendRegistryError::SeccompProfileUnavailable(_))
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rootfs_snapshot_cache_fails_closed_when_a_snapshot_is_corrupted() {
        let root = std::env::temp_dir().join(format!(
            "hivemind-rootfs-cache-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let _ = fs::remove_dir_all(&root);
        let template = root.join("template");
        let bundle_root = root.join("bundles");
        fs::create_dir_all(&template).unwrap();
        fs::write(template.join("runtime.txt"), b"template").unwrap();

        let snapshot = rootfs_snapshot_for_template(&template, &bundle_root).unwrap();
        let cache_dir = snapshot.root.parent().unwrap();
        let record_path = cache_dir.join("record.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
        record["verified_at_ms"] = serde_json::json!(0);
        fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
        fs::write(snapshot.root.join("runtime.txt"), b"tampered").unwrap();

        let error = rootfs_snapshot_for_template(&template, &bundle_root).unwrap_err();
        assert!(matches!(
            error,
            ProductionBackendRegistryError::RootUnavailable(message)
                if message.contains("cache snapshot content")
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_asset_cache_invalidates_when_image_metadata_changes() {
        let root = std::env::temp_dir().join(format!(
            "hivemind-windows-asset-cache-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let _ = fs::remove_dir_all(&root);
        let image = root.join("image");
        fs::create_dir_all(&image).unwrap();
        let runner = image.join("runner.exe");
        fs::write(&runner, b"runner-v1").unwrap();
        let runner_digest = hash_windows_file(&runner).unwrap();
        let image_digest = hash_windows_image_tree(&image).unwrap();

        verify_windows_hcs_assets(&image, &runner, &image_digest, &runner_digest)
            .expect("first Windows asset verification should hash the image");
        verify_windows_hcs_assets(&image, &runner, &image_digest, &runner_digest)
            .expect("unchanged Windows assets should use their cache entry");

        fs::write(image.join("runtime.txt"), b"runtime-v1").unwrap();
        assert_eq!(
            verify_windows_hcs_assets(&image, &runner, &image_digest, &runner_digest).unwrap_err(),
            ProductionBackendRegistryError::WindowsImageDigestMismatch
        );
        let new_image_digest = hash_windows_image_tree(&image).unwrap();
        verify_windows_hcs_assets(&image, &runner, &new_image_digest, &runner_digest)
            .expect("new image digest should create a new verified cache entry");

        let _ = fs::remove_dir_all(root);
    }
}

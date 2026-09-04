//! Operator-owned configuration and routing for production OCI backends.
//!
//! This module deliberately contains no URL, command-line, or Worker-provided
//! path interpretation. Configuration is loaded by the Worker from an
//! operator-controlled file and every path is validated before it can reach
//! the OCI launcher.

use crate::onnx::OnnxBackendConfig;
use crate::sandbox::{
    BackendExecutionMode, ProductionSandboxLaunch, SandboxDevice, SandboxMount,
    WindowsNativeSandboxLaunch, WindowsSandboxPolicy,
};
use crate::{
    GeneralComputeRequest, MANAGED_DSL_RUNTIME_VERSION, MANAGED_DSL_SEMANTICS_MANIFEST_SHA256,
    gpu::GpuSelection, managed_gpu::ManagedGpuCapability,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Operator-owned registration for the cross-platform closed managed DSL.
///
/// Unlike OCI/HCS registrations this contains no executable, image, or host
/// path. The interpreter is the backend and its semantics digest is the trust
/// binding used by Worker admission and proof/settlement validation.
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
        self.validate_mount_contract()?;
        self.as_general_compute_config()
            .materialize_bundle_for_launch(request, task_id, launch)
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
    pub image_root: PathBuf,
    pub entrypoint: Vec<String>,
    pub mounts: Vec<WindowsHcsMountSpec>,
    pub result_path: PathBuf,
    pub result_container_path: String,
    pub max_output_bytes: usize,
    pub network_isolated: bool,
    pub root_read_only: bool,
    pub memory_bytes: u64,
    pub cpu_millis: u64,
    pub process_limit: u32,
    pub thread_limit: u32,
    pub scratch_bytes: u64,
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
        let image_task_root = self.image_root.join(task_id);
        let artifact_task_root = self.artifact_root.join(task_id);
        ensure_contained(&self.image_root, &image_task_root)?;
        ensure_contained(&self.artifact_root, &artifact_task_root)?;
        ensure_no_symlink_ancestors(&image_task_root)?;
        ensure_no_symlink_ancestors(&artifact_task_root)?;
        Ok((image_task_root, artifact_task_root))
    }

    /// Build the operator-owned HCS specification without invoking HCS.
    ///
    /// Every host path comes from this validated registration and the
    /// task-specific operator roots; no Worker-provided path is accepted.
    pub fn hcs_spec(
        &self,
        task_id: &str,
    ) -> Result<WindowsHcsContainerSpec, ProductionBackendRegistryError> {
        self.validate()?;
        let (image_task_root, artifact_task_root) = self.task_root(task_id)?;
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
        Ok(WindowsHcsContainerSpec {
            container_id,
            image_root: image_task_root,
            entrypoint: self.entrypoint.clone(),
            mounts,
            result_path: artifact_task_root.join("scratch").join("result.json"),
            result_container_path: "C:\\work\\output\\result.json".into(),
            max_output_bytes: self.max_output_bytes,
            network_isolated: true,
            root_read_only: true,
            memory_bytes: self.policy.memory_bytes,
            cpu_millis: self.policy.cpu_millis,
            process_limit: self.policy.process_limit,
            thread_limit: self.policy.thread_limit,
            scratch_bytes: self.policy.scratch_bytes,
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
            if path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            }) {
                return Err(ProductionBackendRegistryError::WindowsPathTraversal);
            }
        }
        if !is_sha256_digest(&self.runner_sha256) {
            return Err(ProductionBackendRegistryError::WindowsRunnerDigestInvalid);
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
    WindowsLaunchInvalid(crate::sandbox::ProductionSandboxError),
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
        devices.extend(self.policy.devices.iter().cloned());
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
        std::fs::write(&config_path, bytes)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        Ok((bundle_root, artifact_root))
    }

    /// Materialize a bundle using the exact device set selected by trusted
    /// admission. The legacy two-argument method remains available for CPU
    /// callers and static operator policies.
    pub fn materialize_bundle_for_launch(
        &self,
        request: &GeneralComputeRequest,
        task_id: &str,
        launch: &ProductionSandboxLaunch,
    ) -> Result<(PathBuf, PathBuf), ProductionBackendRegistryError> {
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
        let (bundle_root, artifact_root) = self.materialize_bundle(request, task_id)?;
        let config_path = bundle_root.join("config.json");
        let bytes = std::fs::read(&config_path)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        let mut config: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        let linux = config
            .get_mut("linux")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| {
                ProductionBackendRegistryError::RootUnavailable(
                    "materialized OCI config has no linux object".into(),
                )
            })?;
        let mut devices = crate::sandbox::standard_linux_devices();
        devices.extend(launch.policy.devices.iter().cloned());
        linux.insert(
            "devices".into(),
            serde_json::Value::Array(devices.iter().map(SandboxDevice::oci_spec).collect()),
        );
        linux.insert(
            "resources".into(),
            serde_json::json!({
                "devices": devices.iter().map(SandboxDevice::cgroup_rule).collect::<Vec<_>>()
            }),
        );
        let canonical = serde_json::to_vec(&config)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        std::fs::write(config_path, canonical)
            .map_err(|error| ProductionBackendRegistryError::RootUnavailable(error.to_string()))?;
        Ok((bundle_root, artifact_root))
    }

    fn load_seccomp_profile(&self) -> Result<serde_json::Value, ProductionBackendRegistryError> {
        let metadata = std::fs::symlink_metadata(&self.seccomp_profile_path).map_err(|error| {
            ProductionBackendRegistryError::SeccompProfileUnavailable(error.to_string())
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
                "seccomp profile must be a regular non-symlink file".into(),
            ));
        }
        let bytes = std::fs::read(&self.seccomp_profile_path).map_err(|error| {
            ProductionBackendRegistryError::SeccompProfileUnavailable(error.to_string())
        })?;
        let expected_digest = match &self.policy.seccomp {
            crate::sandbox::SeccompPolicy::DefaultDeny { profile_sha256 } => profile_sha256,
            crate::sandbox::SeccompPolicy::Disabled => {
                return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
                    "production seccomp policy cannot be disabled".into(),
                ));
            }
        };
        if crate::sha256_digest(&bytes) != *expected_digest {
            return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
                "seccomp profile SHA-256 does not match the policy pin".into(),
            ));
        }
        let profile: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            ProductionBackendRegistryError::SeccompProfileUnavailable(format!(
                "seccomp profile is not valid JSON: {error}"
            ))
        })?;
        validate_seccomp_profile(&profile).map_err(|message| {
            ProductionBackendRegistryError::SeccompProfileUnavailable(message)
        })?;
        let canonical = serde_json::to_vec(&profile).map_err(|error| {
            ProductionBackendRegistryError::SeccompProfileUnavailable(error.to_string())
        })?;
        if canonical != bytes {
            return Err(ProductionBackendRegistryError::SeccompProfileUnavailable(
                "seccomp profile must use canonical JSON bytes".into(),
            ));
        }
        Ok(profile)
    }
}

fn copy_directory_no_symlinks(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), ProductionBackendRegistryError> {
    let mut hardlinks = std::collections::HashMap::new();
    copy_directory_no_symlinks_inner(source, destination, &mut hardlinks)
}

fn copy_directory_no_symlinks_inner(
    source: &std::path::Path,
    destination: &std::path::Path,
    hardlinks: &mut std::collections::HashMap<(u64, u64), PathBuf>,
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
        if let Ok(target_metadata) = std::fs::symlink_metadata(&target)
            && target_metadata.file_type().is_symlink()
        {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "task bundle destination contains a symlink".into(),
            ));
        }
        if metadata.file_type().is_symlink() {
            return Err(ProductionBackendRegistryError::RootUnavailable(
                "operator bundle template contains a symlink".into(),
            ));
        }
        if metadata.is_dir() {
            copy_directory_no_symlinks_inner(&entry.path(), &target, hardlinks)?;
        } else if metadata.is_file() {
            #[cfg(unix)]
            let hardlink_key = {
                use std::os::unix::fs::MetadataExt;
                Some((metadata.dev(), metadata.ino()))
            };
            #[cfg(not(unix))]
            let hardlink_key = None;

            if let Some(key) = hardlink_key {
                if let Some(existing) = hardlinks.get(&key) {
                    if std::fs::hard_link(existing, &target).is_err() {
                        std::fs::copy(entry.path(), &target).map_err(|error| {
                            ProductionBackendRegistryError::RootUnavailable(error.to_string())
                        })?;
                    }
                } else {
                    std::fs::copy(entry.path(), &target).map_err(|error| {
                        ProductionBackendRegistryError::RootUnavailable(error.to_string())
                    })?;
                    hardlinks.insert(key, target);
                }
            } else {
                std::fs::copy(entry.path(), &target).map_err(|error| {
                    ProductionBackendRegistryError::RootUnavailable(error.to_string())
                })?;
            }
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

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

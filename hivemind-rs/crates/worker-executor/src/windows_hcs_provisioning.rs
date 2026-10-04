//! Zero-configuration provisioning for the signed native Windows HCS bundle.
//!
//! A packaged Worker may carry its HCS image material and custom guest runner
//! beside the executable. The bundle is accepted only after its root signature,
//! package-relative paths, pinned bytes, Windows policy, and HCS provider have
//! all been checked. Missing or invalid material disables this backend; it never
//! turns into direct host execution or another isolation implementation.

#[cfg(any(windows, test))]
use general_compute_runtime::production::WindowsProductionBackendConfig;
use general_compute_runtime::production::WindowsProductionBackendRegistry;
#[cfg(any(windows, test))]
use general_compute_runtime::sandbox::BackendExecutionMode;
use general_compute_runtime::sandbox::WindowsSandboxPolicy;
use general_compute_runtime::TrustedWorkerCapabilityRegistration;
#[cfg(any(windows, test))]
use general_compute_runtime::{
    BackendRegistration, WorkerCapabilities, MAX_OUTPUT_BYTES, MAX_THREADS, MAX_WALL_TIME_MS,
};
#[cfg(any(windows, test))]
use hivemind_client_runtime::update::{UpdateError, UpdateVerifier};
use serde::{Deserialize, Serialize};
#[cfg(any(windows, test))]
use std::collections::BTreeSet;
#[cfg(any(windows, test))]
use std::fs;
#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const WINDOWS_HCS_RUNTIME_SCHEMA_VERSION: u32 = 1;
pub const WINDOWS_HCS_RUNTIME_PRODUCT: &str = "hivemind-windows-worker-hcs-runtime";
pub const WINDOWS_HCS_RUNTIME_BUNDLE_DIR: &str = "windows-hcs-runtime";
pub const WINDOWS_HCS_RUNTIME_MANIFEST_FILE: &str = "bundle-manifest.json";
pub const WINDOWS_HCS_SCHEMA_MAJOR: u32 = 2;
pub const WINDOWS_HCS_SCHEMA_MINOR: u32 = 1;

#[cfg(any(windows, test))]
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
#[cfg(any(windows, test))]
const MAX_BACKENDS: usize = 64;
#[cfg(any(windows, test))]
const MAX_FIELD_BYTES: usize = 512;
#[cfg(any(windows, test))]
const MAX_PATH_BYTES: usize = 512;
#[cfg(any(windows, test))]
const MAX_ENTRYPOINT_PARTS: usize = 64;
#[cfg(any(windows, test))]
const CLOCK_SKEW_SECS: u64 = 300;

#[derive(Debug, Error)]
pub enum WindowsHcsProvisioningError {
    #[error("Windows HCS runtime bundle is invalid: {0}")]
    InvalidBundle(String),
    #[error("Windows HCS runtime manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("Windows HCS runtime manifest signature is invalid: {0}")]
    InvalidSignature(String),
    #[error("Windows HCS runtime asset verification failed: {0}")]
    AssetVerification(String),
    #[error("Windows HCS runtime host is unavailable: {0}")]
    HostUnavailable(String),
    #[error("Windows HCS runtime state root is unavailable: {0}")]
    StateRootUnavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsHcsRuntimeManifest {
    pub schema_version: u32,
    pub product: String,
    pub architecture: String,
    pub bundle_version: String,
    pub issued_at_unix: u64,
    pub expires_at_unix: u64,
    pub hcs_schema_major: u32,
    pub hcs_schema_minor: u32,
    pub backends: Vec<WindowsHcsRuntimeBackendManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsHcsRuntimeBackendManifest {
    pub backend_id: String,
    pub guest_image_digest: String,
    pub image_path: String,
    pub runner_path: String,
    pub runner_sha256: String,
    pub entrypoint: Vec<String>,
    pub policy: WindowsSandboxPolicy,
    pub max_output_bytes: usize,
    pub timeout_ms: u64,
    pub max_threads: u32,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedWindowsHcsRuntimeManifest {
    pub manifest: WindowsHcsRuntimeManifest,
    pub signature: String,
}

/// All state derived from a verified package bundle. The registration is still
/// subject to Nodepool's operator-owned capability admission before scheduling.
#[derive(Debug, Clone)]
pub struct WindowsHcsRuntimeProvisioning {
    pub bundle_root: PathBuf,
    pub state_root: PathBuf,
    pub registry: WindowsProductionBackendRegistry,
    pub trusted_registration: TrustedWorkerCapabilityRegistration,
}

/// Serialize the exact bytes that the independent release system must sign.
/// Backend rows are sorted by ID so signing is stable even if a build tool
/// assembled them in a different order.
pub fn canonical_windows_hcs_runtime_manifest_bytes(
    manifest: &WindowsHcsRuntimeManifest,
) -> Result<Vec<u8>, WindowsHcsProvisioningError> {
    let mut canonical = manifest.clone();
    canonical
        .backends
        .sort_by(|left, right| left.backend_id.cmp(&right.backend_id));
    serde_json::to_vec(&canonical)
        .map_err(|error| WindowsHcsProvisioningError::InvalidManifest(error.to_string()))
}

/// Load the fixed package-relative bundle. Non-Windows builds deliberately do
/// not advertise or emulate HCS.
pub fn load_package_relative(
) -> Result<Option<WindowsHcsRuntimeProvisioning>, WindowsHcsProvisioningError> {
    #[cfg(not(windows))]
    {
        Ok(None)
    }

    #[cfg(windows)]
    {
        let executable = std::env::current_exe().map_err(|error| {
            WindowsHcsProvisioningError::InvalidBundle(format!(
                "could not locate the Worker executable: {error}"
            ))
        })?;
        let executable_metadata = fs::symlink_metadata(&executable).map_err(|error| {
            WindowsHcsProvisioningError::InvalidBundle(format!(
                "Worker executable metadata is unavailable: {error}"
            ))
        })?;
        if !executable_metadata.is_file() || is_reparse_point(&executable_metadata) {
            return Err(WindowsHcsProvisioningError::InvalidBundle(
                "Worker executable must be a regular non-reparse file".into(),
            ));
        }
        let package_root = executable.parent().ok_or_else(|| {
            WindowsHcsProvisioningError::InvalidBundle(
                "Worker executable has no package directory".into(),
            )
        })?;
        ensure_no_reparse_ancestors(package_root, "Worker package directory")?;
        let bundle_root = package_root.join(WINDOWS_HCS_RUNTIME_BUNDLE_DIR);
        match fs::symlink_metadata(&bundle_root) {
            Ok(metadata) => {
                if !metadata.is_dir() || is_reparse_point(&metadata) {
                    return Err(WindowsHcsProvisioningError::InvalidBundle(
                        "package-relative HCS bundle must be a real directory".into(),
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(WindowsHcsProvisioningError::InvalidBundle(format!(
                    "package-relative HCS bundle cannot be inspected: {error}"
                )))
            }
        }
        let state_root =
            crate::hcs_journal::HcsExecutionJournal::default_windows_worker_state_root().map_err(
                |error| WindowsHcsProvisioningError::StateRootUnavailable(error.to_string()),
            )?;
        load_bundle(&bundle_root, &state_root, now_unix())
    }
}

#[cfg(any(windows, test))]
fn load_bundle(
    bundle_root: &Path,
    state_root: &Path,
    now_unix: u64,
) -> Result<Option<WindowsHcsRuntimeProvisioning>, WindowsHcsProvisioningError> {
    ensure_real_directory(bundle_root, "HCS bundle")?;
    validate_state_root(state_root)?;
    let manifest_path = bundle_root.join(WINDOWS_HCS_RUNTIME_MANIFEST_FILE);
    let signed_manifest = read_signed_manifest(&manifest_path)?;
    validate_manifest(&signed_manifest.manifest, now_unix)?;
    let canonical = canonical_windows_hcs_runtime_manifest_bytes(&signed_manifest.manifest)?;
    let verifier = UpdateVerifier::embedded().map_err(|error| {
        WindowsHcsProvisioningError::InvalidSignature(format!("update trust root: {error}"))
    })?;
    verifier
        .verify_root_signature(&canonical, &signed_manifest.signature)
        .map_err(|error| match error {
            UpdateError::InvalidSignature
            | UpdateError::InvalidRootKey
            | UpdateError::InvalidMetadata(_) => {
                WindowsHcsProvisioningError::InvalidSignature(error.to_string())
            }
            other => WindowsHcsProvisioningError::InvalidSignature(other.to_string()),
        })?;

    let mut configs = Vec::with_capacity(signed_manifest.manifest.backends.len());
    for backend in &signed_manifest.manifest.backends {
        let image_root = bundle_root.join(Path::new(&backend.image_path));
        let runner_executable = bundle_root.join(Path::new(&backend.runner_path));
        let artifact_root = state_root.join("artifacts").join(&backend.backend_id);
        let config = WindowsProductionBackendConfig {
            backend_id: backend.backend_id.clone(),
            guest_image_digest: backend.guest_image_digest.clone(),
            image_root,
            artifact_root,
            runner_executable,
            runner_sha256: backend.runner_sha256.clone(),
            entrypoint: backend.entrypoint.clone(),
            policy: backend.policy.clone(),
            max_output_bytes: backend.max_output_bytes,
            timeout_ms: backend.timeout_ms,
        };
        config
            .validate()
            .map_err(|error| WindowsHcsProvisioningError::InvalidManifest(error.to_string()))?;
        config
            .policy
            .hcs_enforced_resource_limits()
            .map_err(|error| WindowsHcsProvisioningError::InvalidManifest(error.to_string()))?;
        config
            .verify_operator_assets()
            .map_err(|error| WindowsHcsProvisioningError::AssetVerification(error.to_string()))?;
        configs.push(config);
    }

    #[cfg(not(windows))]
    {
        let _ = configs;
        Ok(None)
    }

    #[cfg(windows)]
    {
        general_compute_runtime::windows_hcs::probe_provider(std::time::Duration::from_secs(5))
            .map_err(|error| WindowsHcsProvisioningError::HostUnavailable(error.to_string()))?;
        let trusted_registration = trusted_registration(&signed_manifest.manifest.backends);
        let registry = WindowsProductionBackendRegistry::new(configs)
            .map_err(|error| WindowsHcsProvisioningError::InvalidManifest(error.to_string()))?;
        Ok(Some(WindowsHcsRuntimeProvisioning {
            bundle_root: bundle_root.to_path_buf(),
            state_root: state_root.to_path_buf(),
            registry,
            trusted_registration,
        }))
    }
}

#[cfg(any(windows, test))]
fn read_signed_manifest(
    path: &Path,
) -> Result<SignedWindowsHcsRuntimeManifest, WindowsHcsProvisioningError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        WindowsHcsProvisioningError::InvalidBundle(format!(
            "signed HCS bundle manifest is unavailable: {error}"
        ))
    })?;
    if !metadata.is_file() || is_reparse_point(&metadata) {
        return Err(WindowsHcsProvisioningError::InvalidBundle(
            "signed HCS bundle manifest must be a regular non-reparse file".into(),
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(WindowsHcsProvisioningError::InvalidManifest(format!(
            "manifest size must be between 1 and {MAX_MANIFEST_BYTES} bytes"
        )));
    }
    let bytes = fs::read(path).map_err(|error| {
        WindowsHcsProvisioningError::InvalidManifest(format!(
            "signed HCS bundle manifest cannot be read: {error}"
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        WindowsHcsProvisioningError::InvalidManifest(format!(
            "signed HCS bundle manifest JSON is malformed: {error}"
        ))
    })
}

#[cfg(any(windows, test))]
fn validate_manifest(
    manifest: &WindowsHcsRuntimeManifest,
    now_unix: u64,
) -> Result<(), WindowsHcsProvisioningError> {
    if manifest.schema_version != WINDOWS_HCS_RUNTIME_SCHEMA_VERSION {
        return Err(invalid_manifest("unsupported bundle schema version"));
    }
    if manifest.product != WINDOWS_HCS_RUNTIME_PRODUCT {
        return Err(invalid_manifest("bundle product does not match the Worker"));
    }
    validate_field(&manifest.product, "product", MAX_FIELD_BYTES)?;
    validate_field(&manifest.architecture, "architecture", MAX_FIELD_BYTES)?;
    if manifest.architecture != current_architecture() {
        return Err(invalid_manifest(
            "bundle architecture does not match this Worker",
        ));
    }
    validate_version(&manifest.bundle_version, "bundle_version")?;
    if manifest.issued_at_unix > now_unix.saturating_add(CLOCK_SKEW_SECS)
        || manifest.expires_at_unix <= manifest.issued_at_unix
        || manifest.expires_at_unix <= now_unix
    {
        return Err(invalid_manifest("bundle validity window is not current"));
    }
    if manifest.hcs_schema_major != WINDOWS_HCS_SCHEMA_MAJOR
        || manifest.hcs_schema_minor != WINDOWS_HCS_SCHEMA_MINOR
    {
        return Err(invalid_manifest(
            "bundle HCS schema does not match the Worker",
        ));
    }
    if manifest.backends.is_empty() || manifest.backends.len() > MAX_BACKENDS {
        return Err(invalid_manifest(
            "bundle backend count is outside its limit",
        ));
    }

    let mut backend_ids = BTreeSet::new();
    let mut folded_backend_ids = BTreeSet::new();
    for backend in &manifest.backends {
        validate_backend(backend)?;
        if !backend_ids.insert(backend.backend_id.clone())
            || !folded_backend_ids.insert(backend.backend_id.to_ascii_lowercase())
        {
            return Err(invalid_manifest(
                "bundle contains duplicate backend identities",
            ));
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn validate_backend(
    backend: &WindowsHcsRuntimeBackendManifest,
) -> Result<(), WindowsHcsProvisioningError> {
    validate_identifier(&backend.backend_id, "backend_id")?;
    validate_digest(&backend.guest_image_digest, "guest_image_digest")?;
    validate_digest(&backend.runner_sha256, "runner_sha256")?;
    validate_relative_path(&backend.image_path, "image_path")?;
    validate_relative_path(&backend.runner_path, "runner_path")?;

    let image_components = path_components(&backend.image_path);
    let runner_components = path_components(&backend.runner_path);
    if runner_components.len() <= image_components.len()
        || !runner_components
            .iter()
            .zip(image_components.iter())
            .all(|(runner, image)| runner.eq_ignore_ascii_case(image))
    {
        return Err(invalid_manifest("runner_path must be below image_path"));
    }
    let runner_name = runner_components
        .last()
        .expect("validated runner path has a component");
    if backend
        .entrypoint
        .first()
        .is_none_or(|entrypoint| !entrypoint.eq_ignore_ascii_case(runner_name))
    {
        return Err(invalid_manifest(
            "entrypoint must begin with the bundled runner filename",
        ));
    }
    if backend.entrypoint.is_empty() || backend.entrypoint.len() > MAX_ENTRYPOINT_PARTS {
        return Err(invalid_manifest("entrypoint count is outside its limit"));
    }
    for (index, part) in backend.entrypoint.iter().enumerate() {
        validate_field(part, &format!("entrypoint[{index}]"), MAX_FIELD_BYTES)?;
    }
    if backend.max_output_bytes == 0
        || u64::try_from(backend.max_output_bytes).unwrap_or(u64::MAX) > MAX_OUTPUT_BYTES
    {
        return Err(invalid_manifest(
            "backend output limit is outside its limit",
        ));
    }
    if backend.timeout_ms == 0 || backend.timeout_ms > MAX_WALL_TIME_MS {
        return Err(invalid_manifest("backend timeout is outside its limit"));
    }
    if backend.max_threads == 0 || backend.max_threads > MAX_THREADS {
        return Err(invalid_manifest(
            "backend thread limit is outside its limit",
        ));
    }
    backend
        .policy
        .validate()
        .map_err(|error| invalid_manifest(&format!("Windows policy is invalid: {error:?}")))?;
    let mut capabilities = BTreeSet::new();
    for capability in &backend.capabilities {
        validate_field(capability, "capability", MAX_FIELD_BYTES)?;
        if !capabilities.insert(capability) {
            return Err(invalid_manifest("backend capabilities contain duplicates"));
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn trusted_registration(
    backends: &[WindowsHcsRuntimeBackendManifest],
) -> TrustedWorkerCapabilityRegistration {
    let mut guest_image_digests = BTreeSet::new();
    let mut capabilities = BTreeSet::new();
    let mut worker_max_threads = 0;
    let mut registrations = Vec::with_capacity(backends.len());
    for backend in backends {
        guest_image_digests.insert(backend.guest_image_digest.clone());
        capabilities.extend(backend.capabilities.iter().cloned());
        worker_max_threads = worker_max_threads.max(backend.max_threads);
        registrations.push(BackendRegistration {
            backend_id: backend.backend_id.clone(),
            execution_mode: BackendExecutionMode::ProductionSandboxedWindows,
            guest_image_digest: backend.guest_image_digest.clone(),
            capabilities: backend.capabilities.clone(),
            max_threads: backend.max_threads,
            network_allowed: false,
            filesystem_read_only: true,
            gpu_allowed: false,
        });
    }
    TrustedWorkerCapabilityRegistration {
        worker: WorkerCapabilities {
            guest_image_digests: guest_image_digests.into_iter().collect(),
            capabilities: capabilities.into_iter().collect(),
            max_threads: worker_max_threads,
            gpu_available: false,
        },
        gpu_capabilities: Vec::new(),
        managed_gpu_backends: Vec::new(),
        backends: registrations,
    }
}

#[cfg(any(windows, test))]
fn current_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        _ => "x86_64",
    }
}

#[cfg(any(windows, test))]
fn validate_version(value: &str, field: &str) -> Result<(), WindowsHcsProvisioningError> {
    validate_field(value, field, MAX_FIELD_BYTES)?;
    let mut parts = value.split('.');
    for _ in 0..3 {
        let part = parts
            .next()
            .ok_or_else(|| invalid_manifest(&format!("{field} must use major.minor.patch")))?;
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return Err(invalid_manifest(&format!(
                "{field} contains a non-canonical numeric component"
            )));
        }
        part.parse::<u64>()
            .map_err(|_| invalid_manifest(&format!("{field} contains a non-numeric component")))?;
    }
    if parts.next().is_some() {
        return Err(invalid_manifest(&format!(
            "{field} must use exactly three components"
        )));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn validate_digest(value: &str, field: &str) -> Result<(), WindowsHcsProvisioningError> {
    let valid = value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if !valid {
        return Err(invalid_manifest(&format!(
            "{field} must use sha256:<64 hex characters>"
        )));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn validate_identifier(value: &str, field: &str) -> Result<(), WindowsHcsProvisioningError> {
    validate_field(value, field, MAX_FIELD_BYTES)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid_manifest(&format!(
            "{field} contains unsafe characters"
        )));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn validate_relative_path(value: &str, field: &str) -> Result<(), WindowsHcsProvisioningError> {
    if value.is_empty() || value.len() > MAX_PATH_BYTES || value.contains('\\') {
        return Err(invalid_manifest(&format!(
            "{field} must be a bounded forward-slash relative path"
        )));
    }
    if value.starts_with('/')
        || value.as_bytes().get(1).is_some_and(|byte| *byte == b':')
        || value.chars().any(char::is_control)
    {
        return Err(invalid_manifest(&format!("{field} is rooted or unsafe")));
    }
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(invalid_manifest(&format!("{field} contains traversal")));
        }
        if !component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(invalid_manifest(&format!(
                "{field} contains an unsafe path component"
            )));
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn path_components(value: &str) -> Vec<&str> {
    value.split('/').collect()
}

#[cfg(any(windows, test))]
fn validate_field(
    value: &str,
    field: &str,
    max_bytes: usize,
) -> Result<(), WindowsHcsProvisioningError> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(invalid_manifest(&format!(
            "{field} must be non-empty, bounded, and free of control characters"
        )));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn invalid_manifest(message: &str) -> WindowsHcsProvisioningError {
    WindowsHcsProvisioningError::InvalidManifest(message.into())
}

#[cfg(any(windows, test))]
fn validate_state_root(path: &Path) -> Result<(), WindowsHcsProvisioningError> {
    if !path.is_absolute() || path.to_string_lossy().chars().any(char::is_control) {
        return Err(WindowsHcsProvisioningError::StateRootUnavailable(
            "Worker state root must be an absolute path without control characters".into(),
        ));
    }
    ensure_no_reparse_ancestors(path, "Worker state root")
        .map_err(|error| WindowsHcsProvisioningError::StateRootUnavailable(error.to_string()))
}

#[cfg(any(windows, test))]
fn ensure_real_directory(path: &Path, label: &str) -> Result<(), WindowsHcsProvisioningError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        WindowsHcsProvisioningError::InvalidBundle(format!("{label} is unavailable: {error}"))
    })?;
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(WindowsHcsProvisioningError::InvalidBundle(format!(
            "{label} must be a real directory without reparse points"
        )));
    }
    ensure_no_reparse_ancestors(path, label)
}

#[cfg(any(windows, test))]
fn ensure_no_reparse_ancestors(
    path: &Path,
    label: &str,
) -> Result<(), WindowsHcsProvisioningError> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_reparse_point(&metadata) => {
                return Err(WindowsHcsProvisioningError::InvalidBundle(format!(
                    "{label} crosses a reparse-point boundary"
                )))
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(WindowsHcsProvisioningError::InvalidBundle(format!(
                    "{label} has a non-directory ancestor"
                )))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(WindowsHcsProvisioningError::InvalidBundle(format!(
                    "{label} ancestor cannot be inspected: {error}"
                )))
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x0400 != 0
}

#[cfg(all(not(windows), test))]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn policy() -> WindowsSandboxPolicy {
        WindowsSandboxPolicy {
            isolation: general_compute_runtime::sandbox::WindowsIsolationMode::Process,
            network: general_compute_runtime::sandbox::WindowsSandboxNetworkPolicy::DenyAll,
            root_filesystem:
                general_compute_runtime::sandbox::WindowsRootFilesystemPolicy::ReadOnly,
            mounts: vec![
                general_compute_runtime::sandbox::SandboxMount::ReadOnlyArtifact {
                    artifact_id: "source".into(),
                    destination: "/work/source".into(),
                },
                general_compute_runtime::sandbox::SandboxMount::EphemeralScratch {
                    destination: "/work/output".into(),
                    max_bytes: 1024,
                },
            ],
            memory_bytes: 1024 * 1024,
            cpu_millis: 1,
            processor_maximum: 100,
            process_limit: 1,
            thread_limit: 1,
            scratch_bytes: 1024,
        }
    }

    fn backend() -> WindowsHcsRuntimeBackendManifest {
        WindowsHcsRuntimeBackendManifest {
            backend_id: "python".into(),
            guest_image_digest: digest('a'),
            image_path: "images/python".into(),
            runner_path: "images/python/hivemind-runner.exe".into(),
            runner_sha256: digest('b'),
            entrypoint: vec!["hivemind-runner.exe".into()],
            policy: policy(),
            max_output_bytes: 256 * 1024,
            timeout_ms: 120_000,
            max_threads: 4,
            capabilities: vec!["python".into()],
        }
    }

    fn manifest() -> WindowsHcsRuntimeManifest {
        WindowsHcsRuntimeManifest {
            schema_version: WINDOWS_HCS_RUNTIME_SCHEMA_VERSION,
            product: WINDOWS_HCS_RUNTIME_PRODUCT.into(),
            architecture: current_architecture().into(),
            bundle_version: "1.0.0".into(),
            issued_at_unix: 900,
            expires_at_unix: 2_000,
            hcs_schema_major: WINDOWS_HCS_SCHEMA_MAJOR,
            hcs_schema_minor: WINDOWS_HCS_SCHEMA_MINOR,
            backends: vec![backend()],
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_loader_does_not_advertise_hcs() {
        assert!(load_package_relative().unwrap().is_none());
    }

    #[test]
    fn registration_preserves_backend_limits_and_isolation() {
        let backend = backend();
        let registration = trusted_registration(std::slice::from_ref(&backend));
        assert_eq!(registration.worker.max_threads, backend.max_threads);
        assert_eq!(
            registration.worker.guest_image_digests,
            vec![backend.guest_image_digest.clone()]
        );
        assert!(!registration.worker.gpu_available);
        assert_eq!(registration.backends.len(), 1);
        let registered = &registration.backends[0];
        assert_eq!(registered.backend_id, backend.backend_id);
        assert_eq!(
            registered.execution_mode,
            BackendExecutionMode::ProductionSandboxedWindows
        );
        assert_eq!(registered.capabilities, backend.capabilities);
        assert_eq!(registered.max_threads, backend.max_threads);
        assert!(!registered.network_allowed);
        assert!(registered.filesystem_read_only);
        assert!(!registered.gpu_allowed);
    }

    #[test]
    fn canonical_manifest_sort_is_stable() {
        let mut value = manifest();
        let mut other = backend();
        other.backend_id = "alpha".into();
        value.backends.insert(0, other);
        let first = canonical_windows_hcs_runtime_manifest_bytes(&value).unwrap();
        value.backends.reverse();
        let second = canonical_windows_hcs_runtime_manifest_bytes(&value).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_manifest_traversal_and_rooted_paths() {
        for path in ["../images", "/images", "C:/images", "images\\python"] {
            assert!(validate_relative_path(path, "image_path").is_err());
        }
    }

    #[test]
    fn rejects_runner_outside_image() {
        let mut value = backend();
        value.runner_path = "images/other/hivemind-runner.exe".into();
        assert!(validate_backend(&value).is_err());
    }

    #[test]
    fn rejects_wrong_architecture_and_expired_manifest() {
        let mut value = manifest();
        value.architecture = "wrong".into();
        assert!(validate_manifest(&value, 1_000).is_err());
        let mut value = manifest();
        value.expires_at_unix = 999;
        assert!(validate_manifest(&value, 1_000).is_err());
    }

    #[test]
    fn malformed_root_signature_is_rejected_before_assets() {
        let root =
            std::env::temp_dir().join(format!("hivemind-hcs-bundle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let signed = SignedWindowsHcsRuntimeManifest {
            manifest: manifest(),
            signature: "00".into(),
        };
        fs::write(
            root.join(WINDOWS_HCS_RUNTIME_MANIFEST_FILE),
            serde_json::to_vec(&signed).unwrap(),
        )
        .unwrap();
        let error = load_bundle(&root, &root.join("state"), 1_000).unwrap_err();
        assert!(matches!(
            error,
            WindowsHcsProvisioningError::InvalidSignature(_)
        ));
        let _ = fs::remove_dir_all(root);
    }
}

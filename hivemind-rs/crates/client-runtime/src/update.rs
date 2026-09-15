//! Fail-closed verification and installation primitives for signed client updates.
//!
//! Update metadata is a separate trust domain from Worker execution and
//! Nodepool authentication. The embedded root key is the only authority that
//! can authorize release-key metadata; release keys authorize package
//! manifests. No private signing material belongs in this crate or in a
//! downloaded package.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use zip::ZipArchive;

pub const UPDATE_PROTOCOL_VERSION: u32 = 1;
pub const UPDATE_MANIFEST_MAX_BYTES: usize = 256 * 1024;
pub const UPDATE_KEYSET_MAX_BYTES: usize = 64 * 1024;
pub const UPDATE_MAX_FILES: usize = 4096;
pub const UPDATE_MAX_KEYS: usize = 64;
pub const UPDATE_MAX_PACKAGE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const UPDATE_MAX_ARCHIVE_ENTRIES: usize = UPDATE_MAX_FILES * 2;
pub const UPDATE_MAX_EXTRACTED_BYTES: u64 = UPDATE_MAX_PACKAGE_BYTES;
const UPDATE_MAX_FIELD_BYTES: usize = 512;
const UPDATE_MAX_STATE_BYTES: usize = 64 * 1024;
const UPDATE_ACTIVATION_DESCRIPTOR_MAX_BYTES: usize = 512 * 1024;
const UPDATE_ACTIVATION_DESCRIPTOR_DIR: &str = ".hivemind-update-activation";
const UPDATE_ACTIVATION_DESCRIPTOR_FILE: &str = "descriptor.json";
const UPDATE_HASH_BYTES: usize = 32;
const UPDATE_SIGNATURE_BYTES: usize = 64;

/// Dedicated update-root key. It is intentionally different from every
/// Worker execution or Nodepool authentication key.
///
/// The corresponding private key is held by the release system and is not
/// present in this repository, package output, or test fixtures.
pub const UPDATE_ROOT_PUBLIC_KEY_BYTES: [u8; 32] = [
    0xbe, 0x69, 0x92, 0xf9, 0x73, 0xe7, 0x25, 0x93, 0x28, 0xa6, 0x44, 0xdd, 0x77, 0x55, 0xd6, 0xb0,
    0x53, 0x37, 0x54, 0x88, 0x32, 0x9a, 0x23, 0xe9, 0x3f, 0xbb, 0x85, 0x3e, 0xd9, 0x7b, 0x53, 0xdd,
];

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("update metadata is invalid: {0}")]
    InvalidMetadata(String),
    #[error("update metadata exceeds its size limit")]
    MetadataTooLarge,
    #[error("update signature is invalid")]
    InvalidSignature,
    #[error("update root key is invalid")]
    InvalidRootKey,
    #[error("update key is unknown, expired, or revoked")]
    UntrustedReleaseKey,
    #[error("update is expired or not yet valid")]
    InvalidTimeWindow,
    #[error("update target does not match this client")]
    TargetMismatch,
    #[error("update URL is not an approved HTTPS endpoint")]
    InvalidPackageUrl,
    #[error("update would downgrade or replay an installed package")]
    Downgrade,
    #[error("update package does not match its signed manifest")]
    PackageMismatch,
    #[error("update archive is invalid: {0}")]
    InvalidArchive(String),
    #[error("update path is unsafe: {0}")]
    UnsafePath(String),
    #[error("update path uses a symlink or reparse point: {0}")]
    ReparsePoint(String),
    #[error("update state is corrupt: {0}")]
    CorruptState(String),
    #[error("no installed update state exists")]
    MissingState,
    #[error("the requested release is already installed")]
    ReleaseAlreadyExists,
    #[error("HTTP update download failed: {0}")]
    Download(String),
    #[error("update filesystem operation failed: {0}")]
    Io(String),
    #[error("signed update activation is unavailable: {0}")]
    ActivationUnavailable(String),
    #[error("signed update activation failed: {0}")]
    ActivationFailed(String),
    #[error("signed update activation is supported only on native Windows")]
    UnsupportedPlatform,
}

impl From<io::Error> for UpdateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdatePolicy {
    pub product: String,
    pub channel: String,
    pub platform: String,
    pub architecture: String,
    pub allowed_hosts: BTreeSet<String>,
    pub max_package_bytes: u64,
}

impl UpdatePolicy {
    #[must_use]
    pub fn new(
        product: impl Into<String>,
        channel: impl Into<String>,
        platform: impl Into<String>,
        architecture: impl Into<String>,
        allowed_hosts: &[&str],
    ) -> Self {
        Self {
            product: product.into(),
            channel: channel.into(),
            platform: platform.into(),
            architecture: architecture.into(),
            allowed_hosts: allowed_hosts
                .iter()
                .map(|host| host.trim().to_ascii_lowercase())
                .filter(|host| !host.is_empty())
                .collect(),
            max_package_bytes: UPDATE_MAX_PACKAGE_BYTES,
        }
    }

    #[must_use]
    pub fn windows_worker(allowed_hosts: &[&str]) -> Self {
        Self::new(
            "hivemind-windows-worker",
            "stable",
            "windows",
            current_windows_architecture(),
            allowed_hosts,
        )
    }

    #[must_use]
    pub fn windows_master(allowed_hosts: &[&str]) -> Self {
        Self::new(
            "hivemind-windows-master",
            "stable",
            "windows",
            current_windows_architecture(),
            allowed_hosts,
        )
    }

    #[must_use]
    pub fn with_max_package_bytes(mut self, max_package_bytes: u64) -> Self {
        self.max_package_bytes = max_package_bytes;
        self
    }
}

fn current_windows_architecture() -> String {
    if cfg!(target_arch = "aarch64") {
        "aarch64".into()
    } else {
        "x86_64".into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub product: String,
    pub channel: String,
    pub platform: String,
    pub architecture: String,
    pub version: String,
    pub sequence: u64,
    pub minimum_supported_version: String,
    pub release_key_id: String,
    pub issued_at_unix: u64,
    pub expires_at_unix: u64,
    pub package_url: String,
    pub package_size: u64,
    pub package_sha256: String,
    pub files: Vec<ReleaseFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignedReleaseManifest {
    pub manifest: ReleaseManifest,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseKey {
    pub key_id: String,
    pub public_key_hex: String,
    pub not_before_unix: u64,
    pub not_after_unix: u64,
    pub revoked_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseKeyset {
    pub schema_version: u32,
    pub product: String,
    pub channel: String,
    pub expires_at_unix: u64,
    pub keys: BTreeMap<String, ReleaseKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignedReleaseKeyset {
    pub keyset: ReleaseKeyset,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct VerifiedReleaseKeyset {
    keyset: ReleaseKeyset,
}

impl VerifiedReleaseKeyset {
    #[must_use]
    pub fn keyset(&self) -> &ReleaseKeyset {
        &self.keyset
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedReleaseManifest {
    manifest: ReleaseManifest,
    manifest_sha256: String,
}

impl VerifiedReleaseManifest {
    #[must_use]
    pub fn manifest(&self) -> &ReleaseManifest {
        &self.manifest
    }

    #[must_use]
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstalledRelease {
    pub release_dir: String,
    pub version: String,
    pub sequence: u64,
    pub package_sha256: String,
    pub manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstalledPackageState {
    pub schema_version: u32,
    pub product: String,
    pub current: InstalledRelease,
    pub last_known_good: Option<InstalledRelease>,
}

/// A one-shot, non-secret handoff from the running client to an updater
/// process copied from that same verified client image.
///
/// The signed keyset and manifest are carried in the descriptor so the updater
/// can re-verify the release after the client exits. Filesystem locations are
/// independently constrained by the updater; they are never treated as signed
/// authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdateActivationDescriptor {
    pub schema_version: u32,
    pub policy: UpdatePolicy,
    pub signed_keyset: SignedReleaseKeyset,
    pub signed_manifest: SignedReleaseManifest,
    pub install_root: String,
    pub release_dir: String,
    pub active_root: String,
    pub service_executable_name: String,
    pub service_arguments: Vec<String>,
    pub helper_executable: String,
    pub helper_sha256: String,
    pub parent_pid: u32,
}

#[derive(Debug, Clone)]
pub struct UpdateVerifier {
    root_key: VerifyingKey,
}

impl UpdateVerifier {
    /// Construct the verifier from the immutable key compiled into the
    /// client. There is no environment or package override for this key.
    pub fn embedded() -> Result<Self, UpdateError> {
        VerifyingKey::from_bytes(&UPDATE_ROOT_PUBLIC_KEY_BYTES)
            .map(|root_key| Self { root_key })
            .map_err(|_| UpdateError::InvalidRootKey)
    }

    /// Verify bytes signed directly by the immutable update root.
    ///
    /// This method is intentionally limited to the release trust domain. It is
    /// not an execution or Nodepool authentication key, and callers must still
    /// validate the signed document's schema and policy before using it.
    pub fn verify_root_signature(
        &self,
        message: &[u8],
        encoded_signature: &str,
    ) -> Result<(), UpdateError> {
        verify_hex_signature(&self.root_key, message, encoded_signature)
    }

    pub fn verify_keyset(
        &self,
        signed_keyset: &SignedReleaseKeyset,
        policy: &UpdatePolicy,
        now_unix: u64,
    ) -> Result<VerifiedReleaseKeyset, UpdateError> {
        let keyset = &signed_keyset.keyset;
        if keyset.schema_version != UPDATE_PROTOCOL_VERSION {
            return Err(UpdateError::InvalidMetadata(
                "unsupported release keyset schema".into(),
            ));
        }
        validate_bounded_field(&keyset.product, "keyset product")?;
        validate_bounded_field(&keyset.channel, "keyset channel")?;
        if keyset.product != policy.product || keyset.channel != policy.channel {
            return Err(UpdateError::TargetMismatch);
        }
        if keyset.expires_at_unix <= now_unix || keyset.keys.is_empty() {
            return Err(UpdateError::InvalidTimeWindow);
        }
        if keyset.keys.len() > UPDATE_MAX_KEYS {
            return Err(UpdateError::InvalidMetadata(
                "release keyset contains too many keys".into(),
            ));
        }

        let canonical = canonical_keyset_bytes(keyset)?;
        verify_hex_signature(&self.root_key, &canonical, &signed_keyset.signature)?;

        for (map_id, key) in &keyset.keys {
            validate_bounded_field(map_id, "release key id")?;
            validate_bounded_field(&key.key_id, "release key id")?;
            if map_id != &key.key_id
                || key.not_after_unix <= key.not_before_unix
                || key.not_after_unix <= now_unix
            {
                return Err(UpdateError::UntrustedReleaseKey);
            }
            let _ = decode_fixed_hex::<UPDATE_HASH_BYTES>(&key.public_key_hex, "release key")?;
            if key
                .revoked_at_unix
                .is_some_and(|revoked| revoked <= now_unix)
            {
                return Err(UpdateError::UntrustedReleaseKey);
            }
        }

        Ok(VerifiedReleaseKeyset {
            keyset: keyset.clone(),
        })
    }

    pub fn verify_manifest(
        &self,
        signed_manifest: &SignedReleaseManifest,
        keyset: &VerifiedReleaseKeyset,
        policy: &UpdatePolicy,
        current: Option<&InstalledPackageState>,
        now_unix: u64,
    ) -> Result<VerifiedReleaseManifest, UpdateError> {
        let manifest = &signed_manifest.manifest;
        validate_manifest_shape(manifest, policy, now_unix)?;
        if keyset.keyset.expires_at_unix <= now_unix {
            return Err(UpdateError::InvalidTimeWindow);
        }
        if keyset.keyset.product != policy.product || keyset.keyset.channel != policy.channel {
            return Err(UpdateError::TargetMismatch);
        }

        if let Some(current) = current {
            validate_state(current)?;
            if current.product != policy.product || manifest.sequence <= current.current.sequence {
                return Err(UpdateError::Downgrade);
            }
        }

        let release_key = keyset
            .keyset
            .keys
            .get(&manifest.release_key_id)
            .ok_or(UpdateError::UntrustedReleaseKey)?;
        if release_key
            .revoked_at_unix
            .is_some_and(|revoked| revoked <= now_unix)
            || now_unix < release_key.not_before_unix
            || now_unix >= release_key.not_after_unix
        {
            return Err(UpdateError::UntrustedReleaseKey);
        }
        let public_key =
            decode_fixed_hex::<UPDATE_HASH_BYTES>(&release_key.public_key_hex, "release key")?;
        let release_key =
            VerifyingKey::from_bytes(&public_key).map_err(|_| UpdateError::UntrustedReleaseKey)?;
        let canonical = canonical_manifest_bytes(manifest)?;
        verify_hex_signature(&release_key, &canonical, &signed_manifest.signature)?;

        Ok(VerifiedReleaseManifest {
            manifest: manifest.clone(),
            manifest_sha256: signed_manifest_digest(signed_manifest)?,
        })
    }
}

pub fn parse_signed_keyset(bytes: &[u8]) -> Result<SignedReleaseKeyset, UpdateError> {
    parse_bounded_json(bytes, UPDATE_KEYSET_MAX_BYTES, "release keyset")
}

pub fn parse_signed_manifest(bytes: &[u8]) -> Result<SignedReleaseManifest, UpdateError> {
    parse_bounded_json(bytes, UPDATE_MANIFEST_MAX_BYTES, "release manifest")
}

pub fn canonical_keyset_bytes(keyset: &ReleaseKeyset) -> Result<Vec<u8>, UpdateError> {
    serde_json::to_vec(keyset).map_err(|error| UpdateError::InvalidMetadata(error.to_string()))
}

pub fn canonical_manifest_bytes(manifest: &ReleaseManifest) -> Result<Vec<u8>, UpdateError> {
    let mut canonical = manifest.clone();
    validate_file_list(&canonical.files)?;
    canonical
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    serde_json::to_vec(&canonical).map_err(|error| UpdateError::InvalidMetadata(error.to_string()))
}

fn canonical_signed_manifest_bytes(
    signed_manifest: &SignedReleaseManifest,
) -> Result<Vec<u8>, UpdateError> {
    #[derive(Serialize)]
    struct CanonicalSignedManifest {
        manifest: ReleaseManifest,
        signature: String,
    }

    let mut manifest = signed_manifest.manifest.clone();
    validate_file_list(&manifest.files)?;
    manifest
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    serde_json::to_vec(&CanonicalSignedManifest {
        manifest,
        signature: signed_manifest.signature.clone(),
    })
    .map_err(|error| UpdateError::InvalidMetadata(error.to_string()))
}

fn signed_manifest_digest(signed_manifest: &SignedReleaseManifest) -> Result<String, UpdateError> {
    let bytes = canonical_signed_manifest_bytes(signed_manifest)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn verify_hex_signature(
    key: &VerifyingKey,
    message: &[u8],
    encoded_signature: &str,
) -> Result<(), UpdateError> {
    let bytes = decode_fixed_hex::<UPDATE_SIGNATURE_BYTES>(encoded_signature, "signature")?;
    let signature = Signature::from_bytes(&bytes);
    key.verify(message, &signature)
        .map_err(|_| UpdateError::InvalidSignature)
}

fn decode_fixed_hex<const N: usize>(value: &str, field: &str) -> Result<[u8; N], UpdateError> {
    if value.len() != N * 2
        || !value.is_ascii()
        || value
            .chars()
            .any(|character| !character.is_ascii_hexdigit())
    {
        return Err(UpdateError::InvalidMetadata(format!(
            "{field} must be exactly {} hexadecimal bytes",
            N
        )));
    }
    let decoded = hex::decode(value)
        .map_err(|_| UpdateError::InvalidMetadata(format!("{field} is not hexadecimal")))?;
    let mut output = [0u8; N];
    output.copy_from_slice(&decoded);
    Ok(output)
}

fn parse_bounded_json<T: DeserializeOwned>(
    bytes: &[u8],
    max_bytes: usize,
    name: &str,
) -> Result<T, UpdateError> {
    if bytes.len() > max_bytes {
        return Err(UpdateError::MetadataTooLarge);
    }
    serde_json::from_slice(bytes)
        .map_err(|error| UpdateError::InvalidMetadata(format!("{name}: {error}")))
}

fn validate_manifest_shape(
    manifest: &ReleaseManifest,
    policy: &UpdatePolicy,
    now_unix: u64,
) -> Result<(), UpdateError> {
    if manifest.schema_version != UPDATE_PROTOCOL_VERSION {
        return Err(UpdateError::InvalidMetadata(
            "unsupported release manifest schema".into(),
        ));
    }
    for (value, name) in [
        (&manifest.product, "product"),
        (&manifest.channel, "channel"),
        (&manifest.platform, "platform"),
        (&manifest.architecture, "architecture"),
        (&manifest.version, "version"),
        (
            &manifest.minimum_supported_version,
            "minimum supported version",
        ),
        (&manifest.release_key_id, "release key id"),
        (&manifest.package_url, "package URL"),
        (&manifest.package_sha256, "package hash"),
    ] {
        validate_bounded_field(value, name)?;
    }
    if manifest.product != policy.product
        || manifest.channel != policy.channel
        || manifest.platform != policy.platform
        || manifest.architecture != policy.architecture
    {
        return Err(UpdateError::TargetMismatch);
    }
    if manifest.sequence == 0
        || manifest.issued_at_unix > now_unix
        || manifest.expires_at_unix <= now_unix
        || manifest.expires_at_unix <= manifest.issued_at_unix
    {
        return Err(UpdateError::InvalidTimeWindow);
    }
    let version = parse_version(&manifest.version)?;
    let minimum = parse_version(&manifest.minimum_supported_version)?;
    if minimum > version {
        return Err(UpdateError::InvalidMetadata(
            "minimum supported version is newer than the release".into(),
        ));
    }
    if manifest.package_size == 0
        || manifest.package_size > policy.max_package_bytes
        || policy.max_package_bytes == 0
    {
        return Err(UpdateError::InvalidMetadata(
            "package size is outside the configured bound".into(),
        ));
    }
    let _ = decode_fixed_hex::<UPDATE_HASH_BYTES>(&manifest.package_sha256, "package hash")?;
    validate_https_package_url(&manifest.package_url, &policy.allowed_hosts)?;
    validate_file_list(&manifest.files)
}

fn validate_file_list(files: &[ReleaseFile]) -> Result<(), UpdateError> {
    if files.is_empty() || files.len() > UPDATE_MAX_FILES {
        return Err(UpdateError::InvalidMetadata(
            "release file list is empty or too large".into(),
        ));
    }
    let mut paths = BTreeSet::new();
    let mut case_insensitive_paths = BTreeSet::new();
    for file in files {
        validate_package_path(&file.path)?;
        validate_bounded_field(&file.sha256, "file hash")?;
        let _ = decode_fixed_hex::<UPDATE_HASH_BYTES>(&file.sha256, "file hash")?;
        if file.size == 0
            || !paths.insert(file.path.clone())
            || !case_insensitive_paths.insert(file.path.to_ascii_lowercase())
        {
            return Err(UpdateError::InvalidMetadata(
                "release file list contains an empty or duplicate file".into(),
            ));
        }
    }
    Ok(())
}

fn validate_bounded_field(value: &str, field: &str) -> Result<(), UpdateError> {
    if value.is_empty()
        || value.len() > UPDATE_MAX_FIELD_BYTES
        || !value.is_ascii()
        || value.chars().any(|character| character.is_control())
    {
        return Err(UpdateError::InvalidMetadata(format!(
            "{field} is empty, non-ASCII, or too large"
        )));
    }
    Ok(())
}

fn parse_version(value: &str) -> Result<[u64; 3], UpdateError> {
    let mut parts = value.split('.');
    let mut parsed = [0u64; 3];
    for slot in &mut parsed {
        let part = parts.next().ok_or_else(|| {
            UpdateError::InvalidMetadata("version must contain major.minor.patch".into())
        })?;
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return Err(UpdateError::InvalidMetadata(
                "version contains a non-canonical numeric component".into(),
            ));
        }
        *slot = part.parse().map_err(|_| {
            UpdateError::InvalidMetadata("version contains a non-numeric component".into())
        })?;
    }
    if parts.next().is_some() {
        return Err(UpdateError::InvalidMetadata(
            "version must contain exactly three components".into(),
        ));
    }
    Ok(parsed)
}

pub(crate) fn validate_https_package_url(
    value: &str,
    allowed_hosts: &BTreeSet<String>,
) -> Result<(), UpdateError> {
    let rest = value
        .strip_prefix("https://")
        .ok_or(UpdateError::InvalidPackageUrl)?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty()
        || authority.contains('@')
        || authority
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(UpdateError::InvalidPackageUrl);
    }
    let host = if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest.split_once(']').ok_or(UpdateError::InvalidPackageUrl)?;
        if let Some(port) = port.strip_prefix(':') {
            if port != "443" {
                return Err(UpdateError::InvalidPackageUrl);
            }
        } else if !port.is_empty() {
            return Err(UpdateError::InvalidPackageUrl);
        }
        host.to_ascii_lowercase()
    } else {
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() => (host, Some(port)),
            _ => (authority, None),
        };
        if let Some(port) = port {
            if port != "443" || host.contains(':') {
                return Err(UpdateError::InvalidPackageUrl);
            }
        }
        if host.is_empty() || host.contains(':') {
            return Err(UpdateError::InvalidPackageUrl);
        }
        host.to_ascii_lowercase()
    };
    if host.is_empty()
        || host.contains('\\')
        || host.contains('%')
        || !allowed_hosts.contains(&host)
    {
        return Err(UpdateError::InvalidPackageUrl);
    }
    Ok(())
}

fn validate_package_path(value: &str) -> Result<(), UpdateError> {
    if value.is_empty()
        || value.len() > UPDATE_MAX_FIELD_BYTES
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
        || !value.is_ascii()
        || value.chars().any(|character| character.is_control())
    {
        return Err(UpdateError::UnsafePath(value.to_string()));
    }
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(UpdateError::UnsafePath(value.to_string()));
        }
    }
    Ok(())
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, UpdateError> {
    validate_package_path(relative)?;
    let mut output = root.to_path_buf();
    for component in relative.split('/') {
        output.push(component);
    }
    Ok(output)
}

fn validate_state(state: &InstalledPackageState) -> Result<(), UpdateError> {
    if state.schema_version != UPDATE_PROTOCOL_VERSION {
        return Err(UpdateError::CorruptState(
            "unsupported installed state schema".into(),
        ));
    }
    validate_bounded_field(&state.product, "state product")?;
    validate_installed_release(&state.current)?;
    if let Some(last_known_good) = &state.last_known_good {
        validate_installed_release(last_known_good)?;
    }
    Ok(())
}

fn validate_installed_release(release: &InstalledRelease) -> Result<(), UpdateError> {
    validate_package_path(&release.release_dir)?;
    let _ = parse_version(&release.version)?;
    if release.sequence == 0 {
        return Err(UpdateError::CorruptState(
            "installed release sequence must be positive".into(),
        ));
    }
    let _ = decode_fixed_hex::<UPDATE_HASH_BYTES>(&release.package_sha256, "state package hash")?;
    let _ = decode_fixed_hex::<UPDATE_HASH_BYTES>(&release.manifest_sha256, "state manifest hash")?;
    Ok(())
}

pub fn verify_installed_package(
    root: &Path,
    verified: &VerifiedReleaseManifest,
) -> Result<(), UpdateError> {
    ensure_safe_directory(root)?;
    let expected: BTreeMap<String, &ReleaseFile> = verified
        .manifest
        .files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect();
    let mut actual = BTreeMap::new();
    collect_package_files(root, "", &mut actual)?;
    if actual.len() != expected.len() || actual.keys().any(|path| !expected.contains_key(path)) {
        return Err(UpdateError::PackageMismatch);
    }
    for (path, file) in expected {
        let (size, digest) = actual.get(&path).ok_or(UpdateError::PackageMismatch)?;
        if *size != file.size || digest != &file.sha256 {
            return Err(UpdateError::PackageMismatch);
        }
    }
    Ok(())
}

/// Verify all signed package files in an active client directory while allowing
/// operator-owned runtime files such as `.env.worker` and `sandbox` to remain.
/// Unknown files are never copied or executed by the activation transaction.
pub fn verify_active_directory(
    root: &Path,
    verified: &VerifiedReleaseManifest,
) -> Result<(), UpdateError> {
    ensure_safe_directory(root)?;
    for file in &verified.manifest.files {
        let path = safe_join(root, &file.path)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(UpdateError::PackageMismatch)
            }
            Err(error) => return Err(UpdateError::ActivationFailed(error.to_string())),
        };
        ensure_metadata_is_not_reparse(&path, &metadata)?;
        if !metadata.is_file() {
            return Err(UpdateError::PackageMismatch);
        }
        let (size, digest) = hash_file(&path)?;
        if size != file.size || digest != file.sha256 {
            return Err(UpdateError::PackageMismatch);
        }
    }
    Ok(())
}

fn collect_package_files(
    root: &Path,
    relative: &str,
    files: &mut BTreeMap<String, (u64, String)>,
) -> Result<(), UpdateError> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        validate_package_path(&path)?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        ensure_metadata_is_not_reparse(&child, &metadata)?;
        if metadata.is_dir() {
            collect_package_files(&child, &path, files)?;
        } else if metadata.is_file() {
            let (size, digest) = hash_file(&child)?;
            if files.insert(path, (size, digest)).is_some() {
                return Err(UpdateError::PackageMismatch);
            }
        } else {
            return Err(UpdateError::PackageMismatch);
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(u64, String), UpdateError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or(UpdateError::PackageMismatch)?;
        if size > UPDATE_MAX_PACKAGE_BYTES {
            return Err(UpdateError::PackageMismatch);
        }
        hasher.update(&buffer[..read]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

fn ensure_safe_directory(path: &Path) -> Result<(), UpdateError> {
    if !path.is_absolute() {
        return Err(UpdateError::UnsafePath(path.display().to_string()));
    }
    ensure_existing_directory_chain(path)?;
    if matches!(
        fs::symlink_metadata(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound
    ) {
        fs::create_dir_all(path)?;
    }
    ensure_existing_directory_chain(path)
}

fn ensure_existing_directory_chain(path: &Path) -> Result<(), UpdateError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                ensure_metadata_is_not_reparse(ancestor, &metadata)?;
                if !metadata.is_dir() {
                    return Err(UpdateError::UnsafePath(ancestor.display().to_string()));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(UpdateError::from(error)),
        }
    }
    Ok(())
}

fn ensure_metadata_is_not_reparse(path: &Path, metadata: &fs::Metadata) -> Result<(), UpdateError> {
    if metadata.file_type().is_symlink() {
        return Err(UpdateError::ReparsePoint(path.display().to_string()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(UpdateError::ReparsePoint(path.display().to_string()));
        }
    }
    Ok(())
}

/// Fetch bounded signed metadata from an approved HTTPS host.
///
/// This helper is deliberately separate from package download: metadata is
/// parsed and signature-checked by the caller before it can authorize any
/// package bytes. Redirects, non-HTTPS URLs, unapproved hosts, oversized
/// responses, and non-200 responses are rejected without a fallback source.
pub async fn download_signed_metadata(
    url: &str,
    max_bytes: usize,
    allowed_hosts: &BTreeSet<String>,
) -> Result<Vec<u8>, UpdateError> {
    if max_bytes == 0 {
        return Err(UpdateError::MetadataTooLarge);
    }
    validate_https_package_url(url, allowed_hosts)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| UpdateError::Download(error.to_string()))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| UpdateError::Download(error.to_string()))?;
    if response.status().is_redirection() || response.status() != StatusCode::OK {
        return Err(UpdateError::Download(
            "metadata endpoint returned an unexpected response".into(),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(UpdateError::MetadataTooLarge);
    }

    let mut body = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(max_bytes as u64) as usize,
    );
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| UpdateError::Download(error.to_string()))?
    {
        let next_len = body
            .len()
            .checked_add(chunk.len())
            .ok_or(UpdateError::MetadataTooLarge)?;
        if next_len > max_bytes {
            return Err(UpdateError::MetadataTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return Err(UpdateError::InvalidMetadata(
            "metadata response is empty".into(),
        ));
    }
    Ok(body)
}

pub async fn download_verified_package(
    verified: &VerifiedReleaseManifest,
    policy: &UpdatePolicy,
    staging_dir: &Path,
) -> Result<PathBuf, UpdateError> {
    validate_https_package_url(&verified.manifest.package_url, &policy.allowed_hosts)?;
    ensure_safe_directory(staging_dir)?;
    let final_path = staging_dir.join(format!("package-{}.bin", verified.manifest.package_sha256));
    match fs::symlink_metadata(&final_path) {
        Ok(metadata) => {
            ensure_metadata_is_not_reparse(&final_path, &metadata)?;
            if !metadata.is_file() {
                return Err(UpdateError::PackageMismatch);
            }
            let (size, digest) = hash_file(&final_path)?;
            if size == verified.manifest.package_size && digest == verified.manifest.package_sha256
            {
                return Ok(final_path);
            }
            return Err(UpdateError::PackageMismatch);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(UpdateError::from(error)),
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|error| UpdateError::Download(error.to_string()))?;
    let response = client
        .get(&verified.manifest.package_url)
        .send()
        .await
        .map_err(|error| UpdateError::Download(error.to_string()))?;
    if response.status().is_redirection()
        || response.status() != StatusCode::OK
        || response.content_length() != Some(verified.manifest.package_size)
    {
        return Err(UpdateError::Download(
            "package endpoint returned an unexpected response".into(),
        ));
    }

    let temporary_path = staging_dir.join(format!(
        ".package-{}-{}-{}.part",
        verified.manifest.package_sha256,
        std::process::id(),
        monotonic_nonce()
    ));
    let result = async {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let mut response = response;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| UpdateError::Download(error.to_string()))?
        {
            size = size
                .checked_add(chunk.len() as u64)
                .ok_or(UpdateError::PackageMismatch)?;
            if size > verified.manifest.package_size || size > policy.max_package_bytes {
                return Err(UpdateError::PackageMismatch);
            }
            hasher.update(&chunk);
            file.write_all(&chunk)?;
        }
        file.flush()?;
        file.sync_all()?;
        let digest = hex::encode(hasher.finalize());
        if size != verified.manifest.package_size || digest != verified.manifest.package_sha256 {
            return Err(UpdateError::PackageMismatch);
        }
        atomic_replace(&temporary_path, &final_path)?;
        Ok::<(), UpdateError>(())
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result.map(|()| final_path)
}

/// Extract a verified archive into a newly created operator-owned directory.
///
/// Archive metadata is treated as hostile even after the outer package hash has
/// been checked. Paths, entry types, entry counts, and expanded byte counts are
/// bounded before the extracted tree is re-verified against the signed file
/// manifest. The destination must not already exist; this prevents extraction
/// from replacing the current verified package or an unrelated operator path.
pub fn extract_verified_zip(
    archive_path: &Path,
    destination: &Path,
    verified: &VerifiedReleaseManifest,
    policy: &UpdatePolicy,
) -> Result<(), UpdateError> {
    if !archive_path.is_absolute() {
        return Err(UpdateError::UnsafePath(archive_path.display().to_string()));
    }
    let archive_metadata =
        fs::symlink_metadata(archive_path).map_err(|error| UpdateError::Io(error.to_string()))?;
    ensure_metadata_is_not_reparse(archive_path, &archive_metadata)?;
    if !archive_metadata.is_file() {
        return Err(UpdateError::UnsafePath(archive_path.display().to_string()));
    }
    let (archive_size, archive_digest) = hash_file(archive_path)?;
    if archive_size != verified.manifest.package_size
        || archive_size > policy.max_package_bytes
        || archive_digest != verified.manifest.package_sha256
    {
        return Err(UpdateError::PackageMismatch);
    }

    let parent = destination
        .parent()
        .ok_or_else(|| UpdateError::UnsafePath(destination.display().to_string()))?;
    ensure_safe_directory(parent)?;
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            ensure_metadata_is_not_reparse(destination, &metadata)?;
            return Err(UpdateError::UnsafePath(destination.display().to_string()));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(UpdateError::from(error)),
    }
    fs::create_dir(destination)?;
    let destination_metadata = fs::symlink_metadata(destination)?;
    ensure_metadata_is_not_reparse(destination, &destination_metadata)?;

    let result = extract_archive_to_directory(archive_path, destination, verified, policy);
    if let Err(error) = result {
        return match fs::remove_dir_all(destination) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(UpdateError::Io(format!(
                "archive extraction failed: {error}; extraction cleanup failed: {cleanup_error}"
            ))),
        };
    }
    Ok(())
}

fn archive_entry_path(name: &str, is_directory: bool) -> Result<&str, UpdateError> {
    if is_directory {
        let relative = name
            .strip_suffix('/')
            .ok_or_else(|| UpdateError::UnsafePath(name.to_owned()))?;
        validate_package_path(relative)?;
        Ok(relative)
    } else {
        if name.ends_with('/') {
            return Err(UpdateError::UnsafePath(name.to_owned()));
        }
        validate_package_path(name)?;
        Ok(name)
    }
}

fn manifest_contains_directory(expected_files: &BTreeSet<&str>, directory: &str) -> bool {
    let prefix = format!("{directory}/");
    expected_files
        .iter()
        .any(|path| path.starts_with(prefix.as_str()))
}

#[derive(Debug, Clone, Copy)]
enum ArchiveEntryKind {
    Directory { explicit: bool },
    File,
}

fn register_archive_path(
    paths: &mut BTreeMap<String, ArchiveEntryKind>,
    relative: &str,
    is_directory: bool,
) -> Result<(), UpdateError> {
    let normalized = relative.to_ascii_lowercase();
    let components: Vec<&str> = normalized.split('/').collect();
    let mut prefix = String::new();
    for component in &components[..components.len() - 1] {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(component);
        if matches!(paths.get(&prefix), Some(ArchiveEntryKind::File)) {
            return Err(UpdateError::InvalidArchive(
                "archive contains a file/directory collision".into(),
            ));
        }
        paths
            .entry(prefix.clone())
            .or_insert(ArchiveEntryKind::Directory { explicit: false });
    }

    match paths.get(&normalized).copied() {
        Some(ArchiveEntryKind::File) => {
            return Err(UpdateError::InvalidArchive(
                "archive contains a file/directory collision".into(),
            ));
        }
        Some(ArchiveEntryKind::Directory { explicit: true }) => {
            return Err(UpdateError::InvalidArchive(
                "archive contains duplicate entries".into(),
            ));
        }
        Some(ArchiveEntryKind::Directory { explicit: false }) if !is_directory => {
            return Err(UpdateError::InvalidArchive(
                "archive contains a file/directory collision".into(),
            ));
        }
        Some(ArchiveEntryKind::Directory { explicit: false }) => {
            paths.insert(normalized, ArchiveEntryKind::Directory { explicit: true });
        }
        None if is_directory => {
            paths.insert(normalized, ArchiveEntryKind::Directory { explicit: true });
        }
        None => {
            if paths
                .keys()
                .any(|path| path.starts_with(&format!("{normalized}/")))
            {
                return Err(UpdateError::InvalidArchive(
                    "archive contains a file/directory collision".into(),
                ));
            }
            paths.insert(normalized, ArchiveEntryKind::File);
        }
    }
    Ok(())
}

fn validate_archive_entry_count_bound(archive_path: &Path) -> Result<(), UpdateError> {
    const EOCD_BYTES: usize = 22;
    const ZIP64_LOCATOR_BYTES: usize = 20;
    const ZIP64_EOCD_BYTES: usize = 56;
    let mut file = File::open(archive_path)?;
    let file_size = file.metadata()?.len();
    let tail_size = file_size.min((EOCD_BYTES + u16::MAX as usize) as u64) as usize;
    if tail_size < EOCD_BYTES {
        return Err(UpdateError::InvalidArchive(
            "archive is shorter than its end record".into(),
        ));
    }
    file.seek(SeekFrom::Start(file_size - tail_size as u64))?;
    let mut tail = vec![0u8; tail_size];
    file.read_exact(&mut tail)?;

    let eocd_index = (0..=tail.len() - EOCD_BYTES)
        .rev()
        .find(|index| {
            if &tail[*index..*index + 4] != b"PK\x05\x06" {
                return false;
            }
            let comment_length =
                u16::from_le_bytes([tail[*index + 20], tail[*index + 21]]) as usize;
            index
                .checked_add(EOCD_BYTES)
                .and_then(|end| end.checked_add(comment_length))
                == Some(tail.len())
        })
        .ok_or_else(|| UpdateError::InvalidArchive("archive end record is missing".into()))?;
    let eocd_offset = file_size - tail_size as u64 + eocd_index as u64;
    let disk_number = u16::from_le_bytes([tail[eocd_index + 4], tail[eocd_index + 5]]);
    let central_disk = u16::from_le_bytes([tail[eocd_index + 6], tail[eocd_index + 7]]);
    let entries_on_disk = u16::from_le_bytes([tail[eocd_index + 8], tail[eocd_index + 9]]);
    let entries_total = u16::from_le_bytes([tail[eocd_index + 10], tail[eocd_index + 11]]);
    if disk_number != 0 || central_disk != 0 {
        return Err(UpdateError::InvalidArchive(
            "archive uses unsupported disk layout".into(),
        ));
    }
    let central_size = u32::from_le_bytes([
        tail[eocd_index + 12],
        tail[eocd_index + 13],
        tail[eocd_index + 14],
        tail[eocd_index + 15],
    ]);
    let central_offset = u32::from_le_bytes([
        tail[eocd_index + 16],
        tail[eocd_index + 17],
        tail[eocd_index + 18],
        tail[eocd_index + 19],
    ]);

    let entry_count = if entries_on_disk == u16::MAX
        || entries_total == u16::MAX
        || central_size == u32::MAX
        || central_offset == u32::MAX
    {
        let locator_offset = eocd_offset
            .checked_sub(ZIP64_LOCATOR_BYTES as u64)
            .ok_or_else(|| UpdateError::InvalidArchive("ZIP64 locator is missing".into()))?;
        file.seek(SeekFrom::Start(locator_offset))?;
        let mut locator = [0u8; ZIP64_LOCATOR_BYTES];
        file.read_exact(&mut locator)?;
        if &locator[..4] != b"PK\x06\x07"
            || u32::from_le_bytes(locator[4..8].try_into().unwrap()) != 0
            || u32::from_le_bytes(locator[16..20].try_into().unwrap()) != 1
        {
            return Err(UpdateError::InvalidArchive(
                "ZIP64 archive uses unsupported disk layout".into(),
            ));
        }
        let zip64_offset = u64::from_le_bytes(locator[8..16].try_into().unwrap());
        let zip64_end = zip64_offset
            .checked_add(ZIP64_EOCD_BYTES as u64)
            .ok_or_else(|| UpdateError::InvalidArchive("ZIP64 end record overflows".into()))?;
        if zip64_end > file_size {
            return Err(UpdateError::InvalidArchive(
                "ZIP64 end record is outside the archive".into(),
            ));
        }
        file.seek(SeekFrom::Start(zip64_offset))?;
        let mut zip64 = [0u8; ZIP64_EOCD_BYTES];
        file.read_exact(&mut zip64)?;
        if &zip64[..4] != b"PK\x06\x06"
            || u64::from_le_bytes(zip64[4..12].try_into().unwrap()) < 44
            || u32::from_le_bytes(zip64[16..20].try_into().unwrap()) != 0
            || u32::from_le_bytes(zip64[20..24].try_into().unwrap()) != 0
            || u64::from_le_bytes(zip64[32..40].try_into().unwrap())
                != u64::from_le_bytes(zip64[24..32].try_into().unwrap())
        {
            return Err(UpdateError::InvalidArchive(
                "ZIP64 end record is malformed".into(),
            ));
        }
        let zip64_size = u64::from_le_bytes(zip64[4..12].try_into().unwrap());
        let zip64_record_end = 12u64
            .checked_add(zip64_size)
            .and_then(|record_size| zip64_offset.checked_add(record_size));
        if zip64_record_end.is_none_or(|end| end > file_size) {
            return Err(UpdateError::InvalidArchive(
                "ZIP64 end record exceeds the archive".into(),
            ));
        }
        u64::from_le_bytes(zip64[32..40].try_into().unwrap())
    } else if entries_on_disk != entries_total {
        return Err(UpdateError::InvalidArchive(
            "archive uses unsupported disk layout".into(),
        ));
    } else {
        entries_total as u64
    };

    if entry_count > UPDATE_MAX_ARCHIVE_ENTRIES as u64 {
        return Err(UpdateError::InvalidArchive(
            "archive contains too many entries".into(),
        ));
    }
    Ok(())
}

fn scan_archive_directory(archive_path: &Path) -> Result<(usize, BTreeSet<String>), UpdateError> {
    validate_archive_entry_count_bound(archive_path)?;
    let archive = ZipArchive::new(File::open(archive_path)?)
        .map_err(|error| UpdateError::InvalidArchive(error.to_string()))?;
    let central_directory_start = archive.central_directory_start();
    let parsed_entry_count = archive.len();
    drop(archive);

    let mut file = File::open(archive_path)?;
    file.seek(SeekFrom::Start(central_directory_start))?;
    let mut names = BTreeSet::new();
    let mut entry_count = 0usize;
    loop {
        let mut signature = [0u8; 4];
        file.read_exact(&mut signature)
            .map_err(|error| UpdateError::InvalidArchive(error.to_string()))?;
        match &signature {
            b"PK\x01\x02" => {
                let mut fixed = [0u8; 42];
                file.read_exact(&mut fixed)
                    .map_err(|error| UpdateError::InvalidArchive(error.to_string()))?;
                entry_count = entry_count
                    .checked_add(1)
                    .ok_or_else(|| UpdateError::InvalidArchive("entry count overflow".into()))?;
                if entry_count > UPDATE_MAX_ARCHIVE_ENTRIES {
                    return Err(UpdateError::InvalidArchive(
                        "archive contains too many entries".into(),
                    ));
                }
                let version_made_by = u16::from_le_bytes([fixed[0], fixed[1]]);
                let flags = u16::from_le_bytes([fixed[4], fixed[5]]);
                let name_length = u16::from_le_bytes([fixed[24], fixed[25]]) as usize;
                let extra_length = u16::from_le_bytes([fixed[26], fixed[27]]) as usize;
                let comment_length = u16::from_le_bytes([fixed[28], fixed[29]]) as usize;
                if name_length == 0 || name_length > UPDATE_MAX_FIELD_BYTES {
                    return Err(UpdateError::InvalidArchive(
                        "archive entry name is outside the configured bound".into(),
                    ));
                }
                if flags & 1 != 0 {
                    return Err(UpdateError::InvalidArchive(
                        "encrypted entries are not accepted".into(),
                    ));
                }
                let mut raw_name = vec![0u8; name_length];
                file.read_exact(&mut raw_name)
                    .map_err(|error| UpdateError::InvalidArchive(error.to_string()))?;
                if raw_name.contains(&0) {
                    return Err(UpdateError::InvalidArchive(
                        "archive entry contains a NUL byte".into(),
                    ));
                }
                if !raw_name.is_ascii() && flags & (1 << 11) == 0 {
                    return Err(UpdateError::InvalidArchive(
                        "non-ASCII archive names must use UTF-8".into(),
                    ));
                }
                let name = std::str::from_utf8(&raw_name).map_err(|_| {
                    UpdateError::InvalidArchive("archive entry name is not valid UTF-8".into())
                })?;
                let is_directory = name.ends_with('/');
                let relative = archive_entry_path(name, is_directory)?;
                if !names.insert(name.to_owned()) {
                    return Err(UpdateError::InvalidArchive(
                        "archive contains duplicate entries".into(),
                    ));
                }
                let file_type = if version_made_by >> 8 == 3 {
                    let external_attributes =
                        u32::from_le_bytes([fixed[34], fixed[35], fixed[36], fixed[37]]);
                    Some((external_attributes >> 16) & 0o170000)
                } else {
                    None
                };
                if file_type == Some(0o120000) {
                    return Err(UpdateError::ReparsePoint(relative.to_owned()));
                }
                if is_directory {
                    if file_type.is_some_and(|file_type| file_type != 0 && file_type != 0o040000) {
                        return Err(UpdateError::InvalidArchive(
                            "directory entry has a non-directory Unix mode".into(),
                        ));
                    }
                } else if file_type.is_some_and(|file_type| file_type != 0 && file_type != 0o100000)
                {
                    return Err(UpdateError::InvalidArchive(
                        "archive contains a non-regular file".into(),
                    ));
                }
                let metadata_length =
                    extra_length.checked_add(comment_length).ok_or_else(|| {
                        UpdateError::InvalidArchive("archive metadata overflow".into())
                    })?;
                file.seek(SeekFrom::Current(metadata_length as i64))?;
            }
            b"PK\x05\x06" | b"PK\x06\x06" | b"PK\x06\x07" => break,
            _ => {
                return Err(UpdateError::InvalidArchive(
                    "archive central directory is malformed".into(),
                ));
            }
        }
    }
    if entry_count != parsed_entry_count {
        return Err(UpdateError::InvalidArchive(
            "archive entry table is inconsistent".into(),
        ));
    }
    Ok((entry_count, names))
}

fn extract_archive_to_directory(
    archive_path: &Path,
    destination: &Path,
    verified: &VerifiedReleaseManifest,
    policy: &UpdatePolicy,
) -> Result<(), UpdateError> {
    let (archive_entry_count, _) = scan_archive_directory(archive_path)?;
    let archive_file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(archive_file)
        .map_err(|error| UpdateError::InvalidArchive(error.to_string()))?;
    if archive_entry_count > UPDATE_MAX_ARCHIVE_ENTRIES {
        return Err(UpdateError::InvalidArchive(
            "archive contains too many entries".into(),
        ));
    }
    let expanded_limit = policy.max_package_bytes.min(UPDATE_MAX_EXTRACTED_BYTES);
    if expanded_limit == 0 {
        return Err(UpdateError::InvalidArchive(
            "expanded archive limit is zero".into(),
        ));
    }
    let expected_files: BTreeSet<&str> = verified
        .manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();

    let mut paths = BTreeMap::new();
    let mut extracted_bytes = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| UpdateError::InvalidArchive(error.to_string()))?;
        if entry.encrypted() {
            return Err(UpdateError::InvalidArchive(
                "encrypted entries are not accepted".into(),
            ));
        }
        if entry.name_raw().contains(&0) || entry.name().contains('\0') {
            return Err(UpdateError::InvalidArchive(
                "archive entry contains a NUL byte".into(),
            ));
        }
        if std::str::from_utf8(entry.name_raw()).is_err() {
            return Err(UpdateError::InvalidArchive(
                "archive entry name is not valid UTF-8".into(),
            ));
        }
        if entry.is_symlink() {
            return Err(UpdateError::ReparsePoint(entry.name().to_string()));
        }
        validate_archive_entry_type(&entry)?;

        if entry.is_dir() {
            let relative = archive_entry_path(entry.name(), true)?;
            if !manifest_contains_directory(&expected_files, relative) {
                return Err(UpdateError::PackageMismatch);
            }
            register_archive_path(&mut paths, relative, true)?;
            ensure_safe_directory(&safe_join(destination, relative)?)?;
            continue;
        }

        let relative = archive_entry_path(entry.name(), false)?;
        if !expected_files.contains(relative) {
            return Err(UpdateError::PackageMismatch);
        }
        register_archive_path(&mut paths, relative, false)?;
        let declared_size = entry.size();
        if declared_size == 0 || declared_size > expanded_limit {
            return Err(UpdateError::InvalidArchive(
                "archive entry size is outside the configured bound".into(),
            ));
        }
        extracted_bytes = extracted_bytes
            .checked_add(declared_size)
            .ok_or_else(|| UpdateError::InvalidArchive("archive size overflow".into()))?;
        if extracted_bytes > expanded_limit {
            return Err(UpdateError::InvalidArchive(
                "expanded archive exceeds the configured bound".into(),
            ));
        }

        let output_path = safe_join(destination, relative)?;
        let parent = output_path
            .parent()
            .ok_or_else(|| UpdateError::UnsafePath(output_path.display().to_string()))?;
        ensure_safe_directory(parent)?;
        if let Ok(metadata) = fs::symlink_metadata(&output_path) {
            ensure_metadata_is_not_reparse(&output_path, &metadata)?;
            return Err(UpdateError::InvalidArchive(
                "archive entry would overwrite an existing path".into(),
            ));
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)?;
        let mut written = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            written = written
                .checked_add(read as u64)
                .ok_or_else(|| UpdateError::InvalidArchive("entry size overflow".into()))?;
            if written > declared_size {
                return Err(UpdateError::InvalidArchive(
                    "archive entry expanded beyond its declared size".into(),
                ));
            }
            output.write_all(&buffer[..read])?;
        }
        if written != declared_size {
            return Err(UpdateError::InvalidArchive(
                "archive entry ended before its declared size".into(),
            ));
        }
        output.flush()?;
        output.sync_all()?;
        let metadata = fs::symlink_metadata(&output_path)?;
        ensure_metadata_is_not_reparse(&output_path, &metadata)?;
        if !metadata.is_file() {
            return Err(UpdateError::InvalidArchive(
                "archive extraction did not produce a regular file".into(),
            ));
        }
    }

    verify_installed_package(destination, verified)
}

fn validate_archive_entry_type(entry: &zip::read::ZipFile<'_>) -> Result<(), UpdateError> {
    let file_type = entry.unix_mode().map(|mode| mode & 0o170000);
    if entry.is_dir() {
        if entry.size() != 0 {
            return Err(UpdateError::InvalidArchive(
                "directory entry contains data".into(),
            ));
        }
        if file_type.is_some_and(|file_type| file_type != 0 && file_type != 0o040000) {
            return Err(UpdateError::InvalidArchive(
                "directory entry has a non-directory Unix mode".into(),
            ));
        }
        return Ok(());
    }
    if file_type.is_some_and(|file_type| file_type != 0 && file_type != 0o100000) {
        return Err(UpdateError::InvalidArchive(
            "archive contains a non-regular file".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct UpdateInstaller {
    root: PathBuf,
    state_path: PathBuf,
}

impl UpdateInstaller {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, UpdateError> {
        let root = root.into();
        if !root.is_absolute() {
            return Err(UpdateError::UnsafePath(root.display().to_string()));
        }
        Ok(Self {
            state_path: root.join("state.json"),
            root,
        })
    }

    pub fn load_state(&self) -> Result<Option<InstalledPackageState>, UpdateError> {
        ensure_safe_directory(&self.root)?;
        let metadata = match fs::symlink_metadata(&self.state_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(UpdateError::from(error)),
        };
        ensure_metadata_is_not_reparse(&self.state_path, &metadata)?;
        if !metadata.is_file() || metadata.len() > UPDATE_MAX_STATE_BYTES as u64 {
            return Err(UpdateError::CorruptState(
                "state file is not a bounded regular file".into(),
            ));
        }
        let bytes = fs::read(&self.state_path)?;
        let state: InstalledPackageState = serde_json::from_slice(&bytes)
            .map_err(|error| UpdateError::CorruptState(error.to_string()))?;
        validate_state(&state)?;
        Ok(Some(state))
    }

    pub fn install_verified_directory(
        &self,
        staged_dir: &Path,
        signed_manifest: &SignedReleaseManifest,
        keyset: &VerifiedReleaseKeyset,
        verifier: &UpdateVerifier,
        policy: &UpdatePolicy,
        now_unix: u64,
    ) -> Result<InstalledPackageState, UpdateError> {
        ensure_safe_directory(staged_dir)?;
        let current = self.load_state()?;
        let verified = verifier.verify_manifest(
            signed_manifest,
            keyset,
            policy,
            current.as_ref(),
            now_unix,
        )?;
        verify_installed_package(staged_dir, &verified)?;

        let releases = self.root.join("releases");
        ensure_safe_directory(&self.root)?;
        ensure_safe_directory(&releases)?;
        let release_dir = format!(
            "release-{}-{}",
            verified.manifest.sequence,
            &verified.manifest.package_sha256[..16]
        );
        let final_dir = releases.join(&release_dir);
        if final_dir.exists() {
            return Err(UpdateError::ReleaseAlreadyExists);
        }
        let temporary_dir = releases.join(format!(
            ".staging-{}-{}",
            std::process::id(),
            monotonic_nonce()
        ));
        fs::create_dir(&temporary_dir)?;
        let copy_result = copy_verified_tree(staged_dir, &temporary_dir);
        if copy_result.is_err() {
            let _ = fs::remove_dir_all(&temporary_dir);
            return copy_result.map(|()| unreachable!());
        }
        verify_installed_package(&temporary_dir, &verified)?;
        fs::rename(&temporary_dir, &final_dir)?;

        let next_state = InstalledPackageState {
            schema_version: UPDATE_PROTOCOL_VERSION,
            product: policy.product.clone(),
            current: InstalledRelease {
                release_dir,
                version: verified.manifest.version,
                sequence: verified.manifest.sequence,
                package_sha256: verified.manifest.package_sha256,
                manifest_sha256: verified.manifest_sha256,
            },
            last_known_good: current.and_then(|state| state.last_known_good),
        };
        write_state_atomic(&self.state_path, &next_state)?;
        Ok(next_state)
    }

    pub fn verify_current(
        &self,
        signed_manifest: &SignedReleaseManifest,
        keyset: &VerifiedReleaseKeyset,
        verifier: &UpdateVerifier,
        policy: &UpdatePolicy,
        now_unix: u64,
    ) -> Result<InstalledPackageState, UpdateError> {
        let state = self.load_state()?.ok_or(UpdateError::MissingState)?;
        if state.product != policy.product {
            return Err(UpdateError::CorruptState(
                "installed product does not match update policy".into(),
            ));
        }
        let verified = verifier.verify_manifest(signed_manifest, keyset, policy, None, now_unix)?;
        if !release_matches_manifest(&state.current, &verified) {
            return Err(UpdateError::CorruptState(
                "installed state does not match the signed manifest".into(),
            ));
        }
        let release_root = self.release_path(&state.current.release_dir)?;
        verify_installed_package(&release_root, &verified)?;
        Ok(state)
    }

    pub fn mark_current_verified(
        &self,
        signed_manifest: &SignedReleaseManifest,
        keyset: &VerifiedReleaseKeyset,
        verifier: &UpdateVerifier,
        policy: &UpdatePolicy,
        now_unix: u64,
    ) -> Result<InstalledPackageState, UpdateError> {
        let mut state = self.verify_current(signed_manifest, keyset, verifier, policy, now_unix)?;
        state.last_known_good = Some(state.current.clone());
        write_state_atomic(&self.state_path, &state)?;
        Ok(state)
    }

    pub fn rollback_verified(
        &self,
        signed_manifest: &SignedReleaseManifest,
        keyset: &VerifiedReleaseKeyset,
        verifier: &UpdateVerifier,
        policy: &UpdatePolicy,
        now_unix: u64,
    ) -> Result<InstalledPackageState, UpdateError> {
        let mut state = self.load_state()?.ok_or(UpdateError::MissingState)?;
        let last_known_good = state.last_known_good.clone().ok_or_else(|| {
            UpdateError::CorruptState("no verified rollback target exists".into())
        })?;
        let verified = verifier.verify_manifest(signed_manifest, keyset, policy, None, now_unix)?;
        if !release_matches_manifest(&last_known_good, &verified) {
            return Err(UpdateError::CorruptState(
                "rollback manifest does not match the verified rollback target".into(),
            ));
        }
        let release_root = self.release_path(&last_known_good.release_dir)?;
        verify_installed_package(&release_root, &verified)?;
        state.current = last_known_good;
        write_state_atomic(&self.state_path, &state)?;
        Ok(state)
    }

    pub fn pending_activation_path(&self) -> Result<Option<PathBuf>, UpdateError> {
        let descriptor_dir = self.root.join(UPDATE_ACTIVATION_DESCRIPTOR_DIR);
        match fs::symlink_metadata(&descriptor_dir) {
            Ok(metadata) => {
                ensure_metadata_is_not_reparse(&descriptor_dir, &metadata)?;
                if !metadata.is_dir() {
                    return Err(UpdateError::CorruptState(
                        "update activation descriptor root is not a directory".into(),
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(UpdateError::from(error)),
        }
        let descriptor = descriptor_dir.join(UPDATE_ACTIVATION_DESCRIPTOR_FILE);
        match fs::symlink_metadata(&descriptor) {
            Ok(metadata) => {
                ensure_metadata_is_not_reparse(&descriptor, &metadata)?;
                if !metadata.is_file()
                    || metadata.len() > UPDATE_ACTIVATION_DESCRIPTOR_MAX_BYTES as u64
                {
                    return Err(UpdateError::CorruptState(
                        "update activation descriptor is not a bounded regular file".into(),
                    ));
                }
                Ok(Some(descriptor))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(UpdateError::from(error)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_activation(
        &self,
        signed_keyset: &SignedReleaseKeyset,
        signed_manifest: &SignedReleaseManifest,
        verifier: &UpdateVerifier,
        policy: &UpdatePolicy,
        now_unix: u64,
        active_root: &Path,
        service_executable_name: &str,
        service_arguments: &[String],
        helper_executable: &Path,
        parent_pid: u32,
    ) -> Result<PathBuf, UpdateError> {
        let state = self.load_state()?.ok_or(UpdateError::MissingState)?;
        let verified_keyset = verifier.verify_keyset(signed_keyset, policy, now_unix)?;
        let verified =
            verifier.verify_manifest(signed_manifest, &verified_keyset, policy, None, now_unix)?;
        if !release_matches_manifest(&state.current, &verified) {
            return Err(UpdateError::CorruptState(
                "activation state does not match the signed release".into(),
            ));
        }
        let release_root = self.release_path(&state.current.release_dir)?;
        verify_installed_package(&release_root, &verified)?;
        validate_absolute_directory(active_root)?;
        validate_package_path(service_executable_name)?;
        if !verified
            .manifest()
            .files
            .iter()
            .any(|file| file.path == service_executable_name)
        {
            return Err(UpdateError::ActivationFailed(
                "signed release does not contain the service executable".into(),
            ));
        }
        validate_service_arguments(service_arguments)?;
        validate_absolute_file(helper_executable)?;
        let (_, helper_sha256) = hash_file(helper_executable)?;
        if parent_pid == 0 {
            return Err(UpdateError::ActivationUnavailable(
                "parent process identity is missing".into(),
            ));
        }

        let descriptor_dir = self.root.join(UPDATE_ACTIVATION_DESCRIPTOR_DIR);
        ensure_safe_directory(&descriptor_dir)?;
        let descriptor = UpdateActivationDescriptor {
            schema_version: UPDATE_PROTOCOL_VERSION,
            policy: policy.clone(),
            signed_keyset: signed_keyset.clone(),
            signed_manifest: signed_manifest.clone(),
            install_root: self.root.display().to_string(),
            release_dir: state.current.release_dir,
            active_root: active_root.display().to_string(),
            service_executable_name: service_executable_name.to_owned(),
            service_arguments: service_arguments.to_vec(),
            helper_executable: helper_executable.display().to_string(),
            helper_sha256,
            parent_pid,
        };
        let descriptor_path = descriptor_dir.join(UPDATE_ACTIVATION_DESCRIPTOR_FILE);
        write_activation_descriptor_atomic(&descriptor_path, &descriptor)?;
        Ok(descriptor_path)
    }

    pub fn prepare_activation_from_current_process(
        &self,
        signed_keyset: &SignedReleaseKeyset,
        signed_manifest: &SignedReleaseManifest,
        verifier: &UpdateVerifier,
        policy: &UpdatePolicy,
        now_unix: u64,
        service_arguments: &[String],
    ) -> Result<PathBuf, UpdateError> {
        if !cfg!(target_os = "windows") {
            return Err(UpdateError::UnsupportedPlatform);
        }
        let current_exe = std::env::current_exe()
            .map_err(|error| UpdateError::ActivationUnavailable(error.to_string()))?;
        validate_absolute_file(&current_exe)?;
        let active_root = current_exe.parent().ok_or_else(|| {
            UpdateError::ActivationUnavailable("running executable has no parent directory".into())
        })?;
        let service_executable_name = current_exe
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                UpdateError::ActivationUnavailable(
                    "running executable name is not valid UTF-8".into(),
                )
            })?;
        let descriptor_dir = self.root.join(UPDATE_ACTIVATION_DESCRIPTOR_DIR);
        ensure_safe_directory(&descriptor_dir)?;
        let helper_executable = descriptor_dir.join(format!(
            "helper-{}-{}.exe",
            std::process::id(),
            monotonic_nonce()
        ));
        fs::copy(&current_exe, &helper_executable).map_err(|error| {
            UpdateError::ActivationUnavailable(format!("could not stage updater helper: {error}"))
        })?;
        let helper_result = self.prepare_activation(
            signed_keyset,
            signed_manifest,
            verifier,
            policy,
            now_unix,
            active_root,
            service_executable_name,
            service_arguments,
            &helper_executable,
            std::process::id(),
        );
        if helper_result.is_err() {
            let _ = fs::remove_file(&helper_executable);
        }
        helper_result
    }

    pub fn verified_release_path(&self, release_dir: &str) -> Result<PathBuf, UpdateError> {
        self.release_path(release_dir)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn release_path(&self, release_dir: &str) -> Result<PathBuf, UpdateError> {
        validate_package_path(release_dir)?;
        safe_join(&self.root.join("releases"), release_dir)
    }
}

fn release_matches_manifest(
    release: &InstalledRelease,
    verified: &VerifiedReleaseManifest,
) -> bool {
    release.version == verified.manifest.version
        && release.sequence == verified.manifest.sequence
        && release.package_sha256 == verified.manifest.package_sha256
        && release.manifest_sha256 == verified.manifest_sha256
}

fn copy_verified_tree(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    ensure_safe_directory(source)?;
    ensure_safe_directory(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        validate_package_path(&name)?;
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let metadata = fs::symlink_metadata(&source_path)?;
        ensure_metadata_is_not_reparse(&source_path, &metadata)?;
        if metadata.is_dir() {
            fs::create_dir(&destination_path)?;
            copy_verified_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
            let copied_metadata = fs::symlink_metadata(&destination_path)?;
            ensure_metadata_is_not_reparse(&destination_path, &copied_metadata)?;
        } else {
            return Err(UpdateError::PackageMismatch);
        }
    }
    Ok(())
}

fn validate_absolute_directory(path: &Path) -> Result<(), UpdateError> {
    if !path.is_absolute() {
        return Err(UpdateError::UnsafePath(path.display().to_string()));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| UpdateError::ActivationUnavailable(error.to_string()))?;
    ensure_metadata_is_not_reparse(path, &metadata)?;
    if !metadata.is_dir() {
        return Err(UpdateError::UnsafePath(path.display().to_string()));
    }
    Ok(())
}

fn validate_absolute_file(path: &Path) -> Result<(), UpdateError> {
    if !path.is_absolute() {
        return Err(UpdateError::UnsafePath(path.display().to_string()));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| UpdateError::ActivationUnavailable(error.to_string()))?;
    ensure_metadata_is_not_reparse(path, &metadata)?;
    if !metadata.is_file() {
        return Err(UpdateError::UnsafePath(path.display().to_string()));
    }
    Ok(())
}

fn validate_service_arguments(arguments: &[String]) -> Result<(), UpdateError> {
    const MAX_ARGUMENTS: usize = 32;
    const MAX_ARGUMENT_BYTES: usize = 4096;
    const MAX_TOTAL_BYTES: usize = 16 * 1024;
    if arguments.len() > MAX_ARGUMENTS {
        return Err(UpdateError::ActivationFailed(
            "service argument list is too large".into(),
        ));
    }
    let mut total = 0usize;
    for argument in arguments {
        if argument.is_empty()
            || argument.len() > MAX_ARGUMENT_BYTES
            || argument.contains('\0')
            || argument.chars().any(|character| character.is_control())
            || argument == "--hivemind-apply-update"
        {
            return Err(UpdateError::ActivationFailed(
                "service argument list contains an unsafe argument".into(),
            ));
        }
        total = total
            .checked_add(argument.len())
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| {
                UpdateError::ActivationFailed("service argument list overflows".into())
            })?;
        if total > MAX_TOTAL_BYTES {
            return Err(UpdateError::ActivationFailed(
                "service argument list is too large".into(),
            ));
        }
    }
    Ok(())
}

fn write_activation_descriptor_atomic(
    path: &Path,
    descriptor: &UpdateActivationDescriptor,
) -> Result<(), UpdateError> {
    if !path.is_absolute() {
        return Err(UpdateError::UnsafePath(path.display().to_string()));
    }
    let parent = path
        .parent()
        .ok_or_else(|| UpdateError::UnsafePath(path.display().to_string()))?;
    ensure_safe_directory(parent)?;
    let bytes = serde_json::to_vec(descriptor)
        .map_err(|error| UpdateError::CorruptState(error.to_string()))?;
    if bytes.len() > UPDATE_ACTIVATION_DESCRIPTOR_MAX_BYTES {
        return Err(UpdateError::MetadataTooLarge);
    }
    let temporary = parent.join(format!(
        ".activation-{}-{}.tmp",
        std::process::id(),
        monotonic_nonce()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = atomic_replace(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn write_state_atomic(path: &Path, state: &InstalledPackageState) -> Result<(), UpdateError> {
    validate_state(state)?;
    let parent = path
        .parent()
        .ok_or_else(|| UpdateError::UnsafePath(path.display().to_string()))?;
    ensure_safe_directory(parent)?;
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        ensure_metadata_is_not_reparse(path, &metadata)?;
    }
    let temporary = parent.join(format!(
        ".state-{}-{}.tmp",
        std::process::id(),
        monotonic_nonce()
    ));
    let bytes =
        serde_json::to_vec(state).map_err(|error| UpdateError::CorruptState(error.to_string()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = atomic_replace(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn atomic_replace(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        extern "system" {
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
            return Err(UpdateError::Io(io::Error::last_os_error().to_string()));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination).map_err(UpdateError::from)
    }
}

/// Parse the private updater handoff argument before normal service startup.
///
/// The handoff is intentionally not part of the public CLI. Any malformed or
/// unexpected argument is rejected instead of being interpreted as a service
/// command or an update fallback.
pub fn activation_request_path(args: &[String]) -> Result<Option<PathBuf>, UpdateError> {
    match args {
        [] | [_] => Ok(None),
        [_, flag, path] if flag == "--hivemind-apply-update" => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(UpdateError::UnsafePath(path.display().to_string()));
            }
            Ok(Some(path))
        }
        [_, flag, ..] if flag == "--hivemind-apply-update" => Err(UpdateError::ActivationFailed(
            "update handoff accepts exactly one descriptor".into(),
        )),
        _ => Ok(None),
    }
}

/// Validate a pending handoff and return the copied updater plus the service
/// arguments that must be restored after activation. The copied executable is
/// authenticated by the hash recorded before the running client requested a
/// restart; a descriptor cannot redirect startup to an arbitrary file.
pub fn activation_launch_spec(path: &Path) -> Result<(PathBuf, Vec<String>), UpdateError> {
    let descriptor = read_activation_descriptor(path)?;
    validate_activation_descriptor(path, &descriptor)?;
    let helper = PathBuf::from(&descriptor.helper_executable);
    let (_, helper_sha256) = hash_file(&helper)?;
    if helper_sha256 != descriptor.helper_sha256 {
        return Err(UpdateError::ActivationFailed(
            "update helper no longer matches its recorded digest".into(),
        ));
    }
    Ok((helper, descriptor.service_arguments))
}

fn validate_activation_preflight(path: &Path) -> Result<(), UpdateError> {
    let descriptor = read_activation_descriptor(path)?;
    validate_activation_descriptor(path, &descriptor)?;
    let now_unix = unix_now()?;
    let verifier = UpdateVerifier::embedded()?;
    let keyset = verifier.verify_keyset(&descriptor.signed_keyset, &descriptor.policy, now_unix)?;
    let verified = verifier.verify_manifest(
        &descriptor.signed_manifest,
        &keyset,
        &descriptor.policy,
        None,
        now_unix,
    )?;
    let installer = UpdateInstaller::new(&descriptor.install_root)?;
    let state = installer.load_state()?.ok_or(UpdateError::MissingState)?;
    if state.current.release_dir != descriptor.release_dir
        || !release_matches_manifest(&state.current, &verified)
    {
        return Err(UpdateError::CorruptState(
            "activation descriptor does not match installed state".into(),
        ));
    }
    if !verified
        .manifest()
        .files
        .iter()
        .any(|file| file.path == descriptor.service_executable_name)
    {
        return Err(UpdateError::ActivationFailed(
            "signed release does not contain the service executable".into(),
        ));
    }
    let service_path = safe_join(
        Path::new(&descriptor.active_root),
        &descriptor.service_executable_name,
    )?;
    validate_absolute_file(&service_path)?;
    let release_root = installer.verified_release_path(&descriptor.release_dir)?;
    verify_installed_package(&release_root, &verified)
}

pub fn spawn_activation_helper(path: &Path) -> Result<(), UpdateError> {
    if !cfg!(target_os = "windows") {
        return Err(UpdateError::UnsupportedPlatform);
    }
    validate_activation_preflight(path)?;
    let (helper, _) = activation_launch_spec(path)?;
    std::process::Command::new(&helper)
        .arg("--hivemind-apply-update")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| UpdateError::ActivationUnavailable(error.to_string()))
}

/// Execute a verified update handoff in a separate process.
///
/// The helper is created by copying the currently running executable to a
/// transaction directory before the service exits. That copy is not locked by
/// the service, so it can replace the service executable after the parent has
/// stopped. On non-Windows targets there is deliberately no process-replacement
/// fallback.
pub async fn run_activation_helper(descriptor_path: PathBuf) -> Result<(), UpdateError> {
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(move || apply_activation(&descriptor_path))
            .await
            .map_err(|error| UpdateError::ActivationFailed(error.to_string()))??;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = descriptor_path;
        Err(UpdateError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
fn spawn_activated_service(
    service_path: &Path,
    active_root: &Path,
    service_arguments: &[String],
    helper_executable: &Path,
) -> Result<std::process::Child, UpdateError> {
    std::process::Command::new(service_path)
        .args(service_arguments)
        .current_dir(active_root)
        .env("HIVEMIND_UPDATE_HELPER_TEMP", helper_executable)
        .spawn()
        .map_err(|error| UpdateError::ActivationFailed(error.to_string()))
}

#[cfg(windows)]
fn apply_activation(descriptor_path: &Path) -> Result<(), UpdateError> {
    let descriptor = read_activation_descriptor(descriptor_path)?;
    validate_activation_descriptor(descriptor_path, &descriptor)?;
    let current_exe = std::env::current_exe()
        .map_err(|error| UpdateError::ActivationFailed(error.to_string()))?;
    validate_absolute_file(&current_exe)?;
    if canonical_path(&current_exe)? != canonical_path(Path::new(&descriptor.helper_executable))? {
        return Err(UpdateError::ActivationFailed(
            "updater executable does not match the handoff descriptor".into(),
        ));
    }
    let (_, current_sha256) = hash_file(&current_exe)?;
    if current_sha256 != descriptor.helper_sha256 {
        return Err(UpdateError::ActivationFailed(
            "updater executable digest does not match the handoff descriptor".into(),
        ));
    }
    let active_root = Path::new(&descriptor.active_root);
    let service_path = safe_join(active_root, &descriptor.service_executable_name)?;
    validate_absolute_file(&service_path)?;
    wait_for_parent_exit(descriptor.parent_pid, service_path.clone())?;

    let now_unix = unix_now()?;
    let verifier = UpdateVerifier::embedded()?;
    let keyset = verifier.verify_keyset(&descriptor.signed_keyset, &descriptor.policy, now_unix)?;
    let verified = verifier.verify_manifest(
        &descriptor.signed_manifest,
        &keyset,
        &descriptor.policy,
        None,
        now_unix,
    )?;
    if !verified
        .manifest()
        .files
        .iter()
        .any(|file| file.path == descriptor.service_executable_name)
    {
        return Err(UpdateError::ActivationFailed(
            "signed release does not contain the service executable".into(),
        ));
    }

    let installer = UpdateInstaller::new(&descriptor.install_root)?;
    let state = installer.load_state()?.ok_or(UpdateError::MissingState)?;
    if state.current.release_dir != descriptor.release_dir
        || !release_matches_manifest(&state.current, &verified)
    {
        return Err(UpdateError::CorruptState(
            "activation descriptor does not match installed state".into(),
        ));
    }
    let release_root = installer.verified_release_path(&descriptor.release_dir)?;
    verify_installed_package(&release_root, &verified)?;

    let backup_dir = apply_release_files(&release_root, active_root, &verified)?;
    let mut child = match spawn_activated_service(
        &service_path,
        active_root,
        &descriptor.service_arguments,
        &current_exe,
    ) {
        Ok(child) => child,
        Err(error) => {
            let rollback = rollback_activation_files(active_root, &backup_dir, &verified);
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(UpdateError::ActivationFailed(format!(
                    "new client could not start: {error}; rollback failed: {rollback_error}"
                ))),
            };
        }
    };

    if let Err(error) = fs::remove_file(descriptor_path) {
        let _ = child.kill();
        let _ = child.wait();
        let rollback = rollback_activation_files(active_root, &backup_dir, &verified);
        return match rollback {
            Ok(()) => Err(UpdateError::ActivationFailed(format!(
                "activation descriptor could not be removed: {error}"
            ))),
            Err(rollback_error) => Err(UpdateError::ActivationFailed(format!(
                "activation descriptor could not be removed: {error}; rollback failed: {rollback_error}"
            ))),
        };
    }
    let _ = fs::remove_dir_all(&backup_dir);
    Ok(())
}

fn read_activation_descriptor(path: &Path) -> Result<UpdateActivationDescriptor, UpdateError> {
    let metadata = fs::symlink_metadata(path)?;
    ensure_metadata_is_not_reparse(path, &metadata)?;
    if !metadata.is_file() || metadata.len() > UPDATE_ACTIVATION_DESCRIPTOR_MAX_BYTES as u64 {
        return Err(UpdateError::CorruptState(
            "update activation descriptor is not a bounded regular file".into(),
        ));
    }
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|error| UpdateError::CorruptState(error.to_string()))
}

fn validate_activation_descriptor(
    descriptor_path: &Path,
    descriptor: &UpdateActivationDescriptor,
) -> Result<(), UpdateError> {
    if descriptor.schema_version != UPDATE_PROTOCOL_VERSION {
        return Err(UpdateError::CorruptState(
            "unsupported update activation descriptor schema".into(),
        ));
    }
    let install_root = Path::new(&descriptor.install_root);
    validate_absolute_directory(install_root)?;
    let transaction_dir = descriptor_path
        .parent()
        .ok_or_else(|| UpdateError::UnsafePath(descriptor_path.display().to_string()))?;
    let expected_transaction_dir = install_root.join(UPDATE_ACTIVATION_DESCRIPTOR_DIR);
    if canonical_path(transaction_dir)? != canonical_path(&expected_transaction_dir)?
        || descriptor_path.file_name().and_then(|name| name.to_str())
            != Some(UPDATE_ACTIVATION_DESCRIPTOR_FILE)
    {
        return Err(UpdateError::ActivationFailed(
            "activation descriptor is outside its installed update root".into(),
        ));
    }
    validate_absolute_directory(Path::new(&descriptor.active_root))?;
    validate_package_path(&descriptor.service_executable_name)?;
    validate_service_arguments(&descriptor.service_arguments)?;
    let helper = Path::new(&descriptor.helper_executable);
    validate_absolute_file(helper)?;
    let helper_parent = helper
        .parent()
        .ok_or_else(|| UpdateError::UnsafePath(helper.display().to_string()))?;
    if canonical_path(helper_parent)? != canonical_path(transaction_dir)? {
        return Err(UpdateError::ActivationFailed(
            "update helper is outside the activation transaction directory".into(),
        ));
    }
    let _ = decode_fixed_hex::<UPDATE_HASH_BYTES>(&descriptor.helper_sha256, "helper hash")?;
    if descriptor.parent_pid == 0 {
        return Err(UpdateError::ActivationFailed(
            "activation parent process identity is invalid".into(),
        ));
    }
    validate_package_path(&descriptor.release_dir)?;
    Ok(())
}

fn canonical_path(path: &Path) -> Result<PathBuf, UpdateError> {
    fs::canonicalize(path).map_err(|error| UpdateError::ActivationFailed(error.to_string()))
}

#[cfg(windows)]
fn apply_release_files(
    release_root: &Path,
    active_root: &Path,
    verified: &VerifiedReleaseManifest,
) -> Result<PathBuf, UpdateError> {
    validate_absolute_directory(active_root)?;
    let transaction_dir = active_root.join(format!(
        ".hivemind-activation-backup-{}-{}",
        std::process::id(),
        monotonic_nonce()
    ));
    ensure_safe_directory(&transaction_dir)?;
    let mut backups = Vec::new();
    let result = (|| {
        for file in &verified.manifest.files {
            validate_package_path(&file.path)?;
            let source = safe_join(release_root, &file.path)?;
            let source_metadata = fs::symlink_metadata(&source)?;
            ensure_metadata_is_not_reparse(&source, &source_metadata)?;
            if !source_metadata.is_file() {
                return Err(UpdateError::PackageMismatch);
            }
            let destination = safe_join(active_root, &file.path)?;
            let parent = destination
                .parent()
                .ok_or_else(|| UpdateError::UnsafePath(destination.display().to_string()))?;
            ensure_safe_directory(parent)?;
            let backup = match fs::symlink_metadata(&destination) {
                Ok(metadata) => {
                    ensure_metadata_is_not_reparse(&destination, &metadata)?;
                    if !metadata.is_file() {
                        return Err(UpdateError::ActivationFailed(
                            "active package path is not a regular file".into(),
                        ));
                    }
                    let backup_path = safe_join(&transaction_dir, &file.path)?;
                    let backup_parent = backup_path.parent().ok_or_else(|| {
                        UpdateError::UnsafePath(backup_path.display().to_string())
                    })?;
                    ensure_safe_directory(backup_parent)?;
                    fs::copy(&destination, &backup_path)?;
                    Some(backup_path)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(UpdateError::from(error)),
            };
            let temporary = parent.join(format!(
                ".hivemind-activation-{}-{}.tmp",
                std::process::id(),
                monotonic_nonce()
            ));
            if let Err(error) = fs::copy(&source, &temporary) {
                let _ = fs::remove_file(&temporary);
                return Err(UpdateError::from(error));
            }
            let temporary_metadata = fs::symlink_metadata(&temporary)?;
            ensure_metadata_is_not_reparse(&temporary, &temporary_metadata)?;
            if let Err(error) = atomic_replace(&temporary, &destination) {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
            backups.push((destination, backup));
        }
        Ok::<(), UpdateError>(())
    })();
    if let Err(error) = result {
        let rollback = rollback_activation_backups(&backups, &transaction_dir);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(UpdateError::ActivationFailed(format!(
                "activation failed: {error}; rollback failed: {rollback_error}"
            ))),
        };
    }
    Ok(transaction_dir)
}

#[cfg(windows)]
fn rollback_activation_backups(
    backups: &[(PathBuf, Option<PathBuf>)],
    transaction_dir: &Path,
) -> Result<(), UpdateError> {
    for (destination, backup) in backups.iter().rev() {
        match backup {
            Some(backup) => {
                let temporary = destination.with_extension(format!(
                    "restore-{}-{}",
                    std::process::id(),
                    monotonic_nonce()
                ));
                fs::copy(backup, &temporary)?;
                if let Err(error) = atomic_replace(&temporary, destination) {
                    let _ = fs::remove_file(&temporary);
                    return Err(error);
                }
            }
            None => match fs::remove_file(destination) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(UpdateError::from(error)),
            },
        }
    }
    fs::remove_dir_all(transaction_dir).map_err(UpdateError::from)
}

#[cfg(windows)]
fn rollback_activation_files(
    active_root: &Path,
    transaction_dir: &Path,
    verified: &VerifiedReleaseManifest,
) -> Result<(), UpdateError> {
    let mut backups = Vec::with_capacity(verified.manifest.files.len());
    for file in &verified.manifest.files {
        let destination = safe_join(active_root, &file.path)?;
        let backup = safe_join(transaction_dir, &file.path)?;
        let backup = match fs::symlink_metadata(&backup) {
            Ok(metadata) => {
                ensure_metadata_is_not_reparse(&backup, &metadata)?;
                if !metadata.is_file() {
                    return Err(UpdateError::ActivationFailed(
                        "activation backup is not a regular file".into(),
                    ));
                }
                Some(backup)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(UpdateError::from(error)),
        };
        backups.push((destination, backup));
    }
    rollback_activation_backups(&backups, transaction_dir)
}

#[cfg(windows)]
fn wait_for_parent_exit(parent_pid: u32, expected_executable: PathBuf) -> Result<(), UpdateError> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::raw::HANDLE;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = 0xffff_ffff;
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> HANDLE;
        fn QueryFullProcessImageNameW(
            process: HANDLE,
            flags: u32,
            exe_name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn WaitForSingleObject(handle: HANDLE, milliseconds: u32) -> u32;
        fn CloseHandle(handle: HANDLE) -> i32;
    }
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
            0,
            parent_pid,
        )
    };
    if handle.is_null() {
        if std::io::Error::last_os_error().raw_os_error() == Some(87) {
            return Ok(());
        }
        return Err(UpdateError::ActivationFailed(
            "could not open the update parent process".into(),
        ));
    }
    let result = (|| {
        let mut buffer = vec![0u16; 32768];
        let mut length = buffer.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
            return Err(UpdateError::ActivationFailed(
                "could not identify the update parent process".into(),
            ));
        }
        let image = OsString::from_wide(&buffer[..length as usize]);
        if canonical_path(Path::new(&image))? != canonical_path(&expected_executable)? {
            return Err(UpdateError::ActivationFailed(
                "update parent process is not the expected service".into(),
            ));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(UpdateError::ActivationFailed(
                    "timed out waiting for the service to stop".into(),
                ));
            }
            let wait_ms = remaining.as_millis().min(1000) as u32;
            match unsafe { WaitForSingleObject(handle, wait_ms) } {
                WAIT_OBJECT_0 => return Ok(()),
                WAIT_TIMEOUT => continue,
                WAIT_FAILED => {
                    return Err(UpdateError::ActivationFailed(
                        "waiting for the service process failed".into(),
                    ))
                }
                _ => {
                    return Err(UpdateError::ActivationFailed(
                        "waiting for the service process returned an unknown status".into(),
                    ))
                }
            }
        }
    })();
    unsafe {
        CloseHandle(handle);
    }
    result
}

fn unix_now() -> Result<u64, UpdateError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| UpdateError::Io(error.to_string()))
}

fn monotonic_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::fs;
    use zip::write::{SimpleFileOptions, ZipWriter};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn sign(key: &SigningKey, bytes: &[u8]) -> String {
        hex::encode(key.sign(bytes).to_bytes())
    }

    fn policy() -> UpdatePolicy {
        UpdatePolicy::new(
            "hivemind-windows-worker",
            "stable",
            "windows",
            "x86_64",
            &["updates.example.test"],
        )
    }

    fn manifest(sequence: u64, content: &[u8]) -> ReleaseManifest {
        ReleaseManifest {
            schema_version: UPDATE_PROTOCOL_VERSION,
            product: "hivemind-windows-worker".into(),
            channel: "stable".into(),
            platform: "windows".into(),
            architecture: "x86_64".into(),
            version: format!("0.1.{sequence}"),
            sequence,
            minimum_supported_version: "0.1.0".into(),
            release_key_id: "release-1".into(),
            issued_at_unix: 900,
            expires_at_unix: 2_000,
            package_url: "https://updates.example.test/worker.zip".into(),
            package_size: content.len() as u64 + 1,
            package_sha256: hex::encode(Sha256::digest(content)),
            files: vec![ReleaseFile {
                path: "hivemind-worker.exe".into(),
                size: content.len() as u64,
                sha256: hex::encode(Sha256::digest(content)),
            }],
        }
    }

    fn signed_release(
        root: &SigningKey,
        release: &SigningKey,
        sequence: u64,
        content: &[u8],
    ) -> (SignedReleaseKeyset, SignedReleaseManifest, UpdateVerifier) {
        let release_key = ReleaseKey {
            key_id: "release-1".into(),
            public_key_hex: hex::encode(release.verifying_key().to_bytes()),
            not_before_unix: 800,
            not_after_unix: 2_000,
            revoked_at_unix: None,
        };
        let keyset = ReleaseKeyset {
            schema_version: UPDATE_PROTOCOL_VERSION,
            product: "hivemind-windows-worker".into(),
            channel: "stable".into(),
            expires_at_unix: 1_900,
            keys: BTreeMap::from([("release-1".into(), release_key)]),
        };
        let signed_keyset = SignedReleaseKeyset {
            signature: sign(root, &canonical_keyset_bytes(&keyset).unwrap()),
            keyset,
        };
        let manifest = manifest(sequence, content);
        let signed_manifest = SignedReleaseManifest {
            signature: sign(release, &canonical_manifest_bytes(&manifest).unwrap()),
            manifest,
        };
        (
            signed_keyset,
            signed_manifest,
            UpdateVerifier {
                root_key: root.verifying_key(),
            },
        )
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hivemind-update-{label}-{}-{}",
            std::process::id(),
            monotonic_nonce()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn test_activation_descriptor(root: &Path, helper: &Path) -> UpdateActivationDescriptor {
        let root_signer = key(9);
        let release_signer = key(7);
        let (signed_keyset, signed_manifest, _) =
            signed_release(&root_signer, &release_signer, 1, b"worker");
        let (_, helper_sha256) = hash_file(helper).unwrap();
        UpdateActivationDescriptor {
            schema_version: UPDATE_PROTOCOL_VERSION,
            policy: policy(),
            signed_keyset,
            signed_manifest,
            install_root: root.display().to_string(),
            release_dir: "release-1-abcdef0123456789".into(),
            active_root: root.join("active").display().to_string(),
            service_executable_name: "hivemind-worker.exe".into(),
            service_arguments: vec!["worker".into()],
            helper_executable: helper.display().to_string(),
            helper_sha256,
            parent_pid: 1234,
        }
    }

    fn write_test_activation_descriptor(
        root: &Path,
        helper: &Path,
    ) -> (PathBuf, UpdateActivationDescriptor) {
        let transaction = root.join(UPDATE_ACTIVATION_DESCRIPTOR_DIR);
        fs::create_dir_all(&transaction).unwrap();
        fs::create_dir_all(root.join("active")).unwrap();
        fs::write(helper, b"verified updater helper").unwrap();
        let descriptor = test_activation_descriptor(root, helper);
        let path = transaction.join(UPDATE_ACTIVATION_DESCRIPTOR_FILE);
        write_activation_descriptor_atomic(&path, &descriptor).unwrap();
        (path, descriptor)
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        for &(name, content) in entries {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
    }

    fn write_zip_with_directory(path: &Path, directory: &str, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .add_directory(directory, SimpleFileOptions::default())
            .unwrap();
        for &(name, content) in entries {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
    }

    fn write_zip_with_symlink(path: &Path, name: &str, target: &str) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .add_symlink(name, target, SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
    }

    fn write_zip_with_entry_count(path: &Path, count: usize) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        for index in 0..count {
            let name = format!("entry-{index}.bin");
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
        }
        writer.finish().unwrap();
    }

    fn duplicate_first_central_entry(path: &Path) {
        let bytes = fs::read(path).unwrap();
        let end = bytes
            .windows(4)
            .rposition(|signature| signature == b"PK\x05\x06")
            .unwrap();
        let central_offset =
            u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
        let central_size =
            u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap()) as usize;
        let central_entry = bytes[central_offset..central_offset + central_size].to_vec();
        let mut output = bytes[..end].to_vec();
        output.extend_from_slice(&central_entry);
        let mut end_record = bytes[end..].to_vec();
        let entries = u16::from_le_bytes(end_record[10..12].try_into().unwrap());
        end_record[8..10].copy_from_slice(&(entries + 1).to_le_bytes());
        end_record[10..12].copy_from_slice(&(entries + 1).to_le_bytes());
        let new_central_size = central_size + central_entry.len();
        end_record[12..16].copy_from_slice(&(new_central_size as u32).to_le_bytes());
        output.extend_from_slice(&end_record);
        fs::write(path, output).unwrap();
    }

    fn patch_zip_entry_count(path: &Path, count: u16) {
        let mut bytes = fs::read(path).unwrap();
        let end = bytes
            .windows(4)
            .rposition(|signature| signature == b"PK\x05\x06")
            .unwrap();
        bytes[end + 8..end + 10].copy_from_slice(&count.to_le_bytes());
        bytes[end + 10..end + 12].copy_from_slice(&count.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    fn patch_zip_unix_mode(path: &Path, mode: u32) {
        let mut bytes = fs::read(path).unwrap();
        let central_header = bytes
            .windows(4)
            .position(|signature| signature == b"PK\x01\x02")
            .unwrap();
        let version_made_by = ((3u16) << 8) | 20;
        bytes[central_header + 4..central_header + 6]
            .copy_from_slice(&version_made_by.to_le_bytes());
        bytes[central_header + 38..central_header + 42]
            .copy_from_slice(&(mode << 16).to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    fn patch_zip_encrypted(path: &Path) {
        let mut bytes = fs::read(path).unwrap();
        for (signature, flags_offset) in [(b"PK\x03\x04", 6usize), (b"PK\x01\x02", 8usize)] {
            let mut search_from = 0;
            while let Some(relative) = bytes[search_from..]
                .windows(4)
                .position(|candidate| candidate == signature)
            {
                let header = search_from + relative;
                let flags = u16::from_le_bytes([
                    bytes[header + flags_offset],
                    bytes[header + flags_offset + 1],
                ]);
                bytes[header + flags_offset..header + flags_offset + 2]
                    .copy_from_slice(&(flags | 1).to_le_bytes());
                search_from = header + 4;
            }
        }
        fs::write(path, bytes).unwrap();
    }

    fn verified_archive(
        archive_path: &Path,
        expected_files: &[(&str, &[u8])],
    ) -> VerifiedReleaseManifest {
        let (package_size, package_sha256) = hash_file(archive_path).unwrap();
        let manifest = ReleaseManifest {
            schema_version: UPDATE_PROTOCOL_VERSION,
            product: "hivemind-windows-worker".into(),
            channel: "stable".into(),
            platform: "windows".into(),
            architecture: "x86_64".into(),
            version: "0.1.1".into(),
            sequence: 1,
            minimum_supported_version: "0.1.0".into(),
            release_key_id: "release-1".into(),
            issued_at_unix: 900,
            expires_at_unix: 2_000,
            package_url: "https://updates.example.test/worker.zip".into(),
            package_size,
            package_sha256,
            files: expected_files
                .iter()
                .map(|&(path, content)| ReleaseFile {
                    path: path.into(),
                    size: content.len() as u64,
                    sha256: hex::encode(Sha256::digest(content)),
                })
                .collect(),
        };
        VerifiedReleaseManifest {
            manifest,
            manifest_sha256: "a".repeat(64),
        }
    }

    fn extract_test_archive(
        label: &str,
        entries: &[(&str, &[u8])],
        expected_files: &[(&str, &[u8])],
    ) -> (PathBuf, PathBuf, VerifiedReleaseManifest) {
        let root = temp_root(label);
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        write_zip(&archive, entries);
        let verified = verified_archive(&archive, expected_files);
        (root.clone(), root.join("release"), verified)
    }

    #[test]
    fn embedded_root_is_independent_from_worker_execution_key() {
        let embedded = UpdateVerifier::embedded().unwrap();
        let worker_key = key(1).verifying_key();
        assert_ne!(embedded.root_key.to_bytes(), worker_key.to_bytes());
    }

    #[test]
    fn canonical_manifest_sorts_files_before_signing() {
        let mut first = manifest(1, b"worker");
        first.files.push(ReleaseFile {
            path: "worker-ui/index.html".into(),
            size: 4,
            sha256: hex::encode(Sha256::digest(b"page")),
        });
        let mut second = first.clone();
        second.files.reverse();
        assert_eq!(
            canonical_manifest_bytes(&first).unwrap(),
            canonical_manifest_bytes(&second).unwrap()
        );
    }

    #[test]
    fn verifies_root_signed_keyset_and_release_signed_manifest() {
        let root = key(9);
        let release = key(7);
        let (signed_keyset, signed_manifest, verifier) =
            signed_release(&root, &release, 1, b"worker");
        let policy = policy();
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        let verified = verifier
            .verify_manifest(&signed_manifest, &keyset, &policy, None, 1_000)
            .unwrap();
        assert_eq!(verified.manifest.sequence, 1);
        assert_eq!(verified.manifest_sha256.len(), 64);
    }

    #[test]
    fn rejects_wrong_root_and_release_signatures() {
        let root = key(9);
        let release = key(7);
        let (mut signed_keyset, _signed_manifest, verifier) =
            signed_release(&root, &release, 1, b"worker");
        signed_keyset.signature = sign(
            &key(8),
            &canonical_keyset_bytes(&signed_keyset.keyset).unwrap(),
        );
        let policy = policy();
        assert!(matches!(
            verifier.verify_keyset(&signed_keyset, &policy, 1_000),
            Err(UpdateError::InvalidSignature)
        ));

        let (signed_keyset, mut signed_manifest, verifier) =
            signed_release(&root, &release, 1, b"worker");
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        signed_manifest.signature = sign(
            &key(8),
            &canonical_manifest_bytes(&signed_manifest.manifest).unwrap(),
        );
        assert!(matches!(
            verifier.verify_manifest(&signed_manifest, &keyset, &policy, None, 1_000),
            Err(UpdateError::InvalidSignature)
        ));
    }

    #[test]
    fn rejects_expired_revoked_wrong_target_and_unapproved_urls() {
        let root = key(9);
        let release = key(7);
        let (signed_keyset, mut signed_manifest, verifier) =
            signed_release(&root, &release, 1, b"worker");
        let policy = policy();
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();

        signed_manifest.manifest.expires_at_unix = 999;
        signed_manifest.signature = sign(
            &release,
            &canonical_manifest_bytes(&signed_manifest.manifest).unwrap(),
        );
        assert!(matches!(
            verifier.verify_manifest(&signed_manifest, &keyset, &policy, None, 1_000),
            Err(UpdateError::InvalidTimeWindow)
        ));

        let (mut signed_keyset, _signed_manifest, verifier) =
            signed_release(&root, &release, 1, b"worker");
        signed_keyset
            .keyset
            .keys
            .get_mut("release-1")
            .unwrap()
            .revoked_at_unix = Some(999);
        signed_keyset.signature = sign(
            &root,
            &canonical_keyset_bytes(&signed_keyset.keyset).unwrap(),
        );
        assert!(matches!(
            verifier.verify_keyset(&signed_keyset, &policy, 1_000),
            Err(UpdateError::UntrustedReleaseKey)
        ));

        let (signed_keyset, mut signed_manifest, verifier) =
            signed_release(&root, &release, 1, b"worker");
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        signed_manifest.manifest.package_url = "http://updates.example.test/worker.zip".into();
        signed_manifest.signature = sign(
            &release,
            &canonical_manifest_bytes(&signed_manifest.manifest).unwrap(),
        );
        assert!(matches!(
            verifier.verify_manifest(&signed_manifest, &keyset, &policy, None, 1_000),
            Err(UpdateError::InvalidPackageUrl)
        ));
    }

    #[test]
    fn rejects_replay_and_unsafe_manifest_paths() {
        let root = key(9);
        let release = key(7);
        let (signed_keyset, mut signed_manifest, verifier) =
            signed_release(&root, &release, 1, b"worker");
        let policy = policy();
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        let state = InstalledPackageState {
            schema_version: UPDATE_PROTOCOL_VERSION,
            product: policy.product.clone(),
            current: InstalledRelease {
                release_dir: "release-1-abcd".into(),
                version: "0.1.0".into(),
                sequence: 1,
                package_sha256: signed_manifest.manifest.package_sha256.clone(),
                manifest_sha256: "a".repeat(64),
            },
            last_known_good: None,
        };
        assert!(matches!(
            verifier.verify_manifest(&signed_manifest, &keyset, &policy, Some(&state), 1_000),
            Err(UpdateError::Downgrade)
        ));

        signed_manifest.manifest.files[0].path = "../outside.exe".into();
        assert!(matches!(
            canonical_manifest_bytes(&signed_manifest.manifest),
            Err(UpdateError::UnsafePath(_))
        ));
    }

    #[test]
    fn installs_verifies_promotes_and_rolls_back_only_to_verified_release() {
        let root_signer = key(9);
        let release_signer = key(7);
        let (signed_keyset, signed_first, verifier) =
            signed_release(&root_signer, &release_signer, 1, b"first");
        let policy = policy();
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        let root = temp_root("installer");
        let staged_first = root.join("staged-first");
        fs::create_dir_all(&staged_first).unwrap();
        fs::write(staged_first.join("hivemind-worker.exe"), b"first").unwrap();
        let installer = UpdateInstaller::new(root.join("installed")).unwrap();
        let first_state = installer
            .install_verified_directory(
                &staged_first,
                &signed_first,
                &keyset,
                &verifier,
                &policy,
                1_000,
            )
            .unwrap();
        assert!(first_state.last_known_good.is_none());
        installer
            .mark_current_verified(&signed_first, &keyset, &verifier, &policy, 1_000)
            .unwrap();

        let (signed_keyset, signed_second, verifier) =
            signed_release(&root_signer, &release_signer, 2, b"second");
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        let staged_second = root.join("staged-second");
        fs::create_dir_all(&staged_second).unwrap();
        fs::write(staged_second.join("hivemind-worker.exe"), b"second").unwrap();
        let second_state = installer
            .install_verified_directory(
                &staged_second,
                &signed_second,
                &keyset,
                &verifier,
                &policy,
                1_000,
            )
            .unwrap();
        assert_eq!(second_state.current.sequence, 2);
        assert_eq!(second_state.last_known_good.as_ref().unwrap().sequence, 1);

        let rolled_back = installer
            .rollback_verified(&signed_first, &keyset, &verifier, &policy, 1_000)
            .unwrap();
        assert_eq!(rolled_back.current.sequence, 1);
        assert_eq!(
            installer
                .verify_current(&signed_first, &keyset, &verifier, &policy, 1_000)
                .unwrap()
                .current
                .sequence,
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_verified_zip_and_rejects_unexpected_files() {
        let (root, destination, verified) = extract_test_archive(
            "archive-valid",
            &[
                ("hivemind-worker.exe", b"worker"),
                ("worker-ui/index.html", b"page"),
            ],
            &[
                ("hivemind-worker.exe", b"worker"),
                ("worker-ui/index.html", b"page"),
            ],
        );
        extract_verified_zip(
            &root.join("package.zip"),
            &destination,
            &verified,
            &policy(),
        )
        .unwrap();
        assert_eq!(
            fs::read(destination.join("hivemind-worker.exe")).unwrap(),
            b"worker"
        );
        assert_eq!(
            fs::read(destination.join("worker-ui/index.html")).unwrap(),
            b"page"
        );
        let _ = fs::remove_dir_all(root);

        let (root, destination, verified) = extract_test_archive(
            "archive-unexpected",
            &[
                ("hivemind-worker.exe", b"worker"),
                ("unexpected.dll", b"extra"),
            ],
            &[("hivemind-worker.exe", b"worker")],
        );
        assert!(matches!(
            extract_verified_zip(
                &root.join("package.zip"),
                &destination,
                &verified,
                &policy(),
            ),
            Err(UpdateError::PackageMismatch)
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_explicit_directories_and_rejects_manifest_mismatch() {
        let root = temp_root("archive-directory");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        write_zip_with_directory(&archive, "worker-ui/", &[("worker-ui/index.html", b"page")]);
        let verified = verified_archive(&archive, &[("worker-ui/index.html", b"page")]);
        let destination = root.join("release");
        extract_verified_zip(&archive, &destination, &verified, &policy()).unwrap();
        assert_eq!(
            fs::read(destination.join("worker-ui/index.html")).unwrap(),
            b"page"
        );
        let _ = fs::remove_dir_all(root);

        let (root, destination, verified) = extract_test_archive(
            "archive-mismatch",
            &[("hivemind-worker.exe", b"actual")],
            &[("hivemind-worker.exe", b"expected")],
        );
        assert!(matches!(
            extract_verified_zip(
                &root.join("package.zip"),
                &destination,
                &verified,
                &policy(),
            ),
            Err(UpdateError::PackageMismatch)
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unsafe_archive_paths_and_preserves_existing_destination() {
        for (index, name) in [
            ("parent", "../outside.exe"),
            ("absolute", "/outside.exe"),
            ("backslash", "worker\\\\outside.exe"),
            ("drive", "C:outside.exe"),
            ("dot", "worker/./outside.exe"),
            ("empty", "worker//outside.exe"),
        ] {
            let (root, destination, verified) =
                extract_test_archive(index, &[(name, b"bad")], &[("safe.exe", b"safe")]);
            assert!(matches!(
                extract_verified_zip(
                    &root.join("package.zip"),
                    &destination,
                    &verified,
                    &policy(),
                ),
                Err(UpdateError::UnsafePath(_))
            ));
            assert!(!destination.exists());
            let _ = fs::remove_dir_all(root);
        }

        let (root, destination, verified) = extract_test_archive(
            "archive-existing-destination",
            &[("hivemind-worker.exe", b"worker")],
            &[("hivemind-worker.exe", b"worker")],
        );
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("keep.txt"), b"keep").unwrap();
        assert!(matches!(
            extract_verified_zip(
                &root.join("package.zip"),
                &destination,
                &verified,
                &policy(),
            ),
            Err(UpdateError::UnsafePath(_))
        ));
        assert_eq!(fs::read(destination.join("keep.txt")).unwrap(), b"keep");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_duplicate_entries_and_file_directory_collisions() {
        let root = temp_root("archive-duplicate");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        write_zip(&archive, &[("hivemind-worker.exe", b"worker")]);
        duplicate_first_central_entry(&archive);
        let destination = root.join("release");
        let verified = verified_archive(&archive, &[("hivemind-worker.exe", b"worker")]);
        let result = extract_verified_zip(
            &root.join("package.zip"),
            &destination,
            &verified,
            &policy(),
        );
        assert!(result.is_err(), "duplicate archive result: {result:?}");
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);

        let (root, destination, verified) = extract_test_archive(
            "archive-collision",
            &[("foo", b"file"), ("foo/bar", b"nested")],
            &[("foo", b"file"), ("foo/bar", b"nested")],
        );
        assert!(matches!(
            extract_verified_zip(
                &root.join("package.zip"),
                &destination,
                &verified,
                &policy(),
            ),
            Err(UpdateError::InvalidArchive(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);

        let (root, destination, verified) = extract_test_archive(
            "archive-case-collision",
            &[("Worker.exe", b"one"), ("worker.exe", b"two")],
            &[("Worker.exe", b"one"), ("worker.exe", b"two")],
        );
        assert!(matches!(
            extract_verified_zip(
                &root.join("package.zip"),
                &destination,
                &verified,
                &policy(),
            ),
            Err(UpdateError::InvalidArchive(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_symlinks_special_modes_encryption_and_malformed_archives() {
        let root = temp_root("archive-symlink");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        write_zip_with_symlink(&archive, "hivemind-worker.exe", "outside.exe");
        let verified = verified_archive(&archive, &[("hivemind-worker.exe", b"outside.exe")]);
        let destination = root.join("release");
        assert!(matches!(
            extract_verified_zip(&archive, &destination, &verified, &policy()),
            Err(UpdateError::ReparsePoint(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);

        let root = temp_root("archive-special-mode");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        write_zip(&archive, &[("hivemind-worker.exe", b"worker")]);
        patch_zip_unix_mode(&archive, 0o020000);
        let verified = verified_archive(&archive, &[("hivemind-worker.exe", b"worker")]);
        let destination = root.join("release");
        assert!(matches!(
            extract_verified_zip(&archive, &destination, &verified, &policy()),
            Err(UpdateError::InvalidArchive(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);

        let root = temp_root("archive-encrypted");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        write_zip(&archive, &[("hivemind-worker.exe", b"worker")]);
        patch_zip_encrypted(&archive);
        let verified = verified_archive(&archive, &[("hivemind-worker.exe", b"worker")]);
        let destination = root.join("release");
        assert!(matches!(
            extract_verified_zip(&archive, &destination, &verified, &policy()),
            Err(UpdateError::InvalidArchive(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);

        let root = temp_root("archive-malformed");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        fs::write(&archive, b"not a zip archive").unwrap();
        let verified = verified_archive(&archive, &[("hivemind-worker.exe", b"worker")]);
        let destination = root.join("release");
        assert!(matches!(
            extract_verified_zip(&archive, &destination, &verified, &policy()),
            Err(UpdateError::InvalidArchive(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_archive_entry_count_and_expanded_size_limits() {
        let root = temp_root("archive-entry-limit");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        write_zip_with_entry_count(&archive, UPDATE_MAX_ARCHIVE_ENTRIES + 1);
        let verified = verified_archive(&archive, &[("hivemind-worker.exe", b"worker")]);
        let destination = root.join("release");
        assert!(matches!(
            extract_verified_zip(&archive, &destination, &verified, &policy()),
            Err(UpdateError::InvalidArchive(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);

        let root = temp_root("archive-forged-entry-count");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        write_zip(&archive, &[("hivemind-worker.exe", b"worker")]);
        patch_zip_entry_count(&archive, u16::MAX);
        let verified = verified_archive(&archive, &[("hivemind-worker.exe", b"worker")]);
        let destination = root.join("release");
        assert!(matches!(
            extract_verified_zip(&archive, &destination, &verified, &policy()),
            Err(UpdateError::InvalidArchive(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);

        let root = temp_root("archive-expanded-limit");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.zip");
        let content = vec![b'x'; 16 * 1024];
        write_zip(&archive, &[("hivemind-worker.exe", &content)]);
        let verified = verified_archive(&archive, &[("hivemind-worker.exe", &content)]);
        let limit = fs::metadata(&archive).unwrap().len() + 1;
        let limited_policy = policy().with_max_package_bytes(limit);
        let destination = root.join("release");
        assert!(matches!(
            extract_verified_zip(&archive, &destination, &verified, &limited_policy),
            Err(UpdateError::InvalidArchive(_))
        ));
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_request_rejects_malformed_handoffs() {
        let program = "hivemind-worker".to_string();
        let flag = "--hivemind-apply-update".to_string();
        let absolute = std::env::temp_dir().join("descriptor.json");
        let absolute = absolute.display().to_string();

        assert_eq!(activation_request_path(&[]).unwrap(), None);
        assert_eq!(
            activation_request_path(std::slice::from_ref(&program)).unwrap(),
            None
        );
        assert_eq!(
            activation_request_path(&[program.clone(), flag.clone(), absolute.clone()])
                .unwrap()
                .unwrap(),
            PathBuf::from(absolute.clone())
        );
        assert!(matches!(
            activation_request_path(&[
                program.clone(),
                flag.clone(),
                "relative/descriptor.json".into()
            ]),
            Err(UpdateError::UnsafePath(_))
        ));
        assert!(matches!(
            activation_request_path(&[
                program.clone(),
                flag.clone(),
                absolute.clone(),
                "unexpected".into()
            ]),
            Err(UpdateError::ActivationFailed(_))
        ));
        assert_eq!(
            activation_request_path(&[program, "worker".into(), absolute]).unwrap(),
            None
        );
    }

    #[test]
    fn activation_service_arguments_fail_closed() {
        assert!(validate_service_arguments(&[String::new()]).is_err());
        assert!(validate_service_arguments(&["--hivemind-apply-update".into()]).is_err());
        assert!(validate_service_arguments(&["contains\0nul".into()]).is_err());
        assert!(validate_service_arguments(&["contains\ncontrol".into()]).is_err());
        assert!(validate_service_arguments(&["x".repeat(4097)]).is_err());
        assert!(validate_service_arguments(&vec!["x".into(); 33]).is_err());
        assert!(validate_service_arguments(&["x".repeat(16 * 1024)]).is_err());
        validate_service_arguments(&["worker".into(), "--config".into()]).unwrap();
    }

    #[test]
    fn activation_descriptor_rejects_outside_paths_and_helper_tampering() {
        let root = temp_root("activation-descriptor-validation");
        fs::create_dir_all(&root).unwrap();
        let helper = root
            .join(UPDATE_ACTIVATION_DESCRIPTOR_DIR)
            .join("helper.exe");
        let (descriptor_path, mut descriptor) = write_test_activation_descriptor(&root, &helper);
        validate_activation_descriptor(&descriptor_path, &descriptor).unwrap();

        let outside_descriptor = root.join("outside").join(UPDATE_ACTIVATION_DESCRIPTOR_FILE);
        write_activation_descriptor_atomic(&outside_descriptor, &descriptor).unwrap();
        assert!(matches!(
            validate_activation_descriptor(&outside_descriptor, &descriptor),
            Err(UpdateError::ActivationFailed(_))
        ));

        descriptor.helper_executable = root.join("outside-helper.exe").display().to_string();
        fs::write(&descriptor.helper_executable, b"outside helper").unwrap();
        write_activation_descriptor_atomic(&descriptor_path, &descriptor).unwrap();
        assert!(matches!(
            validate_activation_descriptor(&descriptor_path, &descriptor),
            Err(UpdateError::ActivationFailed(_))
        ));

        let (descriptor_path, descriptor) = write_test_activation_descriptor(&root, &helper);
        fs::write(&helper, b"tampered updater helper").unwrap();
        assert!(matches!(
            activation_launch_spec(&descriptor_path),
            Err(UpdateError::ActivationFailed(_))
        ));
        assert_eq!(descriptor.service_arguments, vec!["worker".to_string()]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pending_activation_is_bounded_and_discoverable() {
        let root = temp_root("pending-activation");
        let installer = UpdateInstaller::new(&root).unwrap();
        assert_eq!(installer.pending_activation_path().unwrap(), None);

        let helper = root
            .join(UPDATE_ACTIVATION_DESCRIPTOR_DIR)
            .join("helper.exe");
        let (descriptor_path, _) = write_test_activation_descriptor(&root, &helper);
        assert_eq!(
            installer.pending_activation_path().unwrap(),
            Some(descriptor_path)
        );

        fs::write(
            root.join(UPDATE_ACTIVATION_DESCRIPTOR_DIR)
                .join(UPDATE_ACTIVATION_DESCRIPTOR_FILE),
            vec![b'x'; UPDATE_ACTIVATION_DESCRIPTOR_MAX_BYTES + 1],
        )
        .unwrap();
        assert!(matches!(
            installer.pending_activation_path(),
            Err(UpdateError::CorruptState(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_prepare_records_verified_helper_and_service_arguments() {
        let root = temp_root("activation-prepare");
        fs::create_dir_all(&root).unwrap();
        let install_root = root.join("installed");
        let active_root = root.join("active");
        let staged = root.join("staged");
        fs::create_dir_all(&staged).unwrap();
        fs::create_dir_all(&active_root).unwrap();
        fs::write(staged.join("hivemind-worker.exe"), b"worker").unwrap();

        let root_signer = key(9);
        let release_signer = key(7);
        let (signed_keyset, signed_manifest, verifier) =
            signed_release(&root_signer, &release_signer, 1, b"worker");
        let policy = policy();
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        let installer = UpdateInstaller::new(&install_root).unwrap();
        installer
            .install_verified_directory(
                &staged,
                &signed_manifest,
                &keyset,
                &verifier,
                &policy,
                1_000,
            )
            .unwrap();
        let descriptor_dir = install_root.join(UPDATE_ACTIVATION_DESCRIPTOR_DIR);
        fs::create_dir_all(&descriptor_dir).unwrap();
        let helper = descriptor_dir.join("helper.exe");
        fs::write(&helper, b"helper").unwrap();
        let service_arguments = vec!["worker".into(), "--config".into(), "worker.env".into()];
        let descriptor_path = installer
            .prepare_activation(
                &signed_keyset,
                &signed_manifest,
                &verifier,
                &policy,
                1_000,
                &active_root,
                "hivemind-worker.exe",
                &service_arguments,
                &helper,
                1234,
            )
            .unwrap();
        let descriptor = read_activation_descriptor(&descriptor_path).unwrap();
        assert_eq!(descriptor.service_arguments, service_arguments);
        assert_eq!(
            descriptor.release_dir,
            "release-1-".to_string() + &signed_manifest.manifest.package_sha256[..16]
        );
        let (launch_helper, launch_arguments) = activation_launch_spec(&descriptor_path).unwrap();
        assert_eq!(launch_helper, helper);
        assert_eq!(launch_arguments, service_arguments);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_prepare_rejects_state_mismatch() {
        let root = temp_root("activation-state-mismatch");
        fs::create_dir_all(&root).unwrap();
        let install_root = root.join("installed");
        let active_root = root.join("active");
        let staged = root.join("staged");
        fs::create_dir_all(&staged).unwrap();
        fs::create_dir_all(&active_root).unwrap();
        fs::write(staged.join("hivemind-worker.exe"), b"worker").unwrap();

        let root_signer = key(9);
        let release_signer = key(7);
        let (first_keyset, first_manifest, first_verifier) =
            signed_release(&root_signer, &release_signer, 1, b"worker");
        let policy = policy();
        let first_keyset_verified = first_verifier
            .verify_keyset(&first_keyset, &policy, 1_000)
            .unwrap();
        let installer = UpdateInstaller::new(&install_root).unwrap();
        let state = installer
            .install_verified_directory(
                &staged,
                &first_manifest,
                &first_keyset_verified,
                &first_verifier,
                &policy,
                1_000,
            )
            .unwrap();
        let helper = install_root
            .join(UPDATE_ACTIVATION_DESCRIPTOR_DIR)
            .join("helper.exe");
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::write(&helper, b"helper").unwrap();

        let (second_keyset, second_manifest, second_verifier) =
            signed_release(&root_signer, &release_signer, 2, b"worker");
        let result = installer.prepare_activation(
            &second_keyset,
            &second_manifest,
            &second_verifier,
            &policy,
            1_000,
            &active_root,
            "hivemind-worker.exe",
            &[],
            &helper,
            1234,
        );
        assert!(
            matches!(result, Err(UpdateError::CorruptState(_))),
            "{result:?}"
        );
        assert_eq!(state.current.sequence, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_prepare_rejects_release_without_service_executable() {
        let root = temp_root("activation-missing-service");
        fs::create_dir_all(&root).unwrap();
        let install_root = root.join("installed");
        let active_root = root.join("active");
        let staged = root.join("staged");
        fs::create_dir_all(&staged).unwrap();
        fs::create_dir_all(&active_root).unwrap();
        fs::write(staged.join("other.exe"), b"worker").unwrap();

        let root_signer = key(9);
        let release_signer = key(7);
        let (signed_keyset, mut signed_manifest, verifier) =
            signed_release(&root_signer, &release_signer, 1, b"worker");
        signed_manifest.manifest.files[0].path = "other.exe".into();
        signed_manifest.signature = sign(
            &release_signer,
            &canonical_manifest_bytes(&signed_manifest.manifest).unwrap(),
        );
        let policy = policy();
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        let installer = UpdateInstaller::new(&install_root).unwrap();
        let state = installer
            .install_verified_directory(
                &staged,
                &signed_manifest,
                &keyset,
                &verifier,
                &policy,
                1_000,
            )
            .unwrap();
        let descriptor_dir = install_root.join(UPDATE_ACTIVATION_DESCRIPTOR_DIR);
        fs::create_dir_all(&descriptor_dir).unwrap();
        let helper = descriptor_dir.join("helper.exe");
        fs::write(&helper, b"helper").unwrap();
        let result = installer.prepare_activation(
            &signed_keyset,
            &signed_manifest,
            &verifier,
            &policy,
            1_000,
            &active_root,
            "hivemind-worker.exe",
            &[],
            &helper,
            1234,
        );
        assert!(
            matches!(result, Err(UpdateError::ActivationFailed(_))),
            "{result:?}"
        );
        assert_eq!(state.current.sequence, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn activation_file_replacement_rolls_back_on_later_failure() {
        let root = temp_root("activation-replacement-rollback");
        let release_root = root.join("release");
        let active_root = root.join("active");
        fs::create_dir_all(&release_root).unwrap();
        fs::create_dir_all(&active_root).unwrap();
        fs::write(release_root.join("first.dll"), b"new first").unwrap();
        fs::write(active_root.join("first.dll"), b"old first").unwrap();
        let verified = VerifiedReleaseManifest {
            manifest: ReleaseManifest {
                schema_version: UPDATE_PROTOCOL_VERSION,
                product: "hivemind-windows-worker".into(),
                channel: "stable".into(),
                platform: "windows".into(),
                architecture: "x86_64".into(),
                version: "0.1.1".into(),
                sequence: 1,
                minimum_supported_version: "0.1.0".into(),
                release_key_id: "release-1".into(),
                issued_at_unix: 900,
                expires_at_unix: 2_000,
                package_url: "https://updates.example.test/worker.zip".into(),
                package_size: 1,
                package_sha256: "a".repeat(64),
                files: vec![
                    ReleaseFile {
                        path: "first.dll".into(),
                        size: 9,
                        sha256: hex::encode(Sha256::digest(b"new first")),
                    },
                    ReleaseFile {
                        path: "missing.dll".into(),
                        size: 7,
                        sha256: hex::encode(Sha256::digest(b"missing")),
                    },
                ],
            },
            manifest_sha256: "b".repeat(64),
        };
        assert!(apply_release_files(&release_root, &active_root, &verified).is_err());
        assert_eq!(
            fs::read(active_root.join("first.dll")).unwrap(),
            b"old first"
        );
        let backup_left = fs::read_dir(&active_root)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".hivemind-activation-backup-")
            });
        assert!(!backup_left);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn activation_rolls_back_when_replacement_process_cannot_start() {
        let root = temp_root("activation-process-start-rollback");
        let release_root = root.join("release");
        let active_root = root.join("active");
        fs::create_dir_all(&release_root).unwrap();
        fs::create_dir_all(&active_root).unwrap();
        let service_path = active_root.join("hivemind-worker.exe");
        fs::write(release_root.join("hivemind-worker.exe"), b"new client").unwrap();
        fs::write(&service_path, b"old client").unwrap();
        let verified = VerifiedReleaseManifest {
            manifest: ReleaseManifest {
                schema_version: UPDATE_PROTOCOL_VERSION,
                product: "hivemind-windows-worker".into(),
                channel: "stable".into(),
                platform: "windows".into(),
                architecture: "x86_64".into(),
                version: "0.1.1".into(),
                sequence: 1,
                minimum_supported_version: "0.1.0".into(),
                release_key_id: "release-1".into(),
                issued_at_unix: 900,
                expires_at_unix: 2_000,
                package_url: "https://updates.example.test/worker.zip".into(),
                package_size: 1,
                package_sha256: "a".repeat(64),
                files: vec![ReleaseFile {
                    path: "hivemind-worker.exe".into(),
                    size: 10,
                    sha256: hex::encode(Sha256::digest(b"new client")),
                }],
            },
            manifest_sha256: "b".repeat(64),
        };
        let backup_dir = apply_release_files(&release_root, &active_root, &verified).unwrap();
        let result =
            spawn_activated_service(&service_path, &active_root, &[], &root.join("helper.exe"));
        assert!(
            matches!(result, Err(UpdateError::ActivationFailed(_))),
            "{result:?}"
        );
        rollback_activation_files(&active_root, &backup_dir, &verified).unwrap();
        assert_eq!(fs::read(&service_path).unwrap(), b"old client");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn activation_is_rejected_off_windows() {
        assert!(matches!(
            run_activation_helper(PathBuf::from("/tmp/descriptor.json")).await,
            Err(UpdateError::UnsupportedPlatform)
        ));
        assert!(matches!(
            spawn_activation_helper(Path::new("/tmp/descriptor.json")),
            Err(UpdateError::UnsupportedPlatform)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn installed_package_rejects_symlink_entries() {
        let root_signer = key(9);
        let release_signer = key(7);
        let (signed_keyset, signed_manifest, verifier) =
            signed_release(&root_signer, &release_signer, 1, b"worker");
        let policy = policy();
        let keyset = verifier
            .verify_keyset(&signed_keyset, &policy, 1_000)
            .unwrap();
        let root = temp_root("symlink");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("outside");
        fs::write(&target, b"worker").unwrap();
        std::os::unix::fs::symlink(&target, root.join("hivemind-worker.exe")).unwrap();
        let verified = verifier
            .verify_manifest(&signed_manifest, &keyset, &policy, None, 1_000)
            .unwrap();
        assert!(matches!(
            verify_installed_package(&root, &verified),
            Err(UpdateError::ReparsePoint(_))
        ));
        let _ = fs::remove_dir_all(root);
    }
}

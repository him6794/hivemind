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
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const UPDATE_PROTOCOL_VERSION: u32 = 1;
pub const UPDATE_MANIFEST_MAX_BYTES: usize = 256 * 1024;
pub const UPDATE_KEYSET_MAX_BYTES: usize = 64 * 1024;
pub const UPDATE_MAX_FILES: usize = 4096;
pub const UPDATE_MAX_KEYS: usize = 64;
pub const UPDATE_MAX_PACKAGE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const UPDATE_MAX_FIELD_BYTES: usize = 512;
const UPDATE_MAX_STATE_BYTES: usize = 64 * 1024;
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
}

impl From<io::Error> for UpdateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    for file in files {
        validate_package_path(&file.path)?;
        validate_bounded_field(&file.sha256, "file hash")?;
        let _ = decode_fixed_hex::<UPDATE_HASH_BYTES>(&file.sha256, "file hash")?;
        if file.size == 0 || !paths.insert(file.path.clone()) {
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

fn validate_https_package_url(
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
            .ok_or_else(|| UpdateError::PackageMismatch)?;
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

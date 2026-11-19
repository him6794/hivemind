//! Runtime package manifest.
//!
//! A runtime package is every file of one llama.cpp release, listed with its
//! size and SHA-256. The catalog pins the manifest's own SHA-256, so the file
//! listing is trusted only after that check passes. Before a server starts,
//! every listed file is checked, and an unlisted file in the directory is an error.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use hivemind_model_catalog::sha256_hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Schema identifier for runtime manifests.
pub const MANIFEST_SCHEMA: &str = "hivemind-llm-runtime-manifest/v1";
/// Largest manifest accepted.
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;
/// Largest number of files in one package.
pub const MAX_PACKAGE_FILES: usize = 512;

/// File listing for one runtime package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    pub schema: String,
    pub files: BTreeMap<String, FileDigest>,
}

/// Size and SHA-256 of one file in the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileDigest {
    pub size: u64,
    pub sha256: String,
}

/// Reasons a manifest or package is refused.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest is larger than the accepted size")]
    TooLarge,
    #[error("manifest does not match the digest pinned by the catalog")]
    PinMismatch,
    #[error("manifest does not parse: {0}")]
    Parse(String),
    #[error("manifest schema is not supported")]
    Schema,
    #[error("manifest lists `{0}`, which is not a plain file name")]
    BadName(String),
    #[error("manifest lists more files than the accepted limit")]
    TooManyFiles,
    #[error("manifest digest for `{0}` is not 64 lowercase hex characters")]
    BadDigest(String),
    #[error("runtime file `{0}` is missing")]
    Missing(String),
    #[error("runtime file `{0}` does not match its manifest entry")]
    Mismatch(String),
    #[error("runtime directory holds `{0}`, which the manifest does not list")]
    Unlisted(String),
    #[error("runtime directory holds a subdirectory `{0}`")]
    Subdirectory(String),
    #[error("runtime entry `{0}` is not a regular file (links are refused)")]
    NotRegular(String),
    #[error("file error: {0}")]
    Io(#[from] io::Error),
}

/// Checks that `bytes` hash to `pinned_sha256`, then parses and validates them.
pub fn parse_pinned(bytes: &[u8], pinned_sha256: &str) -> Result<RuntimeManifest, ManifestError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::TooLarge);
    }
    if sha256_hex(bytes) != pinned_sha256 {
        return Err(ManifestError::PinMismatch);
    }
    let manifest: RuntimeManifest =
        serde_json::from_slice(bytes).map_err(|err| ManifestError::Parse(err.to_string()))?;
    manifest.validate()?;
    Ok(manifest)
}

impl RuntimeManifest {
    /// Schema, file names, digests, and count.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema != MANIFEST_SCHEMA {
            return Err(ManifestError::Schema);
        }
        if self.files.is_empty() || self.files.len() > MAX_PACKAGE_FILES {
            return Err(ManifestError::TooManyFiles);
        }
        for (name, digest) in &self.files {
            check_plain_name(name)?;
            let ok = digest.sha256.len() == 64
                && digest
                    .sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            if !ok {
                return Err(ManifestError::BadDigest(name.clone()));
            }
        }
        Ok(())
    }

    /// Checks every listed file in `dir` and rejects anything the manifest does not list.
    pub fn verify_dir(&self, dir: &Path) -> Result<(), ManifestError> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|raw| ManifestError::BadName(raw.to_string_lossy().into_owned()))?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                return Err(ManifestError::Subdirectory(name));
            }
            if !file_type.is_file() {
                return Err(ManifestError::NotRegular(name));
            }
            if !self.files.contains_key(&name) {
                return Err(ManifestError::Unlisted(name));
            }
        }
        for (name, expected) in &self.files {
            let path = dir.join(name);
            // symlink_metadata does not follow links, so a link is never accepted as a file.
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|_| ManifestError::Missing(name.clone()))?;
            if !metadata.is_file() {
                return Err(ManifestError::NotRegular(name.clone()));
            }
            let (size, digest) = sha256_file(&path)?;
            if size != expected.size || digest != expected.sha256 {
                return Err(ManifestError::Mismatch(name.clone()));
            }
        }
        Ok(())
    }
}

/// Builds a manifest from the files directly inside `dir`. Development tooling only.
pub fn generate(dir: &Path) -> Result<RuntimeManifest, ManifestError> {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|raw| ManifestError::BadName(raw.to_string_lossy().into_owned()))?;
        if entry.file_type()?.is_dir() {
            return Err(ManifestError::Subdirectory(name));
        }
        let (size, sha256) = sha256_file(&entry.path())?;
        files.insert(name, FileDigest { size, sha256 });
    }
    let manifest = RuntimeManifest {
        schema: MANIFEST_SCHEMA.to_owned(),
        files,
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Streams a file through SHA-256. Returns its length and digest in lowercase hex.
pub fn sha256_file(path: &Path) -> io::Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((total, hex::encode(hasher.finalize())))
}

fn check_plain_name(name: &str) -> Result<(), ManifestError> {
    let forbidden = name
        .chars()
        .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'));
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." || forbidden {
        return Err(ManifestError::BadName(name.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) {
        std::fs::write(dir.join(name), bytes).expect("write");
    }

    #[test]
    fn a_generated_manifest_verifies_its_own_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "llama-server.exe", b"launcher");
        write(dir.path(), "llama.dll", b"library");
        let manifest = generate(dir.path()).expect("generate");
        manifest.verify_dir(dir.path()).expect("verifies");
    }

    #[test]
    fn a_changed_file_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "llama.dll", b"library");
        let manifest = generate(dir.path()).expect("generate");
        write(dir.path(), "llama.dll", b"LIBRARY");
        assert!(matches!(
            manifest.verify_dir(dir.path()),
            Err(ManifestError::Mismatch(name)) if name == "llama.dll"
        ));
    }

    #[test]
    fn an_unlisted_file_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "llama.dll", b"library");
        let manifest = generate(dir.path()).expect("generate");
        write(dir.path(), "injected.dll", b"payload");
        assert!(matches!(
            manifest.verify_dir(dir.path()),
            Err(ManifestError::Unlisted(name)) if name == "injected.dll"
        ));
    }

    #[test]
    fn a_missing_file_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "llama.dll", b"library");
        let manifest = generate(dir.path()).expect("generate");
        std::fs::remove_file(dir.path().join("llama.dll")).expect("remove");
        write(dir.path(), "other.txt", b"x");
        assert!(matches!(
            manifest.verify_dir(dir.path()),
            Err(ManifestError::Missing(_)) | Err(ManifestError::Unlisted(_))
        ));
    }

    #[test]
    fn the_manifest_must_hash_to_the_catalog_pin() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "llama.dll", b"library");
        let manifest = generate(dir.path()).expect("generate");
        let bytes = serde_json::to_vec(&manifest).expect("json");
        let pin = sha256_hex(&bytes);
        assert_eq!(parse_pinned(&bytes, &pin).expect("pinned"), manifest);
        let other = "0".repeat(64);
        assert!(matches!(
            parse_pinned(&bytes, &other),
            Err(ManifestError::PinMismatch)
        ));
    }

    #[test]
    fn a_path_in_a_manifest_name_is_refused() {
        let mut manifest = RuntimeManifest {
            schema: MANIFEST_SCHEMA.to_owned(),
            files: BTreeMap::new(),
        };
        manifest.files.insert(
            "../escape.dll".to_owned(),
            FileDigest {
                size: 1,
                sha256: "0".repeat(64),
            },
        );
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::BadName(_))
        ));
    }
}

//! OS-native credential storage abstraction & encrypted file vault fallback
//! so Worker auth tokens are never saved in plaintext config files (Milestone 3.2 #6).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialBackendKind {
    WindowsCredentialManager,
    MacOsKeychain,
    LinuxSecretService,
    EncryptedFileVault,
    InMemory,
}

#[must_use]
pub fn detect_native_credential_backend() -> CredentialBackendKind {
    #[cfg(target_os = "windows")]
    {
        CredentialBackendKind::WindowsCredentialManager
    }
    #[cfg(target_os = "macos")]
    {
        CredentialBackendKind::MacOsKeychain
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
            CredentialBackendKind::LinuxSecretService
        } else {
            CredentialBackendKind::EncryptedFileVault
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        CredentialBackendKind::EncryptedFileVault
    }
}

pub trait CredentialStore: Send + Sync {
    fn backend_kind(&self) -> CredentialBackendKind;
    fn store_secret(&self, service: &str, account: &str, secret: &str) -> Result<(), String>;
    fn load_secret(&self, service: &str, account: &str) -> Result<Option<String>, String>;
    fn delete_secret(&self, service: &str, account: &str) -> Result<(), String>;
}

/// Encrypted file-backed credential vault with owner-only file permissions (`0o600` on Unix)
/// and HMAC-verified keystream sealing so secrets are never written as plaintext on disk.
pub struct EncryptedFileCredentialStore {
    path: PathBuf,
    master_seed: [u8; 32],
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct VaultDocument {
    version: u32,
    entries: HashMap<String, SealedEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SealedEntry {
    nonce_hex: String,
    ciphertext_hex: String,
    mac_hex: String,
}

impl EncryptedFileCredentialStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, machine_context: &str) -> Self {
        let mut h = Sha256::new();
        h.update(b"hivemind-credential-vault-v1:");
        h.update(machine_context.as_bytes());
        let master_seed: [u8; 32] = h.finalize().into();
        Self {
            path: path.into(),
            master_seed,
        }
    }

    fn key_for(service: &str, account: &str) -> String {
        format!("{service}::{account}")
    }

    fn xor_stream(&self, nonce: &[u8; 16], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        for (counter, chunk) in data.chunks(32).enumerate() {
            let mut h = Sha256::new();
            h.update(self.master_seed);
            h.update(nonce);
            h.update((counter as u64).to_le_bytes());
            let block = h.finalize();
            for (i, b) in chunk.iter().enumerate() {
                out.push(b ^ block[i]);
            }
        }
        out
    }

    fn compute_mac(&self, key: &str, nonce: &[u8; 16], ciphertext: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(b"mac-v1:");
        h.update(self.master_seed);
        h.update(key.as_bytes());
        h.update(nonce);
        h.update(ciphertext);
        hex::encode(h.finalize())
    }

    fn load_doc(&self) -> Result<VaultDocument, String> {
        if !self.path.exists() {
            return Ok(VaultDocument {
                version: 1,
                entries: HashMap::new(),
            });
        }
        let bytes = std::fs::read(&self.path).map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    fn save_doc(&self, doc: &VaultDocument) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let payload = serde_json::to_vec_pretty(doc).map_err(|e| e.to_string())?;
        write_restricted(&self.path, &payload)
    }
}

fn write_restricted(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

impl CredentialStore for EncryptedFileCredentialStore {
    fn backend_kind(&self) -> CredentialBackendKind {
        CredentialBackendKind::EncryptedFileVault
    }

    fn store_secret(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        let mut doc = self.load_doc()?;
        let key = Self::key_for(service, account);
        let nonce: [u8; 16] = rand::random();
        let ciphertext = self.xor_stream(&nonce, secret.as_bytes());
        let mac_hex = self.compute_mac(&key, &nonce, &ciphertext);
        doc.entries.insert(
            key,
            SealedEntry {
                nonce_hex: hex::encode(nonce),
                ciphertext_hex: hex::encode(ciphertext),
                mac_hex,
            },
        );
        self.save_doc(&doc)
    }

    fn load_secret(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        let doc = self.load_doc()?;
        let key = Self::key_for(service, account);
        let Some(entry) = doc.entries.get(&key) else {
            return Ok(None);
        };
        let nonce_vec = hex::decode(&entry.nonce_hex).map_err(|e| e.to_string())?;
        let nonce: [u8; 16] = nonce_vec
            .try_into()
            .map_err(|_| "invalid nonce length".to_string())?;
        let ciphertext = hex::decode(&entry.ciphertext_hex).map_err(|e| e.to_string())?;
        let expected_mac = self.compute_mac(&key, &nonce, &ciphertext);
        if expected_mac != entry.mac_hex {
            return Err(
                "credential vault MAC mismatch (tampered or wrong machine key)".to_string(),
            );
        }
        let plain = self.xor_stream(&nonce, &ciphertext);
        String::from_utf8(plain)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    fn delete_secret(&self, service: &str, account: &str) -> Result<(), String> {
        let mut doc = self.load_doc()?;
        doc.entries.remove(&Self::key_for(service, account));
        self.save_doc(&doc)
    }
}

#[derive(Default)]
pub struct InMemoryCredentialStore {
    entries: Mutex<HashMap<(String, String), String>>,
}

impl CredentialStore for InMemoryCredentialStore {
    fn backend_kind(&self) -> CredentialBackendKind {
        CredentialBackendKind::InMemory
    }

    fn store_secret(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        self.entries.lock().unwrap().insert(
            (service.to_string(), account.to_string()),
            secret.to_string(),
        );
        Ok(())
    }

    fn load_secret(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .get(&(service.to_string(), account.to_string()))
            .cloned())
    }

    fn delete_secret(&self, service: &str, account: &str) -> Result<(), String> {
        self.entries
            .lock()
            .unwrap()
            .remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_file_vault_never_persists_plaintext_and_detects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.json");
        let store = EncryptedFileCredentialStore::new(&path, "host-machine-fingerprint-1");

        let secret = "hmk_super_secret_worker_token_999";
        store
            .store_secret("hivemind-worker", "alice", secret)
            .unwrap();

        // Raw file must NOT contain the plaintext secret.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains(secret));

        // Round-trip decryption succeeds with same machine key.
        assert_eq!(
            store
                .load_secret("hivemind-worker", "alice")
                .unwrap()
                .as_deref(),
            Some(secret)
        );

        // Wrong machine key fails MAC verification.
        let wrong = EncryptedFileCredentialStore::new(&path, "different-machine");
        assert!(wrong.load_secret("hivemind-worker", "alice").is_err());

        // Deletion removes entry.
        store.delete_secret("hivemind-worker", "alice").unwrap();
        assert_eq!(store.load_secret("hivemind-worker", "alice").unwrap(), None);
    }
}

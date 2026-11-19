//! Local Content-Addressed Storage (CAS) cache manager with capacity cap,
//! LRU eviction, active-digest pinning, and corruption quarantine (Milestone 6.3 #2).

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasGcReport {
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub evicted_digests: Vec<String>,
    pub quarantined_digests: Vec<String>,
}

pub struct LocalCasCache {
    root: PathBuf,
    quarantine_dir: PathBuf,
    max_bytes: u64,
    last_access_ns: HashMap<String, u128>,
    pinned: HashSet<String>,
}

impl LocalCasCache {
    pub fn open(root: impl Into<PathBuf>, max_bytes: u64) -> std::io::Result<Self> {
        let root = root.into();
        let quarantine_dir = root.join("quarantine");
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(&quarantine_dir)?;
        Ok(Self {
            root,
            quarantine_dir,
            max_bytes,
            last_access_ns: HashMap::new(),
            pinned: HashSet::new(),
        })
    }

    fn now_ns() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    #[must_use]
    pub fn blob_path(&self, sha256_hex: &str) -> PathBuf {
        self.root.join(format!("{sha256_hex}.blob"))
    }

    pub fn put(&mut self, bytes: &[u8]) -> std::io::Result<String> {
        let digest = hex::encode(Sha256::digest(bytes));
        let path = self.blob_path(&digest);
        if !path.exists() {
            std::fs::write(&path, bytes)?;
        }
        self.touch(&digest);
        Ok(digest)
    }

    pub fn touch(&mut self, digest: &str) {
        let next = self
            .last_access_ns
            .values()
            .copied()
            .max()
            .unwrap_or(0)
            .max(Self::now_ns())
            + 1;
        self.last_access_ns.insert(digest.to_string(), next);
    }

    pub fn pin(&mut self, digest: &str) {
        self.pinned.insert(digest.to_string());
    }

    pub fn unpin(&mut self, digest: &str) {
        self.pinned.remove(digest);
    }

    /// Read a blob and verify its SHA-256 integrity; automatically quarantines corrupted blobs.
    pub fn get_verified(&mut self, digest: &str) -> std::io::Result<Option<Vec<u8>>> {
        let path = self.blob_path(digest);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        let actual = hex::encode(Sha256::digest(&bytes));
        if actual != digest {
            self.quarantine_blob(digest, &path)?;
            return Ok(None);
        }
        self.touch(digest);
        Ok(Some(bytes))
    }

    fn quarantine_blob(&mut self, digest: &str, path: &Path) -> std::io::Result<()> {
        let dest = self.quarantine_dir.join(format!("{digest}.corrupt"));
        let _ = std::fs::rename(path, dest);
        self.last_access_ns.remove(digest);
        Ok(())
    }

    /// Verify all cached blobs (quarantining corrupt files) and evict oldest unpinned
    /// entries until total disk usage is within `max_bytes`.
    pub fn run_gc(&mut self) -> std::io::Result<CasGcReport> {
        let mut entries: Vec<(String, PathBuf, u64, u128)> = Vec::new();
        let mut quarantined_digests = Vec::new();
        let mut bytes_before = 0u64;

        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(digest) = name.strip_suffix(".blob") else {
                continue;
            };
            let bytes = std::fs::read(&path)?;
            let size = bytes.len() as u64;
            bytes_before += size;
            let actual = hex::encode(Sha256::digest(&bytes));
            if actual != digest {
                self.quarantine_blob(digest, &path)?;
                quarantined_digests.push(digest.to_string());
                continue;
            }
            let lru = *self.last_access_ns.get(digest).unwrap_or(&0);
            entries.push((digest.to_string(), path, size, lru));
        }

        let mut total_bytes: u64 = entries.iter().map(|e| e.2).sum();
        // Sort oldest LRU first.
        entries.sort_by_key(|e| e.3);

        let mut evicted_digests = Vec::new();
        for (digest, path, size, _) in entries {
            if total_bytes <= self.max_bytes {
                break;
            }
            if self.pinned.contains(&digest) {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                total_bytes = total_bytes.saturating_sub(size);
                self.last_access_ns.remove(&digest);
                evicted_digests.push(digest);
            }
        }

        Ok(CasGcReport {
            bytes_before,
            bytes_after: total_bytes,
            evicted_digests,
            quarantined_digests,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cas_gc_evicts_lru_unpinned_and_quarantines_corrupt_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let mut cas = LocalCasCache::open(dir.path(), 15).unwrap();

        let d1 = cas.put(b"aaaaaaaaaa").unwrap(); // 10 bytes (oldest)
        let d2 = cas.put(b"bbbbbbbbbb").unwrap(); // 10 bytes (middle)
        let d3 = cas.put(b"ccccc").unwrap(); // 5 bytes (newest)
        cas.pin(&d1); // pin oldest so d2 must be evicted instead

        // Corrupt d3 on disk.
        std::fs::write(cas.blob_path(&d3), b"CORRUPTED").unwrap();

        let report = cas.run_gc().unwrap();
        assert_eq!(report.quarantined_digests, vec![d3.clone()]);
        assert_eq!(report.evicted_digests, vec![d2.clone()]);
        assert!(report.bytes_after <= 15);
        assert!(cas.get_verified(&d1).unwrap().is_some());
        assert!(cas.get_verified(&d2).unwrap().is_none());
        assert!(cas.get_verified(&d3).unwrap().is_none());
        assert!(dir
            .path()
            .join("quarantine")
            .join(format!("{d3}.corrupt"))
            .exists());
    }
}

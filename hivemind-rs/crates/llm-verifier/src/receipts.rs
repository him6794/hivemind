//! Ed25519-signed `model-swarm` upload contribution receipts with distinct-account
//! verification, time windows, and per-pair anti-farming caps.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadReceipt {
    pub receipt_id: String,
    pub infohash: String,
    pub uploader_worker_id: String,
    pub uploader_owner: String,
    pub downloader_worker_id: String,
    pub downloader_owner: String,
    pub bytes_verified: u64,
    /// Coarse time bucket (e.g. UTC hour `unix_secs / 3600`).
    pub window_bucket: i64,
    /// Hex-encoded Ed25519 public key of the downloader.
    pub downloader_public_key_hex: String,
    /// Hex-encoded Ed25519 signature over `canonical_payload()`.
    pub signature_hex: String,
}

impl UploadReceipt {
    #[must_use]
    pub fn canonical_payload(&self) -> Vec<u8> {
        let canonical = format!(
            "hivemind-swarm-receipt/v1|{}|{}|{}|{}|{}|{}|{}|{}",
            self.receipt_id,
            self.infohash,
            self.uploader_worker_id,
            self.uploader_owner,
            self.downloader_worker_id,
            self.downloader_owner,
            self.bytes_verified,
            self.window_bucket
        );
        Sha256::digest(canonical.as_bytes()).to_vec()
    }

    pub fn sign(&mut self, signing_key: &SigningKey) {
        self.downloader_public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
        let payload = self.canonical_payload();
        let sig = signing_key.sign(&payload);
        self.signature_hex = hex::encode(sig.to_bytes());
    }

    pub fn verify_signature(&self) -> Result<(), ReceiptError> {
        let pk_bytes = hex::decode(&self.downloader_public_key_hex)
            .map_err(|_| ReceiptError::InvalidSignature)?;
        let pk_arr: [u8; 32] = pk_bytes
            .try_into()
            .map_err(|_| ReceiptError::InvalidSignature)?;
        let vk = VerifyingKey::from_bytes(&pk_arr).map_err(|_| ReceiptError::InvalidSignature)?;
        let sig_bytes =
            hex::decode(&self.signature_hex).map_err(|_| ReceiptError::InvalidSignature)?;
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| ReceiptError::InvalidSignature)?;
        let sig = Signature::from_bytes(&sig_arr);
        vk.verify(&self.canonical_payload(), &sig)
            .map_err(|_| ReceiptError::InvalidSignature)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReceiptError {
    #[error("receipt signature is invalid")]
    InvalidSignature,
    #[error("uploader and downloader must belong to distinct accounts")]
    SameAccountForbidden,
    #[error("duplicate receipt id `{0}`")]
    DuplicateReceipt(String),
    #[error("receipt window bucket {bucket} is outside the active window around {now_bucket}")]
    ExpiredWindow { bucket: i64, now_bucket: i64 },
    #[error("per-pair anti-farming byte cap exceeded for this window")]
    PairCapExceeded,
    #[error("bytes_verified must be positive")]
    ZeroBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiptPolicy {
    /// Maximum credited bytes between any `(uploader_owner, downloader_owner)` pair per window.
    pub max_bytes_per_pair_per_window: u64,
    /// Maximum window age in buckets allowed on submission.
    pub max_window_skew_buckets: i64,
    /// Maximum routing priority boost (e.g., 0.25 = +25% score bonus).
    pub max_boost_milli: u32,
    /// Bytes required to reach full routing priority boost.
    pub full_boost_bytes: u64,
}

impl Default for ReceiptPolicy {
    fn default() -> Self {
        Self {
            max_bytes_per_pair_per_window: 8 << 30, // 8 GiB per pair per window
            max_window_skew_buckets: 2,
            max_boost_milli: 250, // +0.25
            full_boost_bytes: 32 << 30,
        }
    }
}

#[derive(Debug, Default)]
pub struct ReceiptLedger {
    policy: ReceiptPolicy,
    seen_ids: HashSet<String>,
    /// `(uploader_owner, downloader_owner, window_bucket) -> credited_bytes`
    pair_window_bytes: HashMap<(String, String, i64), u64>,
    /// `uploader_worker_id -> total_credited_bytes`
    worker_credited_bytes: HashMap<String, u64>,
}

impl ReceiptLedger {
    #[must_use]
    pub fn new(policy: ReceiptPolicy) -> Self {
        Self {
            policy,
            ..Default::default()
        }
    }

    pub fn submit(
        &mut self,
        receipt: &UploadReceipt,
        current_window_bucket: i64,
    ) -> Result<u64, ReceiptError> {
        if receipt.bytes_verified == 0 {
            return Err(ReceiptError::ZeroBytes);
        }
        if receipt
            .uploader_owner
            .eq_ignore_ascii_case(&receipt.downloader_owner)
            || receipt.uploader_worker_id == receipt.downloader_worker_id
        {
            return Err(ReceiptError::SameAccountForbidden);
        }
        if (current_window_bucket - receipt.window_bucket).abs()
            > self.policy.max_window_skew_buckets
        {
            return Err(ReceiptError::ExpiredWindow {
                bucket: receipt.window_bucket,
                now_bucket: current_window_bucket,
            });
        }
        if self.seen_ids.contains(&receipt.receipt_id) {
            return Err(ReceiptError::DuplicateReceipt(receipt.receipt_id.clone()));
        }
        receipt.verify_signature()?;

        let pair_key = (
            receipt.uploader_owner.to_lowercase(),
            receipt.downloader_owner.to_lowercase(),
            receipt.window_bucket,
        );
        let already = self.pair_window_bytes.get(&pair_key).copied().unwrap_or(0);
        let remaining_cap = self
            .policy
            .max_bytes_per_pair_per_window
            .saturating_sub(already);
        if remaining_cap == 0 {
            return Err(ReceiptError::PairCapExceeded);
        }
        let credited = receipt.bytes_verified.min(remaining_cap);

        self.seen_ids.insert(receipt.receipt_id.clone());
        *self.pair_window_bytes.entry(pair_key).or_insert(0) += credited;
        *self
            .worker_credited_bytes
            .entry(receipt.uploader_worker_id.clone())
            .or_insert(0) += credited;
        Ok(credited)
    }

    /// Routing priority bonus in `[0.0, max_boost_milli / 1000.0]`.
    #[must_use]
    pub fn routing_priority_boost(&self, worker_id: &str) -> f64 {
        let bytes = self
            .worker_credited_bytes
            .get(worker_id)
            .copied()
            .unwrap_or(0);
        if bytes == 0 || self.policy.full_boost_bytes == 0 {
            return 0.0;
        }
        let ratio = (bytes as f64 / self.policy.full_boost_bytes as f64).min(1.0);
        ratio * (f64::from(self.policy.max_boost_milli) / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_distinct_account_receipt_credits_and_blocks_self_farming() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let mut ledger = ReceiptLedger::new(ReceiptPolicy {
            max_bytes_per_pair_per_window: 1_000,
            max_window_skew_buckets: 1,
            max_boost_milli: 200,
            full_boost_bytes: 2_000,
        });

        let mut r = UploadReceipt {
            receipt_id: "r1".into(),
            infohash: "a".repeat(40),
            uploader_worker_id: "w-up".into(),
            uploader_owner: "alice".into(),
            downloader_worker_id: "w-down".into(),
            downloader_owner: "bob".into(),
            bytes_verified: 800,
            window_bucket: 100,
            downloader_public_key_hex: String::new(),
            signature_hex: String::new(),
        };
        r.sign(&key);

        assert_eq!(ledger.submit(&r, 100).unwrap(), 800);
        assert!(ledger.routing_priority_boost("w-up") > 0.07);

        // Duplicate rejected
        assert_eq!(
            ledger.submit(&r, 100),
            Err(ReceiptError::DuplicateReceipt("r1".into()))
        );

        // Same-account self-farming rejected
        let mut self_r = r.clone();
        self_r.receipt_id = "r-self".into();
        self_r.downloader_owner = "alice".into();
        self_r.sign(&key);
        assert_eq!(
            ledger.submit(&self_r, 100),
            Err(ReceiptError::SameAccountForbidden)
        );
    }
}

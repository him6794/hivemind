//! Pricing model, periodic free allowance, WASM fuel pricing, and anti-sybil guard.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPricingTier {
    pub model_id: String,
    pub input_cpt_per_million: i64,
    pub output_cpt_per_million: i64,
    pub platform_fee_bps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeAllowancePolicy {
    /// Daily free CPT grant for verified accounts.
    pub daily_verified_grant_cpt: i64,
    /// Minimum account age (seconds) before claiming recurring grants.
    pub min_account_age_secs: i64,
    /// Maximum accounts on the same IP prefix allowed to claim in one 24h window.
    pub max_claims_per_subnet_per_day: usize,
    /// Maximum claims per hardware fingerprint per day.
    pub max_claims_per_fingerprint_per_day: usize,
}

impl Default for FreeAllowancePolicy {
    fn default() -> Self {
        Self {
            daily_verified_grant_cpt: 200,
            min_account_age_secs: 3600,
            max_claims_per_subnet_per_day: 3,
            max_claims_per_fingerprint_per_day: 1,
        }
    }
}

/// Computes CPT cost for a bounded WASM/WASI task based on executed fuel instructions
/// and peak linear memory pages (64 KiB/page).
#[must_use]
pub fn compute_wasm_fuel_cost(
    executed_fuel: u64,
    peak_memory_pages: u32,
    cpt_per_million_fuel: i64,
    platform_fee_bps: u32,
) -> (i64, i64) {
    let fuel_blocks = executed_fuel.div_ceil(1_000_000) as i64;
    let mem_surcharge = i64::from(peak_memory_pages / 64); // 1 CPT per 4 MiB above baseline
    let base = fuel_blocks
        .saturating_mul(cpt_per_million_fuel.max(1))
        .saturating_add(mem_surcharge)
        .max(1);
    let fee = ((i128::from(base) * i128::from(platform_fee_bps.min(10_000))) / 10_000) as i64;
    (base.saturating_add(fee), fee)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AntiSybilError {
    #[error("account must be email-verified before claiming free allowance")]
    EmailNotVerified,
    #[error("account is too new ({age_secs}s < {min_secs}s cooldown)")]
    AccountTooNew { age_secs: i64, min_secs: i64 },
    #[error("account `{0}` already claimed allowance for day {1}")]
    AlreadyClaimedToday(String, i64),
    #[error("network subnet `{0}` reached daily free-claim sybil cap")]
    SubnetCapReached(String),
    #[error("device fingerprint `{0}` reached daily free-claim sybil cap")]
    FingerprintCapReached(String),
}

#[derive(Debug, Default)]
pub struct AntiSybilGuard {
    policy: FreeAllowancePolicy,
    claimed_accounts: HashSet<(String, i64)>,
    subnet_counts: HashMap<(String, i64), usize>,
    fingerprint_counts: HashMap<(String, i64), usize>,
}

impl AntiSybilGuard {
    #[must_use]
    pub fn new(policy: FreeAllowancePolicy) -> Self {
        Self {
            policy,
            ..Default::default()
        }
    }

    pub fn evaluate_free_allowance_claim(
        &mut self,
        username: &str,
        email_verified: bool,
        account_age_secs: i64,
        subnet_prefix: &str,
        device_fingerprint: &str,
        day_bucket: i64,
    ) -> Result<i64, AntiSybilError> {
        if !email_verified {
            return Err(AntiSybilError::EmailNotVerified);
        }
        if account_age_secs < self.policy.min_account_age_secs {
            return Err(AntiSybilError::AccountTooNew {
                age_secs: account_age_secs,
                min_secs: self.policy.min_account_age_secs,
            });
        }
        let acct_key = (username.to_lowercase(), day_bucket);
        if self.claimed_accounts.contains(&acct_key) {
            return Err(AntiSybilError::AlreadyClaimedToday(
                username.to_owned(),
                day_bucket,
            ));
        }
        let subnet_key = (subnet_prefix.to_owned(), day_bucket);
        let subnet_used = self.subnet_counts.get(&subnet_key).copied().unwrap_or(0);
        if subnet_used >= self.policy.max_claims_per_subnet_per_day {
            return Err(AntiSybilError::SubnetCapReached(subnet_prefix.to_owned()));
        }
        let fp_key = (device_fingerprint.to_owned(), day_bucket);
        let fp_used = self.fingerprint_counts.get(&fp_key).copied().unwrap_or(0);
        if fp_used >= self.policy.max_claims_per_fingerprint_per_day {
            return Err(AntiSybilError::FingerprintCapReached(
                device_fingerprint.to_owned(),
            ));
        }

        self.claimed_accounts.insert(acct_key);
        *self.subnet_counts.entry(subnet_key).or_insert(0) += 1;
        *self.fingerprint_counts.entry(fp_key).or_insert(0) += 1;
        Ok(self.policy.daily_verified_grant_cpt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anti_sybil_blocks_multi_account_same_device_and_subnet_farming() {
        let mut guard = AntiSybilGuard::new(FreeAllowancePolicy {
            daily_verified_grant_cpt: 100,
            min_account_age_secs: 600,
            max_claims_per_subnet_per_day: 2,
            max_claims_per_fingerprint_per_day: 1,
        });

        assert_eq!(
            guard
                .evaluate_free_allowance_claim("u1", true, 1200, "203.0.113.0/24", "fp-1", 10)
                .unwrap(),
            100
        );
        // Same device fingerprint on a second sybil account is blocked
        assert!(matches!(
            guard.evaluate_free_allowance_claim("u2", true, 1200, "203.0.113.0/24", "fp-1", 10),
            Err(AntiSybilError::FingerprintCapReached(_))
        ));
        // Second distinct device on same subnet succeeds
        assert_eq!(
            guard
                .evaluate_free_allowance_claim("u2", true, 1200, "203.0.113.0/24", "fp-2", 10)
                .unwrap(),
            100
        );
        // Third account on same subnet hits subnet cap
        assert!(matches!(
            guard.evaluate_free_allowance_claim("u3", true, 1200, "203.0.113.0/24", "fp-3", 10),
            Err(AntiSybilError::SubnetCapReached(_))
        ));
    }

    #[test]
    fn wasm_fuel_pricing_computes_fee_inclusive_total() {
        let (total, fee) = compute_wasm_fuel_cost(2_500_000, 128, 10, 500);
        // 3 million-fuel blocks * 10 + 2 mem pages = 32 base; 5% fee = 1 -> 33 total
        assert_eq!(fee, 1);
        assert_eq!(total, 33);
    }
}

//! Spot checks: Nodepool re-runs a sample of completed managed tasks on a verifier of its own choosing, and compares
//! the re-run's output with the output the replicas agreed on.
//!
//! This module holds only the pure parts: which tasks are sampled, and how a re-run is classified. Nothing here
//! dispatches work or changes settlement. Wiring it into dispatch needs two decisions that are not made yet: where the
//! verifier runs, and who pays for its computation.
//!
//! Replica agreement is not proof of correctness, and a re-run is one more claim. `Verified` means that one
//! independent re-run produced the same bytes as the agreed result. It is the only verdict that may be shown as
//! verified.

use crate::output_digest;
use sha2::{Digest, Sha256};

/// Sample rates are in basis points: 10_000 samples every task, and 0 samples none.
pub const SAMPLE_RATE_DENOMINATOR: u64 = 10_000;

const SAMPLE_DOMAIN: &[u8] = b"hivemind-spot-check-sample-v1";

/// Decides whether a task is re-run. The answer depends only on the task id, the seed, and the rate, so anyone who
/// holds the seed can recompute the sample without asking Nodepool.
pub fn is_sampled(task_id: &str, seed: &[u8], rate_bps: u64) -> bool {
    if rate_bps == 0 {
        return false;
    }
    if rate_bps >= SAMPLE_RATE_DENOMINATOR {
        return true;
    }
    let mut hasher = Sha256::new();
    hasher.update(SAMPLE_DOMAIN);
    hasher.update((seed.len() as u64).to_be_bytes());
    hasher.update(seed);
    hasher.update(task_id.as_bytes());
    let digest = hasher.finalize();
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(head) % SAMPLE_RATE_DENOMINATOR < rate_bps
}

/// What the verifier's re-run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RerunOutcome {
    Completed(Vec<u8>),
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotCheckVerdict {
    /// The re-run completed and its output digest equals the agreed digest.
    Verified,
    /// The re-run completed with different output. This needs investigation. It does not change settlement by itself.
    Mismatch,
    /// Nothing can be concluded, because the re-run did not complete.
    Unverified,
}

impl SpotCheckVerdict {
    /// Only `Verified` may be displayed as verified.
    pub fn shows_as_verified(self) -> bool {
        self == Self::Verified
    }
}

/// Compares a re-run with the agreed output digest from a consensus certificate.
pub fn classify(agreed_output_digest: &[u8; 32], rerun: &RerunOutcome) -> SpotCheckVerdict {
    match rerun {
        RerunOutcome::Completed(output) if output_digest(output) == *agreed_output_digest => {
            SpotCheckVerdict::Verified
        }
        RerunOutcome::Completed(_) => SpotCheckVerdict::Mismatch,
        RerunOutcome::Failed | RerunOutcome::TimedOut => SpotCheckVerdict::Unverified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_edges_are_exact() {
        for id in ["task-1", "task-2"] {
            assert!(!is_sampled(id, b"seed", 0));
            assert!(is_sampled(id, b"seed", SAMPLE_RATE_DENOMINATOR));
            assert!(is_sampled(id, b"seed", SAMPLE_RATE_DENOMINATOR + 1));
        }
    }

    #[test]
    fn the_same_task_and_seed_always_give_the_same_answer() {
        let ask = || -> Vec<bool> {
            (0..200)
                .map(|i| is_sampled(&format!("task-{i}"), b"seed", 2_500))
                .collect()
        };
        assert_eq!(ask(), ask());
    }

    #[test]
    fn the_sample_is_close_to_the_requested_fraction() {
        let sampled = (0..20_000)
            .filter(|i| is_sampled(&format!("task-{i}"), b"seed", 1_000))
            .count();
        // 10% of 20,000 is 2,000. The digest is uniform, so the count stays well inside this window.
        assert!((1_800..=2_200).contains(&sampled), "sampled {sampled}");
    }

    #[test]
    fn a_different_seed_draws_a_different_sample() {
        let differs = (0..2_000)
            .filter(|i| {
                let id = format!("task-{i}");
                is_sampled(&id, b"seed-a", 5_000) != is_sampled(&id, b"seed-b", 5_000)
            })
            .count();
        assert!(differs > 0);
    }

    #[test]
    fn a_completed_rerun_with_the_agreed_bytes_is_verified() {
        let agreed = output_digest(b"42");
        assert_eq!(
            classify(&agreed, &RerunOutcome::Completed(b"42".to_vec())),
            SpotCheckVerdict::Verified
        );
    }

    #[test]
    fn any_byte_difference_is_a_mismatch_including_a_trailing_newline() {
        let agreed = output_digest(b"42");
        assert_eq!(
            classify(&agreed, &RerunOutcome::Completed(b"43".to_vec())),
            SpotCheckVerdict::Mismatch
        );
        assert_eq!(
            classify(&agreed, &RerunOutcome::Completed(b"42\n".to_vec())),
            SpotCheckVerdict::Mismatch
        );
    }

    #[test]
    fn a_rerun_that_did_not_complete_proves_nothing() {
        let agreed = output_digest(b"42");
        assert_eq!(
            classify(&agreed, &RerunOutcome::Failed),
            SpotCheckVerdict::Unverified
        );
        assert_eq!(
            classify(&agreed, &RerunOutcome::TimedOut),
            SpotCheckVerdict::Unverified
        );
    }

    #[test]
    fn only_a_verified_result_is_shown_as_verified() {
        assert!(SpotCheckVerdict::Verified.shows_as_verified());
        assert!(!SpotCheckVerdict::Mismatch.shows_as_verified());
        assert!(!SpotCheckVerdict::Unverified.shows_as_verified());
    }
}

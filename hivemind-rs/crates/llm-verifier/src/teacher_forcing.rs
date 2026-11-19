//! Teacher-forcing logprob / top-k consistency verification for non-deterministic
//! LLM inference across heterogeneous hardware (CPU / GPU / quantizations).

use serde::{Deserialize, Serialize};

/// One sampled token step observed from the candidate worker and re-evaluated
/// by an independent reference node via a single teacher-forcing forward pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenLogprobSample {
    pub position: u32,
    pub candidate_token_id: u32,
    /// Log-probability assigned to `candidate_token_id` by the candidate worker.
    pub candidate_logprob: f32,
    /// Log-probability assigned to `candidate_token_id` by the reference verifier.
    pub reference_logprob: f32,
    /// Reference verifier's top-k `(token_id, logprob)` distribution at this step.
    pub reference_top_k: Vec<(u32, f32)>,
}

/// Calibrated tolerance profile accounting for legitimate numeric drift between
/// backends (CUDA/Metal/Vulkan/CPU) and quantizations (Q4_K_M/Q8_0/FP16).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VerificationPolicy {
    /// Maximum allowed mean absolute logprob delta across sampled positions.
    pub max_mean_abs_logprob_diff: f32,
    /// Maximum allowed single-token logprob divergence before counting as an outlier.
    pub max_single_token_logprob_diff: f32,
    /// Minimum fraction of sampled tokens that must fall inside the reference top-k.
    pub min_top_k_hit_rate: f32,
    /// Maximum fraction of outlier positions tolerated (absorbs occasional argmax ties).
    pub max_outlier_rate: f32,
    /// Minimum sampled tokens required to issue a definitive verdict.
    pub min_samples: usize,
}

impl VerificationPolicy {
    /// Strict tolerance when candidate and reference share identical quantization & backend.
    #[must_use]
    pub const fn same_hardware_strict() -> Self {
        Self {
            max_mean_abs_logprob_diff: 0.15,
            max_single_token_logprob_diff: 0.60,
            min_top_k_hit_rate: 0.95,
            max_outlier_rate: 0.05,
            min_samples: 4,
        }
    }

    /// Calibrated tolerance across heterogeneous backends (e.g., GPU vs CPU, Q4 vs Q8).
    #[must_use]
    pub const fn cross_hardware_quantized() -> Self {
        Self {
            max_mean_abs_logprob_diff: 0.45,
            max_single_token_logprob_diff: 1.50,
            min_top_k_hit_rate: 0.85,
            max_outlier_rate: 0.15,
            min_samples: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VerdictReason {
    Consistent,
    InsufficientSamples,
    MeanLogprobDivergence { observed: f32, limit: f32 },
    LowTopKAgreement { observed: f32, min_required: f32 },
    ExcessiveOutliers { observed: f32, max_allowed: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationVerdict {
    pub passed: bool,
    pub sample_count: usize,
    pub mean_abs_logprob_diff: f32,
    pub top_k_hit_rate: f32,
    pub outlier_rate: f32,
    pub reason: VerdictReason,
}

/// Evaluates whether a candidate worker's generated tokens are consistent with an
/// independent teacher-forcing reference pass.
#[must_use]
pub fn verify_teacher_forcing_consistency(
    samples: &[TokenLogprobSample],
    policy: &VerificationPolicy,
) -> VerificationVerdict {
    if samples.len() < policy.min_samples {
        return VerificationVerdict {
            passed: false,
            sample_count: samples.len(),
            mean_abs_logprob_diff: 0.0,
            top_k_hit_rate: 0.0,
            outlier_rate: 0.0,
            reason: VerdictReason::InsufficientSamples,
        };
    }

    let n = samples.len() as f32;
    let mut abs_diff_sum = 0.0_f32;
    let mut top_k_hits = 0usize;
    let mut outliers = 0usize;

    for s in samples {
        let diff = (s.candidate_logprob - s.reference_logprob).abs();
        abs_diff_sum += diff;
        if diff > policy.max_single_token_logprob_diff || !diff.is_finite() {
            outliers += 1;
        }
        if s.reference_top_k
            .iter()
            .any(|(tok, _)| *tok == s.candidate_token_id)
        {
            top_k_hits += 1;
        }
    }

    let mean_abs_logprob_diff = abs_diff_sum / n;
    let top_k_hit_rate = (top_k_hits as f32) / n;
    let outlier_rate = (outliers as f32) / n;

    if !mean_abs_logprob_diff.is_finite()
        || mean_abs_logprob_diff > policy.max_mean_abs_logprob_diff
    {
        return VerificationVerdict {
            passed: false,
            sample_count: samples.len(),
            mean_abs_logprob_diff,
            top_k_hit_rate,
            outlier_rate,
            reason: VerdictReason::MeanLogprobDivergence {
                observed: mean_abs_logprob_diff,
                limit: policy.max_mean_abs_logprob_diff,
            },
        };
    }
    if top_k_hit_rate < policy.min_top_k_hit_rate {
        return VerificationVerdict {
            passed: false,
            sample_count: samples.len(),
            mean_abs_logprob_diff,
            top_k_hit_rate,
            outlier_rate,
            reason: VerdictReason::LowTopKAgreement {
                observed: top_k_hit_rate,
                min_required: policy.min_top_k_hit_rate,
            },
        };
    }
    if outlier_rate > policy.max_outlier_rate {
        return VerificationVerdict {
            passed: false,
            sample_count: samples.len(),
            mean_abs_logprob_diff,
            top_k_hit_rate,
            outlier_rate,
            reason: VerdictReason::ExcessiveOutliers {
                observed: outlier_rate,
                max_allowed: policy.max_outlier_rate,
            },
        };
    }

    VerificationVerdict {
        passed: true,
        sample_count: samples.len(),
        mean_abs_logprob_diff,
        top_k_hit_rate,
        outlier_rate,
        reason: VerdictReason::Consistent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_hardware_drift_passes_while_fabricated_garbage_fails() {
        let honest: Vec<TokenLogprobSample> = (0..8)
            .map(|i| TokenLogprobSample {
                position: i,
                candidate_token_id: 100 + i,
                candidate_logprob: -0.40,
                reference_logprob: -0.58, // 0.18 drift from Q4 vs Q8
                reference_top_k: vec![(100 + i, -0.58), (200 + i, -1.2)],
            })
            .collect();
        let v = verify_teacher_forcing_consistency(
            &honest,
            &VerificationPolicy::cross_hardware_quantized(),
        );
        assert!(v.passed, "expected honest cross-hw drift to pass: {v:?}");

        // Cheating worker returns cheap smaller-model/random tokens not in top-k
        let cheating: Vec<TokenLogprobSample> = (0..8)
            .map(|i| TokenLogprobSample {
                position: i,
                candidate_token_id: 9999,
                candidate_logprob: -0.3,
                reference_logprob: -6.5,
                reference_top_k: vec![(100 + i, -0.2), (200 + i, -0.9)],
            })
            .collect();
        let bad = verify_teacher_forcing_consistency(
            &cheating,
            &VerificationPolicy::cross_hardware_quantized(),
        );
        assert!(!bad.passed);
    }
}

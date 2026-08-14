//! Deterministic, replayable random-number primitives for the reference runtime.
//!
//! The contract is intentionally explicit: `splitmix64-v1` fixes the mixing
//! algorithm, while the seed, stream, and subsequence identify a reproducible
//! sequence. This module is a bounded CPU reference primitive, not a
//! cryptographic random source.

use std::fmt;

pub const RNG_ALGORITHM_VERSION: &str = "splitmix64-v1";
pub const MAX_RNG_SAMPLES: usize = 1_000_000;

const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const STREAM_MIX: u64 = 0xD1B5_4A32_D192_ED03;
const SUBSEQUENCE_MIX: u64 = 0x94D0_49BB_1331_11EB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RngError {
    SampleCountExceeded { requested: usize, max: usize },
}

impl fmt::Display for RngError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SampleCountExceeded { requested, max } => {
                write!(
                    formatter,
                    "requested {requested} RNG samples, maximum is {max}"
                )
            }
        }
    }
}

impl std::error::Error for RngError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    #[must_use]
    pub fn new(seed: u64, stream: u64, subsequence: u64) -> Self {
        let state = splitmix64(
            seed ^ splitmix64(stream ^ STREAM_MIX) ^ splitmix64(subsequence ^ SUBSEQUENCE_MIX),
        );
        Self { state }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLITMIX_GAMMA);
        splitmix64(self.state)
    }

    pub fn next_f64(&mut self) -> f64 {
        let mantissa = self.next_u64() >> 11;
        mantissa as f64 / (1u64 << 53) as f64
    }

    pub fn sample_f64(&mut self, count: usize) -> Result<Vec<f64>, RngError> {
        if count > MAX_RNG_SAMPLES {
            return Err(RngError::SampleCountExceeded {
                requested: count,
                max: MAX_RNG_SAMPLES,
            });
        }
        let mut samples = Vec::with_capacity(count);
        for _ in 0..count {
            samples.push(self.next_f64());
        }
        Ok(samples)
    }
}

fn splitmix64(input: u64) -> u64 {
    let mut value = input.wrapping_add(SPLITMIX_GAMMA);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

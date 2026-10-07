use anyhow::{bail, Result};
use general_compute_runtime::numeric::DenseTensor;
use std::ops::{Add, Div, Mul, Sub};

pub(super) const CPU_DIMENSION: u32 = 128;
pub(super) const GPU_DIMENSIONS: [u32; 2] = [512, 1024];
pub(super) const MAX_MATRIX_DIMENSION: u32 = 1024;

pub(super) trait BenchmarkFloat:
    Copy
    + Default
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Send
    + Sync
    + 'static
{
    fn from_ratio(numerator: u64, denominator: u64) -> Self;
    fn as_f64(self) -> f64;
}

impl BenchmarkFloat for f32 {
    fn from_ratio(numerator: u64, denominator: u64) -> Self {
        numerator as f32 / denominator as f32
    }

    fn as_f64(self) -> f64 {
        f64::from(self)
    }
}

impl BenchmarkFloat for f64 {
    fn from_ratio(numerator: u64, denominator: u64) -> Self {
        numerator as f64 / denominator as f64
    }

    fn as_f64(self) -> f64 {
        self
    }
}

pub(super) fn matrix_inputs<T: BenchmarkFloat>(
    dimension: u32,
) -> Result<(DenseTensor<T>, DenseTensor<T>)> {
    let (left, right) = matrix_values::<T>(dimension)?;
    let shape = vec![u64::from(dimension), u64::from(dimension)];
    let left = DenseTensor::new(shape.clone(), left)?;
    let right = DenseTensor::new(shape, right)?;
    Ok((left, right))
}

pub(super) fn matrix_values<T: BenchmarkFloat>(dimension: u32) -> Result<(Vec<T>, Vec<T>)> {
    if dimension == 0 || dimension > MAX_MATRIX_DIMENSION {
        bail!("benchmark matrix dimension is outside the fixed workload bounds");
    }
    let dimension_usize = usize::try_from(dimension)?;
    let elements = dimension_usize
        .checked_mul(dimension_usize)
        .ok_or_else(|| anyhow::anyhow!("benchmark matrix element count overflow"))?;
    let mut left = Vec::with_capacity(elements);
    let mut right = Vec::with_capacity(elements);

    for row in 0..dimension {
        for shared in 0..dimension {
            let row_weight = u64::from(row % 7 + 1);
            let shared_weight = u64::from(shared % 5 + 1);
            left.push(T::from_ratio(row_weight * shared_weight, 64));
        }
    }
    for shared in 0..dimension {
        for column in 0..dimension {
            let shared_weight = u64::from(shared % 3 + 1);
            let column_weight = u64::from(column % 11 + 1);
            right.push(T::from_ratio(shared_weight * column_weight, 32));
        }
    }
    Ok((left, right))
}

pub(super) fn validate_values<T: BenchmarkFloat>(dimension: u32, values: &[T]) -> Result<()> {
    if dimension == 0 || dimension > MAX_MATRIX_DIMENSION {
        bail!("benchmark output dimension is outside the fixed workload bounds");
    }
    let dimension_usize = usize::try_from(dimension)?;
    let expected_len = dimension_usize
        .checked_mul(dimension_usize)
        .ok_or_else(|| anyhow::anyhow!("benchmark output element count overflow"))?;
    if values.len() != expected_len {
        bail!(
            "benchmark output has {} elements; expected {expected_len}",
            values.len()
        );
    }

    let shared_weight_sum = shared_weight_sum(dimension);
    for row in 0..dimension {
        for column in 0..dimension {
            let offset = usize::try_from(row)?
                .checked_mul(dimension_usize)
                .and_then(|base| base.checked_add(usize::try_from(column).ok()?))
                .ok_or_else(|| anyhow::anyhow!("benchmark output offset overflow"))?;
            let actual = values[offset].as_f64();
            let numerator = u64::from(row % 7 + 1)
                .checked_mul(u64::from(column % 11 + 1))
                .and_then(|weight| weight.checked_mul(shared_weight_sum))
                .ok_or_else(|| anyhow::anyhow!("benchmark reference value overflow"))?;
            let expected = numerator as f64 / 2048.0;
            let tolerance = expected.abs().max(1.0) * 1.0e-5;
            if !actual.is_finite() || (actual - expected).abs() > tolerance {
                bail!(
                    "benchmark output mismatch at ({row},{column}): got {actual}, expected {expected}"
                );
            }
        }
    }
    Ok(())
}

fn shared_weight_sum(dimension: u32) -> u64 {
    // The two shared-index factors repeat every lcm(5, 3) = 15 terms. Sum
    // complete cycles and the bounded remainder rather than duplicating the
    // matrix multiplication in the reference calculation.
    const PERIOD: u32 = 15;
    let cycle_sum: u64 = (0..PERIOD)
        .map(|shared| u64::from(shared % 5 + 1) * u64::from(shared % 3 + 1))
        .sum();
    let cycles = u64::from(dimension / PERIOD);
    let remainder = dimension % PERIOD;
    let remainder_sum: u64 = (0..remainder)
        .map(|shared| u64::from(shared % 5 + 1) * u64::from(shared % 3 + 1))
        .sum();
    cycles * cycle_sum + remainder_sum
}

/// Return a descriptive CPU identity for local cache/report binding. This is
/// an inventory string, not a hardware-attestation claim.
pub fn cpu_identity() -> String {
    let system = sysinfo::System::new_all();
    let (vendor, brand) = system
        .cpus()
        .first()
        .map(|cpu| (cpu.vendor_id().trim(), cpu.brand().trim()))
        .unwrap_or(("", ""));
    let parallelism = std::thread::available_parallelism()
        .map(|value| value.get().to_string())
        .unwrap_or_else(|_| "unavailable".to_owned());
    format!(
        "vendor={};brand={};arch={};os={};available_parallelism={parallelism}",
        if vendor.is_empty() {
            "unavailable"
        } else {
            vendor
        },
        if brand.is_empty() {
            "unavailable"
        } else {
            brand
        },
        std::env::consts::ARCH,
        std::env::consts::OS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_cpu_inputs_have_full_exact_analytical_products() {
        let dimension = CPU_DIMENSION;
        let (left, right) = matrix_inputs::<f32>(dimension).expect("fixed inputs are valid");
        let output = left.matmul(&right).expect("fixed matrix product is valid");
        assert_eq!(
            output.shape(),
            &[u64::from(dimension), u64::from(dimension)]
        );
        validate_values(dimension, output.values()).expect("all outputs match the reference");
    }

    #[test]
    fn validator_rejects_nonfinite_incorrect_and_incomplete_outputs() {
        let mut values = vec![1.0f32; 4];
        assert!(validate_values(2, &values).is_err());
        assert!(validate_values(3, &values).is_err());
        values[0] = f32::NAN;
        assert!(validate_values(2, &values).is_err());
    }

    #[test]
    fn cpu_identity_is_descriptive_and_bounded() {
        let identity = cpu_identity();
        assert!(!identity.trim().is_empty());
        assert!(identity.len() <= 1024);
        assert!(identity.contains("arch="));
        assert!(identity.contains("available_parallelism="));
    }
}

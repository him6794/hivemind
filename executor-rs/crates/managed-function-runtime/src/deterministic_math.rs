//! Deterministic elementary functions for CPU hcode.
//!
//! Replicas run on different hosts and must agree bit for bit, but the host
//! `libm` may round the last bit differently. These routines use only the
//! operations IEEE-754 binary64 fixes exactly (`+`, `-`, `*`, `/`, `round`, and
//! bit casts) in a fixed order, with fixed-length series. Rust does not fuse
//! `a * b + c` into an FMA unless asked, so every replica performs the same
//! rounding steps. Changing a series or constant changes CPU hcode results, so
//! the constants and loop bounds are part of the hcode semantics.

use std::f64::consts::{FRAC_2_PI, LOG2_E, SQRT_2};

// Constants are the shortest decimal that round-trips to the value shown.
// `LN2_HI`, `PIO2_HI`, and `PIO2_MID` carry at most 33 significant bits, so
// `k * constant` is exact for the reduction ranges used below.
const LN2_HI: f64 = 0.693_147_180_601_954_5;
const LN2_LO: f64 = -4.200_915_072_681_084_6e-11;
const INV_LN2: f64 = LOG2_E;
const SQRT2: f64 = SQRT_2;
const PIO2_HI: f64 = 1.570_796_326_734_125_6;
const PIO2_MID: f64 = 6.077_100_506_506_192e-11;
const PIO2_LO: f64 = 3.521_559_865_183_2e-27;
const TWO_OVER_PI: f64 = FRAC_2_PI;

/// `exp` overflows above this argument (`ln(f64::MAX)`).
const EXP_OVERFLOW: f64 = 709.782_712_893_384;
/// `exp` underflows to zero below this argument.
const EXP_UNDERFLOW: f64 = -746.0;
/// `sin` and `cos` accept arguments up to this magnitude. Beyond it the
/// three-part reduction is no longer accurate.
const TRIG_LIMIT: f64 = 1.0e6;

/// `e^x`, or `None` when the result overflows.
#[must_use]
pub(crate) fn exp(x: f64) -> Option<f64> {
    if x > EXP_OVERFLOW {
        return None;
    }
    if x < EXP_UNDERFLOW {
        return Some(0.0);
    }
    let value = exp_within_range(x);
    value.is_finite().then_some(value)
}

/// Natural logarithm for finite positive `x`; `None` otherwise.
#[must_use]
pub(crate) fn ln(x: f64) -> Option<f64> {
    if !(x > 0.0 && x.is_finite()) {
        return None;
    }
    let (mut mantissa, mut exponent) = normalized_parts(x);
    // Center the mantissa in [sqrt(1/2), sqrt(2)) so the series converges fast.
    if mantissa > SQRT2 {
        mantissa *= 0.5;
        exponent += 1;
    }
    // ln(m) = 2 * atanh(s) with s = (m - 1) / (m + 1), and |s| < 0.172.
    let s = (mantissa - 1.0) / (mantissa + 1.0);
    let s_squared = s * s;
    let mut term = s;
    let mut sum = 0.0;
    for n in 0..16_u32 {
        sum += term / f64::from(2 * n + 1);
        term *= s_squared;
    }
    let exponent = i64_to_f64(exponent);
    Some(exponent * LN2_HI + (exponent * LN2_LO + 2.0 * sum))
}

/// `sin(x)` for `|x| <= 1e6`; `None` outside that range or for non-finite input.
#[must_use]
pub(crate) fn sin(x: f64) -> Option<f64> {
    let (quadrant, r) = reduce_half_pi(x)?;
    Some(sin_by_quadrant(quadrant, r))
}

/// `cos(x)` for `|x| <= 1e6`; `None` outside that range or for non-finite input.
#[must_use]
pub(crate) fn cos(x: f64) -> Option<f64> {
    // cos(x) = sin(x + pi/2), which moves the quadrant by one and keeps r.
    let (quadrant, r) = reduce_half_pi(x)?;
    Some(sin_by_quadrant(quadrant + 1, r))
}

/// Hyperbolic tangent. Always finite: it saturates to +-1 beyond |x| = 20.
#[must_use]
pub(crate) fn tanh(x: f64) -> f64 {
    if x >= 20.0 {
        return 1.0;
    }
    if x <= -20.0 {
        return -1.0;
    }
    // tanh(x) = (e^(2x) - 1) / (e^(2x) + 1), with e^(2x) - 1 computed directly
    // for small arguments so that it keeps its significant digits.
    let e = expm1(2.0 * x);
    e / (e + 2.0)
}

/// `base^exponent` for the cases hcode accepts; `None` for undefined or
/// overflowing results. Exponent 0.5 uses the correctly rounded square root.
/// Integral exponents up to 64 use repeated squaring, and other exponents use
/// `exp(exponent * ln(base))`, whose error grows with `|exponent * ln(base)|`.
#[must_use]
pub(crate) fn pow(base: f64, exponent: f64) -> Option<f64> {
    if !(base.is_finite() && exponent.is_finite()) {
        return None;
    }
    if is_zero(exponent) {
        return Some(1.0);
    }
    if is_zero(base) {
        return (exponent > 0.0).then_some(0.0);
    }
    if exponent.to_bits() == 0.5_f64.to_bits() && base > 0.0 {
        return Some(base.sqrt());
    }
    let integral = is_integral(exponent);
    if base < 0.0 && !integral {
        return None;
    }
    let magnitude = base.abs();
    let value = if integral && exponent.abs() <= 64.0 {
        integer_power(magnitude, exponent)?
    } else {
        exp(exponent * ln(magnitude)?)?
    };
    let negative = base < 0.0 && is_odd_integer(exponent);
    Some(if negative { -value } else { value })
}

/// `exp` for `|x| <= 745`, where the result is always representable.
fn exp_within_range(x: f64) -> f64 {
    let k = (x * INV_LN2).round();
    let r = (x - k * LN2_HI) - k * LN2_LO;
    // |r| <= ln(2) / 2, so fourteen terms of the Taylor series are below the
    // rounding error of the sum.
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..=14_u32 {
        term = term * r / f64::from(n);
        sum += term;
    }
    scale_by_power_of_two(sum, f64_to_i64(k))
}

/// `e^x - 1` for `|x| < 40`.
fn expm1(y: f64) -> f64 {
    if y.abs() < 0.5 {
        // The series keeps relative precision where `exp(y) - 1` would cancel.
        let mut term = y;
        let mut sum = y;
        for n in 2..=18_u32 {
            term = term * y / f64::from(n);
            sum += term;
        }
        sum
    } else {
        exp_within_range(y) - 1.0
    }
}

/// `x^n` by repeated squaring for an integral exponent with `|n| <= 64`.
fn integer_power(base: f64, exponent: f64) -> Option<f64> {
    let n = f64_to_i64(exponent);
    let mut remaining = n.unsigned_abs();
    // Invert first: base^|n| can overflow even when base^n is a
    // representable subnormal (for example 1e155^-2).
    let mut factor = if n < 0 { 1.0 / base } else { base };
    let mut result = 1.0;
    while remaining > 0 {
        if remaining & 1 == 1 {
            result *= factor;
        }
        remaining >>= 1;
        if remaining > 0 {
            factor *= factor;
        }
    }
    result.is_finite().then_some(result)
}

/// Reduces `x` to `r + q * pi/2` with `|r| <= pi/4`, returning `(q mod 4, r)`.
fn reduce_half_pi(x: f64) -> Option<(i64, f64)> {
    if !(x.is_finite() && x.abs() <= TRIG_LIMIT) {
        return None;
    }
    let k = (x * TWO_OVER_PI).round();
    let r = ((x - k * PIO2_HI) - k * PIO2_MID) - k * PIO2_LO;
    Some((f64_to_i64(k).rem_euclid(4), r))
}

fn sin_by_quadrant(quadrant: i64, r: f64) -> f64 {
    match quadrant.rem_euclid(4) {
        0 => sin_series(r),
        1 => cos_series(r),
        2 => -sin_series(r),
        _ => -cos_series(r),
    }
}

/// `sin(r)` for `|r| <= pi/4`: eleven terms bound the truncation error below
/// the rounding error of the sum.
fn sin_series(r: f64) -> f64 {
    let r_squared = r * r;
    let mut term = r;
    let mut sum = r;
    for n in 1..=11_u32 {
        term *= -r_squared / f64::from((2 * n) * (2 * n + 1));
        sum += term;
    }
    sum
}

/// `cos(r)` for `|r| <= pi/4`.
fn cos_series(r: f64) -> f64 {
    let r_squared = r * r;
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..=11_u32 {
        term *= -r_squared / f64::from((2 * n - 1) * (2 * n));
        sum += term;
    }
    sum
}

/// Splits a positive finite `x` into `m * 2^e` with `m` in `[1, 2)`. Subnormal
/// inputs are scaled by `2^54` first, so the split is exact for every input.
fn normalized_parts(x: f64) -> (f64, i64) {
    let (scaled, adjustment) = if x < f64::MIN_POSITIVE {
        (x * pow2(54), -54)
    } else {
        (x, 0)
    };
    let bits = scaled.to_bits();
    let biased = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let mantissa = f64::from_bits((bits & ((1_u64 << 52) - 1)) | (1023_u64 << 52));
    (mantissa, biased - 1023 + adjustment)
}

/// Multiplies by `2^exponent` exactly, except where the result is subnormal.
fn scale_by_power_of_two(mut value: f64, mut exponent: i64) -> f64 {
    while exponent > 1000 {
        value *= pow2(1000);
        exponent -= 1000;
    }
    while exponent < -1000 {
        value *= pow2(-1000);
        exponent += 1000;
    }
    value * pow2(exponent)
}

/// `2^exponent` for `exponent` in `-1000..=1000`, built from its bit pattern.
fn pow2(exponent: i64) -> f64 {
    let biased = u64::try_from(exponent + 1023).unwrap_or(0);
    f64::from_bits(biased << 52)
}

fn is_zero(value: f64) -> bool {
    value.abs().to_bits() == 0
}

fn is_integral(value: f64) -> bool {
    value.trunc().to_bits() == value.to_bits()
}

fn is_odd_integer(value: f64) -> bool {
    is_integral(value) && (value % 2.0).abs().to_bits() == 1.0_f64.to_bits()
}

/// Converts an integral `f64` that the caller has bounded to `i64`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "every caller passes an integral value bounded well inside i64"
)]
fn f64_to_i64(value: f64) -> i64 {
    value as i64
}

/// Converts an `i64` bounded to +-2^53 without rounding.
#[expect(
    clippy::cast_precision_loss,
    reason = "every caller passes an exponent far below 2^53"
)]
fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

#[cfg(test)]
mod tests {
    use super::{cos, exp, integer_power, ln, pow, sin, tanh};

    fn relative_error(actual: f64, expected: f64) -> f64 {
        ((actual - expected) / expected).abs()
    }

    #[test]
    fn exp_and_ln_track_the_host_within_a_few_units_in_the_last_place() {
        for step in -700..=700 {
            let x = f64::from(step) * 0.997_3;
            let actual = exp(x).expect("finite exp result");
            assert!(relative_error(actual, x.exp()) < 1e-14, "exp({x})");
        }
        for step in 1..=2000 {
            let x = f64::from(step) * 0.013_7;
            let actual = ln(x).expect("positive ln argument");
            assert!(
                (actual - x.ln()).abs() < 1e-14 * x.ln().abs().max(1.0),
                "ln({x})"
            );
        }
        assert_eq!(exp(0.0), Some(1.0));
        assert_eq!(ln(1.0), Some(0.0));
        assert!(exp(710.0).is_none());
        assert!(ln(0.0).is_none());
        assert!(ln(-1.0).is_none());
        // Subnormal arguments take the scaled path.
        let tiny = f64::MIN_POSITIVE / 1024.0;
        assert!((ln(tiny).unwrap() - tiny.ln()).abs() < 1e-13);
    }

    #[test]
    fn trigonometry_tracks_the_host_across_the_reduction_range() {
        for step in -4000..=4000 {
            let x = f64::from(step) * 0.113_9;
            assert!((sin(x).unwrap() - x.sin()).abs() < 2e-15, "sin({x})");
            assert!((cos(x).unwrap() - x.cos()).abs() < 2e-15, "cos({x})");
        }
        for x in [1.0e5 + 0.25, -9.5e5, 1.0e6] {
            assert!((sin(x).unwrap() - x.sin()).abs() < 1e-9, "sin({x})");
        }
        assert!(sin(1.0e6 + 1.0).is_none());
        assert!(cos(f64::INFINITY).is_none());
    }

    #[test]
    fn tanh_and_pow_track_the_host() {
        for step in -300..=300 {
            let x = f64::from(step) * 0.071_3;
            assert!((tanh(x) - x.tanh()).abs() < 1e-15, "tanh({x})");
        }
        assert_eq!(tanh(25.0).to_bits(), 1.0_f64.to_bits());
        assert_eq!(tanh(-25.0).to_bits(), (-1.0_f64).to_bits());
        assert_eq!(pow(2.0, 10.0), Some(1024.0));
        assert!((pow(2.0, 0.5).unwrap() - 2.0_f64.sqrt()).abs() < 1e-15);
        assert!(relative_error(pow(3.7, 2.3).unwrap(), 3.7_f64.powf(2.3)) < 1e-13);
        assert!(relative_error(pow(-2.0, 3.0).unwrap(), -8.0) < 1e-15);
        assert_eq!(pow(0.0, 0.0), Some(1.0));
        assert_eq!(pow(0.0, 2.0), Some(0.0));
        assert!(pow(0.0, -1.0).is_none());
        assert!(pow(-2.0, 0.5).is_none());
        assert!(pow(10.0, 400.0).is_none());
        assert!(integer_power(2.0, -3.0).unwrap() > 0.0);
    }

    #[test]
    fn negative_integer_powers_preserve_representable_subnormals() {
        let actual = pow(1e155, -2.0).unwrap();
        assert!(
            actual > 0.0,
            "a representable reciprocal must not become zero"
        );
        assert!(relative_error(actual, 1e-310) < 1e-12);
        assert_eq!(
            pow(2.0, -64.0).unwrap().to_bits(),
            super::pow2(-64).to_bits()
        );
        assert!(pow(1e-310, -1.0).is_none());
    }

    #[test]
    fn results_are_pinned_bit_for_bit() {
        // A change to a constant or series changes CPU hcode results, so these
        // bit patterns are the regression tripwire for the pinned semantics.
        assert_eq!(exp(1.0).unwrap().to_bits(), 0x4005_bf0a_8b14_5768);
        assert_eq!(ln(2.0).unwrap().to_bits(), 0x3fe6_2e42_fefa_39ef);
        assert_eq!(sin(1.0).unwrap().to_bits(), 0x3fea_ed54_8f09_0cee);
        assert_eq!(cos(1.0).unwrap().to_bits(), 0x3fe1_4a28_0fb5_068b);
        assert_eq!(tanh(0.5).to_bits(), 0x3fdd_9353_d756_8af2);
        assert_eq!(pow(2.0, 0.5).unwrap().to_bits(), 0x3ff6_a09e_667f_3bcd);
    }
}

//! Inter-node activation tensor compression (FP32 / FP16 / BF16 / INT8 dynamic).
//!
//! Compressing intermediate hidden states between hops cuts cross-network
//! bandwidth by 2x (FP16/BF16) or ~4x (INT8 dynamic quantization) while keeping
//! numerical error strictly bounded.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationCodec {
    Fp32,
    Fp16,
    Bf16,
    Int8Dynamic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompressedActivation {
    pub codec: ActivationCodec,
    pub shape: Vec<u32>,
    /// Scale factor used by `Int8Dynamic` (0.0 for floating-point codecs).
    pub scale: f32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    #[error("compressed payload byte length {actual} does not match expected {expected}")]
    LengthMismatch { expected: usize, actual: usize },
    #[error("tensor element count overflow")]
    ShapeOverflow,
}

impl CompressedActivation {
    pub fn element_count(&self) -> Result<usize, CodecError> {
        self.shape.iter().try_fold(1usize, |acc, &dim| {
            acc.checked_mul(dim as usize)
                .ok_or(CodecError::ShapeOverflow)
        })
    }

    pub fn encode(
        codec: ActivationCodec,
        shape: Vec<u32>,
        values: &[f32],
    ) -> Result<Self, CodecError> {
        let expected = shape.iter().try_fold(1usize, |acc, &dim| {
            acc.checked_mul(dim as usize)
                .ok_or(CodecError::ShapeOverflow)
        })?;
        if values.len() != expected {
            return Err(CodecError::LengthMismatch {
                expected,
                actual: values.len(),
            });
        }
        match codec {
            ActivationCodec::Fp32 => {
                let mut data = Vec::with_capacity(values.len() * 4);
                for &v in values {
                    data.extend_from_slice(&v.to_le_bytes());
                }
                Ok(Self {
                    codec,
                    shape,
                    scale: 0.0,
                    data,
                })
            }
            ActivationCodec::Bf16 => {
                let mut data = Vec::with_capacity(values.len() * 2);
                for &v in values {
                    let bits = f32_to_bf16(v);
                    data.extend_from_slice(&bits.to_le_bytes());
                }
                Ok(Self {
                    codec,
                    shape,
                    scale: 0.0,
                    data,
                })
            }
            ActivationCodec::Fp16 => {
                let mut data = Vec::with_capacity(values.len() * 2);
                for &v in values {
                    let bits = f32_to_f16(v);
                    data.extend_from_slice(&bits.to_le_bytes());
                }
                Ok(Self {
                    codec,
                    shape,
                    scale: 0.0,
                    data,
                })
            }
            ActivationCodec::Int8Dynamic => {
                let max_abs = values.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
                let scale = if max_abs == 0.0 { 1.0 } else { max_abs / 127.0 };
                let mut data = Vec::with_capacity(values.len());
                for &v in values {
                    let q = (v / scale).round().clamp(-127.0, 127.0) as i8;
                    data.push(q as u8);
                }
                Ok(Self {
                    codec,
                    shape,
                    scale,
                    data,
                })
            }
        }
    }

    pub fn decode(&self) -> Result<Vec<f32>, CodecError> {
        let n = self.element_count()?;
        match self.codec {
            ActivationCodec::Fp32 => {
                if self.data.len() != n * 4 {
                    return Err(CodecError::LengthMismatch {
                        expected: n * 4,
                        actual: self.data.len(),
                    });
                }
                Ok(self
                    .data
                    .as_chunks::<4>().0.iter()
                    .map(|c| f32::from_le_bytes(*c))
                    .collect())
            }
            ActivationCodec::Bf16 => {
                if self.data.len() != n * 2 {
                    return Err(CodecError::LengthMismatch {
                        expected: n * 2,
                        actual: self.data.len(),
                    });
                }
                Ok(self
                    .data
                    .as_chunks::<2>().0.iter()
                    .map(|c| bf16_to_f32(u16::from_le_bytes(*c)))
                    .collect())
            }
            ActivationCodec::Fp16 => {
                if self.data.len() != n * 2 {
                    return Err(CodecError::LengthMismatch {
                        expected: n * 2,
                        actual: self.data.len(),
                    });
                }
                Ok(self
                    .data
                    .as_chunks::<2>().0.iter()
                    .map(|c| f16_to_f32(u16::from_le_bytes(*c)))
                    .collect())
            }
            ActivationCodec::Int8Dynamic => {
                if self.data.len() != n {
                    return Err(CodecError::LengthMismatch {
                        expected: n,
                        actual: self.data.len(),
                    });
                }
                Ok(self
                    .data
                    .iter()
                    .map(|&b| f32::from(b as i8) * self.scale)
                    .collect())
            }
        }
    }
}

/// Round-to-nearest-even conversion from IEEE-754 f32 to bfloat16.
#[inline]
fn f32_to_bf16(v: f32) -> u16 {
    let bits = v.to_bits();
    if v.is_nan() {
        return ((bits >> 16) as u16) | 0x0040;
    }
    let rounding_bias = 0x7FFF_u32 + ((bits >> 16) & 1);
    (bits.wrapping_add(rounding_bias) >> 16) as u16
}

#[inline]
fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits(u32::from(bits) << 16)
}

/// IEEE-754 f32 to binary16 (FP16) conversion.
#[inline]
fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let frac = bits & 0x007F_FFFF;

    if exp == 0xFF {
        // Inf or NaN
        let nan_bit = if frac != 0 { 0x0200 } else { 0 };
        return sign | 0x7C00 | nan_bit | ((frac >> 13) as u16);
    }

    let new_exp = exp - 127 + 15;
    if new_exp >= 31 {
        return sign | 0x7C00; // overflow -> Inf
    }
    if new_exp <= 0 {
        if new_exp < -10 {
            return sign; // underflow -> signed zero
        }
        let mant = (frac | 0x0080_0000) >> (1 - new_exp + 13);
        return sign | (mant as u16);
    }
    let rounded = frac + 0x1000;
    if (rounded & 0x0080_0000) != 0 {
        let next_exp = new_exp + 1;
        if next_exp >= 31 {
            return sign | 0x7C00;
        }
        return sign | ((next_exp as u16) << 10);
    }
    sign | ((new_exp as u16) << 10) | ((rounded >> 13) as u16)
}

/// IEEE-754 binary16 (FP16) to f32 conversion.
#[inline]
fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exp = (bits >> 10) & 0x1F;
    let frac = u32::from(bits & 0x03FF);

    if exp == 0 {
        if frac == 0 {
            return f32::from_bits(sign);
        }
        // Subnormal
        let mut mant = frac;
        let mut e: i32 = -14;
        while (mant & 0x0400) == 0 {
            mant <<= 1;
            e -= 1;
        }
        mant &= 0x03FF;
        let f32_exp = ((e + 127) as u32) << 23;
        return f32::from_bits(sign | f32_exp | (mant << 13));
    }
    if exp == 0x1F {
        return f32::from_bits(sign | 0x7F80_0000 | (frac << 13));
    }
    let f32_exp = (u32::from(exp) + 127 - 15) << 23;
    f32::from_bits(sign | f32_exp | (frac << 13))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_codecs_round_trip_within_expected_precision_and_size() {
        let input: Vec<f32> = (-16..16).map(|i| (i as f32) * 0.125).collect();
        let shape = vec![1, 32];

        for (codec, expected_bytes, max_err) in [
            (ActivationCodec::Fp32, 128, 0.0_f32),
            (ActivationCodec::Fp16, 64, 0.01),
            (ActivationCodec::Bf16, 64, 0.02),
            (ActivationCodec::Int8Dynamic, 32, 0.03),
        ] {
            let enc = CompressedActivation::encode(codec, shape.clone(), &input).unwrap();
            assert_eq!(enc.data.len(), expected_bytes);
            let dec = enc.decode().unwrap();
            assert_eq!(dec.len(), input.len());
            for (a, b) in input.iter().zip(dec.iter()) {
                assert!(
                    (a - b).abs() <= max_err,
                    "codec {codec:?}: |{a} - {b}| > {max_err}"
                );
            }
        }
    }
}

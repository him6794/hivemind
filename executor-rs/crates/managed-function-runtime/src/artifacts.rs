//! Immutable, digest-pinned tensor inputs for CPU hcode.
//!
//! The host supplies bytes, never paths or URLs. Only named tensors and bounded
//! contiguous slices are exposed to the interpreter. The safetensors header is
//! validated once; numeric decoding happens after the evaluator charges the work.

use std::{collections::BTreeMap, fmt, sync::Arc};

use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor},
};
use sha2::{Digest, Sha256};

use crate::{RuntimeError, Tensor, Value, finite_float, tensor, value_limit_error};

pub const MAX_TENSOR_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TENSOR_ARTIFACT_TOTAL_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_TENSOR_ARTIFACTS: usize = 16;
const MAX_HEADER_BYTES: usize = 1024 * 1024;
const MAX_TENSORS: usize = 4096;
const MAX_NAME_BYTES: usize = 255;

/// Execution-local immutable inputs. Adding one verifies its complete SHA-256,
/// byte limits, and safetensors layout before it becomes visible to hcode.
#[derive(Debug, Default)]
pub struct TensorArtifacts {
    artifacts: BTreeMap<String, TensorArtifact>,
    total_bytes: usize,
}

impl TensorArtifacts {
    pub fn insert(
        &mut self,
        name: String,
        bytes: Vec<u8>,
        expected_sha256: &str,
    ) -> Result<(), RuntimeError> {
        if name.is_empty() || name.len() > MAX_NAME_BYTES || self.artifacts.contains_key(&name) {
            return Err(artifact_error(
                "artifact names must be nonempty, unique, and at most 255 bytes",
            ));
        }
        let next = self
            .total_bytes
            .checked_add(bytes.len())
            .ok_or_else(value_limit_error)?;
        if self.artifacts.len() >= MAX_TENSOR_ARTIFACTS
            || bytes.len() > MAX_TENSOR_ARTIFACT_BYTES
            || next > MAX_TENSOR_ARTIFACT_TOTAL_BYTES
        {
            return Err(value_limit_error());
        }
        let expected = expected_sha256
            .strip_prefix("sha256:")
            .unwrap_or(expected_sha256);
        if expected.len() != 64
            || !expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(artifact_error("artifact digest must be lowercase SHA-256"));
        }
        if format!("{:x}", Sha256::digest(&bytes)) != expected {
            return Err(artifact_error("artifact SHA-256 mismatch"));
        }
        let artifact = TensorArtifact::parse(bytes)?;
        self.artifacts.insert(name, artifact);
        self.total_bytes = next;
        Ok(())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }

    pub(crate) fn get(&self, artifact: &str, name: &str) -> Result<TensorInput<'_>, RuntimeError> {
        let artifact = self
            .artifacts
            .get(artifact)
            .ok_or_else(|| artifact_error("unknown input artifact"))?;
        let descriptor = artifact
            .header
            .0
            .get(name)
            .ok_or_else(|| artifact_error("unknown artifact tensor"))?;
        Ok(TensorInput {
            bytes: &artifact.bytes[artifact.data_start..],
            descriptor,
        })
    }
}

#[derive(Debug)]
struct TensorArtifact {
    bytes: Arc<[u8]>,
    data_start: usize,
    header: Header,
}

impl TensorArtifact {
    fn parse(bytes: Vec<u8>) -> Result<Self, RuntimeError> {
        let prefix: [u8; 8] = bytes
            .get(..8)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| artifact_error("truncated safetensors header length"))?;
        let header_len =
            usize::try_from(u64::from_le_bytes(prefix)).map_err(|_| value_limit_error())?;
        if header_len == 0 || header_len > MAX_HEADER_BYTES {
            return Err(artifact_error("safetensors header exceeds the limit"));
        }
        let data_start = 8_usize
            .checked_add(header_len)
            .ok_or_else(value_limit_error)?;
        let header_bytes = bytes
            .get(8..data_start)
            .ok_or_else(|| artifact_error("truncated safetensors header"))?;
        if header_bytes.first() != Some(&b'{') {
            return Err(artifact_error(
                "safetensors header must begin with an object",
            ));
        }
        let header: Header = serde_json::from_slice(header_bytes)
            .map_err(|error| artifact_error(format!("invalid safetensors header: {error}")))?;
        if header.0.is_empty() {
            return Err(artifact_error("safetensors artifact contains no tensors"));
        }
        let data_len = bytes.len() - data_start;
        let mut ranges = Vec::with_capacity(header.0.len());
        for descriptor in header.0.values() {
            let count = tensor::shape_count(&descriptor.shape)?;
            let size = count
                .checked_mul(descriptor.dtype.bytes())
                .ok_or_else(value_limit_error)?;
            let [start, end] = descriptor.data_offsets;
            if end < start || end > data_len || end - start != size {
                return Err(artifact_error(
                    "tensor byte range does not match its shape and dtype",
                ));
            }
            // Bound even zero-size tensors' dimensions. Larger nonempty tensors
            // remain legal artifacts but can only be decoded in bounded slices.
            if tensor::walk_count(&descriptor.shape)? > MAX_TENSOR_ARTIFACT_BYTES as u64 {
                return Err(value_limit_error());
            }
            ranges.push((start, end));
        }
        ranges.sort_unstable();
        let mut cursor = 0;
        for (start, end) in ranges {
            if start != cursor {
                return Err(artifact_error(
                    "safetensors ranges must be contiguous and non-overlapping",
                ));
            }
            cursor = end;
        }
        if cursor != data_len {
            return Err(artifact_error(
                "safetensors artifact has unreferenced trailing bytes",
            ));
        }
        Ok(Self {
            bytes: bytes.into(),
            data_start,
            header,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    dtype: StorageDType,
    shape: Vec<usize>,
    data_offsets: [usize; 2],
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum StorageDType {
    I8,
    U8,
    I16,
    I32,
    I64,
    F16,
    BF16,
    F32,
    F64,
}

impl StorageDType {
    const fn bytes(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::F16 | Self::BF16 => 2,
            Self::I32 | Self::F32 => 4,
            Self::I64 | Self::F64 => 8,
        }
    }

    const fn is_float(self) -> bool {
        matches!(self, Self::F16 | Self::BF16 | Self::F32 | Self::F64)
    }
}

/// Unlike a plain serde map, reject duplicate tensor names rather than silently
/// letting the last descriptor replace the first one.
#[derive(Debug)]
struct Header(BTreeMap<String, Descriptor>);

impl<'de> Deserialize<'de> for Header {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HeaderVisitor;
        impl<'de> Visitor<'de> for HeaderVisitor {
            type Value = Header;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a safetensors header with unique names")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Header, M::Error> {
                let mut tensors = BTreeMap::new();
                let mut metadata_seen = false;
                while let Some(name) = map.next_key::<String>()? {
                    if name == "__metadata__" {
                        if metadata_seen {
                            return Err(de::Error::custom("duplicate metadata"));
                        }
                        metadata_seen = true;
                        let _ = map.next_value::<BTreeMap<String, String>>()?;
                        continue;
                    }
                    if name.is_empty()
                        || name.len() > MAX_NAME_BYTES
                        || tensors.len() >= MAX_TENSORS
                    {
                        return Err(de::Error::custom("tensor name/count exceeds the limit"));
                    }
                    if tensors.contains_key(&name) {
                        return Err(de::Error::custom("duplicate tensor name"));
                    }
                    tensors.insert(name, map.next_value::<Descriptor>()?);
                }
                Ok(Header(tensors))
            }
        }
        deserializer.deserialize_map(HeaderVisitor)
    }
}

pub(crate) struct TensorInput<'a> {
    bytes: &'a [u8],
    descriptor: &'a Descriptor,
}

impl TensorInput<'_> {
    pub(crate) fn shape(&self) -> &[usize] {
        &self.descriptor.shape
    }

    pub(crate) fn info(&self) -> Value {
        Value::Dict(BTreeMap::from([
            (
                "dtype".to_string(),
                Value::String(format!("{:?}", self.descriptor.dtype)),
            ),
            (
                "shape".to_string(),
                Value::List(
                    self.shape()
                        .iter()
                        .map(|dim| Value::Int(i64::try_from(*dim).unwrap_or(i64::MAX)))
                        .collect(),
                ),
            ),
        ]))
    }

    pub(crate) fn decode(&self, offset: usize, shape: Vec<usize>) -> Result<Tensor, RuntimeError> {
        let count = tensor::shape_count(&shape)?;
        let width = self.descriptor.dtype.bytes();
        let start = offset
            .checked_mul(width)
            .and_then(|offset| self.descriptor.data_offsets[0].checked_add(offset))
            .ok_or_else(value_limit_error)?;
        let end = count
            .checked_mul(width)
            .and_then(|length| start.checked_add(length))
            .ok_or_else(value_limit_error)?;
        if end > self.descriptor.data_offsets[1] {
            return Err(artifact_error("tensor slice is outside the stored tensor"));
        }
        let bytes = self
            .bytes
            .get(start..end)
            .ok_or_else(|| artifact_error("invalid tensor slice"))?;
        if self.descriptor.dtype.is_float() {
            let mut values = Vec::with_capacity(count);
            for chunk in bytes.chunks_exact(width) {
                let value = match self.descriptor.dtype {
                    StorageDType::F64 => f64::from_le_bytes(array(chunk)?),
                    StorageDType::F32 => f64::from(f32::from_le_bytes(array(chunk)?)),
                    StorageDType::BF16 => f64::from(f32::from_bits(
                        u32::from(u16::from_le_bytes(array(chunk)?)) << 16,
                    )),
                    StorageDType::F16 => decode_f16(u16::from_le_bytes(array(chunk)?)),
                    _ => return Err(artifact_error("invalid float dtype")),
                };
                values.push(finite_float(value)?);
            }
            Tensor::from_f64(shape, values)
        } else {
            let mut values = Vec::with_capacity(count);
            for chunk in bytes.chunks_exact(width) {
                values.push(match self.descriptor.dtype {
                    StorageDType::I8 => i64::from(i8::from_le_bytes(array(chunk)?)),
                    StorageDType::U8 => i64::from(u8::from_le_bytes(array(chunk)?)),
                    StorageDType::I16 => i64::from(i16::from_le_bytes(array(chunk)?)),
                    StorageDType::I32 => i64::from(i32::from_le_bytes(array(chunk)?)),
                    StorageDType::I64 => i64::from_le_bytes(array(chunk)?),
                    _ => return Err(artifact_error("invalid integer dtype")),
                });
            }
            Tensor::from_i64(shape, values)
        }
    }
}

fn array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], RuntimeError> {
    bytes
        .try_into()
        .map_err(|_| artifact_error("truncated tensor element"))
}

fn decode_f16(bits: u16) -> f64 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = (bits >> 10) & 31;
    let fraction = bits & 1023;
    match exponent {
        0 => sign * f64::from(fraction) / 16_777_216.0,
        31 => f64::NAN, // All non-finite encodings fail through finite_float.
        _ => sign * f64::from(1024 + fraction) * f64::from_bits(u64::from(exponent + 998) << 52),
    }
}

fn artifact_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new("artifact_error", message)
}

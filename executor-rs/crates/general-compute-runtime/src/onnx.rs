//! Operator-pinned ONNX inference contract for production sandbox backends.
//!
//! The Worker does not load ONNX Runtime, TensorRT, or other native inference
//! libraries. Those libraries live in the operator-pinned guest image and are
//! invoked by its fixed runner. This module binds the artifact roles and
//! execution provider advertised to that runner.

use serde::{Deserialize, Serialize};

pub const ONNX_BACKEND_PROTOCOL_VERSION: &str = "general-compute-onnx-v1";
/// Canonical tensor payload protocol used by the operator-pinned ONNX runner.
pub const ONNX_TENSOR_PROTOCOL_VERSION: &str = "hivemind-onnx-tensor-v1";
pub const ONNX_TENSOR_MIME_TYPE: &str = "application/vnd.hivemind.onnx.tensor+json";
const MAX_ARTIFACT_ID_LENGTH: usize = 128;
const MAX_TENSOR_NAME_LENGTH: usize = 128;
const MAX_TENSOR_RANK: usize = 8;
const MAX_TENSOR_ELEMENTS: u64 = 16_777_216;
const MAX_TENSOR_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnnxExecutionProvider {
    Cpu,
    Cuda,
    #[serde(rename = "tensorrt")]
    TensorRt,
}

impl OnnxExecutionProvider {
    #[must_use]
    pub fn requires_cuda_gpu(self) -> bool {
        matches!(self, Self::Cuda | Self::TensorRt)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
            Self::TensorRt => "tensorrt",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnnxBackendError {
    ProtocolVersionMismatch,
    ModelArtifactIdInvalid,
    InputArtifactIdInvalid,
    DuplicateInputArtifact(String),
    ModelMustBeSourceArtifact,
    InputArtifactSetMismatch,
    InputTensorInvalid(OnnxTensorError),
    DuplicateTensorName(String),
}

impl std::fmt::Display for OnnxBackendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProtocolVersionMismatch => {
                formatter.write_str("ONNX backend protocol version is unsupported")
            }
            Self::ModelArtifactIdInvalid => {
                formatter.write_str("ONNX model artifact id is invalid")
            }
            Self::InputArtifactIdInvalid => {
                formatter.write_str("ONNX input artifact id is invalid")
            }
            Self::DuplicateInputArtifact(id) => {
                write!(formatter, "ONNX input artifact is duplicated: {id}")
            }
            Self::ModelMustBeSourceArtifact => {
                formatter.write_str("ONNX model artifact must be the request source artifact")
            }
            Self::InputArtifactSetMismatch => {
                formatter.write_str("ONNX input artifact ids do not match the request inputs")
            }
            Self::InputTensorInvalid(error) => {
                write!(formatter, "ONNX input tensor is invalid: {error}")
            }
            Self::DuplicateTensorName(name) => {
                write!(formatter, "ONNX input tensor name is duplicated: {name}")
            }
        }
    }
}

impl std::error::Error for OnnxBackendError {}

/// Operator-owned ONNX runner configuration. The model and tensors are
/// delivered through the existing verified artifact mounts; no task-provided
/// path or native library name is accepted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnnxBackendConfig {
    pub protocol_version: String,
    pub model_artifact_id: String,
    pub input_artifact_ids: Vec<String>,
    pub execution_provider: OnnxExecutionProvider,
}

impl OnnxBackendConfig {
    pub fn new(
        model_artifact_id: impl Into<String>,
        input_artifact_ids: Vec<String>,
        execution_provider: OnnxExecutionProvider,
    ) -> Result<Self, OnnxBackendError> {
        let config = Self {
            protocol_version: ONNX_BACKEND_PROTOCOL_VERSION.into(),
            model_artifact_id: model_artifact_id.into(),
            input_artifact_ids,
            execution_provider,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), OnnxBackendError> {
        if self.protocol_version != ONNX_BACKEND_PROTOCOL_VERSION {
            return Err(OnnxBackendError::ProtocolVersionMismatch);
        }
        if !valid_artifact_id(&self.model_artifact_id) {
            return Err(OnnxBackendError::ModelArtifactIdInvalid);
        }
        let mut ids = std::collections::BTreeSet::new();
        for id in &self.input_artifact_ids {
            if !valid_artifact_id(id) {
                return Err(OnnxBackendError::InputArtifactIdInvalid);
            }
            if !ids.insert(id.as_str()) {
                return Err(OnnxBackendError::DuplicateInputArtifact(id.clone()));
            }
        }
        Ok(())
    }

    pub fn validate_request_artifacts(
        &self,
        source_artifact_id: &str,
        input_artifact_ids: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<(), OnnxBackendError> {
        self.validate()?;
        if source_artifact_id != self.model_artifact_id {
            return Err(OnnxBackendError::ModelMustBeSourceArtifact);
        }
        let requested = input_artifact_ids
            .into_iter()
            .map(|id| id.as_ref().to_owned())
            .collect::<Vec<_>>();
        if requested != self.input_artifact_ids {
            return Err(OnnxBackendError::InputArtifactSetMismatch);
        }
        Ok(())
    }

    /// Decode and validate the ordered tensor payloads before they reach the
    /// guest. The model graph is still checked by the pinned runner, but the
    /// Worker owns the transport-level ABI and rejects malformed payloads here.
    pub fn validate_input_tensors(
        &self,
        input_bytes: &[&[u8]],
    ) -> Result<Vec<OnnxTensorEnvelope>, OnnxBackendError> {
        self.validate()?;
        if input_bytes.len() != self.input_artifact_ids.len() {
            return Err(OnnxBackendError::InputArtifactSetMismatch);
        }
        let mut names = std::collections::BTreeSet::new();
        input_bytes
            .iter()
            .map(|bytes| {
                let tensor = OnnxTensorEnvelope::decode_canonical(bytes)
                    .map_err(OnnxBackendError::InputTensorInvalid)?;
                if !names.insert(tensor.name.clone()) {
                    return Err(OnnxBackendError::DuplicateTensorName(tensor.name));
                }
                Ok(tensor)
            })
            .collect()
    }
}

fn valid_artifact_id(value: &str) -> bool {
    value.len() <= MAX_ARTIFACT_ID_LENGTH && crate::validate_artifact_id(value).is_ok()
}

/// The portable tensor payload exchanged with an operator-pinned ONNX guest.
///
/// The model artifact is an ONNX protobuf. Each configured input artifact is a
/// canonical JSON tensor envelope, and each output artifact uses the same
/// envelope. Tensor bytes are little-endian and hex encoded so the ABI has no
/// dependency on a language-specific serializer or a floating-point text
/// representation. There is no implicit batching: the first dimension is part
/// of the declared shape and must be supplied explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnnxTensorEnvelope {
    pub protocol_version: String,
    pub name: String,
    pub dtype: OnnxTensorDType,
    pub shape: Vec<u64>,
    pub data_hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnnxTensorDType {
    Float32,
    Float64,
    Int32,
    Int64,
    Uint8,
    Bool,
}

impl OnnxTensorDType {
    #[must_use]
    pub fn byte_width(self) -> u64 {
        match self {
            Self::Float32 | Self::Int32 => 4,
            Self::Float64 | Self::Int64 => 8,
            Self::Uint8 | Self::Bool => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnnxTensorError {
    Json,
    ProtocolVersionMismatch,
    TensorNameInvalid,
    RankInvalid,
    ShapeInvalid,
    DataEncodingInvalid,
    DataLengthMismatch,
    NonFiniteValue,
    BoolValueInvalid,
    NonCanonicalEncoding,
}

impl std::fmt::Display for OnnxTensorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Json => "ONNX tensor envelope is not valid JSON",
            Self::ProtocolVersionMismatch => "ONNX tensor protocol version is unsupported",
            Self::TensorNameInvalid => "ONNX tensor name is invalid",
            Self::RankInvalid => "ONNX tensor rank is unsupported",
            Self::ShapeInvalid => "ONNX tensor shape is invalid",
            Self::DataEncodingInvalid => "ONNX tensor data is not valid lowercase hex",
            Self::DataLengthMismatch => "ONNX tensor data length does not match its shape",
            Self::NonFiniteValue => "ONNX tensor contains a non-finite floating-point value",
            Self::BoolValueInvalid => "ONNX bool tensor contains a value other than zero or one",
            Self::NonCanonicalEncoding => "ONNX tensor envelope is not canonically encoded",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for OnnxTensorError {}

impl OnnxTensorEnvelope {
    pub fn new(
        name: impl Into<String>,
        dtype: OnnxTensorDType,
        shape: Vec<u64>,
        data: &[u8],
    ) -> Result<Self, OnnxTensorError> {
        if data.len() as u64 > MAX_TENSOR_BYTES {
            return Err(OnnxTensorError::ShapeInvalid);
        }
        let envelope = Self {
            protocol_version: ONNX_TENSOR_PROTOCOL_VERSION.into(),
            name: name.into(),
            dtype,
            shape,
            data_hex: encode_hex(data),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), OnnxTensorError> {
        if self.protocol_version != ONNX_TENSOR_PROTOCOL_VERSION {
            return Err(OnnxTensorError::ProtocolVersionMismatch);
        }
        if self.name.is_empty()
            || self.name.len() > MAX_TENSOR_NAME_LENGTH
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(OnnxTensorError::TensorNameInvalid);
        }
        if self.shape.len() > MAX_TENSOR_RANK {
            return Err(OnnxTensorError::RankInvalid);
        }
        let elements = self.shape.iter().try_fold(1u64, |total, dimension| {
            total
                .checked_mul(*dimension)
                .ok_or(OnnxTensorError::ShapeInvalid)
        })?;
        if elements > MAX_TENSOR_ELEMENTS {
            return Err(OnnxTensorError::ShapeInvalid);
        }
        let expected_bytes = elements
            .checked_mul(self.dtype.byte_width())
            .filter(|bytes| *bytes <= MAX_TENSOR_BYTES)
            .ok_or(OnnxTensorError::ShapeInvalid)?;
        let bytes = decode_hex(&self.data_hex)?;
        if bytes.len() as u64 != expected_bytes {
            return Err(OnnxTensorError::DataLengthMismatch);
        }
        validate_scalar_bytes(self.dtype, &bytes)
    }

    /// Decode only the exact JSON representation emitted by `encode_canonical`.
    pub fn decode_canonical(bytes: &[u8]) -> Result<Self, OnnxTensorError> {
        let envelope: Self = serde_json::from_slice(bytes).map_err(|_| OnnxTensorError::Json)?;
        envelope.validate()?;
        if serde_json::to_vec(&envelope).map_err(|_| OnnxTensorError::Json)? != bytes {
            return Err(OnnxTensorError::NonCanonicalEncoding);
        }
        Ok(envelope)
    }

    pub fn encode_canonical(&self) -> Result<Vec<u8>, OnnxTensorError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|_| OnnxTensorError::Json)
    }

    pub fn data(&self) -> Result<Vec<u8>, OnnxTensorError> {
        self.validate()?;
        decode_hex(&self.data_hex)
    }
}

fn validate_scalar_bytes(dtype: OnnxTensorDType, bytes: &[u8]) -> Result<(), OnnxTensorError> {
    match dtype {
        OnnxTensorDType::Float32 => {
            for chunk in bytes.as_chunks::<4>().0 {
                if !f32::from_le_bytes(*chunk).is_finite() {
                    return Err(OnnxTensorError::NonFiniteValue);
                }
            }
        }
        OnnxTensorDType::Float64 => {
            for chunk in bytes.as_chunks::<8>().0 {
                if !f64::from_le_bytes(*chunk).is_finite() {
                    return Err(OnnxTensorError::NonFiniteValue);
                }
            }
        }
        OnnxTensorDType::Bool => {
            if bytes.iter().any(|byte| *byte > 1) {
                return Err(OnnxTensorError::BoolValueInvalid);
            }
        }
        OnnxTensorDType::Int32 | OnnxTensorDType::Int64 | OnnxTensorDType::Uint8 => {}
    }
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex(value: &str) -> Result<Vec<u8>, OnnxTensorError> {
    if !value.len().is_multiple_of(2)
        || value.len() as u64 > MAX_TENSOR_BYTES.saturating_mul(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(OnnxTensorError::DataEncodingInvalid);
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = hex_digit(pair[0]).ok_or(OnnxTensorError::DataEncodingInvalid)?;
            let low = hex_digit(pair[1]).ok_or(OnnxTensorError::DataEncodingInvalid)?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_binds_model_and_input_artifacts() {
        let config = OnnxBackendConfig::new(
            "source",
            vec!["tensor-0".into(), "tensor-1".into()],
            OnnxExecutionProvider::Cuda,
        )
        .unwrap();
        config
            .validate_request_artifacts("source", ["tensor-0", "tensor-1"])
            .unwrap();
        assert!(config.execution_provider.requires_cuda_gpu());
        assert_eq!(config.execution_provider.as_str(), "cuda");
    }

    #[test]
    fn config_rejects_artifact_drift_and_duplicate_inputs() {
        assert_eq!(
            OnnxBackendConfig::new(
                "source",
                vec!["tensor-0".into(), "tensor-0".into()],
                OnnxExecutionProvider::Cpu,
            )
            .unwrap_err(),
            OnnxBackendError::DuplicateInputArtifact("tensor-0".into())
        );
        let config = OnnxBackendConfig::new(
            "source",
            vec!["tensor-0".into()],
            OnnxExecutionProvider::Cpu,
        )
        .unwrap();
        assert_eq!(
            config
                .validate_request_artifacts("other", ["tensor-0"])
                .unwrap_err(),
            OnnxBackendError::ModelMustBeSourceArtifact
        );
        assert_eq!(
            config
                .validate_request_artifacts("source", ["tensor-1", "tensor-0"])
                .unwrap_err(),
            OnnxBackendError::InputArtifactSetMismatch
        );
        assert_eq!(
            config
                .validate_request_artifacts("source", ["tensor-1"])
                .unwrap_err(),
            OnnxBackendError::InputArtifactSetMismatch
        );
    }

    #[test]
    fn tensor_envelope_round_trips_canonically_with_explicit_little_endian_data() {
        let data = [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40];
        let tensor =
            OnnxTensorEnvelope::new("input_0", OnnxTensorDType::Float32, vec![2], &data).unwrap();
        assert_eq!(tensor.data_hex, "0000803f00000040");
        let encoded = tensor.encode_canonical().unwrap();
        assert_eq!(
            OnnxTensorEnvelope::decode_canonical(&encoded).unwrap(),
            tensor
        );
        assert_eq!(tensor.data().unwrap(), data);
    }

    #[test]
    fn tensor_envelope_rejects_noncanonical_or_unsafe_payloads() {
        let tensor = OnnxTensorEnvelope::new(
            "input",
            OnnxTensorDType::Int64,
            vec![1],
            &1i64.to_le_bytes(),
        )
        .unwrap();
        let encoded = tensor.encode_canonical().unwrap();
        let with_whitespace = [encoded.as_slice(), b"\n"].concat();
        assert_eq!(
            OnnxTensorEnvelope::decode_canonical(&with_whitespace).unwrap_err(),
            OnnxTensorError::NonCanonicalEncoding
        );

        let mut bad = tensor;
        bad.data_hex = "00".into();
        assert_eq!(
            bad.validate().unwrap_err(),
            OnnxTensorError::DataLengthMismatch
        );
        bad.data_hex = "000000000000000000".into();
        assert_eq!(
            bad.validate().unwrap_err(),
            OnnxTensorError::DataLengthMismatch
        );
        bad.data_hex = "000000000000000g".into();
        assert_eq!(
            bad.validate().unwrap_err(),
            OnnxTensorError::DataEncodingInvalid
        );
    }

    #[test]
    fn tensor_envelope_rejects_nonfinite_floats_and_invalid_bools() {
        let nonfinite = OnnxTensorEnvelope::new(
            "output",
            OnnxTensorDType::Float32,
            vec![1],
            &f32::NAN.to_le_bytes(),
        );
        assert_eq!(nonfinite.unwrap_err(), OnnxTensorError::NonFiniteValue);

        let invalid_bool = OnnxTensorEnvelope::new("output", OnnxTensorDType::Bool, vec![1], &[2]);
        assert_eq!(invalid_bool.unwrap_err(), OnnxTensorError::BoolValueInvalid);
    }

    #[test]
    fn backend_validates_ordered_tensor_payloads_and_unique_names() {
        let config = OnnxBackendConfig::new(
            "source",
            vec!["input-0".into(), "input-1".into()],
            OnnxExecutionProvider::Cpu,
        )
        .unwrap();
        let first = OnnxTensorEnvelope::new(
            "input_a",
            OnnxTensorDType::Float32,
            vec![1],
            &1.0f32.to_le_bytes(),
        )
        .unwrap()
        .encode_canonical()
        .unwrap();
        let second = OnnxTensorEnvelope::new(
            "input_b",
            OnnxTensorDType::Int64,
            vec![1],
            &2i64.to_le_bytes(),
        )
        .unwrap()
        .encode_canonical()
        .unwrap();
        let tensors = config
            .validate_input_tensors(&[first.as_slice(), second.as_slice()])
            .unwrap();
        assert_eq!(tensors[0].name, "input_a");
        assert_eq!(tensors[1].dtype, OnnxTensorDType::Int64);

        let duplicate = OnnxTensorEnvelope::new(
            "input_a",
            OnnxTensorDType::Int64,
            vec![1],
            &2i64.to_le_bytes(),
        )
        .unwrap()
        .encode_canonical()
        .unwrap();
        assert_eq!(
            config
                .validate_input_tensors(&[first.as_slice(), duplicate.as_slice()])
                .unwrap_err(),
            OnnxBackendError::DuplicateTensorName("input_a".into())
        );
        assert!(matches!(
            config.validate_input_tensors(&[b"not-json", b"not-json"]),
            Err(OnnxBackendError::InputTensorInvalid(_))
        ));
    }

    #[test]
    fn tensor_provider_serialization_uses_annotation_name() {
        let encoded = serde_json::to_string(&OnnxExecutionProvider::TensorRt).unwrap();
        assert_eq!(encoded, "\"tensorrt\"");
        let decoded: OnnxExecutionProvider = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, OnnxExecutionProvider::TensorRt);
    }
}

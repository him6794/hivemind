use general_compute_runtime::tensor::{
    ByteOrder, TENSOR_ABI_VERSION, TensorDType, TensorLayout, TensorManifest,
};
use general_compute_runtime::{ArtifactChunk, ArtifactManifest, ArtifactRole, sha256_digest};

fn binary_artifact(bytes: &[u8]) -> ArtifactManifest {
    ArtifactManifest {
        artifact_id: "tensor-data".into(),
        role: ArtifactRole::Input,
        size_bytes: bytes.len() as u64,
        mime_type: "application/octet-stream".into(),
        sha256: sha256_digest(bytes),
        chunks: if bytes.is_empty() {
            Vec::new()
        } else {
            vec![ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(bytes),
            }]
        },
        inline_bytes: Some(bytes.to_vec()),
    }
}

fn valid_f64_tensor() -> TensorManifest {
    let mut tensor = TensorManifest {
        abi_version: TENSOR_ABI_VERSION.into(),
        dtype: TensorDType::Float64,
        shape: vec![2, 2],
        byte_order: ByteOrder::Little,
        layout: TensorLayout::C,
        data_artifact: binary_artifact(&[0; 32]),
        logical_sha256: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .into(),
    };
    tensor.logical_sha256 = tensor.canonical_logical_sha256();
    tensor
}

fn tensor_for(
    dtype: TensorDType,
    shape: Vec<u64>,
    byte_order: ByteOrder,
    bytes: &[u8],
) -> TensorManifest {
    let mut tensor = TensorManifest {
        abi_version: TENSOR_ABI_VERSION.into(),
        dtype,
        shape,
        byte_order,
        layout: TensorLayout::C,
        data_artifact: binary_artifact(bytes),
        logical_sha256: String::new(),
    };
    tensor.logical_sha256 = tensor.canonical_logical_sha256();
    tensor
}

#[test]
fn contiguous_tensor_round_trips_and_hashes_independent_of_transport_form() {
    let tensor = valid_f64_tensor();
    tensor
        .validate()
        .expect("valid contiguous tensor should validate");

    let encoded = serde_json::to_vec(&tensor).expect("tensor serializes");
    let decoded: TensorManifest = serde_json::from_slice(&encoded).expect("tensor decodes");
    assert_eq!(decoded, tensor);

    let mut cas = tensor.clone();
    cas.data_artifact.inline_bytes = None;
    cas.validate().expect("chunked CAS tensor should validate");
    assert_eq!(cas.canonical_logical_sha256(), tensor.logical_sha256);
}

#[test]
fn tensor_rejects_shape_product_overflow() {
    let mut tensor = valid_f64_tensor();
    tensor.shape = vec![u64::MAX, 2];
    tensor.logical_sha256 = tensor.canonical_logical_sha256();

    let error = tensor
        .validate()
        .expect_err("shape product overflow must fail closed");
    assert!(error.contains("shape element count overflows"));
}

#[test]
fn tensor_rejects_wrong_contiguous_byte_length() {
    let mut tensor = valid_f64_tensor();
    tensor.data_artifact = binary_artifact(&[0; 8]);
    tensor.logical_sha256 = tensor.canonical_logical_sha256();

    let error = tensor
        .validate()
        .expect_err("wrong byte length must fail closed");
    assert!(error.contains("does not match dtype and shape"));
}

#[test]
fn tensor_accepts_empty_and_zero_dimensional_arrays() {
    let mut empty = TensorManifest {
        abi_version: TENSOR_ABI_VERSION.into(),
        dtype: TensorDType::Float32,
        shape: vec![0, 4],
        byte_order: ByteOrder::Big,
        layout: TensorLayout::F,
        data_artifact: binary_artifact(&[]),
        logical_sha256: String::new(),
    };
    empty.logical_sha256 = empty.canonical_logical_sha256();
    empty.validate().expect("empty tensor should validate");

    let mut scalar = TensorManifest {
        abi_version: TENSOR_ABI_VERSION.into(),
        dtype: TensorDType::Complex128,
        shape: Vec::new(),
        byte_order: ByteOrder::Little,
        layout: TensorLayout::C,
        data_artifact: binary_artifact(&[0; 16]),
        logical_sha256: String::new(),
    };
    scalar.logical_sha256 = scalar.canonical_logical_sha256();
    scalar
        .validate()
        .expect("zero-dimensional tensor should validate");
}

#[test]
fn tensor_rejects_pickle_and_object_payloads() {
    let mut tensor = valid_f64_tensor();
    tensor.data_artifact.mime_type = "application/python-pickle".into();
    tensor.logical_sha256 = tensor.canonical_logical_sha256();
    let error = tensor
        .validate()
        .expect_err("pickle payloads must fail closed");
    assert!(error.contains("binary tensor data"));

    let json = serde_json::to_value(valid_f64_tensor()).expect("tensor serializes");
    let mut object = json;
    object["dtype"] = serde_json::json!("object");
    let error = serde_json::from_value::<TensorManifest>(object)
        .expect_err("object dtype must not be accepted by the ABI");
    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn tensor_rejects_unknown_fields_and_tampered_logical_hash() {
    let json = serde_json::to_value(valid_f64_tensor()).expect("tensor serializes");
    let mut unknown = json.clone();
    unknown["secret"] = serde_json::json!("leak");
    assert!(serde_json::from_value::<TensorManifest>(unknown).is_err());

    let mut tampered = valid_f64_tensor();
    tampered.logical_sha256 = sha256_digest(b"tampered");
    let error = tampered
        .validate()
        .expect_err("logical hash tampering must fail closed");
    assert!(error.contains("logical hash does not match"));
}

#[test]
fn tensor_validates_materialized_bytes_and_canonicalizes_big_endian_components() {
    let tensor = tensor_for(
        TensorDType::Complex64,
        vec![1],
        ByteOrder::Big,
        &[0x3f, 0x80, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00],
    );

    tensor
        .validate_bytes(
            tensor
                .data_artifact
                .inline_bytes
                .as_deref()
                .expect("test tensor has inline bytes"),
        )
        .expect("materialized tensor bytes should validate");
    assert_eq!(
        tensor
            .canonical_little_endian_bytes(
                tensor
                    .data_artifact
                    .inline_bytes
                    .as_deref()
                    .expect("test tensor has inline bytes"),
            )
            .expect("big-endian complex bytes should canonicalize"),
        vec![0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x80]
    );
}

#[test]
fn tensor_validates_signed_magnitude_bigint_and_rejects_noncanonical_payloads() {
    let tensor = tensor_for(
        TensorDType::BigInt,
        Vec::new(),
        ByteOrder::Big,
        &[1, 0x12, 0x34],
    );
    assert_eq!(
        tensor
            .canonical_little_endian_bytes(
                tensor
                    .data_artifact
                    .inline_bytes
                    .as_deref()
                    .expect("test tensor has inline bytes"),
            )
            .expect("big-endian BigInt magnitude should canonicalize"),
        vec![1, 0x34, 0x12]
    );

    let noncanonical = tensor_for(
        TensorDType::BigInt,
        Vec::new(),
        ByteOrder::Big,
        &[0, 0, 0x01],
    );
    let error = noncanonical
        .validate_bytes(
            noncanonical
                .data_artifact
                .inline_bytes
                .as_deref()
                .expect("test tensor has inline bytes"),
        )
        .expect_err("BigInt magnitude must not contain redundant leading zeroes");
    assert!(error.contains("canonical sign-magnitude"));
}

#[test]
fn tensor_rejects_materialized_bytes_that_do_not_match_the_manifest() {
    let tensor = valid_f64_tensor();
    let error = tensor
        .validate_bytes(&[0; 32 - 1])
        .expect_err("truncated tensor bytes must fail closed");
    assert!(error.contains("size") || error.contains("checksum"));
}

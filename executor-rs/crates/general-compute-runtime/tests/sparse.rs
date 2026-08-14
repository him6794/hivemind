use general_compute_runtime::tensor::{
    ByteOrder, SPARSE_TENSOR_ABI_VERSION, SparseFormat, SparseIndexDType, SparseTensorManifest,
    TensorDType,
};
use general_compute_runtime::{ArtifactChunk, ArtifactManifest, ArtifactRole, sha256_digest};

fn binary_artifact(id: &str, bytes: &[u8]) -> ArtifactManifest {
    ArtifactManifest {
        artifact_id: id.into(),
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

fn csr_manifest() -> SparseTensorManifest {
    let mut manifest = SparseTensorManifest {
        abi_version: SPARSE_TENSOR_ABI_VERSION.into(),
        format: SparseFormat::Csr,
        shape: vec![2, 3],
        index_dtype: SparseIndexDType::Uint32,
        byte_order: ByteOrder::Little,
        index_base: 0,
        sorted_indices: true,
        allow_duplicates: false,
        indptr_artifact: Some(binary_artifact(
            "indptr",
            &0u32
                .to_le_bytes()
                .into_iter()
                .chain(1u32.to_le_bytes())
                .chain(2u32.to_le_bytes())
                .collect::<Vec<_>>(),
        )),
        indices_artifact: binary_artifact(
            "indices",
            &1u32
                .to_le_bytes()
                .into_iter()
                .chain(2u32.to_le_bytes())
                .collect::<Vec<_>>(),
        ),
        data_artifact: binary_artifact("data", &[0; 16]),
        data_dtype: TensorDType::Float64,
        logical_sha256: String::new(),
    };
    manifest.logical_sha256 = manifest.canonical_logical_sha256();
    manifest
}

#[test]
fn sparse_csr_manifest_validates_shape_indices_and_data_contract() {
    csr_manifest()
        .validate()
        .expect("valid CSR metadata should validate");
}

#[test]
fn sparse_csc_and_coo_manifests_use_format_specific_index_contracts() {
    let mut csc = csr_manifest();
    csc.format = SparseFormat::Csc;
    csc.indptr_artifact = Some(binary_artifact(
        "csc-indptr",
        &0u32
            .to_le_bytes()
            .into_iter()
            .chain(1u32.to_le_bytes())
            .chain(1u32.to_le_bytes())
            .chain(2u32.to_le_bytes())
            .collect::<Vec<_>>(),
    ));
    csc.logical_sha256 = csc.canonical_logical_sha256();
    csc.validate()
        .expect("CSC should use the same matrix contract");

    let mut coo = csr_manifest();
    coo.format = SparseFormat::Coo;
    coo.indptr_artifact = None;
    coo.indices_artifact = binary_artifact(
        "coo-indices",
        &0u32
            .to_le_bytes()
            .into_iter()
            .chain(1u32.to_le_bytes())
            .chain(1u32.to_le_bytes())
            .chain(2u32.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    coo.logical_sha256 = coo.canonical_logical_sha256();
    coo.validate()
        .expect("COO should validate interleaved coordinates");
}

#[test]
fn sparse_manifest_rejects_format_shape_and_size_mismatches() {
    let mut missing_indptr = csr_manifest();
    missing_indptr.indptr_artifact = None;
    missing_indptr.logical_sha256 = missing_indptr.canonical_logical_sha256();
    let error = missing_indptr
        .validate()
        .expect_err("CSR requires an indptr artifact");
    assert!(error.contains("indptr"));

    let mut coo_with_indptr = csr_manifest();
    coo_with_indptr.format = SparseFormat::Coo;
    coo_with_indptr.logical_sha256 = coo_with_indptr.canonical_logical_sha256();
    let error = coo_with_indptr
        .validate()
        .expect_err("COO must not carry an indptr artifact");
    assert!(error.contains("indptr"));

    let mut bad_data = csr_manifest();
    bad_data.data_artifact = binary_artifact("data", &[0; 8]);
    bad_data.logical_sha256 = bad_data.canonical_logical_sha256();
    let error = bad_data
        .validate()
        .expect_err("sparse data size must match nonzero count and dtype");
    assert!(error.contains("data size"));
}

#[test]
fn sparse_manifest_rejects_invalid_index_policy_and_tampered_hash() {
    let mut invalid_base = csr_manifest();
    invalid_base.index_base = 2;
    invalid_base.logical_sha256 = invalid_base.canonical_logical_sha256();
    let error = invalid_base
        .validate()
        .expect_err("only zero- and one-based sparse indices are supported");
    assert!(error.contains("index base"));

    let mut tampered = csr_manifest();
    tampered.logical_sha256 = sha256_digest(b"tampered");
    let error = tampered
        .validate()
        .expect_err("sparse logical hash tampering must fail closed");
    assert!(error.contains("logical hash"));
}

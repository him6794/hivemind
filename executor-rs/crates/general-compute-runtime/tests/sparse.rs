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

fn manifest_with_bytes(
    mut manifest: SparseTensorManifest,
    indptr: Option<&[u8]>,
    indices: &[u8],
    data: &[u8],
) -> SparseTensorManifest {
    manifest.indptr_artifact = indptr.map(|bytes| binary_artifact("indptr", bytes));
    manifest.indices_artifact = binary_artifact("indices", indices);
    manifest.data_artifact = binary_artifact("data", data);
    manifest.logical_sha256 = manifest.canonical_logical_sha256();
    manifest
}

fn validate_inline(manifest: &SparseTensorManifest) -> Result<(), String> {
    manifest.validate_bytes(
        manifest
            .indptr_artifact
            .as_ref()
            .and_then(|artifact| artifact.inline_bytes.as_deref()),
        manifest
            .indices_artifact
            .inline_bytes
            .as_deref()
            .expect("test indices should be inline"),
        manifest
            .data_artifact
            .inline_bytes
            .as_deref()
            .expect("test data should be inline"),
    )
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

#[test]
fn sparse_materialized_bytes_enforce_csr_bounds_and_indptr_monotonicity() {
    let valid = csr_manifest();
    validate_inline(&valid).expect("valid CSR bytes should validate");

    let bad_indptr = manifest_with_bytes(
        valid.clone(),
        Some(
            &0u32
                .to_le_bytes()
                .into_iter()
                .chain(2u32.to_le_bytes())
                .chain(1u32.to_le_bytes())
                .collect::<Vec<_>>(),
        ),
        valid
            .indices_artifact
            .inline_bytes
            .as_deref()
            .expect("indices should be inline"),
        valid
            .data_artifact
            .inline_bytes
            .as_deref()
            .expect("data should be inline"),
    );
    let error = validate_inline(&bad_indptr).expect_err("indptr must be monotonic");
    assert!(error.contains("indptr"));

    let bad_indices = manifest_with_bytes(
        valid.clone(),
        valid
            .indptr_artifact
            .as_ref()
            .and_then(|artifact| artifact.inline_bytes.as_deref()),
        &0u32
            .to_le_bytes()
            .into_iter()
            .chain(3u32.to_le_bytes())
            .collect::<Vec<_>>(),
        valid
            .data_artifact
            .inline_bytes
            .as_deref()
            .expect("data should be inline"),
    );
    let error = validate_inline(&bad_indices).expect_err("CSR column bounds must be enforced");
    assert!(error.contains("bounds"));
}

#[test]
fn sparse_materialized_bytes_enforce_sorted_and_duplicate_policy_per_segment() {
    let base = csr_manifest();
    let indptr = 0u32
        .to_le_bytes()
        .into_iter()
        .chain(2u32.to_le_bytes())
        .chain(2u32.to_le_bytes())
        .collect::<Vec<_>>();
    let data = base
        .data_artifact
        .inline_bytes
        .as_deref()
        .expect("data should be inline");

    let unsorted = manifest_with_bytes(
        base.clone(),
        Some(&indptr),
        &1u32
            .to_le_bytes()
            .into_iter()
            .chain(0u32.to_le_bytes())
            .collect::<Vec<_>>(),
        data,
    );
    let error = validate_inline(&unsorted).expect_err("sorted policy must be enforced");
    assert!(error.contains("sorted"));

    let duplicate = manifest_with_bytes(
        base.clone(),
        Some(&indptr),
        &0u32
            .to_le_bytes()
            .into_iter()
            .chain(0u32.to_le_bytes())
            .collect::<Vec<_>>(),
        data,
    );
    let error = validate_inline(&duplicate).expect_err("duplicate policy must be enforced");
    assert!(error.contains("duplicate"));

    let mut duplicate_allowed = duplicate;
    duplicate_allowed.allow_duplicates = true;
    duplicate_allowed.logical_sha256 = duplicate_allowed.canonical_logical_sha256();
    validate_inline(&duplicate_allowed).expect("allowed duplicates should validate");

    let mut unsorted_allowed = unsorted;
    unsorted_allowed.sorted_indices = false;
    unsorted_allowed.logical_sha256 = unsorted_allowed.canonical_logical_sha256();
    validate_inline(&unsorted_allowed).expect("unsorted indices should validate when allowed");
}

#[test]
fn sparse_materialized_bytes_validate_csc_and_coo_pairs() {
    let base = csr_manifest();
    let data = base
        .data_artifact
        .inline_bytes
        .as_deref()
        .expect("data should be inline");

    let mut csc = base.clone();
    csc.format = SparseFormat::Csc;
    csc.indptr_artifact = Some(binary_artifact(
        "csc-indptr",
        &0u32
            .to_le_bytes()
            .into_iter()
            .chain(1u32.to_le_bytes())
            .chain(2u32.to_le_bytes())
            .chain(2u32.to_le_bytes())
            .collect::<Vec<_>>(),
    ));
    csc.indices_artifact = binary_artifact(
        "csc-indices",
        &0u32
            .to_le_bytes()
            .into_iter()
            .chain(1u32.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    csc.data_artifact = binary_artifact("csc-data", data);
    csc.logical_sha256 = csc.canonical_logical_sha256();
    validate_inline(&csc).expect("valid CSC bytes should validate");

    let mut coo = base.clone();
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
    coo.data_artifact = binary_artifact("coo-data", data);
    coo.logical_sha256 = coo.canonical_logical_sha256();
    validate_inline(&coo).expect("valid COO pairs should validate");

    let mut bad_coo = coo;
    bad_coo.indices_artifact = binary_artifact(
        "coo-indices",
        &0u32
            .to_le_bytes()
            .into_iter()
            .chain(3u32.to_le_bytes())
            .chain(1u32.to_le_bytes())
            .chain(2u32.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    bad_coo.logical_sha256 = bad_coo.canonical_logical_sha256();
    let error = validate_inline(&bad_coo).expect_err("COO coordinate bounds must be enforced");
    assert!(error.contains("bounds"));
}

#[test]
fn sparse_materialized_bytes_respect_big_endian_one_based_and_signed_indices() {
    let mut one_based = csr_manifest();
    one_based.index_dtype = SparseIndexDType::Int32;
    one_based.byte_order = ByteOrder::Big;
    one_based.index_base = 1;
    let indptr = 1u32
        .to_be_bytes()
        .into_iter()
        .chain(2u32.to_be_bytes())
        .chain(3u32.to_be_bytes())
        .collect::<Vec<_>>();
    let indices = 1u32
        .to_be_bytes()
        .into_iter()
        .chain(2u32.to_be_bytes())
        .collect::<Vec<_>>();
    let data = vec![0; 16];
    one_based = manifest_with_bytes(one_based, Some(&indptr), &indices, &data);
    validate_inline(&one_based).expect("big-endian one-based indices should validate");

    let negative_indices = (-1i32)
        .to_be_bytes()
        .into_iter()
        .chain(2i32.to_be_bytes())
        .collect::<Vec<_>>();
    let negative = manifest_with_bytes(one_based, Some(&indptr), &negative_indices, &data);
    let error = validate_inline(&negative).expect_err("negative signed indices must fail closed");
    assert!(error.contains("non-negative"));
}

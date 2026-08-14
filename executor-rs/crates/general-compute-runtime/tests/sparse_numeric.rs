use general_compute_runtime::sparse_numeric::{
    SparseF64Matrix, SparseNumericError, MAX_REFERENCE_SPARSE_DIM,
};
use general_compute_runtime::tensor::{
    ByteOrder, SparseFormat, SparseIndexDType, SparseTensorManifest, TensorDType,
    SPARSE_TENSOR_ABI_VERSION,
};
use general_compute_runtime::{sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRole};

fn artifact(id: &str, bytes: &[u8]) -> ArtifactManifest {
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

fn u32_bytes(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn f64_bytes(values: &[f64]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn csr_manifest() -> SparseTensorManifest {
    let indptr = u32_bytes(&[0, 1, 2]);
    let indices = u32_bytes(&[1, 2]);
    let data = f64_bytes(&[1.5, -2.0]);
    let mut manifest = SparseTensorManifest {
        abi_version: SPARSE_TENSOR_ABI_VERSION.into(),
        format: SparseFormat::Csr,
        shape: vec![2, 3],
        index_dtype: SparseIndexDType::Uint32,
        byte_order: ByteOrder::Little,
        index_base: 0,
        sorted_indices: true,
        allow_duplicates: false,
        indptr_artifact: Some(artifact("indptr", &indptr)),
        indices_artifact: artifact("indices", &indices),
        data_artifact: artifact("data", &data),
        data_dtype: TensorDType::Float64,
        logical_sha256: String::new(),
    };
    manifest.logical_sha256 = manifest.canonical_logical_sha256();
    manifest
}

fn matrix_from_manifest(
    manifest: &SparseTensorManifest,
) -> Result<SparseF64Matrix, SparseNumericError> {
    SparseF64Matrix::from_materialized(
        manifest,
        manifest
            .indptr_artifact
            .as_ref()
            .and_then(|artifact| artifact.inline_bytes.as_deref()),
        manifest
            .indices_artifact
            .inline_bytes
            .as_deref()
            .expect("indices"),
        manifest
            .data_artifact
            .inline_bytes
            .as_deref()
            .expect("data"),
    )
}

#[test]
fn csr_f64_matrix_vector_product_uses_verified_materialized_bytes() {
    let manifest = csr_manifest();
    let matrix =
        matrix_from_manifest(&manifest).expect("valid CSR materialization should construct");

    assert_eq!(matrix.shape(), [2, 3]);
    assert_eq!(
        matrix.matvec(&[4.0, 3.0, 5.0]).expect("matvec"),
        [4.5, -10.0]
    );
}

#[test]
fn csc_and_coo_f64_matrix_vector_products_match_csr() {
    let vector = [4.0, 3.0, 5.0];
    let expected = [4.5, -10.0];

    let mut csc = csr_manifest();
    csc.format = SparseFormat::Csc;
    csc.indptr_artifact = Some(artifact("indptr", &u32_bytes(&[0, 0, 1, 2])));
    csc.indices_artifact = artifact("indices", &u32_bytes(&[0, 1]));
    csc.logical_sha256 = csc.canonical_logical_sha256();

    let mut coo = csr_manifest();
    coo.format = SparseFormat::Coo;
    coo.indptr_artifact = None;
    coo.indices_artifact = artifact("indices", &u32_bytes(&[0, 1, 1, 2]));
    coo.logical_sha256 = coo.canonical_logical_sha256();

    assert_eq!(
        matrix_from_manifest(&csc)
            .expect("CSC")
            .matvec(&vector)
            .unwrap(),
        expected
    );
    assert_eq!(
        matrix_from_manifest(&coo)
            .expect("COO")
            .matvec(&vector)
            .unwrap(),
        expected
    );
}

#[test]
fn sparse_matvec_sums_allowed_duplicates_and_rejects_bad_vectors() {
    let mut duplicate = csr_manifest();
    duplicate.allow_duplicates = true;
    duplicate.indptr_artifact = Some(artifact("indptr", &u32_bytes(&[0, 2, 2])));
    duplicate.indices_artifact = artifact("indices", &u32_bytes(&[1, 1]));
    duplicate.data_artifact = artifact("data", &f64_bytes(&[1.0, 2.0]));
    duplicate.logical_sha256 = duplicate.canonical_logical_sha256();
    let matrix = matrix_from_manifest(&duplicate).expect("duplicate entries should be allowed");
    assert_eq!(matrix.matvec(&[4.0, 3.0, 5.0]).unwrap(), [9.0, 0.0]);

    assert_eq!(
        matrix.matvec(&[1.0, 2.0]),
        Err(SparseNumericError::VectorLengthMismatch {
            expected: 3,
            actual: 2,
        })
    );
    assert_eq!(
        matrix.matvec(&[1.0, f64::NAN, 2.0]),
        Err(SparseNumericError::NonFiniteValue)
    );
}

#[test]
fn sparse_f64_reference_rejects_unsupported_data_and_dimensions() {
    let mut unsupported = csr_manifest();
    unsupported.data_dtype = TensorDType::Float32;
    unsupported.data_artifact = artifact("data", &[0; 8]);
    unsupported.logical_sha256 = unsupported.canonical_logical_sha256();
    assert_eq!(
        matrix_from_manifest(&unsupported),
        Err(SparseNumericError::UnsupportedDataType(
            TensorDType::Float32
        ))
    );

    let mut oversized = csr_manifest();
    oversized.shape = vec![(MAX_REFERENCE_SPARSE_DIM as u64) + 1, 3];
    assert!(matches!(
        matrix_from_manifest(&oversized),
        Err(SparseNumericError::ShapeExceeded { .. })
    ));
}

#[test]
fn sparse_f64_reference_decodes_one_based_big_endian_indices() {
    let mut manifest = csr_manifest();
    manifest.index_dtype = SparseIndexDType::Int32;
    manifest.byte_order = ByteOrder::Big;
    manifest.index_base = 1;
    manifest.indptr_artifact = Some(artifact(
        "indptr",
        &1i32
            .to_be_bytes()
            .into_iter()
            .chain(2i32.to_be_bytes())
            .chain(3i32.to_be_bytes())
            .collect::<Vec<_>>(),
    ));
    manifest.indices_artifact = artifact(
        "indices",
        &2i32
            .to_be_bytes()
            .into_iter()
            .chain(3i32.to_be_bytes())
            .collect::<Vec<_>>(),
    );
    manifest.data_artifact = artifact(
        "data",
        &1.5f64
            .to_be_bytes()
            .into_iter()
            .chain((-2.0f64).to_be_bytes())
            .collect::<Vec<_>>(),
    );
    manifest.logical_sha256 = manifest.canonical_logical_sha256();

    assert_eq!(
        matrix_from_manifest(&manifest)
            .expect("one-based big-endian sparse matrix")
            .matvec(&[4.0, 3.0, 5.0])
            .unwrap(),
        [4.5, -10.0]
    );
}

#[test]
fn sparse_f64_reference_rejects_nonfinite_materialized_values() {
    let mut manifest = csr_manifest();
    manifest.data_artifact = artifact("data", &f64_bytes(&[f64::NAN, -2.0]));
    manifest.logical_sha256 = manifest.canonical_logical_sha256();

    assert_eq!(
        matrix_from_manifest(&manifest),
        Err(SparseNumericError::NonFiniteValue)
    );
}

#[test]
fn sparse_matvec_reports_and_enforces_a_residual_tolerance() {
    let matrix = matrix_from_manifest(&csr_manifest()).expect("valid CSR");
    let vector = [4.0, 3.0, 5.0];
    let rhs = [4.0, -10.0];

    let residual = matrix
        .residual_inf_norm(&vector, &rhs)
        .expect("residual should be computable");
    assert_eq!(residual, 0.5);
    assert!(matrix
        .matvec_with_residual_tolerance(&vector, &rhs, 0.5)
        .is_ok());
    assert_eq!(
        matrix.matvec_with_residual_tolerance(&vector, &rhs, 0.25),
        Err(SparseNumericError::ResidualExceeded)
    );
}

#[test]
fn sparse_residual_gate_rejects_invalid_tolerance_and_rhs() {
    let matrix = matrix_from_manifest(&csr_manifest()).expect("valid CSR");
    let vector = [4.0, 3.0, 5.0];
    let rhs = [4.5, -10.0];

    assert_eq!(
        matrix.matvec_with_residual_tolerance(&vector, &rhs, -1.0),
        Err(SparseNumericError::InvalidResidualTolerance)
    );
    assert_eq!(
        matrix.matvec_with_residual_tolerance(&vector, &rhs, f64::NAN),
        Err(SparseNumericError::InvalidResidualTolerance)
    );
    assert_eq!(
        matrix.residual_inf_norm(&vector, &[4.5]),
        Err(SparseNumericError::ResidualLengthMismatch {
            expected: 2,
            actual: 1,
        })
    );
}

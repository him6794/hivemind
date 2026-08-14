use general_compute_runtime::numeric::{F64Tensor, NumericError, MAX_REFERENCE_SVD_DIM};

fn assert_close(actual: &[f64], expected: &[f64], tolerance: f64) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} is not within {tolerance} of {expected}"
        );
    }
}

#[test]
fn f64_svd_reconstructs_orders_singular_values_and_checks_orthogonality() {
    let matrix = F64Tensor::new(vec![3, 2], vec![0.0, 2.0, 3.0, 0.0, 0.0, 0.0])
        .expect("matrix shape should be valid");

    let factor = matrix.svd().expect("full-rank matrix should factor");
    assert_eq!(factor.u().shape(), &[3, 2]);
    assert_eq!(factor.singular_values().shape(), &[2]);
    assert_eq!(factor.vt().shape(), &[2, 2]);
    assert_close(factor.singular_values().values(), &[3.0, 2.0], 1e-12);
    assert!(
        factor
            .orthogonality_inf_norm()
            .expect("factors should be finite")
            <= 1e-14
    );
    assert_close(
        factor
            .reconstruct()
            .expect("U*S*Vᵀ should reconstruct")
            .values(),
        matrix.values(),
        1e-12,
    );
    assert!(
        factor
            .reconstruction_inf_norm(&matrix)
            .expect("reconstruction error should be computable")
            <= 1e-12
    );
    assert!(matrix.svd_with_tolerance(1e-12).is_ok());
    let nontrivial = F64Tensor::new(vec![3, 2], vec![12.0, -51.0, 6.0, 167.0, -4.0, 24.0]).unwrap();
    assert_eq!(
        nontrivial.svd_with_tolerance(1e-18),
        Err(NumericError::SvdErrorExceeded)
    );
}

#[test]
fn f64_svd_reconstructs_wide_rank_deficient_matrices() {
    let matrix = F64Tensor::new(vec![2, 3], vec![1.0, 2.0, 3.0, 2.0, 4.0, 6.0])
        .expect("matrix shape should be valid");

    let factor = matrix.svd().expect("rank-deficient matrix should factor");
    assert_eq!(factor.u().shape(), &[2, 2]);
    assert_eq!(factor.singular_values().shape(), &[2]);
    assert_eq!(factor.vt().shape(), &[2, 3]);
    assert!(factor.singular_values().values()[0] > 8.0);
    assert!(factor.singular_values().values()[1] <= 1e-12);
    assert!(
        factor
            .orthogonality_inf_norm()
            .expect("factors should be finite")
            <= 1e-12
    );
    assert!(
        factor
            .reconstruction_inf_norm(&matrix)
            .expect("reconstruction error should be computable")
            <= 1e-12
    );
}

#[test]
fn f64_svd_rejects_invalid_inputs_and_dimension_cap() {
    let vector = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).unwrap();
    assert_eq!(vector.svd(), Err(NumericError::SvdRequiresTwoDimensions));

    let nonfinite = F64Tensor::new(vec![2, 2], vec![1.0, f64::NAN, 0.0, 1.0]).unwrap();
    assert_eq!(nonfinite.svd(), Err(NumericError::NonFiniteValue));

    let matrix = F64Tensor::new(vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]).unwrap();
    assert_eq!(
        matrix.svd_with_tolerance(-1.0),
        Err(NumericError::InvalidSvdTolerance)
    );
    assert_eq!(
        matrix.svd_with_tolerance(f64::NAN),
        Err(NumericError::InvalidSvdTolerance)
    );

    let rows = MAX_REFERENCE_SVD_DIM + 1;
    let over_cap = F64Tensor::new(vec![rows as u64, 1], vec![1.0; rows]).unwrap();
    assert_eq!(
        over_cap.svd(),
        Err(NumericError::SvdDimensionExceeded {
            rows,
            columns: 1,
            max: MAX_REFERENCE_SVD_DIM,
        })
    );
}

#[test]
fn f64_svd_reconstruction_rejects_an_incompatible_original() {
    let matrix = F64Tensor::new(vec![2, 1], vec![3.0, 4.0]).unwrap();
    let factor = matrix.svd().expect("column should factor");
    let wrong_shape = F64Tensor::new(vec![2, 2], vec![3.0, 4.0, 0.0, 0.0]).unwrap();

    assert_eq!(
        factor.reconstruction_inf_norm(&wrong_shape),
        Err(NumericError::SvdDimensionMismatch {
            expected_rows: 2,
            expected_columns: 1,
            actual: vec![2, 2],
        })
    );
}

use general_compute_runtime::numeric::{F64Tensor, NumericError, MAX_REFERENCE_QR_DIM};

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
fn f64_qr_reconstructs_and_checks_orthogonality() {
    let matrix = F64Tensor::new(vec![3, 2], vec![12.0, -51.0, 6.0, 167.0, -4.0, 24.0])
        .expect("matrix shape should be valid");

    let factor = matrix.qr().expect("full-rank matrix should factor");
    assert_eq!(factor.orthogonal().shape(), &[3, 2]);
    assert_eq!(factor.upper().shape(), &[2, 2]);
    assert!(factor.orthogonality_inf_norm().expect("Q should be finite") <= 1e-14);
    assert_close(
        factor
            .reconstruct()
            .expect("Q*R should reconstruct")
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
    assert!(matrix.qr_with_tolerance(1e-12).is_ok());
    assert_eq!(
        matrix.qr_with_tolerance(1e-18),
        Err(NumericError::QrErrorExceeded)
    );
}

#[test]
fn f64_qr_rejects_invalid_shapes_rank_and_nonfinite_inputs() {
    let vector = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).unwrap();
    assert_eq!(vector.qr(), Err(NumericError::QrRequiresTwoDimensions));

    let wide = F64Tensor::new(vec![2, 3], vec![1.0; 6]).unwrap();
    assert_eq!(
        wide.qr(),
        Err(NumericError::QrRequiresAtLeastAsManyRows {
            rows: 2,
            columns: 3,
        })
    );

    let singular = F64Tensor::new(vec![2, 2], vec![1.0, 2.0, 2.0, 4.0]).unwrap();
    assert_eq!(singular.qr(), Err(NumericError::SingularMatrix));

    let nonfinite = F64Tensor::new(vec![2, 2], vec![1.0, f64::NAN, 0.0, 1.0]).unwrap();
    assert_eq!(nonfinite.qr(), Err(NumericError::NonFiniteValue));

    let matrix = F64Tensor::new(vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]).unwrap();
    assert_eq!(
        matrix.qr_with_tolerance(-1.0),
        Err(NumericError::InvalidQrTolerance)
    );
    assert_eq!(
        matrix.qr_with_tolerance(f64::NAN),
        Err(NumericError::InvalidQrTolerance)
    );
}

#[test]
fn f64_qr_rejects_dimensions_above_the_reference_cap() {
    let rows = MAX_REFERENCE_QR_DIM + 1;
    let matrix = F64Tensor::new(vec![rows as u64, 1], vec![1.0; rows])
        .expect("bounded test matrix should be valid");

    assert_eq!(
        matrix.qr(),
        Err(NumericError::QrDimensionExceeded {
            rows,
            columns: 1,
            max: MAX_REFERENCE_QR_DIM,
        })
    );
}

#[test]
fn f64_qr_reconstruction_rejects_an_incompatible_original() {
    let matrix = F64Tensor::new(vec![2, 1], vec![3.0, 4.0]).unwrap();
    let factor = matrix.qr().expect("column should factor");
    let wrong_shape = F64Tensor::new(vec![2, 2], vec![3.0, 4.0, 0.0, 0.0]).unwrap();

    assert_eq!(
        factor.reconstruction_inf_norm(&wrong_shape),
        Err(NumericError::QrDimensionMismatch {
            expected_rows: 2,
            expected_columns: 1,
            actual: vec![2, 2],
        })
    );
}

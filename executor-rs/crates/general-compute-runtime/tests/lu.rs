use general_compute_runtime::numeric::{F64Tensor, NumericError, MAX_REFERENCE_LU_DIM};

#[test]
fn f64_lu_factorization_pivots_reconstructs_and_solves() {
    let matrix =
        F64Tensor::new(vec![2, 2], vec![0.0, 2.0, 1.0, 2.0]).expect("matrix shape should be valid");
    let rhs = F64Tensor::new(vec![2], vec![4.0, 5.0]).expect("rhs shape should be valid");

    let factor = matrix.lu().expect("pivoted matrix should factor");
    assert_eq!(factor.permutation(), &[1, 0]);
    assert_eq!(factor.lower().values(), &[1.0, 0.0, 0.0, 1.0]);
    assert_eq!(factor.upper().values(), &[1.0, 2.0, 0.0, 2.0]);
    assert_eq!(
        factor
            .reconstruct_permuted()
            .expect("LU product should reconstruct")
            .values(),
        &[1.0, 2.0, 0.0, 2.0]
    );
    assert_eq!(
        factor
            .solve(&rhs)
            .expect("LU solve should succeed")
            .values(),
        &[1.0, 2.0]
    );
}

#[test]
fn f64_lu_rejects_invalid_shapes_singular_and_nonfinite_inputs() {
    let non_square = F64Tensor::new(vec![2, 3], vec![1.0; 6]).expect("shape should be valid");
    assert_eq!(non_square.lu(), Err(NumericError::LuRequiresSquareMatrix));

    let singular = F64Tensor::new(vec![2, 2], vec![1.0, 2.0, 2.0, 4.0]).unwrap();
    assert_eq!(singular.lu(), Err(NumericError::SingularMatrix));

    let nonfinite = F64Tensor::new(vec![2, 2], vec![1.0, f64::NAN, 0.0, 1.0]).unwrap();
    assert_eq!(nonfinite.lu(), Err(NumericError::NonFiniteValue));

    let matrix = F64Tensor::new(vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]).unwrap();
    let factor = matrix.lu().expect("identity should factor");
    let wrong_rhs = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).unwrap();
    assert_eq!(
        factor.solve(&wrong_rhs),
        Err(NumericError::SolveDimensionMismatch { matrix: 2, rhs: 3 })
    );
}

#[test]
fn f64_lu_rejects_dimensions_above_the_reference_cap() {
    let dimension = MAX_REFERENCE_LU_DIM + 1;
    let matrix = F64Tensor::new(
        vec![dimension as u64, dimension as u64],
        vec![0.0; dimension * dimension],
    )
    .expect("bounded test matrix should be valid");

    assert_eq!(
        matrix.lu(),
        Err(NumericError::LuDimensionExceeded {
            dimension,
            max: MAX_REFERENCE_LU_DIM,
        })
    );
}

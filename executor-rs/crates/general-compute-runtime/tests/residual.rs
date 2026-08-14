use general_compute_runtime::numeric::{F64Tensor, NumericError};

#[test]
fn f64_solve_reports_and_enforces_a_residual_tolerance() {
    let matrix = F64Tensor::new(
        vec![3, 3],
        vec![0.7, 0.2, 0.1, 0.3, 0.8, 0.4, 0.2, 0.5, 0.9],
    )
    .expect("matrix shape should be valid");
    let rhs = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).expect("rhs shape should be valid");

    let solution = matrix.solve(&rhs).expect("system should solve");
    let residual = matrix
        .residual_inf_norm(&solution, &rhs)
        .expect("residual should be computable");
    assert!(residual.is_finite());
    assert!(residual <= 1e-15);
    assert!(matrix.solve_with_residual(&rhs, 1e-15).is_ok());
    assert_eq!(
        matrix.solve_with_residual(&rhs, 1e-18),
        Err(NumericError::ResidualExceeded)
    );
}

#[test]
fn f64_solve_residual_gate_rejects_invalid_tolerances() {
    let matrix = F64Tensor::new(vec![1, 1], vec![2.0]).expect("matrix shape should be valid");
    let rhs = F64Tensor::new(vec![1], vec![4.0]).expect("rhs shape should be valid");

    assert_eq!(
        matrix.solve_with_residual(&rhs, -1.0),
        Err(NumericError::InvalidResidualTolerance)
    );
    assert_eq!(
        matrix.solve_with_residual(&rhs, f64::NAN),
        Err(NumericError::InvalidResidualTolerance)
    );
}

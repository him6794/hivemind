use general_compute_runtime::numeric::{BinaryOp, F64Tensor, NumericError};

#[test]
fn elementwise_add_uses_numpy_style_trailing_broadcast() {
    let lhs = F64Tensor::new(vec![2, 1], vec![1.0, 2.0]).expect("lhs should be valid");
    let rhs = F64Tensor::new(vec![1, 3], vec![10.0, 20.0, 30.0]).expect("rhs should be valid");

    let result = lhs
        .elementwise_binary(&rhs, BinaryOp::Add)
        .expect("broadcast add should succeed");

    assert_eq!(result.shape(), &[2, 3]);
    assert_eq!(result.values(), &[11.0, 21.0, 31.0, 12.0, 22.0, 32.0]);
}

#[test]
fn reduce_sum_supports_axes_and_keep_dimensions() {
    let tensor = F64Tensor::new(vec![2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
        .expect("tensor should be valid");

    let reduced = tensor
        .reduce_sum(&[1], false)
        .expect("axis reduction should succeed");
    assert_eq!(reduced.shape(), &[2]);
    assert_eq!(reduced.values(), &[6.0, 15.0]);

    let kept = tensor
        .reduce_sum(&[0, 1], true)
        .expect("all-axis reduction should succeed");
    assert_eq!(kept.shape(), &[1, 1]);
    assert_eq!(kept.values(), &[21.0]);
}

#[test]
fn numeric_kernels_handle_scalars_empty_dimensions_and_reject_invalid_shapes() {
    let scalar = F64Tensor::new(Vec::new(), vec![2.5]).expect("scalar should be valid");
    let vector = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).expect("vector should be valid");
    let result = scalar
        .elementwise_binary(&vector, BinaryOp::Mul)
        .expect("scalar should broadcast");
    assert_eq!(result.shape(), &[3]);
    assert_eq!(result.values(), &[2.5, 5.0, 7.5]);

    let empty = F64Tensor::new(vec![2, 0], Vec::new()).expect("empty dimension is valid");
    let reduced = empty
        .reduce_sum(&[1], false)
        .expect("sum over an empty dimension should be zero");
    assert_eq!(reduced.shape(), &[2]);
    assert_eq!(reduced.values(), &[0.0, 0.0]);

    let mismatch = F64Tensor::new(vec![2], vec![1.0, 2.0]).expect("tensor should be valid");
    let other = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).expect("tensor should be valid");
    assert!(matches!(
        mismatch.elementwise_binary(&other, BinaryOp::Add),
        Err(NumericError::BroadcastIncompatible { .. })
    ));
    assert!(matches!(
        vector.reduce_sum(&[1], false),
        Err(NumericError::AxisOutOfBounds { .. })
    ));
}

#[test]
fn numeric_kernels_reject_duplicate_axes_and_value_count_mismatches() {
    let error = F64Tensor::new(vec![2, 2], vec![1.0]).expect_err("value count must match shape");
    assert!(matches!(error, NumericError::ValueCountMismatch { .. }));

    let tensor = F64Tensor::new(vec![2, 2], vec![1.0, 2.0, 3.0, 4.0]).unwrap();
    assert!(matches!(
        tensor.reduce_sum(&[0, 0], false),
        Err(NumericError::DuplicateAxis { axis: 0 })
    ));
}

use general_compute_runtime::numeric::{
    BinaryOp, Complex128, Complex128Tensor, Complex64, Complex64Tensor, F32Tensor, F64Tensor,
    NumericError,
};

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

#[test]
fn matmul_computes_a_bounded_two_dimensional_reference_result() {
    let lhs = F64Tensor::new(vec![2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).unwrap();
    let rhs = F64Tensor::new(vec![3, 2], vec![7.0, 8.0, 9.0, 10.0, 11.0, 12.0]).unwrap();

    let result = lhs.matmul(&rhs).expect("2-D matmul should succeed");

    assert_eq!(result.shape(), &[2, 2]);
    assert_eq!(result.values(), &[58.0, 64.0, 139.0, 154.0]);
}

#[test]
fn matmul_rejects_non_matrix_inputs_and_inner_dimension_mismatches() {
    let vector = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).unwrap();
    let matrix = F64Tensor::new(vec![3, 1], vec![4.0, 5.0, 6.0]).unwrap();
    assert!(matches!(
        vector.matmul(&matrix),
        Err(NumericError::MatmulRequiresTwoDimensions)
    ));

    let lhs = F64Tensor::new(vec![2, 3], vec![1.0; 6]).unwrap();
    let rhs = F64Tensor::new(vec![2, 2], vec![1.0; 4]).unwrap();
    assert!(matches!(
        lhs.matmul(&rhs),
        Err(NumericError::MatmulInnerDimensionMismatch { lhs: 3, rhs: 2 })
    ));
}

#[test]
fn matmul_handles_zero_inner_dimensions_without_nan_or_allocation_errors() {
    let lhs = F64Tensor::new(vec![2, 0], Vec::new()).unwrap();
    let rhs = F64Tensor::new(vec![0, 3], Vec::new()).unwrap();

    let result = lhs.matmul(&rhs).expect("zero-width matmul should succeed");

    assert_eq!(result.shape(), &[2, 3]);
    assert_eq!(result.values(), &[0.0; 6]);
}

#[test]
fn dtype_specific_dense_tensors_preserve_f32_and_complex64_arithmetic() {
    let f32_lhs = F32Tensor::new(vec![2], vec![1.5_f32, 2.5]).unwrap();
    let f32_rhs = F32Tensor::new(vec![1], vec![2.0_f32]).unwrap();
    let f32_result = f32_lhs
        .elementwise_binary(&f32_rhs, BinaryOp::Mul)
        .expect("f32 multiplication should broadcast");
    assert_eq!(f32_result.values(), &[3.0_f32, 5.0]);

    let complex_lhs = Complex64Tensor::new(vec![1], vec![Complex64::new(1.0, 2.0)]).unwrap();
    let complex_rhs = Complex64Tensor::new(vec![1], vec![Complex64::new(3.0, -4.0)]).unwrap();
    let complex_result = complex_lhs
        .elementwise_binary(&complex_rhs, BinaryOp::Mul)
        .expect("complex64 multiplication should succeed");
    assert_eq!(complex_result.values(), &[Complex64::new(11.0, 2.0)]);

    let complex128_lhs = Complex128Tensor::new(vec![1], vec![Complex128::new(2.0, 1.0)]).unwrap();
    let complex128_rhs = Complex128Tensor::new(vec![1], vec![Complex128::new(4.0, -3.0)]).unwrap();
    let complex128_result = complex128_lhs
        .elementwise_binary(&complex128_rhs, BinaryOp::Add)
        .expect("complex128 addition should succeed");
    assert_eq!(complex128_result.values(), &[Complex128::new(6.0, -2.0)]);
}

#[test]
fn dot_computes_a_bounded_f64_vector_product() {
    let lhs = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).unwrap();
    let rhs = F64Tensor::new(vec![3], vec![4.0, 5.0, 6.0]).unwrap();

    assert_eq!(lhs.dot(&rhs).expect("matching vectors should dot"), 32.0);
}

#[test]
fn dot_preserves_f32_and_complex64_arithmetic() {
    let f32_lhs = F32Tensor::new(vec![2], vec![1.5_f32, 2.0]).unwrap();
    let f32_rhs = F32Tensor::new(vec![2], vec![2.0_f32, 4.0]).unwrap();
    assert_eq!(f32_lhs.dot(&f32_rhs).unwrap(), 11.0_f32);

    let complex_lhs = Complex64Tensor::new(
        vec![2],
        vec![Complex64::new(1.0, 2.0), Complex64::new(3.0, 4.0)],
    )
    .unwrap();
    let complex_rhs = Complex64Tensor::new(
        vec![2],
        vec![Complex64::new(5.0, 6.0), Complex64::new(7.0, 8.0)],
    )
    .unwrap();
    assert_eq!(
        complex_lhs.dot(&complex_rhs).unwrap(),
        Complex64::new(-18.0, 68.0)
    );
}

#[test]
fn dot_rejects_non_vectors_and_mismatched_lengths() {
    let vector = F64Tensor::new(vec![2], vec![1.0, 2.0]).unwrap();
    let matrix = F64Tensor::new(vec![1, 2], vec![1.0, 2.0]).unwrap();
    assert_eq!(
        vector.dot(&matrix),
        Err(NumericError::DotRequiresOneDimension)
    );

    let other = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).unwrap();
    assert_eq!(
        vector.dot(&other),
        Err(NumericError::DotLengthMismatch { lhs: 2, rhs: 3 })
    );
}

#[test]
fn batched_matmul_broadcasts_a_single_batch_and_preserves_typed_results() {
    let lhs = F64Tensor::new(vec![2, 2, 2], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]).unwrap();
    let rhs = F64Tensor::new(vec![1, 2, 2], vec![2.0, 0.0, 1.0, 2.0]).unwrap();

    let result = lhs
        .batched_matmul(&rhs)
        .expect("a single rhs batch should broadcast");
    assert_eq!(result.shape(), &[2, 2, 2]);
    assert_eq!(
        result.values(),
        &[4.0, 4.0, 10.0, 8.0, 16.0, 12.0, 22.0, 16.0]
    );
}

#[test]
fn batched_matmul_handles_zero_inner_dimensions_and_complex_values() {
    let empty_lhs = F64Tensor::new(vec![2, 1, 0], vec![]).unwrap();
    let empty_rhs = F64Tensor::new(vec![1, 0, 3], vec![]).unwrap();
    let empty_result = empty_lhs.batched_matmul(&empty_rhs).unwrap();
    assert_eq!(empty_result.shape(), &[2, 1, 3]);
    assert_eq!(empty_result.values(), &[0.0; 6]);

    let complex_lhs = Complex64Tensor::new(
        vec![1, 1, 2],
        vec![Complex64::new(1.0, 1.0), Complex64::new(2.0, 0.0)],
    )
    .unwrap();
    let complex_rhs = Complex64Tensor::new(
        vec![1, 2, 1],
        vec![Complex64::new(3.0, 0.0), Complex64::new(4.0, 1.0)],
    )
    .unwrap();
    let complex_result = complex_lhs.batched_matmul(&complex_rhs).unwrap();
    assert_eq!(complex_result.values(), &[Complex64::new(11.0, 5.0)]);
}

#[test]
fn batched_matmul_rejects_rank_batch_and_inner_mismatches() {
    let vector = F64Tensor::new(vec![2], vec![1.0, 2.0]).unwrap();
    let matrix = F64Tensor::new(vec![1, 2], vec![1.0, 2.0]).unwrap();
    assert_eq!(
        vector.batched_matmul(&matrix),
        Err(NumericError::BatchedMatmulRequiresThreeDimensions)
    );

    let lhs = F64Tensor::new(vec![2, 1, 2], vec![1.0; 4]).unwrap();
    let rhs_batch = F64Tensor::new(vec![3, 2, 1], vec![1.0; 6]).unwrap();
    assert_eq!(
        lhs.batched_matmul(&rhs_batch),
        Err(NumericError::BatchedMatmulBatchMismatch { lhs: 2, rhs: 3 })
    );

    let rhs_inner = F64Tensor::new(vec![1, 3, 1], vec![1.0; 3]).unwrap();
    assert_eq!(
        lhs.batched_matmul(&rhs_inner),
        Err(NumericError::BatchedMatmulInnerDimensionMismatch { lhs: 2, rhs: 3 })
    );
}

use general_compute_runtime::numeric::{Complex128, Complex128Tensor, NumericError};

#[test]
fn complex128_fft_reports_and_enforces_round_trip_error() {
    let input = Complex128Tensor::new(
        vec![4],
        vec![
            Complex128::new(1.0, 0.0),
            Complex128::new(2.0, -1.0),
            Complex128::new(3.0, 2.0),
            Complex128::new(4.0, 0.5),
        ],
    )
    .expect("FFT input should be valid");

    let error = input
        .fft_round_trip_error_inf_norm()
        .expect("round-trip error should be computable");
    assert!(error.is_finite());
    assert!(error < 1e-10);
    assert!(input.fft_with_round_trip_tolerance(1e-10).is_ok());
    assert_eq!(
        input.fft_with_round_trip_tolerance(0.0),
        Err(NumericError::FftErrorExceeded)
    );
}

#[test]
fn complex128_fft_accuracy_gate_rejects_invalid_tolerances() {
    let input = Complex128Tensor::new(vec![1], vec![Complex128::new(2.0, -1.0)])
        .expect("FFT input should be valid");
    assert_eq!(
        input.fft_with_round_trip_tolerance(-1.0),
        Err(NumericError::InvalidFftTolerance)
    );
    assert_eq!(
        input.fft_with_round_trip_tolerance(f64::NAN),
        Err(NumericError::InvalidFftTolerance)
    );
}

#[test]
fn complex128_fft_matches_the_impulse_golden_vector() {
    let input = Complex128Tensor::new(
        vec![4],
        vec![
            Complex128::new(1.0, 0.0),
            Complex128::default(),
            Complex128::default(),
            Complex128::default(),
        ],
    )
    .expect("FFT input should be valid");
    let spectrum = input.fft(false).expect("impulse FFT should run");
    assert_eq!(
        spectrum.values(),
        &[
            Complex128::new(1.0, 0.0),
            Complex128::new(1.0, 0.0),
            Complex128::new(1.0, 0.0),
            Complex128::new(1.0, 0.0),
        ]
    );
}

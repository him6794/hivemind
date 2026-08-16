use general_compute_runtime::numeric::{
    Complex128, Complex128Tensor, F64Tensor, MAX_REFERENCE_FFT_LEN, NumericError,
};

fn assert_complex_close(actual: &[Complex128], expected: &[Complex128], tolerance: f64) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            (actual.re - expected.re).abs() <= tolerance
                && (actual.im - expected.im).abs() <= tolerance,
            "({},{}) is not within {tolerance} of ({},{})",
            actual.re,
            actual.im,
            expected.re,
            expected.im
        );
    }
}

fn assert_real_close(actual: &[f64], expected: &[f64], tolerance: f64) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!((actual - expected).abs() <= tolerance);
    }
}

#[test]
fn real_fft_matches_impulse_spectrum_and_round_trips() {
    let signal = F64Tensor::new(vec![4], vec![1.0, 2.0, 3.0, 4.0]).unwrap();
    let spectrum = signal.real_fft().expect("real signal should transform");
    assert_eq!(spectrum.shape(), &[4]);
    assert_complex_close(
        spectrum.values(),
        &[
            Complex128::new(10.0, 0.0),
            Complex128::new(-2.0, 2.0),
            Complex128::new(-2.0, 0.0),
            Complex128::new(-2.0, -2.0),
        ],
        1e-12,
    );

    let restored = F64Tensor::inverse_real_fft(&spectrum).expect("spectrum should be real");
    assert_real_close(restored.values(), signal.values(), 1e-12);
    assert!(signal.real_fft_with_round_trip_tolerance(1e-12).is_ok());
    assert!(signal.rfft_with_round_trip_tolerance(1e-12).is_ok());
}

#[test]
fn real_fft_rejects_non_real_spectra_nonfinite_inputs_and_invalid_tolerances() {
    let signal = F64Tensor::new(vec![3], vec![1.0, 2.0, 3.0]).unwrap();
    let non_real = Complex128Tensor::new(
        vec![3],
        vec![
            Complex128::new(1.0, 0.0),
            Complex128::new(0.0, 1.0),
            Complex128::new(0.0, 0.0),
        ],
    )
    .unwrap();
    assert_eq!(
        F64Tensor::inverse_real_fft(&non_real),
        Err(NumericError::RealFftSpectrumNotConjugateSymmetric)
    );

    let nonfinite = F64Tensor::new(vec![2], vec![1.0, f64::NAN]).unwrap();
    assert_eq!(nonfinite.real_fft(), Err(NumericError::NonFiniteValue));

    assert_eq!(
        signal.real_fft_with_round_trip_tolerance(-1.0),
        Err(NumericError::InvalidRealFftTolerance)
    );
    assert_eq!(
        signal.real_fft_with_round_trip_tolerance(f64::NAN),
        Err(NumericError::InvalidRealFftTolerance)
    );
    assert_eq!(
        signal.real_fft_with_round_trip_tolerance(1e-18),
        Err(NumericError::RealFftErrorExceeded)
    );
}

#[test]
fn real_fft_rejects_invalid_shapes_and_reference_cap() {
    let matrix = F64Tensor::new(vec![2, 2], vec![1.0; 4]).unwrap();
    assert_eq!(
        matrix.real_fft(),
        Err(NumericError::RealFftRequiresOneDimension)
    );

    let spectrum = Complex128Tensor::new(vec![2, 1], vec![Complex128::default(); 2]).unwrap();
    assert_eq!(
        F64Tensor::inverse_real_fft(&spectrum),
        Err(NumericError::RealFftRequiresOneDimension)
    );

    let over_cap = F64Tensor::new(
        vec![(MAX_REFERENCE_FFT_LEN + 1) as u64],
        vec![0.0; MAX_REFERENCE_FFT_LEN + 1],
    )
    .unwrap();
    assert_eq!(
        over_cap.real_fft(),
        Err(NumericError::FftLengthExceeded {
            length: MAX_REFERENCE_FFT_LEN + 1,
            max: MAX_REFERENCE_FFT_LEN,
        })
    );
}

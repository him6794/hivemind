use general_compute_runtime::rng::{
    DeterministicRng, MAX_RNG_SAMPLES, RNG_ALGORITHM_VERSION, RngError,
};

#[test]
fn splitmix64_v1_replays_a_pinned_vector_and_separates_streams() {
    assert_eq!(RNG_ALGORITHM_VERSION, "splitmix64-v1");

    let mut rng = DeterministicRng::new(42, 7, 3);
    assert_eq!(rng.next_u64(), 0x514f_05fe_1e8c_18a7);
    assert_eq!(rng.next_u64(), 0x01ee_9b24_6bd1_6ac0);
    assert_eq!(rng.next_u64(), 0xfcd8_986f_b399_3738);
    assert_eq!(rng.next_u64(), 0x2100_6436_2f5f_167f);

    let mut different_stream = DeterministicRng::new(42, 8, 3);
    let mut different_subsequence = DeterministicRng::new(42, 7, 4);
    assert_ne!(different_stream.next_u64(), 0x514f_05fe_1e8c_18a7);
    assert_ne!(different_subsequence.next_u64(), 0x514f_05fe_1e8c_18a7);
}

#[test]
fn rng_samples_are_bounded_and_map_to_unit_interval() {
    let mut rng = DeterministicRng::new(42, 7, 3);
    let samples = rng
        .sample_f64(4)
        .expect("bounded sample request should succeed");
    assert_eq!(samples.len(), 4);
    assert!(
        samples
            .iter()
            .all(|sample| sample.is_finite() && (0.0..1.0).contains(sample))
    );

    assert_eq!(
        rng.sample_f64(MAX_RNG_SAMPLES + 1),
        Err(RngError::SampleCountExceeded {
            requested: MAX_RNG_SAMPLES + 1,
            max: MAX_RNG_SAMPLES,
        })
    );
}

#[test]
fn standard_normal_sampling_replays_and_stays_finite() {
    let mut first = DeterministicRng::new(42, 7, 3);
    let mut second = DeterministicRng::new(42, 7, 3);
    let first_samples = first
        .sample_standard_normal(5)
        .expect("bounded standard-normal request should succeed");
    let second_samples = second
        .sample_standard_normal(5)
        .expect("same stream should replay");

    assert_eq!(first_samples, second_samples);
    assert_eq!(
        first_samples,
        vec![
            1.512_843_365_401_011_6,
            0.071_792_487_065_720_01,
            0.108_569_505_938_962_32,
            0.114_042_688_242_474,
            -0.534_206_208_924_615_2,
        ]
    );
    assert_eq!(first_samples.len(), 5);
    assert!(first_samples.iter().all(|sample| sample.is_finite()));
}

#[test]
fn normal_sampling_validates_parameters_and_output_budget() {
    let mut rng = DeterministicRng::new(42, 7, 3);
    assert_eq!(
        rng.sample_normal(0.0, -1.0, 1),
        Err(RngError::InvalidStandardDeviation)
    );
    assert_eq!(
        rng.sample_normal(f64::NAN, 1.0, 1),
        Err(RngError::InvalidMean)
    );
    assert_eq!(
        rng.sample_normal(0.0, 1.0, MAX_RNG_SAMPLES + 1),
        Err(RngError::SampleCountExceeded {
            requested: MAX_RNG_SAMPLES + 1,
            max: MAX_RNG_SAMPLES,
        })
    );
}

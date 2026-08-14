use general_compute_runtime::rng::{
    DeterministicRng, MAX_RNG_SAMPLES, RNG_ALGORITHM_VERSION, RngError,
};

#[test]
fn splitmix64_v1_replays_a_pinned_vector_and_separates_streams() {
    assert_eq!(RNG_ALGORITHM_VERSION, "splitmix64-v1");

    let mut rng = DeterministicRng::new(42, 7, 3);
    assert_eq!(rng.next_u64(), 0x514f05fe1e8c18a7);
    assert_eq!(rng.next_u64(), 0x1ee9b246bd16ac0);
    assert_eq!(rng.next_u64(), 0xfcd8986fb3993738);
    assert_eq!(rng.next_u64(), 0x210064362f5f167f);

    let mut different_stream = DeterministicRng::new(42, 8, 3);
    let mut different_subsequence = DeterministicRng::new(42, 7, 4);
    assert_ne!(different_stream.next_u64(), 0x514f05fe1e8c18a7);
    assert_ne!(different_subsequence.next_u64(), 0x514f05fe1e8c18a7);
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

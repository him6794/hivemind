use general_compute_runtime::backend::{
    BackendRuntimeIdentity, OptimizedBackendPin, OptimizedBackendRegistration,
    OptimizedBackendRegistrationError,
};
use general_compute_runtime::differential::{DifferentialCase, ReferenceObservation};
use general_compute_runtime::sha256_digest;

fn vector() -> DifferentialCase {
    DifferentialCase {
        source: "minsky:inc(0);halt".into(),
        input_json: r#"{"value": 4}"#.into(),
        seed: 7,
        expected: ReferenceObservation {
            status: "halted".into(),
            steps: 2,
            output: "1".into(),
        },
    }
}

fn registration() -> OptimizedBackendRegistration {
    let vectors = vec![vector()];
    let digest = OptimizedBackendRegistration::reference_vector_digest(&vectors)
        .expect("vectors should serialize canonically");
    let image = format!("sha256:{}", "a".repeat(64));
    let pin = OptimizedBackendPin::new_with_image(
        "blas-openblas",
        "0.3.26",
        vec!["avx2".into(), "fma".into(), "sse4.2".into()],
        4,
        digest,
        image.clone(),
    )
    .unwrap();
    OptimizedBackendRegistration::new("blas-openblas", image, pin, vectors).unwrap()
}

#[test]
fn operator_registration_binds_pin_to_image_and_executes_pinned_vectors() {
    let registration = registration();
    let identity = BackendRuntimeIdentity::new_with_image(
        "blas-openblas",
        "0.3.26",
        vec!["avx2".into(), "fma".into(), "sse4.2".into()],
        4,
        registration.pin.reference_vector_sha256.clone(),
        registration.guest_image_digest.clone(),
    )
    .unwrap();

    registration.verify_identity(&identity).unwrap();
    let report = registration.execute_reference_vectors().unwrap();
    assert_eq!(report.vector_count, 1);
    assert_eq!(
        report.reference_vector_sha256,
        registration.pin.reference_vector_sha256
    );
}

#[test]
fn operator_registration_rejects_image_or_vector_drift_before_execution() {
    let mut registration = registration();
    let mut identity = BackendRuntimeIdentity::new_with_image(
        "blas-openblas",
        "0.3.26",
        vec!["avx2".into(), "fma".into(), "sse4.2".into()],
        4,
        registration.pin.reference_vector_sha256.clone(),
        registration.guest_image_digest.clone(),
    )
    .unwrap();
    identity.guest_image_digest = Some(format!("sha256:{}", "b".repeat(64)));
    assert!(matches!(
        registration.verify_identity(&identity),
        Err(OptimizedBackendRegistrationError::Pin(_))
    ));

    registration.reference_vectors[0].expected.output = "2".into();
    assert!(matches!(
        registration.execute_reference_vectors(),
        Err(OptimizedBackendRegistrationError::VectorDigest)
    ));
}

#[test]
fn operator_registration_rejects_observation_count_or_digest_mismatch() {
    let registration = registration();
    let mut observed = registration.reference_vectors[0].expected.clone();
    observed.output = "2".into();
    assert!(matches!(
        registration.verify_observations(&[observed]),
        Err(OptimizedBackendRegistrationError::ReferenceVector(_))
    ));
    assert!(matches!(
        registration.verify_observations(&[]),
        Err(OptimizedBackendRegistrationError::ObservationCount)
    ));
    assert_eq!(
        sha256_digest(b"pinned-vector-fixture").len(),
        "sha256:".len() + 64
    );
}

use general_compute_runtime::backend::{
    BackendPinError, BackendRuntimeIdentity, OptimizedBackendPin,
};
use general_compute_runtime::sha256_digest;

fn pin() -> OptimizedBackendPin {
    OptimizedBackendPin::new(
        "blas-openblas",
        "0.3.26",
        vec!["avx2".into(), "fma".into(), "sse4.2".into()],
        4,
        sha256_digest(b"dense-reference-v1"),
    )
    .expect("valid optimized backend pin")
}

fn identity() -> BackendRuntimeIdentity {
    BackendRuntimeIdentity::new(
        "blas-openblas",
        "0.3.26",
        vec!["avx2".into(), "fma".into(), "sse4.2".into()],
        4,
        sha256_digest(b"dense-reference-v1"),
    )
    .expect("valid runtime identity")
}

#[test]
fn optimized_backend_pin_accepts_an_exact_runtime_identity() {
    assert!(pin().verify(&identity()).is_ok());
}

#[test]
fn optimized_backend_pin_rejects_identity_drift_and_noncanonical_inputs() {
    let pin = pin();
    let mut drifted = identity();
    drifted.thread_count = 2;
    assert_eq!(
        pin.verify(&drifted),
        Err(BackendPinError::IdentityMismatch("thread_count"))
    );

    let mut wrong_digest = identity();
    wrong_digest.reference_vector_sha256 = sha256_digest(b"different-vector");
    assert_eq!(
        pin.verify(&wrong_digest),
        Err(BackendPinError::IdentityMismatch("reference_vector_sha256"))
    );

    assert_eq!(
        OptimizedBackendPin::new(
            "blas-openblas",
            "0.3.26",
            vec!["fma".into(), "avx2".into()],
            4,
            sha256_digest(b"dense-reference-v1"),
        ),
        Err(BackendPinError::FeaturesNotCanonical)
    );
    assert_eq!(
        BackendRuntimeIdentity::new(
            "blas-openblas",
            "0.3.26",
            vec!["avx2".into()],
            0,
            sha256_digest(b"dense-reference-v1"),
        ),
        Err(BackendPinError::InvalidThreadCount)
    );
}

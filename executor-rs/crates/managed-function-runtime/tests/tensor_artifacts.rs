use std::sync::atomic::AtomicBool;

use managed_function_runtime::{ExecutionLimits, ManagedExecutor, TensorArtifacts, render_output};
use sha2::{Digest, Sha256};

fn bundle(header: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = u64::try_from(header.len()).unwrap().to_le_bytes().to_vec();
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn pinned(bytes: Vec<u8>) -> TensorArtifacts {
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let mut inputs = TensorArtifacts::default();
    inputs.insert("model".to_string(), bytes, &digest).unwrap();
    inputs
}

fn run(
    source: &str,
    inputs: &TensorArtifacts,
    budget: u64,
) -> Result<managed_function_runtime::ExecutionResult, managed_function_runtime::RuntimeError> {
    ManagedExecutor.execute_json_input_with_artifacts(
        source,
        ExecutionLimits::for_managed_function_budget(budget),
        "null",
        inputs,
        None,
    )
}

fn weights() -> TensorArtifacts {
    let data: Vec<u8> = [1.0_f32, 2.0, 3.0, 4.0]
        .iter()
        .flat_map(|n| n.to_le_bytes())
        .collect();
    pinned(bundle(
        r#"{"weights":{"dtype":"F32","shape":[2,2],"data_offsets":[0,16]}}"#,
        &data,
    ))
}

#[test]
fn inference_uses_pinned_weights_without_exposing_host_io() {
    let result = run(
        r#"return tolist(matmul(load_tensor("model", "weights"), tensor([1.0, 2.0])));"#,
        &weights(),
        1000,
    )
    .unwrap();
    assert_eq!(render_output(&result.value), "[5.0,11.0]");
    let info = run(
        r#"return tensor_info("model", "weights");"#,
        &weights(),
        1000,
    )
    .unwrap();
    assert_eq!(
        render_output(&info.value),
        r#"{"dtype":"F32","shape":[2,2]}"#
    );
    assert_eq!(
        run(
            r#"return load_tensor("../../model", "weights");"#,
            &weights(),
            1000
        )
        .unwrap_err()
        .code(),
        "artifact_error"
    );
}

#[test]
fn bounded_slices_can_read_artifacts_larger_than_the_json_input_limit() {
    let count = 300_000;
    let data = vec![0_u8; count * 4];
    let header = format!(
        r#"{{"embedding":{{"dtype":"F32","shape":[{count}],"data_offsets":[0,{}]}}}}"#,
        data.len()
    );
    let inputs = pinned(bundle(&header, &data));
    assert_eq!(
        run(
            r#"return load_tensor("model", "embedding");"#,
            &inputs,
            1_000_000
        )
        .unwrap_err()
        .code(),
        "value_limit_exceeded"
    );
    let result = run(
        r#"return tolist(load_tensor_slice("model", "embedding", 299996, [2,2]));"#,
        &inputs,
        1000,
    )
    .unwrap();
    assert_eq!(render_output(&result.value), "[[0.0,0.0],[0.0,0.0]]");
    for source in [
        r#"return load_tensor_slice("model", "embedding", 299999, [2]);"#,
        r#"return load_tensor_slice("model", "embedding", 9223372036854775807, [2]);"#,
        r#"return load_tensor_slice("model", "embedding", -1, [2]);"#,
        r#"return load_tensor_slice("model", "embedding", 0, []);"#,
        r#"return load_tensor_slice("model", "embedding", 0, [1099511627776,0]);"#,
    ] {
        assert!(run(source, &inputs, 1_000_000).is_err(), "{source}");
    }
}

#[test]
fn load_cost_and_budget_boundary_are_exact() {
    // return + call dispatch + overhead + two string arguments = 9;
    // four tensor positions = 4. No arbitrary file-size-dependent work here.
    let source = r#"return load_tensor("model", "weights");"#;
    let result = run(source, &weights(), 13).unwrap();
    assert_eq!(result.receipt.executed_ops, 13);
    assert_eq!(result.receipt.usage_units, 13);
    assert_eq!(
        run(source, &weights(), 12).unwrap_err().code(),
        "budget_exhausted"
    );
}

#[test]
fn budget_is_checked_before_decoding_nonfinite_weights() {
    let inputs = pinned(bundle(
        r#"{"x":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#,
        &f32::NAN.to_le_bytes(),
    ));
    let source = r#"return load_tensor("model", "x");"#;
    assert_eq!(
        run(source, &inputs, 9).unwrap_err().code(),
        "budget_exhausted"
    );
    assert_eq!(
        run(source, &inputs, 10).unwrap_err().code(),
        "runtime_error"
    );
}

#[test]
fn low_precision_and_integer_storage_decode_exactly() {
    for (dtype, data, expected) in [
        ("F16", vec![0x00, 0x3c, 0x00, 0xc0], "[1.0,-2.0]"),
        ("BF16", vec![0x80, 0x3f, 0x00, 0xc0], "[1.0,-2.0]"),
        (
            "F64",
            [1.0_f64.to_le_bytes(), (-2.0_f64).to_le_bytes()].concat(),
            "[1.0,-2.0]",
        ),
        ("I8", vec![0x7f, 0x80], "[127,-128]"),
        ("U8", vec![0x00, 0xff], "[0,255]"),
        (
            "I16",
            [i16::MIN.to_le_bytes(), i16::MAX.to_le_bytes()].concat(),
            "[-32768,32767]",
        ),
        (
            "I32",
            [i32::MIN.to_le_bytes(), i32::MAX.to_le_bytes()].concat(),
            "[-2147483648,2147483647]",
        ),
        (
            "I64",
            [i64::MIN.to_le_bytes(), i64::MAX.to_le_bytes()].concat(),
            "[-9223372036854775808,9223372036854775807]",
        ),
    ] {
        let header = format!(
            r#"{{"x":{{"dtype":"{dtype}","shape":[2],"data_offsets":[0,{}]}}}}"#,
            data.len()
        );
        let result = run(
            r#"return tolist(load_tensor("model", "x"));"#,
            &pinned(bundle(&header, &data)),
            1000,
        )
        .unwrap();
        assert_eq!(render_output(&result.value), expected, "{dtype}");
    }
    // Binary16 subnormal, negative zero, and largest finite value.
    let inputs = pinned(bundle(
        r#"{"x":{"dtype":"F16","shape":[3],"data_offsets":[0,6]}}"#,
        &[1, 0, 0, 0x80, 0xff, 0x7b],
    ));
    assert_eq!(
        render_output(
            &run(r#"return load_tensor("model", "x")[2];"#, &inputs, 1000)
                .unwrap()
                .value
        ),
        "65504.0"
    );
    assert_eq!(
        render_output(
            &run(r#"return load_tensor("model", "x")[1];"#, &inputs, 1000)
                .unwrap()
                .value
        ),
        "0.0"
    );
    assert_eq!(
        render_output(
            &run(
                r#"return load_tensor("model", "x")[0] * 16777216.0;"#,
                &inputs,
                1000
            )
            .unwrap()
            .value
        ),
        "1.0"
    );
}

#[test]
fn malformed_artifacts_fail_closed() {
    for bytes in [
        vec![],
        u64::MAX.to_le_bytes().to_vec(),
        bundle(
            r#"{"x":{"dtype":"F32","shape":[2],"data_offsets":[0,4]}}"#,
            &[0; 4],
        ),
        bundle(
            r#"{"x":{"dtype":"F32","shape":[1],"data_offsets":[1,5]}}"#,
            &[0; 5],
        ),
        bundle(
            r#"{"x":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#,
            &[0; 5],
        ),
        bundle(
            r#"{"x":{"dtype":"F32","shape":[1],"data_offsets":[0,4]},"y":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#,
            &[0; 4],
        ),
        bundle(
            r#"{"x":{"dtype":"F32","shape":[1],"data_offsets":[0,4]},"x":{"dtype":"F32","shape":[1],"data_offsets":[4,8]}}"#,
            &[0; 8],
        ),
        bundle(
            r#"{"x":{"dtype":"F32","dtype":"I32","shape":[1],"data_offsets":[0,4]}}"#,
            &[0; 4],
        ),
        bundle(
            r#"{"x":{"dtype":"F32","shape":[],"data_offsets":[0,4]}}"#,
            &[0; 4],
        ),
        bundle(
            r#"{"x":{"dtype":"F32","shape":[9223372036854775807,0],"data_offsets":[0,0]}}"#,
            &[],
        ),
        bundle(
            r#"{"x":{"dtype":"U64","shape":[1],"data_offsets":[0,8]}}"#,
            &[0; 8],
        ),
    ] {
        let digest = format!("{:x}", Sha256::digest(&bytes));
        assert!(
            TensorArtifacts::default()
                .insert("model".into(), bytes, &digest)
                .is_err()
        );
    }
}

#[test]
fn digest_duplicate_and_count_limits_are_enforced_before_visibility() {
    let bytes = bundle(
        r#"{"x":{"dtype":"I8","shape":[1],"data_offsets":[0,1]}}"#,
        &[1],
    );
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let mut inputs = TensorArtifacts::default();
    assert!(
        inputs
            .insert("model".into(), bytes.clone(), &"0".repeat(64))
            .is_err()
    );
    assert!(inputs.is_empty());
    for i in 0..16 {
        inputs
            .insert(format!("input-{i}"), bytes.clone(), &digest)
            .unwrap();
    }
    assert!(
        inputs
            .insert("input-0".into(), bytes.clone(), &digest)
            .is_err()
    );
    assert!(inputs.insert("overflow".into(), bytes, &digest).is_err());
}

#[test]
fn artifact_limits_and_costs_match_the_pinned_manifest() {
    use managed_function_runtime::artifacts::{
        MAX_TENSOR_ARTIFACT_BYTES, MAX_TENSOR_ARTIFACT_TOTAL_BYTES, MAX_TENSOR_ARTIFACTS,
    };
    let manifest: serde_json::Value =
        serde_json::from_str(managed_function_runtime::V1_SEMANTICS_MANIFEST_JSON).unwrap();
    let limits = &manifest["admission_limits"];
    assert_eq!(limits["artifact_bytes"], MAX_TENSOR_ARTIFACT_BYTES);
    assert_eq!(
        limits["artifact_total_bytes"],
        MAX_TENSOR_ARTIFACT_TOTAL_BYTES
    );
    assert_eq!(limits["artifact_count"], MAX_TENSOR_ARTIFACTS);
    assert_eq!(limits["artifact_header_bytes"], 1_048_576);
    assert_eq!(limits["artifact_tensor_count"], 4096);
    let costs = &manifest["cost_model"]["tensor_operation_units"];
    assert_eq!(costs["load_tensor_per_position"], 1);
    assert_eq!(costs["tensor_info_per_dimension"], 1);
}

#[test]
fn header_names_and_tensor_counts_are_bounded() {
    let descriptor = r#"{"dtype":"I8","shape":[0],"data_offsets":[0,0]}"#;
    for name in [String::new(), "x".repeat(256)] {
        let header = format!("{{{}:{descriptor}}}", serde_json::to_string(&name).unwrap());
        let bytes = bundle(&header, &[]);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        assert!(
            TensorArtifacts::default()
                .insert("model".into(), bytes, &digest)
                .is_err()
        );
    }
    let entries: Vec<_> = (0..4096)
        .map(|i| format!(r#""t{i}":{descriptor}"#))
        .collect();
    let header = format!("{{{}}}", entries.join(","));
    assert!(!pinned(bundle(&header, &[])).is_empty());
    let header = format!(r#"{{{},"overflow":{descriptor}}}"#, entries.join(","));
    let bytes = bundle(&header, &[]);
    let digest = format!("{:x}", Sha256::digest(&bytes));
    assert!(
        TensorArtifacts::default()
            .insert("model".into(), bytes, &digest)
            .is_err()
    );

    let oversized = bundle(&" ".repeat(1_048_577), &[]);
    let digest = format!("{:x}", Sha256::digest(&oversized));
    assert!(
        TensorArtifacts::default()
            .insert("model".into(), oversized, &digest)
            .is_err()
    );
}

#[test]
fn cancellation_and_closed_runtime_gates_still_apply() {
    let cancelled = AtomicBool::new(true);
    let error = ManagedExecutor
        .execute_json_input_with_artifacts(
            "return 1;",
            ExecutionLimits::for_managed_function_budget(100),
            "null",
            &weights(),
            Some(&cancelled),
        )
        .unwrap_err();
    assert_eq!(error.code(), "cancelled");
    assert!(
        ManagedExecutor
            .execute_json_input_with_artifacts(
                "return 1;",
                ExecutionLimits::default(),
                "null",
                &weights(),
                None
            )
            .is_err()
    );
    assert_eq!(
        ManagedExecutor
            .execute(
                r#"return load_tensor("model", "weights");"#,
                ExecutionLimits::default()
            )
            .unwrap_err()
            .code(),
        "name_error"
    );
}

use managed_function_runtime::{
    ExecutionLimits, ManagedExecutor, Status, V0_SEMANTICS_MANIFEST_JSON,
    V0_SEMANTICS_MANIFEST_SHA256,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const EXPECTED_MANIFEST_SHA256: &str =
    "8ed716dc07c7bc9abcfc5338b1888e71dd041c3fb397c45d0efb1ff76af1deee";

#[test]
fn managed_function_v0_manifest_is_canonical_and_hash_pinned() {
    let manifest: Value =
        serde_json::from_str(V0_SEMANTICS_MANIFEST_JSON).expect("v0 manifest must be valid JSON");
    let canonical = serde_json::to_string(&manifest).expect("v0 manifest must serialize");
    let digest = format!("{:x}", Sha256::digest(canonical.as_bytes()));

    assert_eq!(V0_SEMANTICS_MANIFEST_JSON.trim_end(), canonical);
    assert_eq!(V0_SEMANTICS_MANIFEST_SHA256, EXPECTED_MANIFEST_SHA256);
    assert_eq!(digest, EXPECTED_MANIFEST_SHA256);
    assert_eq!(manifest["runtime_id"], "managed-function-v0");
    assert_eq!(
        manifest["cost_model"]["id"],
        "managed-function-v0-metering-v1"
    );
    assert_eq!(manifest["proof_binding"]["protocol_version"], 1);
    assert_eq!(manifest["proof_binding"]["scheme"], "risc0-zkvm-3.0.6");
    assert_eq!(
        manifest["proof_binding"]["fixture"]["sha256"],
        "8221629b1ba7f2a22430cb4b18a8f2ecb02b306bedb1069d6290cbab95f890bb"
    );
}

#[test]
fn public_runtime_doc_links_the_frozen_manifest_and_its_known_limits() {
    let documentation = include_str!("../../../../docs/MANAGED_FUNCTION_RUNTIME.md");

    assert!(documentation.contains("managed-function-v0-semantics.json"));
    assert!(documentation.contains(EXPECTED_MANIFEST_SHA256));
    assert!(documentation.contains("decoded byte by byte"));
    assert!(documentation.contains("does not accept `\\uXXXX`"));
    assert!(documentation.contains("not a portable or proof-stable result"));
    assert!(documentation.contains("does not expose the evaluator's partial receipt"));
}

#[test]
fn managed_function_v0_manifest_matches_runtime_defaults_and_cost_vectors() {
    let manifest: Value = serde_json::from_str(V0_SEMANTICS_MANIFEST_JSON).unwrap();
    let limits = ExecutionLimits::default();
    let frozen = &manifest["default_execution_limits"];

    assert_eq!(frozen["max_ops"], limits.max_ops);
    assert!(frozen["max_usage_units"].is_null());
    assert_eq!(frozen["max_call_depth"], limits.max_call_depth);
    assert_eq!(frozen["max_output_bytes"], limits.max_output_bytes);
    assert_eq!(frozen["max_loop_iterations"], limits.max_loop_iterations);
    assert_eq!(frozen["max_value_bytes"], limits.max_value_bytes);
    assert_eq!(frozen["max_collection_items"], limits.max_collection_items);
    assert_eq!(frozen["max_value_depth"], limits.max_value_depth);
    assert_eq!(
        frozen["max_value_materialization_bytes"],
        limits.max_value_materialization_bytes
    );

    for vector in manifest["cost_model"]["vectors"].as_array().unwrap() {
        let result = ManagedExecutor
            .execute_json_input(
                vector["source"].as_str().unwrap(),
                ExecutionLimits::default(),
                vector["input_json"].as_str().unwrap(),
            )
            .unwrap_or_else(|error| panic!("vector {} failed: {error}", vector["name"]));
        let expected = &vector["expected"];

        assert_eq!(result.status, Status::Completed);
        assert_eq!(
            result.receipt.executed_ops,
            expected["executed_ops"].as_u64().unwrap()
        );
        assert_eq!(
            result.receipt.usage_units,
            expected["usage_units"].as_u64().unwrap()
        );
        assert_eq!(
            result.receipt.function_calls,
            expected["function_calls"].as_u64().unwrap()
        );
        assert_eq!(
            result.receipt.loop_iterations,
            expected["loop_iterations"].as_u64().unwrap()
        );
        assert_eq!(
            result.receipt.max_call_depth,
            usize::try_from(expected["max_call_depth"].as_u64().unwrap()).unwrap()
        );
        assert_eq!(
            result.receipt.output_bytes,
            usize::try_from(expected["receipt_output_bytes"].as_u64().unwrap()).unwrap()
        );
        assert_eq!(result.output, expected["stdout"].as_str().unwrap());
        assert_eq!(
            managed_function_runtime::render_output(&result.value),
            expected["return_value"].as_str().unwrap()
        );
    }
}

#[test]
fn managed_function_v0_manifest_records_known_semantic_limits() {
    let manifest: Value = serde_json::from_str(V0_SEMANTICS_MANIFEST_JSON).unwrap();
    let limitation_ids = manifest["known_limitations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        limitation_ids,
        [
            "integer-arithmetic-overflow",
            "partial-failure-receipts",
            "source-unicode-escapes",
            "unlimited-limits",
        ]
    );
    assert!(
        !manifest["semantics"]["source_unicode_escape_supported"]
            .as_bool()
            .unwrap()
    );
    assert!(
        !manifest["failure_receipts"]["runtime_error_includes_partial_receipt"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(manifest["semantics"]["integer_model"], "signed-i64");

    let literal_utf8 = ManagedExecutor
        .execute(r#"return "☃";"#, ExecutionLimits::default())
        .unwrap();
    assert_eq!(
        literal_utf8.value,
        managed_function_runtime::Value::String("â\u{98}\u{83}".into())
    );
    assert!(
        !manifest["semantics"]["source_non_ascii_literal_preserved"]
            .as_bool()
            .unwrap()
    );
    let input_utf8 = ManagedExecutor
        .execute_json_input("return input;", ExecutionLimits::default(), r#""☃""#)
        .unwrap();
    assert_eq!(
        input_utf8.value,
        managed_function_runtime::Value::String("☃".into())
    );
    let unicode_escape = ManagedExecutor
        .execute(r#"return "\u2603";"#, ExecutionLimits::default())
        .unwrap_err();
    assert_eq!(unicode_escape.code(), "parse_error");
}

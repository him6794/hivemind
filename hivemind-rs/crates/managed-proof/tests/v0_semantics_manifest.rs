use std::{fs, path::PathBuf};

use hivemind_managed_proof::{
    COST_MODEL_ID, MANAGED_RUNTIME_ID, PROOF_PROTOCOL_VERSION, RISC0_MANAGED_GUEST_ID,
    RISC0_MAX_COMPOSITE_SEGMENTS, RISC0_MAX_JOURNAL_BYTES, RISC0_MAX_RECEIPT_JSON_BYTES,
    RISC0_MAX_SEGMENT_SEAL_WORDS, RISC0_PROOF_SCHEME,
};
use hivemind_proto::{
    LEGACY_MANAGED_RECEIPT_MAX_BYTES, LEGACY_WORKER_RPC_MESSAGE_MAX_BYTES,
    MANAGED_BUDGET_MAX_USAGE_UNITS, MANAGED_JSON_INPUT_MAX_BYTES,
    MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES, MANAGED_TASK_SOURCE_MAX_BYTES, TASK_ID_MAX_BYTES,
    WORKER_STATUS_MESSAGE_MAX_BYTES,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const V0_SEMANTICS_MANIFEST_JSON: &str = include_str!(
    "../../../../executor-rs/crates/managed-function-runtime/managed-function-v0-semantics.json"
);

#[test]
fn v0_manifest_matches_nodepool_proof_and_admission_contracts() {
    let manifest: Value = serde_json::from_str(V0_SEMANTICS_MANIFEST_JSON).unwrap();
    let proof = &manifest["proof_binding"];
    let admission = &manifest["admission_limits"];

    assert_eq!(manifest["runtime_id"], MANAGED_RUNTIME_ID);
    assert_eq!(manifest["cost_model"]["id"], COST_MODEL_ID);
    assert_eq!(proof["protocol_version"], PROOF_PROTOCOL_VERSION);
    assert_eq!(proof["scheme"], RISC0_PROOF_SCHEME);
    assert_eq!(
        proof["guest_image_id"],
        serde_json::to_value(RISC0_MANAGED_GUEST_ID).unwrap()
    );
    assert_eq!(proof["max_journal_bytes"], RISC0_MAX_JOURNAL_BYTES);
    assert_eq!(
        proof["max_receipt_json_bytes"],
        RISC0_MAX_RECEIPT_JSON_BYTES
    );
    assert_eq!(
        proof["max_composite_segments"],
        RISC0_MAX_COMPOSITE_SEGMENTS
    );
    assert_eq!(
        proof["max_segment_seal_words"],
        RISC0_MAX_SEGMENT_SEAL_WORDS
    );

    assert_eq!(admission["task_id_bytes"], TASK_ID_MAX_BYTES);
    assert_eq!(admission["source_bytes"], MANAGED_TASK_SOURCE_MAX_BYTES);
    assert_eq!(admission["json_input_bytes"], MANAGED_JSON_INPUT_MAX_BYTES);
    assert_eq!(admission["max_usage_units"], MANAGED_BUDGET_MAX_USAGE_UNITS);
    assert_eq!(
        admission["worker_status_message_bytes"],
        WORKER_STATUS_MESSAGE_MAX_BYTES
    );
    assert_eq!(
        admission["legacy_receipt_bytes"],
        LEGACY_MANAGED_RECEIPT_MAX_BYTES
    );
    assert_eq!(
        admission["proof_rpc_message_bytes"],
        MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES
    );
    assert_eq!(
        admission["worker_rpc_message_bytes"],
        LEGACY_WORKER_RPC_MESSAGE_MAX_BYTES
    );
}

#[test]
fn v0_manifest_pins_the_real_receipt_fixture_bytes() {
    let manifest: Value = serde_json::from_str(V0_SEMANTICS_MANIFEST_JSON).unwrap();
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let fixture_path = repository_root.join(
        manifest["proof_binding"]["fixture"]["path"]
            .as_str()
            .unwrap(),
    );
    let fixture = fs::read(&fixture_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", fixture_path.display()));

    assert_eq!(
        fixture.len(),
        manifest["proof_binding"]["fixture"]["size_bytes"]
            .as_u64()
            .unwrap() as usize
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&fixture)),
        manifest["proof_binding"]["fixture"]["sha256"]
            .as_str()
            .unwrap()
    );
}

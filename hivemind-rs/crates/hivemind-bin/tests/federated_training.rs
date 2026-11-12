#![cfg(feature = "cli")]

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn federated_training_pipeline_reduces_loss_and_matches_the_scalar_reference() {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../examples/hcode/federated-training");
    let output = Command::new(env!("CARGO_BIN_EXE_hivemind-bin"))
        .args(["pipeline", "check"])
        .arg(example.join("pipeline.json"))
        .output()
        .expect("start the CLI");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert_eq!(stdout.matches("    usage ").count(), 16, "{stdout}");
    let final_json = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Final output: "))
        .expect("pipeline check prints the final result");
    let actual: Value = serde_json::from_str(final_json).unwrap();
    assert_eq!(actual.as_array().unwrap().len(), 1);
    let expected: Value =
        serde_json::from_slice(&std::fs::read(example.join("expected.json")).unwrap()).unwrap();
    let losses = actual[0]["loss_history"].as_array().unwrap();
    let expected_losses = expected["loss_history"].as_array().unwrap();
    assert_eq!(losses.len(), 5);
    assert_eq!(losses.len(), expected_losses.len());
    for (round, (actual, expected)) in losses.iter().zip(expected_losses).enumerate() {
        let actual = actual.as_f64().unwrap();
        let expected = expected.as_f64().unwrap();
        assert!(
            (actual - expected).abs() < 1e-12,
            "round {round}: {actual} != {expected}"
        );
    }
    for pair in losses.windows(2) {
        assert!(pair[1].as_f64().unwrap() < pair[0].as_f64().unwrap());
    }
    let weights = actual[0]["weights"].as_array().unwrap();
    let expected_weights = expected["weights"].as_array().unwrap();
    assert_eq!(weights.len(), 3);
    assert_eq!(weights.len(), expected_weights.len());
    for (actual, expected) in weights.iter().zip(expected_weights) {
        let actual = actual.as_array().unwrap();
        let expected = expected.as_array().unwrap();
        assert_eq!(actual.len(), 3);
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            let actual = actual.as_f64().unwrap();
            let expected = expected.as_f64().unwrap();
            assert!(
                (actual - expected).abs() < 1e-12,
                "weight {actual} != {expected}"
            );
        }
    }
}

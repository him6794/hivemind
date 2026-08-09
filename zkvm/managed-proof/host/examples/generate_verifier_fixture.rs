use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};
use hivemind_managed_proof_zkvm::prove_guest_envelope;

const SOURCE: &str = r#"
fn add(a, b) { return a + b; }
let total = add(get(input, "left"), get(input, "right"));
return {"total": total};
"#;
const INPUT: &str = r#"{"left":20,"right":22}"#;
const TASK_ID: &str = "task-zk-golden";
const MAX_USAGE_UNITS: u64 = 1_000;

fn main() -> Result<()> {
    let output_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: generate_verifier_fixture <output-path>")?;
    let envelope = prove_guest_envelope(TASK_ID, SOURCE, INPUT, MAX_USAGE_UNITS)?;
    let encoded = envelope.to_json_bytes()?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create fixture directory {}", parent.display()))?;
    }
    fs::write(&output_path, encoded)
        .with_context(|| format!("write proof fixture {}", output_path.display()))?;

    Ok(())
}

//! Local pipeline runs for the CLI: `hivemind pipeline check`.
//!
//! A pipeline splits one unit of work across many independent hcode tasks:
//! each stage's items run the way a normal task would, and the previous
//! stage's outputs are composed into the next stage's input. `check` runs the
//! whole pipeline on this machine with the limits a replica uses, so it finds
//! an item that cannot run and an output too large for a Worker to report,
//! before anything is sent.

use anyhow::{anyhow, bail, Result};
use general_compute_runtime::managed_budget::required_task_budget;
use managed_function_runtime::dry_run::{DryRunLimits, DEFAULT_MAX_USAGE_UNITS, DEFAULT_WALL_TIME};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::check::{describe_failure, limits, load_artifacts, run_local, DEFAULT_REPLICAS};

/// The JSON file that names a pipeline: its input and its stages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineFile {
    /// The pipeline's input. It becomes the single element of the first
    /// stage's `previous`, so a stage with no inputs of its own still has a
    /// defined starting value. Absent means `null`.
    #[serde(default)]
    pub input: serde_json::Value,
    pub stages: Vec<Stage>,
}

/// A stage is a set of items that all receive the same `previous` — the
/// previous stage's outputs, or the pipeline input for the first stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    #[serde(default)]
    pub items: Vec<StageItem>,
}

/// One hcode task in a stage. Its fixed parameters and tensor files are
/// declared here; the values to compute on arrive in `previous`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageItem {
    /// Path to the hcode source, relative to the pipeline file.
    pub source: String,
    /// Fixed parameters, or the path (relative to the pipeline file) of a JSON
    /// file holding them. Neither means no parameters.
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    #[serde(default)]
    pub params_file: Option<String>,
    /// Tensor artifact names to paths, relative to the pipeline file.
    #[serde(default)]
    pub artifacts: BTreeMap<String, String>,
    /// The cap the item is submitted with. Absent means the pipeline decides.
    #[serde(default)]
    pub max_cpt: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineSubcommand {
    Check,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineCommand {
    pub sub: PipelineSubcommand,
    /// Path to the pipeline JSON file.
    pub path: String,
    /// Local bounds applied to each item while checking.
    pub max_usage_units: u64,
    pub timeout_secs: u64,
}

pub fn parse_pipeline_args(args: &[String]) -> Result<PipelineCommand> {
    const USAGE: &str =
        "Usage: hivemind pipeline check <pipeline.json> [--max-usage <units>] [--timeout-secs <n>]";
    let sub = args
        .get(2)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("{USAGE}"))?;
    let sub = match sub {
        "check" => PipelineSubcommand::Check,
        other => bail!("unknown pipeline subcommand {other}; use check"),
    };
    let path = args
        .get(3)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| anyhow!("{USAGE}"))?
        .clone();

    let mut command = PipelineCommand {
        sub,
        path,
        max_usage_units: DEFAULT_MAX_USAGE_UNITS,
        timeout_secs: DEFAULT_WALL_TIME.as_secs(),
    };
    let mut i = 4;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args
            .get(i + 1)
            .ok_or_else(|| anyhow!("missing value for {}", flag))?;
        match flag {
            "--max-usage" => {
                command.max_usage_units = value
                    .parse::<u64>()
                    .map_err(|error| anyhow!("invalid {flag}: {error}"))?;
                if command.max_usage_units == 0 {
                    bail!("--max-usage must be greater than 0");
                }
            }
            "--timeout-secs" => {
                command.timeout_secs = value
                    .parse::<u64>()
                    .map_err(|error| anyhow!("invalid {flag}: {error}"))?;
                if !(1..=3600).contains(&command.timeout_secs) {
                    bail!("--timeout-secs must be between 1 and 3600");
                }
            }
            _ => bail!("unknown pipeline flag {flag}"),
        }
        i += 2;
    }
    Ok(command)
}

/// The directory a relative path in a pipeline is resolved against.
fn pipeline_base_dir(path: &str) -> PathBuf {
    let absolute = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path));
    absolute
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Reads a pipeline file, rejecting one that is not an object with stages.
fn load_pipeline(path: &Path) -> Result<PipelineFile> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| anyhow!("failed to read {}: {error}", path.display()))?;
    let pipeline: PipelineFile = serde_json::from_str(&text)
        .map_err(|error| anyhow!("{} is not a pipeline: {error}", path.display()))?;
    if pipeline.stages.is_empty() {
        bail!("{} has no stages", path.display());
    }
    Ok(pipeline)
}

/// A path named in a pipeline is resolved relative to the pipeline file.
fn resolve(base_dir: &Path, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base_dir.join(candidate)
    }
}

/// The item's fixed parameters, read from the file it names when one is given.
fn item_params(base_dir: &Path, item: &StageItem) -> Result<serde_json::Value> {
    match (&item.params, &item.params_file) {
        (Some(_), Some(_)) => bail!(
            "item {} may name params or params_file, not both",
            item.source
        ),
        (Some(params), None) => Ok(params.clone()),
        (None, Some(file)) => {
            let text = std::fs::read_to_string(resolve(base_dir, file))
                .map_err(|error| anyhow!("cannot read params file {file}: {error}"))?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|error| anyhow!("params file {file} is not JSON: {error}"))?;
            Ok(value)
        }
        (None, None) => Ok(serde_json::Value::Null),
    }
}

/// The task input a stage item receives: its own parameters and the values to
/// compute on, always under the same two keys.
fn compose_input(params: &serde_json::Value, previous: &[serde_json::Value]) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    if !params.is_null() {
        object.insert("params".to_owned(), params.clone());
    }
    object.insert(
        "previous".to_owned(),
        serde_json::Value::Array(previous.to_vec()),
    );
    serde_json::Value::Object(object)
}

/// The input as a Worker would receive it, and how many bytes that is.
fn input_bytes(input: &serde_json::Value) -> (String, usize) {
    let text = serde_json::to_string(input).expect("a JSON value always serializes");
    let len = text.len();
    (text, len)
}

/// One item's local result: its output as a JSON value, the bytes it took, and
/// the usage one replica would report.
struct ItemResult {
    output: serde_json::Value,
    output_bytes: usize,
    usage_units: u64,
}

/// The first characters of an output, for a message about a non-JSON one.
fn preview(text: &str) -> String {
    let head: String = text.chars().take(40).collect();
    if text.len() > 40 {
        format!("{head}...")
    } else {
        head
    }
}

/// Runs one item of a stage locally and checks its output against what a
/// Worker can report and the next stage can take in.
fn run_item(
    base_dir: &Path,
    item: &StageItem,
    previous: &[serde_json::Value],
    limits: DryRunLimits,
) -> Result<ItemResult> {
    let params = item_params(base_dir, item)?;
    let input = compose_input(&params, previous);
    let (input_json, input_len) = input_bytes(&input);
    if input_len > hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES {
        bail!(
            "the composed input is {input_len} bytes, above the {} byte limit for a task input; shrink the previous stage's outputs or this item's parameters",
            hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES
        );
    }

    // `load_artifacts` reads each named file and verifies its digest, exactly
    // as a Worker would; an item with none gets `None` and runs on its input.
    let specs: Vec<(String, String)> = item
        .artifacts
        .iter()
        .map(|(name, path)| {
            (
                name.clone(),
                resolve(base_dir, path).to_string_lossy().into_owned(),
            )
        })
        .collect();
    let artifacts = load_artifacts(&specs)?;

    let source = std::fs::read_to_string(resolve(base_dir, &item.source))
        .map_err(|error| anyhow!("cannot read item source {}: {error}", item.source))?;
    if source.trim().is_empty() {
        bail!("item source {} is empty", item.source);
    }

    let report = match run_local(&source, &input_json, artifacts.as_ref(), limits) {
        Ok(report) => report,
        Err(failure) => {
            bail!(
                "the item stopped without a result: {}",
                describe_failure(&failure, limits)
            );
        }
    };

    // The item must hand the pipeline a JSON value: its returned value,
    // rendered the way a Worker reports it. A value that is not JSON cannot be
    // composed into the next stage's input, so it fails here rather than on a
    // Worker.
    let output: serde_json::Value = serde_json::from_str(&report.output).map_err(|error| {
        anyhow!(
            "the item returned {} bytes that are not a JSON value: {error} (starts {preview})",
            report.output.len(),
            preview = preview(&report.output)
        )
    })?;
    let output_bytes = report.output.len();
    if output_bytes > hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES {
        bail!(
            "the item's output is {output_bytes} bytes, above the {} byte limit for a consensus result; a Worker cannot report it",
            hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES
        );
    }

    Ok(ItemResult {
        output,
        output_bytes,
        usage_units: report.receipt.usage_units,
    })
}

/// The minimum `--max-cpt` one item's work needs at the network default of
/// three replicas, for the summary a check prints and the `max_cpt` checks.
fn required_max_cpt(usage_units: u64) -> Result<i64> {
    Ok(required_task_budget(usage_units, DEFAULT_REPLICAS)?.task_budget)
}

/// Runs every stage on this machine, composing each stage's outputs into the
/// next, and reports what each item used and whether its output could travel.
pub async fn run_check(command: &PipelineCommand) -> Result<()> {
    let pipeline_path = PathBuf::from(&command.path);
    let base_dir = pipeline_base_dir(&command.path);
    let pipeline = load_pipeline(&pipeline_path)?;
    let limits = limits(command.max_usage_units, command.timeout_secs);

    let mut previous: Vec<serde_json::Value> = vec![pipeline.input.clone()];
    let mut total_usage: u64 = 0;
    let mut summary = String::new();
    summary.push_str("Checking the pipeline locally, stage by stage.\n\n");

    for (stage_index, stage) in pipeline.stages.iter().enumerate() {
        if stage.items.is_empty() {
            bail!("stage {} has no items", stage_index + 1);
        }
        summary.push_str(&format!(
            "Stage {} ({} item(s)):\n",
            stage_index + 1,
            stage.items.len()
        ));
        let mut next: Vec<serde_json::Value> = Vec::with_capacity(stage.items.len());
        for (item_index, item) in stage.items.iter().enumerate() {
            let source_name = item.source.clone();
            let max_cpt = item.max_cpt;
            // Each item runs with the previous stage's outputs as `previous`.
            // The run is CPU-bound, so it stays off the async runtime.
            let base = base_dir.clone();
            let item = item.clone();
            let previous_snapshot = previous.clone();
            let result = tokio::task::spawn_blocking(move || {
                run_item(&base, &item, &previous_snapshot, limits)
            })
            .await
            .map_err(|error| anyhow!("the local run stopped unexpectedly: {error}"))?;
            let result = result.map_err(|error| {
                anyhow!(
                    "stage {} item {} ({}): {}",
                    stage_index + 1,
                    item_index + 1,
                    source_name,
                    error
                )
            })?;
            total_usage = total_usage.saturating_add(result.usage_units);
            let cap = required_max_cpt(result.usage_units)?;
            summary.push_str(&format!(
                "  item {} ({})\n    usage {} units; --max-cpt must be {cap} or more at {} replicas; output {} bytes\n",
                item_index + 1,
                source_name,
                result.usage_units,
                DEFAULT_REPLICAS,
                result.output_bytes
            ));
            if let Some(want) = max_cpt {
                if want < cap {
                    bail!(
                        "stage {} item {} names max_cpt {want}, below the {cap} CPT its work needs; raise it or give the item less to do",
                        stage_index + 1,
                        item_index + 1
                    );
                }
            }
            next.push(result.output);
        }
        previous = next;
        summary.push('\n');
    }

    print!("{summary}");
    let final_text = serde_json::to_string(&previous)?;
    println!("Final output: {final_text}");
    println!(
        "\nTotal usage across the pipeline: {total_usage} units. This is a local estimate at {} replicas per item.",
        DEFAULT_REPLICAS
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    /// A temp directory unique to this test process; the caller writes into it
    /// and it is left for the OS to reclaim, matching the other CLI tests.
    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hivemind-pipeline-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(dir: &std::path::Path, name: &str, text: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn pipeline_arguments_parse_with_defaults() {
        let command =
            parse_pipeline_args(&args(&["hivemind", "pipeline", "check", "pipe.json"])).unwrap();
        assert_eq!(command.sub, PipelineSubcommand::Check);
        assert_eq!(command.path, "pipe.json");
        assert_eq!(command.max_usage_units, DEFAULT_MAX_USAGE_UNITS);
        assert_eq!(command.timeout_secs, DEFAULT_WALL_TIME.as_secs());
    }

    #[test]
    fn pipeline_arguments_reject_unknown_subcommands_and_flags() {
        for bad in [
            vec!["hivemind", "pipeline"],
            vec!["hivemind", "pipeline", "run"],
            vec!["hivemind", "pipeline", "check"],
            vec!["hivemind", "pipeline", "check", "p.json", "--nope", "1"],
            vec![
                "hivemind",
                "pipeline",
                "check",
                "p.json",
                "--max-usage",
                "0",
            ],
            vec![
                "hivemind",
                "pipeline",
                "check",
                "p.json",
                "--timeout-secs",
                "0",
            ],
        ] {
            assert!(parse_pipeline_args(&args(&bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_composed_input_always_carries_previous_and_sets_params_only_when_there_are_some() {
        let prev = vec![serde_json::json!([1, 2])];
        let with_params = compose_input(&serde_json::json!({"layer": 0}), &prev);
        assert_eq!(with_params["params"]["layer"], 0);
        assert_eq!(with_params["previous"][0], serde_json::json!([1, 2]));

        let without_params = compose_input(&serde_json::Value::Null, &prev);
        assert!(without_params.get("params").is_none());
        assert_eq!(without_params["previous"][0], serde_json::json!([1, 2]));
    }

    #[test]
    fn a_pipeline_file_is_read_with_its_stages_and_defaults_its_input_to_null() {
        let dir = temp_dir();
        let path = write_file(
            &dir,
            "pipe.json",
            r#"{"stages":[{"items":[]}],"input":{"a":1}}"#,
        );
        let pipeline = load_pipeline(&path).unwrap();
        assert_eq!(pipeline.input, serde_json::json!({"a": 1}));
        assert_eq!(pipeline.stages.len(), 1);

        let path2 = write_file(&dir, "pipe2.json", r#"{"stages":[{"items":[]}]}"#);
        let pipeline2 = load_pipeline(&path2).unwrap();
        assert_eq!(pipeline2.input, serde_json::Value::Null);
    }

    #[test]
    fn a_pipeline_with_no_stages_is_rejected() {
        let dir = temp_dir();
        let path = write_file(&dir, "pipe.json", r#"{"input":1,"stages":[]}"#);
        assert!(load_pipeline(&path).is_err());
    }

    #[test]
    fn an_item_that_returns_a_value_is_taken_in_by_the_next_stage() {
        let dir = temp_dir();
        write_file(&dir, "ok.hcode", "get(input, \"previous\")[0];");
        let previous = vec![serde_json::json!([3, 4])];
        let item = StageItem {
            source: "ok.hcode".into(),
            params: None,
            params_file: None,
            artifacts: BTreeMap::new(),
            max_cpt: None,
        };
        let result = run_item(dir.as_path(), &item, &previous, limits(20_000_000, 10)).unwrap();
        assert_eq!(result.output, serde_json::json!([3, 4]));
    }

    #[test]
    fn a_params_file_is_read_relative_to_the_pipeline_file() {
        let dir = temp_dir();
        write_file(&dir, "params.json", r#"{"k": 5}"#);
        let item = StageItem {
            source: "x.hcode".into(),
            params: None,
            params_file: Some("params.json".into()),
            artifacts: BTreeMap::new(),
            max_cpt: None,
        };
        let params = item_params(dir.as_path(), &item).unwrap();
        assert_eq!(params, serde_json::json!({"k": 5}));
    }

    fn check_error(dir: &std::path::Path) -> anyhow::Error {
        let command = PipelineCommand {
            sub: PipelineSubcommand::Check,
            path: dir.join("pipe.json").to_string_lossy().into_owned(),
            max_usage_units: DEFAULT_MAX_USAGE_UNITS,
            timeout_secs: DEFAULT_WALL_TIME.as_secs(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(run_check(&command)).unwrap_err()
    }

    #[test]
    fn a_check_rejects_an_item_whose_max_cpt_is_below_what_its_work_needs() {
        let dir = temp_dir();
        write_file(&dir, "work.hcode", "return [zeros([64], \"f64\")];");
        write_file(
            &dir,
            "pipe.json",
            r#"{"stages":[{"items":[{"source":"work.hcode","max_cpt":10}]}]}"#,
        );
        let message = check_error(&dir).to_string();
        assert!(message.contains("stage 1 item 1"), "{message}");
        assert!(message.contains("max_cpt 10"), "{message}");
    }

    #[test]
    fn a_check_rejects_an_output_a_worker_cannot_report() {
        let dir = temp_dir();
        // 65,536 f64 elements render to 262,184 bytes, just over the 256 KiB
        // consensus limit; a smaller output would not trip the check.
        write_file(&dir, "wide.hcode", "return zeros([65536], \"f64\");");
        write_file(
            &dir,
            "pipe.json",
            r#"{"stages":[{"items":[{"source":"wide.hcode"}]}]}"#,
        );
        let message = check_error(&dir).to_string();
        assert!(message.contains("262184"), "{message}");
        assert!(message.contains("consensus result"), "{message}");
    }
}

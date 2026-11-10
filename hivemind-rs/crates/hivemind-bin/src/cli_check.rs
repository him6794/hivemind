//! Local dry runs for the CLI: `hivemind check`, `submit --max-cpt auto`, and the
//! batch pre-check.
//!
//! A run happens on this machine with the limits a replica uses, so its usage is
//! the usage one replica reports. The price it yields is an estimate: Nodepool
//! settles on what the Workers report.

use anyhow::{anyhow, bail, Result};
use general_compute_runtime::managed_budget::{required_task_budget, TaskBudget};
use managed_function_runtime::{
    artifacts::{TensorArtifacts, MAX_TENSOR_ARTIFACT_BYTES},
    dry_run::{
        dry_run, dry_run_with_artifacts, DryRunFailure, DryRunLimits, DryRunSuccess, FailureCause,
        DEFAULT_MAX_USAGE_UNITS, DEFAULT_WALL_TIME,
    },
};
use std::time::Duration;

/// Replicas a task runs on when it names no policy.
pub(super) const DEFAULT_REPLICAS: u16 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckCommand {
    pub source_path: String,
    pub input: String,
    pub artifacts: Vec<(String, String)>,
    pub replicas: u16,
    pub max_usage_units: u64,
    pub timeout_secs: u64,
}

pub(super) fn parse_replicas(flag: &str, value: &str) -> Result<u16> {
    let replicas: u16 = value
        .parse()
        .map_err(|error| anyhow!("invalid {flag}: {error}"))?;
    if !(2..=7).contains(&replicas) {
        bail!("{flag} must be between 2 and 7");
    }
    Ok(replicas)
}

/// The quorum a replica count needs: a strict majority.
pub(super) fn majority(replicas: u16) -> u16 {
    replicas / 2 + 1
}

pub(super) fn parse_check_args(args: &[String]) -> Result<CheckCommand> {
    let source_path = args
        .get(2)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| {
            anyhow!(
                "Usage: hivemind check <function-source> --input <json> [--artifact <name=path.safetensors>] [--replicas <2-7>] [--max-usage <units>] [--timeout-secs <n>]"
            )
        })?
        .clone();
    let mut check = CheckCommand {
        source_path,
        input: String::new(),
        artifacts: Vec::new(),
        replicas: DEFAULT_REPLICAS,
        max_usage_units: DEFAULT_MAX_USAGE_UNITS,
        timeout_secs: DEFAULT_WALL_TIME.as_secs(),
    };
    let mut i = 3;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args
            .get(i + 1)
            .ok_or_else(|| anyhow!("missing value for {}", flag))?;
        match flag {
            "--input" => check.input = value.clone(),
            "--artifact" => check.artifacts.push(super::artifacts::parse_spec(value)?),
            "--replicas" => check.replicas = parse_replicas(flag, value)?,
            "--max-usage" => {
                check.max_usage_units = value
                    .parse::<u64>()
                    .map_err(|error| anyhow!("invalid {flag}: {error}"))?;
                if check.max_usage_units == 0 {
                    bail!("--max-usage must be greater than 0");
                }
            }
            "--timeout-secs" => {
                check.timeout_secs = value
                    .parse::<u64>()
                    .map_err(|error| anyhow!("invalid {flag}: {error}"))?;
                if !(1..=3600).contains(&check.timeout_secs) {
                    bail!("--timeout-secs must be between 1 and 3600");
                }
            }
            _ => bail!("unknown check flag {}", flag),
        }
        i += 2;
    }
    Ok(check)
}

pub(super) fn default_limits() -> DryRunLimits {
    DryRunLimits::default()
}

pub(super) fn limits(max_usage_units: u64, timeout_secs: u64) -> DryRunLimits {
    DryRunLimits {
        max_usage_units,
        wall_time: Duration::from_secs(timeout_secs),
    }
}

/// Reads the tensor files named on the command line, verifying each against its own
/// digest the way a Worker verifies them against the task's manifest.
pub(super) fn load_artifacts(specs: &[(String, String)]) -> Result<Option<TensorArtifacts>> {
    if specs.is_empty() {
        return Ok(None);
    }
    let mut artifacts = TensorArtifacts::default();
    for (name, path) in specs {
        let metadata = std::fs::metadata(path)
            .map_err(|error| anyhow!("cannot open artifact {path}: {error}"))?;
        if !metadata.is_file() || metadata.len() > MAX_TENSOR_ARTIFACT_BYTES as u64 {
            bail!("artifact must be a regular file of at most 64 MiB: {path}");
        }
        let bytes =
            std::fs::read(path).map_err(|error| anyhow!("cannot read artifact {path}: {error}"))?;
        let digest = general_compute_runtime::sha256_digest(&bytes);
        artifacts
            .insert(name.clone(), bytes, &digest)
            .map_err(|error| anyhow!("artifact {name} is not usable: {error}"))?;
    }
    Ok(Some(artifacts))
}

/// Runs one input as a replica would, with tensor files when there are any.
pub(super) fn run_local(
    source: &str,
    input: &str,
    artifacts: Option<&TensorArtifacts>,
    limits: DryRunLimits,
) -> Result<DryRunSuccess, DryRunFailure> {
    match artifacts {
        Some(artifacts) => dry_run_with_artifacts(source, input, artifacts, limits),
        None => dry_run(source, input, limits),
    }
}

/// One sentence for why a local run stopped without a result.
pub(super) fn describe_failure(failure: &DryRunFailure, limits: DryRunLimits) -> String {
    match failure.cause {
        FailureCause::Program => {
            let position = match (failure.line, failure.column) {
                (Some(line), Some(column)) => format!(" (line {line}, column {column})"),
                (Some(line), None) => format!(" (line {line})"),
                _ => String::new(),
            };
            format!("{}: {}{}", failure.code, failure.message, position)
        }
        FailureCause::UsageLimit => format!(
            "the run reached the local limit of {} usage units before it finished; raise it with --max-usage",
            limits.max_usage_units
        ),
        FailureCause::TimedOut => format!(
            "the run was still going after {} seconds; raise it with --timeout-secs",
            limits.wall_time.as_secs()
        ),
    }
}

pub(super) fn budget_for(report: &DryRunSuccess, replicas: u16) -> Result<TaskBudget> {
    required_task_budget(report.receipt.usage_units, replicas).map_err(anyhow::Error::from)
}

/// What a local run says about one batch input: the budget it needs, or why it cannot run.
pub(super) type InputOutcome = std::result::Result<TaskBudget, String>;

/// Runs every input as a replica would. Each run is CPU-bound, so the whole pass is meant
/// to stay on one blocking thread.
pub(super) fn precheck_inputs(
    source: &str,
    inputs: &[String],
    replicas: u16,
    limits: DryRunLimits,
) -> Vec<InputOutcome> {
    inputs
        .iter()
        .map(|input| match run_local(source, input, None, limits) {
            Ok(report) => required_task_budget(report.receipt.usage_units, replicas)
                .map_err(|e| e.to_string()),
            Err(failure) => Err(describe_failure(&failure, limits)),
        })
        .collect()
}

/// The cap each input is submitted with, or the reason it is not submitted.
///
/// With no fixed cap, an input gets exactly what its own run needs, but never less than the
/// resource price, which the Master requires a cap to cover. A fixed cap is used as given, and an
/// input that needs more than it is refused here instead of failing on a Worker.
pub(super) fn plan_caps(
    outcomes: &[InputOutcome],
    fixed_cap: Option<i64>,
    resource_price: i64,
) -> Vec<std::result::Result<i64, String>> {
    outcomes
        .iter()
        .map(|outcome| match (outcome, fixed_cap) {
            (Err(reason), _) => Err(reason.clone()),
            (Ok(budget), Some(cap)) if budget.task_budget > cap => Err(format!(
                "needs {} CPT, above --max-cpt-per-task {}",
                budget.task_budget, cap
            )),
            (Ok(_), Some(cap)) => Ok(cap),
            (Ok(budget), None) => Ok(budget.task_budget.max(resource_price)),
        })
        .collect()
}

/// The text `hivemind check` prints for a completed run.
pub(super) fn format_report(
    report: &DryRunSuccess,
    budget: &TaskBudget,
    replicas_were_chosen: bool,
) -> String {
    let digest = managed_function_runtime::V1_SEMANTICS_MANIFEST_SHA256;
    let replicas_note = if replicas_were_chosen {
        ""
    } else {
        " (the network default; change it with --replicas)"
    };
    let mut text = String::new();
    text.push_str("Output:\n");
    text.push_str(&report.output);
    if !report.output.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!(
        "\nUsage per replica: {} units\nReplicas: {}{}\nReserved usage: {} CPT\nPlatform fee: {} CPT\nMinimum --max-cpt: {} CPT\n",
        budget.replica_usage,
        budget.replica_count,
        replicas_note,
        budget.reserved_usage,
        budget.fee,
        budget.task_budget,
    ));
    if report.output.len() > hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES {
        text.push_str(&format!(
            "Warning: the output is {} bytes, above the {} byte limit for a consensus result; Workers cannot report it.\n",
            report.output.len(),
            hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES
        ));
    }
    text.push_str(&format!(
        "\nThis is an estimate from a run on this machine (interpreter semantics {}). Nodepool settles on what the Workers report.\n",
        &digest[..12]
    ));
    text
}

pub async fn run_check(command: CheckCommand) -> Result<()> {
    let source = tokio::fs::read_to_string(&command.source_path)
        .await
        .map_err(|error| anyhow!("failed to read {}: {}", command.source_path, error))?;
    if source.trim().is_empty() {
        bail!("{} is empty", command.source_path);
    }
    let input = command.input.clone();
    if input.trim().is_empty() {
        bail!("an input is required: pass --input <json> (use {{}} when the function reads none)");
    }
    let artifacts = load_artifacts(&command.artifacts)?;
    let run_limits = limits(command.max_usage_units, command.timeout_secs);
    // The run is CPU-bound and may last up to the time limit, so it stays off the async runtime.
    let report = tokio::task::spawn_blocking(move || {
        run_local(&source, &input, artifacts.as_ref(), run_limits)
    })
    .await
    .map_err(|error| anyhow!("the local run stopped unexpectedly: {error}"))?;
    match report {
        Ok(report) => {
            let budget = budget_for(&report, command.replicas)?;
            print!(
                "{}",
                format_report(&report, &budget, command.replicas != DEFAULT_REPLICAS)
            );
            Ok(())
        }
        Err(failure) => {
            if let Some(usage) = failure.usage_units {
                eprintln!("Usage before the run stopped: {usage} units");
            }
            Err(anyhow!("{}", describe_failure(&failure, run_limits)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    const SUM_LIST: &str = "let total = 0;\nfor value in get(input, \"values\") {\n  let total = total + value;\n}\ntotal;";

    fn run(source: &str, input: &str) -> Result<DryRunSuccess, DryRunFailure> {
        run_local(source, input, None, limits(1_000_000, 30))
    }

    #[test]
    fn check_arguments_parse_with_defaults() {
        let command = parse_check_args(&args(&[
            "hivemind",
            "check",
            "sum.hmf",
            "--input",
            "{\"values\":[1]}",
        ]))
        .unwrap();
        assert_eq!(command.source_path, "sum.hmf");
        assert_eq!(command.input, "{\"values\":[1]}");
        assert_eq!(command.replicas, 3);
        assert_eq!(command.max_usage_units, DEFAULT_MAX_USAGE_UNITS);
        assert_eq!(command.timeout_secs, DEFAULT_WALL_TIME.as_secs());
    }

    #[test]
    fn check_arguments_reject_bad_values() {
        for bad in [
            vec!["hivemind", "check"],
            vec!["hivemind", "check", "--input", "{}"],
            vec!["hivemind", "check", "a.hmf", "--replicas", "1"],
            vec!["hivemind", "check", "a.hmf", "--replicas", "8"],
            vec!["hivemind", "check", "a.hmf", "--max-usage", "0"],
            vec!["hivemind", "check", "a.hmf", "--timeout-secs", "0"],
            vec!["hivemind", "check", "a.hmf", "--nope", "1"],
            vec!["hivemind", "check", "a.hmf", "--input"],
            vec!["hivemind", "check", "a.hmf", "--input-file", "x.json"],
        ] {
            assert!(parse_check_args(&args(&bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_quorum_for_a_replica_count_is_a_strict_majority() {
        let majorities: Vec<u16> = (2..=7).map(majority).collect();
        assert_eq!(majorities, vec![2, 2, 3, 3, 4, 4]);
        assert!((2..=7).all(|replicas| majority(replicas) > replicas / 2));
    }

    #[test]
    fn a_report_prices_the_task_from_the_replica_usage() {
        let report = run(SUM_LIST, r#"{"values": [10, 20, 30]}"#).unwrap();
        let budget = budget_for(&report, 3).unwrap();
        assert_eq!(budget.replica_usage, 25);
        assert_eq!(budget.task_budget, 82);
        let text = format_report(&report, &budget, false);
        assert!(text.starts_with("Output:\n60\n"), "{text}");
        assert!(text.contains("Usage per replica: 25 units"), "{text}");
        assert!(text.contains("Replicas: 3 (the network default"), "{text}");
        assert!(text.contains("Reserved usage: 75 CPT"), "{text}");
        assert!(text.contains("Platform fee: 7 CPT"), "{text}");
        assert!(text.contains("Minimum --max-cpt: 82 CPT"), "{text}");
        assert!(text.contains("estimate"), "{text}");
    }

    #[test]
    fn a_chosen_replica_count_is_not_called_the_default() {
        let report = run(SUM_LIST, r#"{"values": [10, 20, 30]}"#).unwrap();
        let budget = budget_for(&report, 5).unwrap();
        assert_eq!(budget.task_budget, 137);
        let text = format_report(&report, &budget, true);
        assert!(text.contains("Replicas: 5\n"), "{text}");
    }

    #[test]
    fn printed_output_is_what_the_report_shows() {
        let report = run("print(\"step 1\");\nprint(\"step 2\");\n3;", "{}").unwrap();
        let budget = budget_for(&report, 3).unwrap();
        assert!(format_report(&report, &budget, false).starts_with("Output:\nstep 1\nstep 2\n\n"));
    }

    #[test]
    fn failures_say_where_and_why() {
        let limits = limits(1_000_000, 30);
        let syntax = run("let x = 1;\nlet y = ;", "{}").unwrap_err();
        let text = describe_failure(&syntax, limits);
        assert!(text.starts_with("parse_error: "), "{text}");
        assert!(text.contains("line 2"), "{text}");

        let tight = super::limits(10, 30);
        let spent = run_local(SUM_LIST, r#"{"values": [10, 20, 30]}"#, None, tight).unwrap_err();
        let text = describe_failure(&spent, tight);
        assert!(text.contains("local limit of 10 usage units"), "{text}");
        assert!(text.contains("--max-usage"), "{text}");
    }

    #[test]
    fn every_batch_input_is_run_and_priced_on_its_own() {
        let inputs = vec![
            r#"{"values":[10,20,30]}"#.to_owned(),
            r#"{"values":[1,2,3,4,5,6]}"#.to_owned(),
            r#"{"values":5}"#.to_owned(),
        ];
        let outcomes = precheck_inputs(SUM_LIST, &inputs, 3, limits(1_000_000, 30));
        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes[0].as_ref().unwrap().task_budget, 82);
        assert_eq!(outcomes[1].as_ref().unwrap().task_budget, 122);
        assert!(outcomes[2].is_err());

        // The same inputs on five replicas need more.
        let five = precheck_inputs(SUM_LIST, &inputs[..1], 5, limits(1_000_000, 30));
        assert_eq!(five[0].as_ref().unwrap().task_budget, 137);
    }

    #[test]
    fn an_input_that_hits_the_local_limit_is_refused_with_the_way_out() {
        let outcomes = precheck_inputs(
            SUM_LIST,
            &[r#"{"values":[10,20,30]}"#.to_owned()],
            3,
            limits(10, 30),
        );
        let reason = outcomes[0].as_ref().unwrap_err();
        assert!(reason.contains("--max-usage"), "{reason}");
    }

    fn budget(usage: u64) -> InputOutcome {
        required_task_budget(usage, 3).map_err(|e| e.to_string())
    }

    #[test]
    fn caps_default_to_the_exact_need_but_never_below_the_resource_price() {
        let outcomes = vec![budget(25), budget(37), Err("parse_error: x".into())];
        let plan = plan_caps(&outcomes, None, 5);
        assert_eq!(plan[0], Ok(82));
        assert_eq!(plan[1], Ok(122));
        assert_eq!(plan[2], Err("parse_error: x".into()));

        let priced_higher = plan_caps(&outcomes[..1], None, 200);
        assert_eq!(priced_higher[0], Ok(200));
    }

    #[test]
    fn a_fixed_cap_is_used_as_given_and_inputs_above_it_are_refused() {
        let outcomes = vec![budget(25), budget(37)];
        let plan = plan_caps(&outcomes, Some(100), 5);
        assert_eq!(plan[0], Ok(100));
        assert_eq!(
            plan[1],
            Err("needs 122 CPT, above --max-cpt-per-task 100".to_owned())
        );
        // A cap equal to the need is enough.
        assert_eq!(plan_caps(&outcomes[1..], Some(122), 5)[0], Ok(122));
    }

    #[test]
    fn missing_artifact_files_and_bad_digests_are_refused() {
        assert!(load_artifacts(&[]).unwrap().is_none());
        let missing = load_artifacts(&[("model".into(), "no-such-file.safetensors".into())]);
        assert!(missing.is_err());

        let dir = std::env::temp_dir().join(format!("hivemind-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("junk.safetensors");
        std::fs::write(&path, b"not a safetensors file").unwrap();
        let junk = load_artifacts(&[("model".into(), path.to_string_lossy().into_owned())]);
        assert!(junk.is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tensor_files_are_visible_to_the_run() {
        let header = r#"{"weights":{"dtype":"F32","shape":[2,2],"data_offsets":[0,16]}}"#;
        let data: Vec<u8> = [1.0_f32, 2.0, 3.0, 4.0]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        let mut bytes = u64::try_from(header.len()).unwrap().to_le_bytes().to_vec();
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(&data);

        let dir = std::env::temp_dir().join(format!("hivemind-check-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.safetensors");
        std::fs::write(&path, &bytes).unwrap();
        let artifacts = load_artifacts(&[("model".into(), path.to_string_lossy().into_owned())])
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(&dir).ok();

        let source =
            r#"return tolist(matmul(load_tensor("model", "weights"), tensor([1.0, 2.0])));"#;
        let report = run_local(source, "null", Some(&artifacts), limits(1_000_000, 30)).unwrap();
        assert_eq!(report.output, "[5.0,11.0]");
    }
}

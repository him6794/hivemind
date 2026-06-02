use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Service(String),
    Submit(SubmitCommand),
    Status(TaskLookupCommand),
    Result(TaskLookupCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitCommand {
    pub api_base: String,
    pub username: String,
    pub password: String,
    pub zip_path: String,
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

    if command != "submit" {
        return Ok(CliCommand::Service(command.to_string()));
    }

    let zip_path = args
        .get(2)
        .ok_or_else(|| {
            anyhow!("Usage: hivemind submit <job.zip> --username <user> --password <pass>")
        })?
        .clone();
    if !zip_path.to_ascii_lowercase().ends_with(".zip") {
        return Err(anyhow!("submit requires a .zip package"));
    }

    let mut submit = SubmitCommand {
        api_base: "http://localhost:8082".into(),
        username: String::new(),
        password: String::new(),
        task_id: derive_task_id(&zip_path)?,
        zip_path,
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
        if flag == "--download" {
            i += 1;
            continue;
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| anyhow!("missing value for {}", flag))?;
        match flag {
            "--api" => submit.api_base = normalize_api_base(value),
            "--username" => submit.username = value.clone(),
            "--password" => submit.password = value.clone(),
            "--task-id" => submit.task_id = value.clone(),
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
        api_base: "http://localhost:8082".into(),
        username: String::new(),
        password: String::new(),
        task_id,
        download: false,
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
            _ => return Err(anyhow!("unknown {} flag {}", command, flag)),
        }
        i += 2;
    }
    if lookup.username.trim().is_empty() {
        return Err(anyhow!("--username is required"));
    }
    if lookup.password.is_empty() {
        return Err(anyhow!("--password is required"));
    }
    Ok(lookup)
}

fn normalize_api_base(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
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

fn derive_task_id(zip_path: &str) -> Result<String> {
    let stem = Path::new(zip_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("could not derive task id from zip path"))?
        .to_string();
    if is_safe_task_id(&stem) {
        Ok(stem)
    } else {
        Err(anyhow!("derived task id is not safe; pass --task-id"))
    }
}

fn is_safe_task_id(task_id: &str) -> bool {
    !task_id.trim().is_empty()
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !task_id.contains("..")
}

pub fn build_multipart_upload_body(
    submit: &SubmitCommand,
    zip_bytes: &[u8],
    boundary: &str,
) -> Vec<u8> {
    let mut body = Vec::new();
    push_text_part(&mut body, boundary, "task_id", &submit.task_id);
    push_optional_i32(&mut body, boundary, "cpu_score", submit.cpu_score);
    push_optional_i32(&mut body, boundary, "memory_gb", submit.memory_gb);
    push_optional_i32(&mut body, boundary, "gpu_score", submit.gpu_score);
    push_optional_i32(&mut body, boundary, "gpu_memory_gb", submit.gpu_memory_gb);
    push_optional_i64(&mut body, boundary, "storage_gb", submit.storage_gb);
    push_optional_i32(&mut body, boundary, "host_count", submit.host_count);
    push_optional_i64(&mut body, boundary, "max_cpt", submit.max_cpt);

    let filename = Path::new(&submit.zip_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("job.zip");
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            filename
        )
        .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/zip\r\n\r\n");
    body.extend_from_slice(zip_bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
    body
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
    let zip_bytes = tokio::fs::read(&submit.zip_path)
        .await
        .map_err(|e| anyhow!("failed to read {}: {}", submit.zip_path, e))?;
    if zip_bytes.is_empty() {
        return Err(anyhow!("{} is empty", submit.zip_path));
    }

    let client = reqwest::Client::new();
    let token = login(
        &client,
        &submit.api_base,
        &submit.username,
        &submit.password,
    )
    .await?;

    let boundary = format!("hivemind-{}", uuid::Uuid::new_v4());
    let body = build_multipart_upload_body(&submit, &zip_bytes, &boundary);
    let upload_url = format!("{}/api/tasks/upload", submit.api_base);
    let upload = client
        .post(&upload_url)
        .bearer_auth(token)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(body)
        .send()
        .await
        .map_err(|e| anyhow!("task upload request failed: {}", e))?;
    let upload_status = upload.status();
    let task_response: TaskResponse = upload
        .json()
        .await
        .map_err(|e| anyhow!("failed to decode task upload response: {}", e))?;
    if !upload_status.is_success() || !task_response.success {
        return Err(anyhow!(
            "task upload failed: {}",
            task_response
                .message
                .unwrap_or_else(|| upload_status.to_string())
        ));
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
    let url = format!("{}/api/tasks/{}/log", command.api_base, command.task_id);
    let response = client
        .get(&url)
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
    println!("{}", format_task_status(&body)?);
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
    let url = format!("{}/api/tasks/{}/result", command.api_base, command.task_id);
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
    println!("{}", format_task_result(&body)?);
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
    let url = format!("{}/api/tasks/{}/artifact/download", command.api_base, command.task_id);
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
            body.get("message").and_then(|v| v.as_str()).unwrap_or_else(|| status.as_str())
        ));
    }
    let filename = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(|d| d.split("filename=").nth(1))
        .map(|s| s.trim().trim_matches('"'))
        .unwrap_or("artifact.bin")
        .to_string();
    let bytes = response.bytes().await.map_err(|e| anyhow!("download read failed: {}", e))?;
    let mut file = std::fs::File::create(&filename).map_err(|e| anyhow!("cannot create {}: {}", filename, e))?;
    file.write_all(&bytes).map_err(|e| anyhow!("cannot write {}: {}", filename, e))?;
    println!("Downloaded {} ({} bytes)", filename, bytes.len());
    Ok(())
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

    Ok(format!(
        "Task: {}\nStatus: {}\nResult torrent: {}\nMessage: {}",
        task_id, status, result_torrent, message
    ))
}

fn push_text_part(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn push_optional_i32(body: &mut Vec<u8>, boundary: &str, name: &str, value: Option<i32>) {
    if let Some(value) = value {
        push_text_part(body, boundary, name, &value.to_string());
    }
}

fn push_optional_i64(body: &mut Vec<u8>, boundary: &str, name: &str, value: Option<i64>) {
    if let Some(value) = value {
        push_text_part(body, boundary, name, &value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_submit_zip_with_resource_and_budget_flags() {
        let args = vec![
            "hivemind".to_string(),
            "submit".to_string(),
            "job.zip".to_string(),
            "--api".to_string(),
            "http://localhost:8082/".to_string(),
            "--username".to_string(),
            "alice".to_string(),
            "--password".to_string(),
            "secret".to_string(),
            "--task-id".to_string(),
            "render-001".to_string(),
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
        assert_eq!(submit.zip_path, "job.zip");
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
    fn multipart_upload_body_contains_zip_file_and_optional_fields() {
        let submit = SubmitCommand {
            api_base: "http://localhost:8082".into(),
            username: "alice".into(),
            password: "secret".into(),
            zip_path: "job.zip".into(),
            task_id: "job-123".into(),
            cpu_score: Some(250),
            memory_gb: Some(8),
            gpu_score: None,
            gpu_memory_gb: None,
            storage_gb: Some(20),
            host_count: Some(1),
            max_cpt: Some(500),
        };

        let body = build_multipart_upload_body(&submit, b"zip-bytes", "boundary-123");
        let body = String::from_utf8(body).expect("body should be utf8 for this test");

        assert!(body.contains("name=\"task_id\""));
        assert!(body.contains("job-123"));
        assert!(body.contains("name=\"cpu_score\""));
        assert!(body.contains("250"));
        assert!(body.contains("name=\"storage_gb\""));
        assert!(body.contains("20"));
        assert!(body.contains("name=\"file\"; filename=\"job.zip\""));
        assert!(body.contains("Content-Type: application/zip"));
        assert!(body.contains("zip-bytes"));
        assert!(body.ends_with("--boundary-123--\r\n"));
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
}

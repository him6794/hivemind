use super::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct MasterState {
    submitted: Vec<Value>,
    tasks: HashMap<String, Value>,
    reject: HashSet<String>,
    fail: HashSet<String>,
    non_json: HashSet<String>,
    pending: HashSet<String>,
    lost_response: HashSet<String>,
    lost_upload_response: HashSet<(String, u64)>,
    mutate_after_create: HashMap<String, (PathBuf, Vec<u8>)>,
    server_error: HashSet<String>,
    refused_after_create: HashSet<String>,
    unsettled: HashSet<String>,
    race_conflict: HashSet<String>,
    uploads: Vec<Value>,
    artifact_work: HashMap<String, Value>,
    artifact_bytes: HashMap<(String, String), Vec<u8>>,
    quote: i64,
}

struct Master {
    api: String,
    state: Arc<Mutex<MasterState>>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Master {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn request(socket: &mut tokio::net::TcpStream) -> Option<(String, String, Value)> {
    let mut bytes = Vec::new();
    let mut buf = [0; 8192];
    let (header_end, length) = loop {
        let n = socket.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&buf[..n]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            break (end + 4, length);
        }
    };
    while bytes.len() < header_end + length {
        let n = socket.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    let header = String::from_utf8_lossy(&bytes[..header_end]);
    let mut first = header.lines().next()?.split_whitespace();
    Some((
        first.next()?.into(),
        first.next()?.into(),
        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap_or(Value::Null),
    ))
}

fn reply(state: &mut MasterState, method: &str, target: &str, body: &Value) -> (u16, Value, bool) {
    if target == "/api/login" {
        return (200, json!({"success":true,"token":"owner"}), false);
    }
    if target == "/api/tasks/quote" {
        return (200, json!({"success":true,"quoted_cpt":state.quote}), false);
    }
    if method == "GET" {
        for (id, work) in state.artifact_work.clone() {
            let envelope = general_compute_runtime::hcode_artifacts::parse_input(
                work["torrent"].as_str().unwrap(),
            )
            .unwrap()
            .unwrap();
            let mut artifacts = managed_function_runtime::artifacts::TensorArtifacts::default();
            let mut ready = true;
            for manifest in envelope.artifacts {
                let bytes = state
                    .artifact_bytes
                    .get(&(id.clone(), manifest.artifact_id.clone()))
                    .cloned()
                    .unwrap_or_default();
                if bytes.len() as u64 != manifest.size_bytes {
                    ready = false;
                    break;
                }
                artifacts
                    .insert(manifest.artifact_id, bytes, &manifest.sha256)
                    .unwrap();
            }
            if ready {
                let result = run_local(
                    work["task_source"].as_str().unwrap(),
                    &envelope.input.to_string(),
                    Some(&artifacts),
                    limits(DEFAULT_MAX_USAGE_UNITS, 10),
                )
                .unwrap();
                state.tasks.insert(id.clone(),json!({"task_id":id,"status":"COMPLETED","output":result.output,"billed_amount":10,"billing_settled":true,"status_message":""}));
                state.artifact_work.remove(&id);
            }
        }
        return (
            200,
            json!({"success":true,"tasks":state.tasks.values().collect::<Vec<_>>()}),
            false,
        );
    }
    if target.ends_with("/general-compute/artifacts/chunk") {
        state.uploads.push(body.clone());
        let id = target.split('/').nth(3).unwrap().to_owned();
        let bytes: Vec<u8> = serde_json::from_value(body["bytes"].clone()).unwrap();
        let stored = state
            .artifact_bytes
            .entry((id.clone(), body["artifact_id"].as_str().unwrap().into()))
            .or_default();
        let offset = body["offset"].as_u64().unwrap() as usize;
        stored.resize(stored.len().max(offset + bytes.len()), 0);
        stored[offset..offset + bytes.len()].copy_from_slice(&bytes);
        return (
            200,
            json!({"success":true,"accepted_chunks":1}),
            state.lost_upload_response.remove(&(id, offset as u64)),
        );
    }
    assert_eq!(target, "/api/tasks");
    let id = body["task_id"].as_str().unwrap().to_owned();
    state.submitted.push(body.clone());
    if state.reject.contains(&id) {
        return (
            400,
            json!({"success":false,"message":"submission refused"}),
            false,
        );
    }
    if state.tasks.contains_key(&id) || state.race_conflict.contains(&id) {
        return (409, json!({"success":false,"message":"id occupied"}), false);
    }
    if let Some((path, bytes)) = state.mutate_after_create.remove(&id) {
        std::fs::write(path, bytes).unwrap();
    }
    let raw_input = body["torrent"].as_str().unwrap();
    let envelope = general_compute_runtime::hcode_artifacts::parse_input(raw_input).unwrap();
    if envelope.is_some() {
        state.artifact_work.insert(id.clone(), body.clone());
        state.tasks.insert(
            id.clone(),
            json!({"task_id":id,"status":"PENDING","output":"","billed_amount":0,"billing_settled":false}),
        );
        return (
            200,
            json!({"success":true}),
            state.lost_response.remove(&id),
        );
    }
    let input = envelope
        .as_ref()
        .map(|e| e.input.to_string())
        .unwrap_or_else(|| raw_input.into());
    let result = run_local(
        body["task_source"].as_str().unwrap(),
        &input,
        None,
        limits(DEFAULT_MAX_USAGE_UNITS, 10),
    );
    let (mut status, mut output, message) = match result {
        Ok(report) => ("COMPLETED", report.output, String::new()),
        Err(error) => ("FAILED", String::new(), format!("{error:?}")),
    };
    if state.fail.contains(&id) {
        status = "FAILED";
    }
    if state.pending.contains(&id) {
        status = "RUNNING";
    }
    if state.non_json.contains(&id) {
        output = "not json".into();
    }
    let settled = status != "RUNNING" && !state.unsettled.contains(&id);
    state.tasks.insert(id.clone(), json!({"task_id":id,"status":status,"output":output,"billed_amount":if settled { 10 } else { 0 },"billing_settled":settled,"status_message":if status == "FAILED" { "test failure" } else { &message }}));
    if state.server_error.remove(&id) {
        return (
            500,
            json!({"success":false,"message":"gRPC reply lost after commit"}),
            false,
        );
    }
    if state.refused_after_create.remove(&id) {
        state.tasks.get_mut(&id).unwrap()["status"] = json!("FAILED");
        return (
            400,
            json!({"success":false,"message":"policy persistence failed after create"}),
            false,
        );
    }
    (
        200,
        json!({"success":true}),
        state.lost_response.remove(&id),
    )
}

async fn master() -> Master {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(MasterState {
        quote: 5,
        ..Default::default()
    }));
    let shared = state.clone();
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let Some((method, target, body)) = request(&mut socket).await else {
                continue;
            };
            let (status, response, lost) =
                reply(&mut shared.lock().unwrap(), &method, &target, &body);
            if lost {
                continue;
            }
            let payload = response.to_string();
            let wire = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
            let _ = socket.write_all(wire.as_bytes()).await;
        }
    });
    Master { api, state, server }
}

fn fixture(master: &Master) -> (PathBuf, PipelineRunCommand) {
    let dir = std::env::temp_dir().join(format!("hm-pipeline-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("add.hcode"),
        "return get(input, \"previous\")[0] + get(input, \"params\");",
    )
    .unwrap();
    std::fs::write(
        dir.join("sum.hcode"),
        "return get(input, \"previous\")[0] + get(input, \"previous\")[1];",
    )
    .unwrap();
    std::fs::write(dir.join("pipe.json"), json!({"input":4,"stages":[{"items":[{"source":"add.hcode","params":2,"max_cpt":100},{"source":"add.hcode","params":3,"max_cpt":100}]},{"items":[{"source":"sum.hcode","max_cpt":100}]}]}).to_string()).unwrap();
    let args = vec![
        "hivemind".into(),
        "pipeline".into(),
        "run".into(),
        dir.join("pipe.json").to_str().unwrap().into(),
        "--username".into(),
        "alice".into(),
        "--password".into(),
        "secret".into(),
        "--max-total-cpt".into(),
        "1000".into(),
        "--task-prefix".into(),
        "p".into(),
        "--out".into(),
        dir.join("out.json").to_str().unwrap().into(),
        "--api".into(),
        master.api.clone(),
    ];
    let PipelineCommand::Run(mut command) = parse_pipeline_args(&args).unwrap() else {
        panic!()
    };
    command.per_minute = 60_000;
    command.poll_secs = 0;
    command.timeout_secs = 0;
    (dir, command)
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn weight_fixture(dir: &Path, items: usize, padding_bytes: usize) {
    let mut header = json!({"w":{"dtype":"F64","shape":[1],"data_offsets":[0,8]}});
    if padding_bytes != 0 {
        header["padding"] = json!({
            "dtype":"F64", "shape":[padding_bytes / 8], "data_offsets":[8,8 + padding_bytes]
        });
    }
    let header = header.to_string();
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(&3.0f64.to_le_bytes());
    bytes.resize(bytes.len() + padding_bytes, 0);
    std::fs::write(dir.join("w.safetensors"), bytes).unwrap();
    std::fs::write(
        dir.join("weights.hcode"),
        "return tolist(load_tensor(\"model\", \"w\"));",
    )
    .unwrap();
    std::fs::write(dir.join("params.json"), "{\"tag\":1}").unwrap();
    let item = json!({"source":"weights.hcode","params_file":"params.json","artifacts":{"model":"w.safetensors"},"max_cpt":100});
    std::fs::write(
        dir.join("pipe.json"),
        json!({"stages":[{"items":vec![item; items]}]}).to_string(),
    )
    .unwrap();
}

async fn rejects_changed_item_contents(filename: &str) {
    let m = master().await;
    let (dir, c) = fixture(&m);
    weight_fixture(&dir, 2, 0);
    let path = dir.join(filename);
    let changed = match filename {
        "weights.hcode" => b"return 99;".to_vec(),
        "params.json" => b"{\"tag\":2}".to_vec(),
        "w.safetensors" => {
            let mut bytes = std::fs::read(&path).unwrap();
            let end = bytes.len();
            bytes[end - 8..].copy_from_slice(&4.0f64.to_le_bytes());
            bytes
        }
        _ => unreachable!(),
    };
    m.state
        .lock()
        .unwrap()
        .mutate_after_create
        .insert("p-1-1".into(), (path, changed));
    let result = run_pipeline(&c).await;
    assert_eq!(
        m.state.lock().unwrap().submitted.len(),
        1,
        "changed {filename} must never be submitted under the original fingerprint"
    );
    assert!(result.unwrap_err().to_string().contains("changed"));
    let saved = read_json(&dir.join("out.json.state.json"));
    assert_eq!(saved["rows"][0]["attempts"][0]["status"], "COMPLETED");
    assert_eq!(saved["rows"][0]["attempts"][0]["output"], "[3.0]");
    assert_eq!(saved["rows"][1]["attempts"][0]["status"], "NOT_SUBMITTED");
    assert_eq!(read_json(&dir.join("out.json"))["success"], false);
}

#[tokio::test]
async fn source_changed_between_items_is_not_submitted() {
    rejects_changed_item_contents("weights.hcode").await;
}

#[tokio::test]
async fn parameter_file_changed_between_items_is_not_submitted() {
    rejects_changed_item_contents("params.json").await;
}

#[tokio::test]
async fn artifact_changed_between_items_is_not_submitted() {
    rejects_changed_item_contents("w.safetensors").await;
}

#[tokio::test]
async fn lost_upload_reply_resumes_partial_chunks_without_creating_another_task() {
    use general_compute_runtime::hcode_artifacts::CHUNK_BYTES;

    let m = master().await;
    let (dir, mut c) = fixture(&m);
    weight_fixture(&dir, 1, CHUNK_BYTES);
    m.state
        .lock()
        .unwrap()
        .lost_upload_response
        .insert(("p-1-1".into(), 0));
    assert!(run_pipeline(&c).await.is_err());
    {
        let state = m.state.lock().unwrap();
        assert_eq!(state.submitted.len(), 1);
        assert_eq!(state.uploads.len(), 1);
        assert_eq!(
            state.artifact_bytes[&("p-1-1".into(), "model".into())].len(),
            CHUNK_BYTES
        );
    }
    let saved = read_json(&dir.join("out.json.state.json"));
    assert_eq!(saved["rows"][0]["attempts"][0]["status"], "SUBMITTED");
    assert_eq!(saved["rows"][0]["attempts"][0]["uploaded"], false);
    c.resume = true;
    run_pipeline(&c).await.unwrap();
    let state = m.state.lock().unwrap();
    assert_eq!(state.submitted.len(), 1);
    assert_eq!(state.uploads.len(), 3);
    assert_eq!(state.uploads[0]["offset"], 0);
    assert_eq!(state.uploads[1]["offset"], 0);
    assert_eq!(state.uploads[2]["offset"], CHUNK_BYTES);
    assert_eq!(
        read_json(&dir.join("out.json"))["final_output"],
        json!([[3.0]])
    );
}

#[tokio::test]
async fn unsettled_reported_zero_keeps_the_cap_reserved_before_the_next_stage() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    m.state.lock().unwrap().unsettled.insert("p-1-1".into());
    c.max_total_cpt = 200;
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("max-total-cpt"));
    assert_eq!(m.state.lock().unwrap().submitted.len(), 2);
    assert_eq!(
        read_json(&dir.join("out.json"))["settled_or_reserved_cpt"],
        110
    );
}

#[tokio::test]
async fn server_error_after_create_reconciles_the_original_id_on_resume() {
    let m = master().await;
    let (_, mut c) = fixture(&m);
    m.state.lock().unwrap().server_error.insert("p-1-1".into());
    assert!(run_pipeline(&c).await.is_err());
    c.resume = true;
    run_pipeline(&c).await.unwrap();
    assert_eq!(m.state.lock().unwrap().submitted.len(), 3);
}

#[tokio::test]
async fn refused_response_with_a_persisted_task_keeps_its_billing_on_retry() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    m.state
        .lock()
        .unwrap()
        .refused_after_create
        .insert("p-1-1".into());
    assert!(run_pipeline(&c).await.is_err());
    c.resume = true;
    run_pipeline(&c).await.unwrap();
    assert_eq!(read_json(&dir.join("out.json"))["billed_cpt"], 40);
}

#[tokio::test]
async fn stages_execute_real_hcode_in_order_and_forward_replicas_and_inputs() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    c.replicas = Some(5);
    run_pipeline(&c).await.unwrap();
    let state = m.state.lock().unwrap();
    assert_eq!(
        state
            .submitted
            .iter()
            .map(|b| b["task_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["p-1-1", "p-1-2", "p-2-1"]
    );
    for b in &state.submitted {
        assert_eq!(b["managed_replica_count"], 5);
        assert_eq!(b["managed_quorum"], 3);
        assert_eq!(b["max_cpt"], 100);
    }
    assert_eq!(
        serde_json::from_str::<Value>(state.submitted[2]["torrent"].as_str().unwrap()).unwrap(),
        json!({"previous":[6,7]})
    );
    assert_eq!(
        read_json(&dir.join("out.json"))["final_output"],
        json!([13])
    );
}

#[tokio::test]
async fn insufficient_total_and_resource_caps_create_no_tasks_or_state() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    c.max_total_cpt = 199;
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("max-total-cpt"));
    assert!(m.state.lock().unwrap().submitted.is_empty());
    assert!(!dir.join("out.json.state.json").exists());
    c.max_total_cpt = 1000;
    m.state.lock().unwrap().quote = 101;
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("resource"));
    assert!(!dir.join("out.json.state.json").exists());
}

#[tokio::test]
async fn missing_caps_use_local_measurements_and_no_local_run_requires_caps() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    let mut p = read_json(&dir.join("pipe.json"));
    p["stages"][0]["items"][0]
        .as_object_mut()
        .unwrap()
        .remove("max_cpt");
    std::fs::write(dir.join("pipe.json"), p.to_string()).unwrap();
    c.no_local_run = true;
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("max_cpt"));
    c.no_local_run = false;
    run_pipeline(&c).await.unwrap();
    let expected = required_task_budget(
        run_local(
            "return get(input, \"previous\")[0] + get(input, \"params\");",
            r#"{"previous":[4],"params":2}"#,
            None,
            limits(DEFAULT_MAX_USAGE_UNITS, 10),
        )
        .unwrap()
        .receipt
        .usage_units,
        3,
    )
    .unwrap()
    .task_budget;
    assert_eq!(
        m.state.lock().unwrap().submitted[0]["max_cpt"],
        expected.max(5)
    );
}

#[tokio::test]
async fn failed_item_stops_later_submissions_and_resume_persists_new_attempt_and_billing() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    m.state.lock().unwrap().fail.insert("p-1-1".into());
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("stage 1 item 1"));
    assert_eq!(m.state.lock().unwrap().submitted.len(), 1);
    assert!(dir.join("out.json").exists());
    c.resume = true;
    c.max_total_cpt = 209;
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("max-total-cpt"));
    c.max_total_cpt = 1000;
    run_pipeline(&c).await.unwrap();
    let count = m.state.lock().unwrap().submitted.len();
    assert_eq!(count, 4);
    assert_eq!(m.state.lock().unwrap().submitted[1]["task_id"], "p-1-1-r1");
    assert_eq!(read_json(&dir.join("out.json"))["billed_cpt"], 40);
    run_pipeline(&c).await.unwrap();
    assert_eq!(m.state.lock().unwrap().submitted.len(), count);
}

#[tokio::test]
async fn refused_submission_can_resume_without_reusing_its_id() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    m.state.lock().unwrap().reject.insert("p-1-2".into());
    assert!(run_pipeline(&c).await.is_err());
    c.resume = true;
    run_pipeline(&c).await.unwrap();
    let state = m.state.lock().unwrap();
    assert_eq!(
        state
            .submitted
            .iter()
            .filter(|b| b["task_id"] == "p-1-1")
            .count(),
        1
    );
    assert_eq!(state.submitted[2]["task_id"], "p-1-2-r1");
    assert_eq!(
        read_json(&dir.join("out.json"))["final_output"],
        json!([13])
    );
}

#[tokio::test]
async fn lost_create_reply_resumes_without_duplicate_submission() {
    let m = master().await;
    let (_, mut c) = fixture(&m);
    m.state.lock().unwrap().lost_response.insert("p-1-1".into());
    assert!(run_pipeline(&c).await.is_err());
    c.resume = true;
    run_pipeline(&c).await.unwrap();
    assert_eq!(m.state.lock().unwrap().submitted.len(), 3);
}

#[tokio::test]
async fn occupied_ids_and_fresh_conflicts_are_not_adopted() {
    let m = master().await;
    let (dir, c) = fixture(&m);
    m.state.lock().unwrap().tasks.insert(
        "p-1-1".into(),
        json!({"task_id":"p-1-1","status":"COMPLETED","output":"99"}),
    );
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("occupied"));
    assert!(!dir.join("out.json.state.json").exists());
    m.state.lock().unwrap().tasks.clear();
    m.state.lock().unwrap().race_conflict.insert("p-1-1".into());
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("occupied"));
}

#[tokio::test]
async fn changed_sources_parameters_weights_or_input_reject_resume_but_caps_can_change() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    run_pipeline(&c).await.unwrap();
    c.resume = true;
    std::fs::write(dir.join("add.hcode"), "return 99;").unwrap();
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("changed"));
    std::fs::write(
        dir.join("add.hcode"),
        "return get(input, \"previous\")[0] + get(input, \"params\");",
    )
    .unwrap();
    let mut p = read_json(&dir.join("pipe.json"));
    p["stages"][0]["items"][0]["max_cpt"] = json!(200);
    std::fs::write(dir.join("pipe.json"), p.to_string()).unwrap();
    run_pipeline(&c).await.unwrap();
    p["input"] = json!(5);
    std::fs::write(dir.join("pipe.json"), p.to_string()).unwrap();
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("changed"));
}

#[tokio::test]
async fn non_json_output_and_timeout_write_results_and_do_not_advance() {
    let m = master().await;
    let (dir, c) = fixture(&m);
    m.state.lock().unwrap().non_json.insert("p-1-1".into());
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("JSON"));
    assert_eq!(m.state.lock().unwrap().submitted.len(), 1);
    assert!(dir.join("out.json").exists());
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    m.state.lock().unwrap().pending.insert("p-1-1".into());
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("wait"));
    assert!(dir.join("out.json").exists());
    c.resume = true;
    c.max_total_cpt = 109;
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("max-total-cpt"));
}

#[tokio::test]
async fn artifacts_are_uploaded_executed_and_pinned_with_parameter_files_for_resume() {
    let m = master().await;
    let (dir, mut c) = fixture(&m);
    let header = br#"{"w":{"dtype":"F64","shape":[1],"data_offsets":[0,8]}}"#;
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(&3.0f64.to_le_bytes());
    std::fs::write(dir.join("w.safetensors"), &bytes).unwrap();
    std::fs::write(
        dir.join("weights.hcode"),
        "return tolist(load_tensor(\"model\", \"w\"));",
    )
    .unwrap();
    std::fs::write(dir.join("params.json"), "{\"tag\":1}").unwrap();
    std::fs::write(dir.join("pipe.json"), json!({"stages":[{"items":[{"source":"weights.hcode","params_file":"params.json","artifacts":{"model":"w.safetensors"},"max_cpt":100}]}]}).to_string()).unwrap();
    run_pipeline(&c).await.unwrap();
    assert_eq!(
        read_json(&dir.join("out.json"))["final_output"],
        json!([[3.0]])
    );
    assert_eq!(m.state.lock().unwrap().uploads.len(), 1);
    c.resume = true;
    std::fs::write(dir.join("params.json"), "{\"tag\":2}").unwrap();
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("changed"));
    std::fs::write(dir.join("params.json"), "{\"tag\":1}").unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(dir.join("w.safetensors"), bytes).unwrap();
    assert!(run_pipeline(&c)
        .await
        .unwrap_err()
        .to_string()
        .contains("changed"));
}

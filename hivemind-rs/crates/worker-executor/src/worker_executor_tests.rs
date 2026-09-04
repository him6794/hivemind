use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use hivemind_models::TaskStatus;
use tempfile::TempDir;
use tokio::sync::{oneshot, Notify};
use uuid::Uuid;

use super::*;

#[test]
fn test_system_resources_collection() {
    let r = resource_monitor::collect_resources();
    assert!(r.cpu_cores > 0);
    assert!(r.total_memory_gb > 0);
    assert!(r.storage_total_gb > 0);
}

#[test]
fn stop_task_execution_reports_not_running_for_unknown_task() {
    let executor = WorkerExecutor::new(HivemindConfig::default());

    let outcome = executor.stop_task_execution("missing-task");

    assert_eq!(outcome, StopTaskOutcome::NotRunning);
}

#[tokio::test]
async fn managed_task_cannot_succeed_without_a_generated_proof() {
    let executor = WorkerExecutor::new(HivemindConfig::default());

    let result = executor
        .execute_task(&test_task("managed-proof-required"))
        .await
        .expect("a proof-generation failure is represented as a task failure");

    assert!(!result.success);
    assert_eq!(
        result.error.as_deref(),
        Some("Managed proof generation failed")
    );
    assert!(result.output.is_none());
    assert!(result.managed_proof.is_none());
    assert_eq!(result.managed_executed_ops, 0);
    assert_eq!(result.managed_output_bytes, 0);
    assert!(result.managed_receipt_json.is_none());
}

#[tokio::test]
async fn managed_task_forwards_a_generated_proof_before_reporting_success() {
    let temp = TempDir::new().expect("temporary fake prover directory");
    let mut config = HivemindConfig::default();
    config.executor.managed_prover_executable = successful_fake_prover(&temp);
    config.executor.managed_prover_timeout_secs = 5;
    let executor = WorkerExecutor::new(config);

    let result = executor
        .execute_task(&test_task("managed-proof-forwarded"))
        .await
        .expect("a structurally valid sidecar response completes the worker execution");

    assert!(result.success);
    assert_eq!(result.output.as_deref(), Some("1"));
    assert_eq!(
        result
            .managed_proof
            .as_ref()
            .map(|proof| proof.proof_scheme.as_str()),
        Some("test-proof")
    );
    assert_eq!(
        result
            .managed_proof
            .as_ref()
            .map(|proof| proof.image_id.as_slice()),
        Some(&[1, 2, 3, 4, 5, 6, 7, 8][..])
    );
}

#[test]
fn task_result_serializes_proofs_without_losing_legacy_compatibility() {
    let proof = hivemind_proto::ManagedProofEnvelope {
        proof_scheme: "test-proof".into(),
        image_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
        journal: vec![9, 10],
        receipt_json: br#"{\"seal\":\"ok\"}"#.to_vec(),
    };
    let result = TaskResult {
        task_id: "serialized-proof".into(),
        success: true,
        output: Some("42".into()),
        error: None,
        exit_code: 0,
        cpu_time_ms: 1,
        wall_time_ms: 2,
        peak_memory_mb: 3,
        managed_executed_ops: 4,
        managed_output_bytes: 2,
        managed_receipt_json: Some("{}".into()),
        managed_proof: Some(proof.clone()),
        general_compute_result_json: None,
        managed_gpu_result_json: None,
    };

    let serialized = serde_json::to_string(&result)
        .expect("public TaskResult remains serializable with its proof");
    let decoded: TaskResult = serde_json::from_str(&serialized)
        .expect("serialized TaskResult proof round-trips without loss");
    assert_eq!(decoded.managed_proof, Some(proof));

    let legacy_json = serde_json::json!({
        "task_id": "legacy-result",
        "success": false,
        "output": null,
        "error": "legacy failure",
        "exit_code": 1,
        "cpu_time_ms": 0,
        "wall_time_ms": 0,
        "peak_memory_mb": 0,
        "managed_executed_ops": 0,
        "managed_output_bytes": 0,
        "managed_receipt_json": null
    });
    let legacy: TaskResult = serde_json::from_value(legacy_json)
        .expect("TaskResult serialized before proof support remains readable");
    assert!(legacy.managed_proof.is_none());
}

#[tokio::test]
async fn dropped_execute_future_keeps_supervisor_cleanup_alive() {
    let (started_tx, started_rx) = oneshot::channel();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let runner_started = Arc::clone(&started_tx);
    let executor = Arc::new(WorkerExecutor::new_with_task_runner(
        HivemindConfig::default(),
        move |task, mut cancellation| {
            let runner_started = Arc::clone(&runner_started);
            Box::pin(async move {
                let _ = runner_started
                    .lock()
                    .expect("runner start sender poisoned")
                    .take()
                    .expect("runner starts once")
                    .send(());
                while !*cancellation.borrow() {
                    cancellation
                        .changed()
                        .await
                        .expect("supervisor retains cancellation sender");
                }
                Ok(TaskResult {
                    task_id: task.task_id,
                    success: false,
                    output: None,
                    error: Some("Task execution stopped".into()),
                    exit_code: 1,
                    cpu_time_ms: 0,
                    wall_time_ms: 0,
                    peak_memory_mb: 0,
                    managed_executed_ops: 0,
                    managed_output_bytes: 0,
                    managed_receipt_json: None,
                    managed_proof: None,
                    general_compute_result_json: None,
                    managed_gpu_result_json: None,
                })
            })
        },
    ));
    let task = test_task("dropped-execute-future");
    let executing = {
        let executor = Arc::clone(&executor);
        let task = task.clone();
        tokio::spawn(async move { executor.execute_task(&task).await })
    };

    started_rx.await.expect("runner starts");
    executing.abort();
    let _ = executing.await;

    assert_eq!(
        executor.stop_task_execution(&task.task_id),
        StopTaskOutcome::StopRequested
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if executor.stop_task_execution(&task.task_id) == StopTaskOutcome::NotRunning {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("supervisor removes the active task after cancellation");
}

#[tokio::test]
async fn concurrent_duplicate_execution_waits_for_the_original_result() {
    let (started_tx, started_rx) = oneshot::channel();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(WorkerExecutor::new_with_task_runner(
        HivemindConfig::default(),
        {
            let started_tx = Arc::clone(&started_tx);
            let release = Arc::clone(&release);
            let calls = Arc::clone(&calls);
            move |task, _cancellation| {
                let started_tx = Arc::clone(&started_tx);
                let release = Arc::clone(&release);
                let calls = Arc::clone(&calls);
                Box::pin(async move {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        started_tx
                            .lock()
                            .expect("runner start sender poisoned")
                            .take()
                            .expect("runner starts once")
                            .send(())
                            .expect("runner start receiver remains active");
                        release.notified().await;
                    }
                    Ok(TaskResult {
                        task_id: task.task_id,
                        success: true,
                        output: Some("deduplicated".into()),
                        error: None,
                        exit_code: 0,
                        cpu_time_ms: 0,
                        wall_time_ms: 0,
                        peak_memory_mb: 0,
                        managed_executed_ops: 0,
                        managed_output_bytes: 0,
                        managed_receipt_json: None,
                        managed_proof: None,
                        general_compute_result_json: None,
                        managed_gpu_result_json: None,
                    })
                })
            }
        },
    ));
    let task = test_task("session-redelivery");
    let first = {
        let executor = Arc::clone(&executor);
        let task = task.clone();
        tokio::spawn(async move { executor.execute_task(&task).await })
    };
    started_rx.await.expect("original execution starts");

    let second = {
        let executor = Arc::clone(&executor);
        let task = task.clone();
        tokio::spawn(async move { executor.execute_task(&task).await })
    };
    tokio::task::yield_now().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    release.notify_one();
    let first_result = first
        .await
        .expect("original execution task does not panic")
        .expect("original execution succeeds");
    let second_result = second
        .await
        .expect("duplicate execution task does not panic")
        .expect("duplicate execution receives the original result");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(first_result.task_id, second_result.task_id);
    assert_eq!(first_result.success, second_result.success);
    assert_eq!(first_result.output, second_result.output);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overlapping_attempts_keep_execution_and_cancellation_isolated() {
    let started = Arc::new(tokio::sync::Barrier::new(2));
    let first_release = Arc::new(Notify::new());
    let second_release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(WorkerExecutor::new_with_task_runner(
        HivemindConfig::default(),
        {
            let started = Arc::clone(&started);
            let first_release = Arc::clone(&first_release);
            let second_release = Arc::clone(&second_release);
            let calls = Arc::clone(&calls);
            move |task, _cancellation| {
                let index = calls.fetch_add(1, Ordering::SeqCst);
                assert!(index < 2, "only the two distinct attempts should execute");
                let started = Arc::clone(&started);
                let release = if index == 0 {
                    Arc::clone(&first_release)
                } else {
                    Arc::clone(&second_release)
                };
                Box::pin(async move {
                    started.wait().await;
                    release.notified().await;
                    Ok(TaskResult {
                        task_id: task.task_id,
                        success: true,
                        output: Some(format!("attempt-{index}")),
                        error: None,
                        exit_code: 0,
                        cpu_time_ms: 0,
                        wall_time_ms: 0,
                        peak_memory_mb: 0,
                        managed_executed_ops: 0,
                        managed_output_bytes: 0,
                        managed_receipt_json: None,
                        managed_proof: None,
                        general_compute_result_json: None,
                        managed_gpu_result_json: None,
                    })
                })
            }
        },
    ));
    let task = test_task("overlapping-attempts");
    let first = {
        let executor = Arc::clone(&executor);
        let task = task.clone();
        tokio::spawn(async move {
            executor
                .execute_task_with_context_and_attempt(&task, None, "attempt-a")
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first attempt should start before the second attempt");
    let second = {
        let executor = Arc::clone(&executor);
        let task = task.clone();
        tokio::spawn(async move {
            executor
                .execute_task_with_context_and_attempt(&task, None, "attempt-b")
                .await
        })
    };

    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("distinct attempts must execute concurrently");
    assert_eq!(
        executor.stop_task_execution_for_attempt(&task.task_id, Some("attempt-a")),
        StopTaskOutcome::StopRequested
    );
    assert_eq!(
        executor.stop_task_execution_for_attempt(&task.task_id, Some("wrong-attempt")),
        StopTaskOutcome::NotRunning
    );

    first_release.notify_one();
    let first_result = tokio::time::timeout(Duration::from_secs(2), first)
        .await
        .expect("first attempt should finish")
        .expect("first attempt task should not panic")
        .expect("first attempt should succeed");
    assert_eq!(first_result.output.as_deref(), Some("attempt-0"));
    assert_eq!(
        executor.stop_task_execution_for_attempt(&task.task_id, Some("attempt-b")),
        StopTaskOutcome::StopRequested
    );

    second_release.notify_one();
    let second_result = tokio::time::timeout(Duration::from_secs(2), second)
        .await
        .expect("second attempt should finish")
        .expect("second attempt task should not panic")
        .expect("second attempt should succeed");
    assert_eq!(second_result.output.as_deref(), Some("attempt-1"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[cfg(unix)]
fn successful_fake_prover(temp: &TempDir) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = temp.path().join("successful-prover.sh");
    fs::write(
        &path,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"protocol_version\":1,\"proof_scheme\":\"test-proof\",\"image_id\":[1,2,3,4,5,6,7,8],\"journal\":[9],\"receipt_json\":\"{\\\"seal\\\":\\\"ok\\\"}\"}'\n",
    )
    .expect("fake prover script is written");
    let mut permissions = fs::metadata(&path)
        .expect("fake prover metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&path, permissions).expect("fake prover is executable");
    path.to_string_lossy().into_owned()
}

#[cfg(windows)]
fn successful_fake_prover(temp: &TempDir) -> String {
    let path = temp.path().join("successful-prover.cmd");
    fs::write(
        &path,
        "@echo off\r\nfindstr \"^\" > nul\r\necho {\"protocol_version\":1,\"proof_scheme\":\"test-proof\",\"image_id\":[1,2,3,4,5,6,7,8],\"journal\":[9],\"receipt_json\":\"{\\\"seal\\\":\\\"ok\\\"}\"}\r\nexit /b 0\r\n",
    )
    .expect("fake prover script is written");
    path.to_string_lossy().into_owned()
}

fn test_task(task_id: &str) -> Task {
    let now = Utc::now();
    Task {
        id: Uuid::new_v4(),
        task_id: task_id.into(),
        owner: "worker-test".into(),
        worker_id: None,
        worker_ip: None,
        status: TaskStatus::Pending,
        status_message: None,
        output: None,
        result_torrent: None,
        torrent_source: Some("{}".into()),
        runtime: Some("managed-function-v0".into()),
        task_source: Some("return 1;".into()),
        general_compute_manifest_json: None,
        managed_gpu_manifest_json: None,
        managed_dsl_backend_id: None,
        managed_dsl_semantics_manifest_sha256: None,
        expected_btih: None,
        cpu_usage: 0.0,
        memory_usage: 0.0,
        gpu_usage: 0.0,
        gpu_memory_usage: 0.0,
        req_cpu_score: 1,
        req_gpu_score: 0,
        req_memory_gb: 1,
        req_gpu_memory_gb: 0,
        req_storage_gb: 1,
        host_count: 1,
        max_cpt: 100,
        billing_settled: false,
        billed_amount: 0,
        managed_executed_ops: 0,
        managed_output_bytes: 0,
        managed_receipt_json: None,
        retry_count: 0,
        max_retries: 3,
        deadline: None,
        deterministic: false,
        side_effects: false,
        priority: 0,
        cpu_time_ms: 0,
        wall_time_ms: 0,
        peak_memory_mb: 0,
        download_bytes: 0,
        cache_hits: 0,
        created_at: now,
        last_update: now,
        completed_at: None,
    }
}

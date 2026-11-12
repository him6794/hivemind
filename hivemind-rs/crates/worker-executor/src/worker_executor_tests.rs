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
    assert!(r.memory_supported);
    assert!(r.storage_total_gb > 0);
}

#[test]
fn resource_samples_are_cached_for_repeated_capacity_and_usage_reads() {
    let executor = WorkerExecutor::new(HivemindConfig::default());

    let resources = executor.get_system_resources();
    let first_sample = executor
        .latest_resource_sample()
        .expect("resource collection should publish a timestamped sample");
    let next_resources = executor.get_system_resources();
    let next_sample = executor
        .latest_resource_sample()
        .expect("cached resource sample should remain available");

    assert_eq!(resources.cpu_cores, next_resources.cpu_cores);
    assert_eq!(first_sample.sampled_at, next_sample.sampled_at);
}

#[test]
fn resource_sample_cache_expires_after_its_refresh_interval() {
    let now = Utc::now();

    assert!(resource_sample_is_fresh(
        now - chrono::Duration::seconds(10),
        now
    ));
    assert!(!resource_sample_is_fresh(
        now - chrono::Duration::seconds(11),
        now
    ));
    assert!(!resource_sample_is_fresh(
        now + chrono::Duration::seconds(1),
        now
    ));
}

#[test]
fn stop_task_execution_reports_not_running_for_unknown_task() {
    let executor = WorkerExecutor::new(HivemindConfig::default());

    let outcome = executor.stop_task_execution("missing-task");

    assert_eq!(outcome, StopTaskOutcome::NotRunning);
}

#[tokio::test]
async fn changing_sharing_to_busy_does_not_cancel_a_running_task() {
    let started = Arc::new(Notify::new());
    let finish = Arc::new(Notify::new());
    let executor = Arc::new(WorkerExecutor::new_with_task_runner(
        HivemindConfig::default(),
        {
            let started = started.clone();
            let finish = finish.clone();
            move |task, cancellation| {
                let started = started.clone();
                let finish = finish.clone();
                async move {
                    started.notify_one();
                    finish.notified().await;
                    assert!(
                        !*cancellation.borrow(),
                        "availability changes never cancel work"
                    );
                    Ok(TaskResult {
                        task_id: task.task_id,
                        success: true,
                        output: Some("finished".into()),
                        error: None,
                        exit_code: 0,
                        cpu_time_ms: None,
                        wall_time_ms: None,
                        peak_memory_mb: None,
                        managed_executed_ops: 0,
                        managed_output_bytes: 0,
                        managed_receipt_json: None,
                        general_compute_result_json: None,
                        managed_gpu_result_json: None,
                    })
                }
            }
        },
    ));
    let running = {
        let executor = executor.clone();
        tokio::spawn(async move { executor.execute_task(&test_task("idle-transition")).await })
    };
    started.notified().await;
    executor
        .sharing
        .configure(hivemind_config::WorkerSharingConfig {
            mode: hivemind_config::WorkerSharingMode::Idle,
            ..Default::default()
        })
        .unwrap();
    *executor.sharing.observation_override.lock().unwrap() =
        Some(idle::IdleObservation::LastInput(Some(Duration::ZERO)));
    assert!(!executor.sharing_status().accepts_tasks);
    finish.notify_one();
    let result = running.await.unwrap().unwrap();
    assert!(result.success);
    assert_eq!(result.output.as_deref(), Some("finished"));
}

#[tokio::test]
async fn idle_sharing_refuses_new_executor_attempt_before_starting_its_runner() {
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = WorkerExecutor::new_with_task_runner(HivemindConfig::default(), {
        let calls = calls.clone();
        move |task, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move {
                Ok(TaskResult {
                    task_id: task.task_id,
                    success: true,
                    output: Some("finished".into()),
                    error: None,
                    exit_code: 0,
                    cpu_time_ms: None,
                    wall_time_ms: None,
                    peak_memory_mb: None,
                    managed_executed_ops: 0,
                    managed_output_bytes: 0,
                    managed_receipt_json: None,
                    general_compute_result_json: None,
                    managed_gpu_result_json: None,
                })
            }
        }
    });
    executor
        .sharing
        .configure(hivemind_config::WorkerSharingConfig {
            mode: hivemind_config::WorkerSharingMode::Idle,
            ..Default::default()
        })
        .unwrap();
    *executor.sharing.observation_override.lock().unwrap() =
        Some(idle::IdleObservation::LastInput(Some(Duration::ZERO)));
    let error = executor
        .execute_task(&test_task("idle-new-attempt"))
        .await
        .expect_err("the executor must recheck sharing at new-attempt registration");
    assert_eq!(
        error.to_string(),
        "Worker is not accepting new tasks under its sharing settings"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    executor.sharing.configure(Default::default()).unwrap();
    assert!(
        executor
            .execute_task(&test_task("idle-new-attempt"))
            .await
            .unwrap()
            .success
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn confirmed_attempt_stop_fences_late_execution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = WorkerExecutor::new_with_task_runner(HivemindConfig::default(), {
        let calls = Arc::clone(&calls);
        move |_, _| {
            let calls = Arc::clone(&calls);
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::anyhow!("runner invoked"))
            }
        }
    });
    let task = test_task("fenced-attempt");
    let expiry = (Utc::now().timestamp() + 300) as usize;

    assert_eq!(
        executor
            .stop_task_execution_for_attempt_confirmed(&task.task_id, "attempt-a", expiry)
            .await,
        StopTaskOutcome::StoppedBeforeStart
    );
    let error = executor
        .execute_task_with_attempt(&task, "attempt-a")
        .await
        .expect_err("stopped attempt must not start later");
    assert!(error.to_string().contains("stopped before execution"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(executor
        .execute_task_with_attempt(&task, "attempt-b")
        .await
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn confirmed_prestart_stop_survives_executor_restart() {
    let root = TempDir::new().expect("temporary state root should be created");
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = WorkerExecutor::new_with_task_runner_and_stop_fences(
        HivemindConfig::default(),
        {
            let calls = Arc::clone(&calls);
            move |_, _| {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(anyhow::anyhow!("runner invoked"))
                }
            }
        },
        AttemptStopFenceStore::open(root.path()).expect("stop-fence store should open"),
    );
    let task = test_task("restart-fenced-attempt");
    let expiry = (Utc::now().timestamp() + 300) as usize;

    assert_eq!(
        executor
            .stop_task_execution_for_attempt_confirmed(&task.task_id, "attempt-a", expiry)
            .await,
        StopTaskOutcome::StoppedBeforeStart
    );
    drop(executor);

    let restarted = WorkerExecutor::new_with_task_runner_and_stop_fences(
        HivemindConfig::default(),
        {
            let calls = Arc::clone(&calls);
            move |_, _| {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(anyhow::anyhow!("runner invoked"))
                }
            }
        },
        AttemptStopFenceStore::open(root.path()).expect("stop-fence store should reload"),
    );

    let error = restarted
        .execute_task_with_attempt(&task, "attempt-a")
        .await
        .expect_err("persisted stop fence must reject execution after restart");
    assert!(error.to_string().contains("stopped before execution"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(restarted
        .execute_task_with_attempt(&task, "attempt-b")
        .await
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_stop_fence_persistence_never_confirms_prestart_stop() {
    let root = TempDir::new().expect("temporary state root should be created");
    let store = AttemptStopFenceStore::open(root.path()).expect("stop-fence store should open");
    let fence_directory = root.path().join("attempt-stop-fences");
    std::fs::remove_dir_all(&fence_directory).expect("fence directory should be removable");
    std::fs::write(&fence_directory, b"not a directory")
        .expect("fence path should block durable writes");

    let calls = Arc::new(AtomicUsize::new(0));
    let executor = WorkerExecutor::new_with_task_runner_and_stop_fences(
        HivemindConfig::default(),
        {
            let calls = Arc::clone(&calls);
            move |_, _| {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(anyhow::anyhow!("runner invoked"))
                }
            }
        },
        store,
    );
    let task = test_task("failed-persistence-attempt");

    assert_eq!(
        executor
            .stop_task_execution_for_attempt_confirmed(
                &task.task_id,
                "attempt-a",
                (Utc::now().timestamp() + 300) as usize,
            )
            .await,
        StopTaskOutcome::StopConfirmationUnavailable
    );
    assert!(executor
        .execute_task_with_attempt(&task, "attempt-a")
        .await
        .expect_err("failed persistence must fail closed in this process")
        .to_string()
        .contains("stopped before execution"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn confirmed_attempt_stop_waits_until_the_runner_finishes() {
    let (started_tx, started_rx) = oneshot::channel();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let release = Arc::new(Notify::new());
    let executor = Arc::new(WorkerExecutor::new_with_task_runner(
        HivemindConfig::default(),
        {
            let started_tx = Arc::clone(&started_tx);
            let release = Arc::clone(&release);
            move |_, mut cancellation| {
                let started_tx = Arc::clone(&started_tx);
                let release = Arc::clone(&release);
                async move {
                    started_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                    while !*cancellation.borrow() {
                        cancellation.changed().await.unwrap();
                    }
                    release.notified().await;
                    Err(anyhow::anyhow!("stopped"))
                }
            }
        },
    ));
    let task = test_task("running-attempt-stop");
    let running = {
        let executor = Arc::clone(&executor);
        let task = task.clone();
        tokio::spawn(async move { executor.execute_task_with_attempt(&task, "attempt-a").await })
    };
    started_rx.await.unwrap();
    let stopping = {
        let executor = Arc::clone(&executor);
        let task_id = task.task_id.clone();
        tokio::spawn(async move {
            executor
                .stop_task_execution_for_attempt_confirmed(
                    &task_id,
                    "attempt-a",
                    (Utc::now().timestamp() + 300) as usize,
                )
                .await
        })
    };
    tokio::task::yield_now().await;
    assert!(
        !stopping.is_finished(),
        "stop must wait for execution cleanup"
    );
    release.notify_one();
    assert!(running.await.unwrap().is_err());
    assert_eq!(stopping.await.unwrap(), StopTaskOutcome::StopRequested);
    assert!(executor
        .execute_task_with_attempt(&task, "attempt-a")
        .await
        .is_err());
}

#[tokio::test]
async fn confirmed_attempt_stop_does_not_acknowledge_a_panicked_runner() {
    let (started_tx, started_rx) = oneshot::channel();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let (cancelled_tx, cancelled_rx) = oneshot::channel();
    let cancelled_tx = Arc::new(Mutex::new(Some(cancelled_tx)));
    let release = Arc::new(Notify::new());
    let executor = Arc::new(WorkerExecutor::new_with_task_runner(
        HivemindConfig::default(),
        {
            let started_tx = Arc::clone(&started_tx);
            let cancelled_tx = Arc::clone(&cancelled_tx);
            let release = Arc::clone(&release);
            move |_, mut cancellation| {
                let started_tx = Arc::clone(&started_tx);
                let cancelled_tx = Arc::clone(&cancelled_tx);
                let release = Arc::clone(&release);
                async move {
                    started_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                    while !*cancellation.borrow() {
                        cancellation.changed().await.unwrap();
                    }
                    cancelled_tx
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap()
                        .send(())
                        .unwrap();
                    release.notified().await;
                    panic!("fixture runner panicked after cancellation");
                }
            }
        },
    ));
    let task = test_task("panicked-stop-attempt");
    let running = {
        let executor = Arc::clone(&executor);
        let task = task.clone();
        tokio::spawn(async move { executor.execute_task_with_attempt(&task, "attempt-a").await })
    };
    started_rx.await.unwrap();
    let stopping = {
        let executor = Arc::clone(&executor);
        let task_id = task.task_id.clone();
        tokio::spawn(async move {
            executor
                .stop_task_execution_for_attempt_confirmed(
                    &task_id,
                    "attempt-a",
                    (Utc::now().timestamp() + 300) as usize,
                )
                .await
        })
    };
    cancelled_rx.await.unwrap();
    release.notify_one();
    assert!(running.await.unwrap().is_err());
    assert_eq!(
        stopping.await.unwrap(),
        StopTaskOutcome::StopConfirmationUnavailable
    );
    assert!(executor
        .execute_task_with_attempt(&task, "attempt-a")
        .await
        .is_err());
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
                    cpu_time_ms: None,
                    wall_time_ms: None,
                    peak_memory_mb: None,
                    managed_executed_ops: 0,
                    managed_output_bytes: 0,
                    managed_receipt_json: None,
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
                        cpu_time_ms: None,
                        wall_time_ms: None,
                        peak_memory_mb: None,
                        managed_executed_ops: 0,
                        managed_output_bytes: 0,
                        managed_receipt_json: None,
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
                        cpu_time_ms: None,
                        wall_time_ms: None,
                        peak_memory_mb: None,
                        managed_executed_ops: 0,
                        managed_output_bytes: 0,
                        managed_receipt_json: None,
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
        tokio::spawn(async move { executor.execute_task_with_attempt(&task, "attempt-a").await })
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
        tokio::spawn(async move { executor.execute_task_with_attempt(&task, "attempt-b").await })
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
        execution_observation_json: None,
        created_at: now,
        last_update: now,
        completed_at: None,
    }
}

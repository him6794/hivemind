use anyhow::Result;
use hivemind_database::DatabaseManager;
use hivemind_models::{Claims, Task, TaskStatus, WorkerNode};
use hivemind_proto::{ExecuteTaskRequest, ResourceSpec as ProtoResourceSpec};
use jsonwebtoken::{encode, EncodingKey, Header};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::scheduler;
use crate::task_repository::TaskRepository;

pub struct Dispatcher {
    repo: Arc<TaskRepository>,
    db: DatabaseManager,
    task_timeout_secs: u64,
    max_redispatch: i32,
    jwt_secret: String,
}

impl Dispatcher {
    pub fn new(db: DatabaseManager, task_timeout_secs: u64, max_redispatch: i32) -> Self {
        Self {
            repo: Arc::new(TaskRepository::new(db.pool.clone())),
            db,
            task_timeout_secs,
            max_redispatch,
            jwt_secret: std::env::var("JWT_SECRET").unwrap_or_default(),
        }
    }

    pub fn with_jwt_secret(
        db: DatabaseManager,
        task_timeout_secs: u64,
        max_redispatch: i32,
        jwt_secret: String,
    ) -> Self {
        let mut dispatcher = Self::new(db, task_timeout_secs, max_redispatch);
        dispatcher.jwt_secret = jwt_secret;
        dispatcher
    }

    pub async fn dispatch_one(
        &self,
        task: &Task,
        workers: &[WorkerNode],
    ) -> Option<(String, String)> {
        let ranked_workers = match self.rank_workers_by_cache_affinity(task, workers).await {
            Ok(ranked) => ranked,
            Err(e) => {
                warn!(
                    "Failed to compute cache affinity for task {}: {}",
                    task.task_id, e
                );
                workers.to_vec()
            }
        };
        let worker = scheduler::find_best_worker(task, &ranked_workers).await?;
        let wid = worker.worker_id.clone();
        let wip = worker.ip.clone();
        match self.repo.assign_to_worker(&task.task_id, &wid, &wip).await {
            Ok(_) => {
                info!(
                    "Dispatched task {} to worker {} ({})",
                    task.task_id, wid, wip
                );
                Some((wid, wip))
            }
            Err(e) => {
                error!("Failed to dispatch task {}: {}", task.task_id, e);
                None
            }
        }
    }

    pub async fn dispatch_pending(&self, workers: &[WorkerNode]) -> Result<u64> {
        let trusted_workers = self.repo.trusted_workers(workers).await?;
        let pending = self.repo.find_pending().await?;
        let mut dispatched = 0u64;
        for task in &pending {
            if self.dispatch_one(task, &trusted_workers).await.is_some() {
                dispatched += 1;
            }
        }
        if dispatched > 0 {
            info!("Dispatched {} pending tasks", dispatched);
        }
        Ok(dispatched)
    }

    pub async fn registered_workers(&self) -> Result<Vec<WorkerNode>> {
        sqlx::query_as::<_, WorkerNode>(
            "SELECT * FROM worker_nodes WHERE status IN ('ACTIVE', 'IDLE', 'BUSY')",
        )
        .fetch_all(&self.db.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn dispatch_pending_from_registered_workers(&self) -> Result<u64> {
        let workers = self.registered_workers().await?;
        self.dispatch_pending(&workers).await
    }

    pub async fn dispatch_pending_from_registered_workers_and_execute(&self) -> Result<u64> {
        let workers = self.registered_workers().await?;
        let trusted_workers = self.repo.trusted_workers(&workers).await?;
        let pending = self.repo.find_pending().await?;
        let mut dispatched = 0u64;
        for task in &pending {
            if let Some((worker_id, worker_addr)) = self.dispatch_one(task, &trusted_workers).await
            {
                dispatched += 1;
                let repo = self.repo.clone();
                let task = task.clone();
                let jwt_secret = self.jwt_secret.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        execute_on_worker(repo, task, worker_id, worker_addr, &jwt_secret).await
                    {
                        warn!("Worker execution failed: {}", e);
                    }
                });
            }
        }
        if dispatched > 0 {
            info!("Dispatched {} pending tasks", dispatched);
        }
        Ok(dispatched)
    }

    async fn rank_workers_by_cache_affinity(
        &self,
        task: &Task,
        workers: &[WorkerNode],
    ) -> Result<Vec<WorkerNode>> {
        if workers.len() <= 1 {
            return Ok(workers.to_vec());
        }
        let source = match task.torrent_source.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(workers.to_vec()),
        };
        let worker_ids: Vec<String> = workers.iter().map(|w| w.worker_id.clone()).collect();
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT
                worker_id,
                (
                    COALESCE(SUM(
                        CASE
                            WHEN completed_at IS NOT NULL
                                 AND completed_at >= NOW() - INTERVAL '7 days'
                            THEN 3
                            ELSE 1
                        END
                    ), 0)
                    + COALESCE(SUM(cache_hits), 0)
                )::BIGINT AS hit_score
             FROM tasks
             WHERE status = 'COMPLETED'
               AND torrent_source = $1
               AND worker_id = ANY($2)
             GROUP BY worker_id",
        )
        .bind(source)
        .bind(&worker_ids)
        .fetch_all(&self.db.pool)
        .await?;

        let score_map: HashMap<String, i64> = rows.into_iter().collect();
        let original_index: HashMap<String, usize> = workers
            .iter()
            .enumerate()
            .map(|(idx, w)| (w.worker_id.clone(), idx))
            .collect();

        let mut ranked = workers.to_vec();
        ranked.sort_by(|a, b| {
            let a_score = *score_map.get(&a.worker_id).unwrap_or(&0);
            let b_score = *score_map.get(&b.worker_id).unwrap_or(&0);
            b_score.cmp(&a_score).then_with(|| {
                let a_idx = original_index
                    .get(&a.worker_id)
                    .copied()
                    .unwrap_or(usize::MAX);
                let b_idx = original_index
                    .get(&b.worker_id)
                    .copied()
                    .unwrap_or(usize::MAX);
                a_idx.cmp(&b_idx)
            })
        });
        Ok(ranked)
    }

    pub async fn process_timeouts(&self) -> Result<(u64, u64)> {
        let stale = self
            .repo
            .find_stale_dispatched(self.task_timeout_secs)
            .await?;
        let mut redispatched = 0u64;
        let mut failed = 0u64;
        for task in &stale {
            if task.retry_count >= self.max_redispatch {
                if let Some(worker_id) = task.worker_id.as_deref() {
                    match self
                        .repo
                        .fail_for_worker(
                            &task.task_id,
                            worker_id,
                            "Max redispatch attempts exceeded",
                        )
                        .await
                    {
                        Ok(_) => {
                            warn!(
                                "Task {} failed after {} retries",
                                task.task_id, task.retry_count
                            );
                            failed += 1;
                        }
                        Err(e) => error!("Failed to mark task {} as failed: {}", task.task_id, e),
                    }
                }
            } else if let Some(worker_id) = task.worker_id.as_deref() {
                match self
                    .repo
                    .reset_to_pending_for_worker(&task.task_id, worker_id)
                    .await
                {
                    Ok(_) => {
                        info!(
                            "Task {} reset to pending (retry {}/{})",
                            task.task_id,
                            task.retry_count + 1,
                            self.max_redispatch
                        );
                        redispatched += 1;
                    }
                    Err(e) => error!("Failed to reset task {}: {}", task.task_id, e),
                }
            }
        }
        let timed_out = self.repo.mark_stale_running().await?;
        if timed_out > 0 {
            warn!("Marked {} running tasks as timed out", timed_out);
        }
        Ok((redispatched, failed))
    }

    pub fn start_registered_dispatch_loop(
        self: Arc<Self>,
        interval: std::time::Duration,
    ) -> watch::Sender<bool> {
        let (tx, mut rx) = watch::channel(false);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        if let Err(e) = self.dispatch_pending_from_registered_workers_and_execute().await { error!("Dispatch loop error: {}", e); }
                    }
                    _ = rx.changed() => { if *rx.borrow() { info!("Dispatch loop shutting down"); break; } }
                }
            }
        });
        tx
    }

    pub fn start_timeout_loop(
        self: Arc<Self>,
        interval: std::time::Duration,
    ) -> watch::Sender<bool> {
        let (tx, mut rx) = watch::channel(false);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        if let Err(e) = self.process_timeouts().await { error!("Timeout loop error: {}", e); }
                    }
                    _ = rx.changed() => { if *rx.borrow() { info!("Timeout loop shutting down"); break; } }
                }
            }
        });
        tx
    }
}

pub fn worker_endpoint(addr: &str) -> Result<String> {
    let addr = addr.trim();
    if addr.starts_with("http://") || addr.starts_with("https://") {
        Ok(addr.to_string())
    } else if addr.strip_prefix("0.0.0.0:").is_some() || addr.strip_prefix("[::]:").is_some() {
        anyhow::bail!(
            "worker address {addr} is a bind address, not a routable endpoint; set WORKER_ADVERTISE_ADDR on the worker"
        );
    } else {
        Ok(format!("http://{addr}"))
    }
}

pub fn build_execute_task_request(task: &Task) -> ExecuteTaskRequest {
    build_execute_task_request_with_token(task, String::new())
}

fn build_execute_task_request_with_token(task: &Task, token: String) -> ExecuteTaskRequest {
    ExecuteTaskRequest {
        task_id: task.task_id.clone(),
        torrent: task.torrent_source.clone().unwrap_or_default(),
        resource_limits: Some(ProtoResourceSpec {
            cpu_cores: 0,
            memory_mb: task.req_memory_gb as i64 * 1024,
            gpu_count: 0,
            gpu_name: String::new(),
            vram_mb: task.req_gpu_memory_gb as i64 * 1024,
            cpu_score: task.req_cpu_score,
            gpu_score: task.req_gpu_score,
            storage_total_gb: task.req_storage_gb,
            storage_available_gb: task.req_storage_gb,
        }),
        runtime: task.runtime.clone().unwrap_or_default(),
        task_source: task.task_source.clone().unwrap_or_default(),
        token,
    }
}

fn worker_execution_token(secret: &str, task: &Task) -> anyhow::Result<String> {
    if secret.trim().is_empty() {
        anyhow::bail!("JWT_SECRET is required for worker execution dispatch")
    }
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: task.owner.clone(),
        user_id: task.owner.clone(),
        role: Some("worker-execution".into()),
        exp: (now + 300) as usize,
        iat: now as usize,
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

async fn execute_on_worker(
    repo: Arc<TaskRepository>,
    task: Task,
    worker_id: String,
    worker_addr: String,
    jwt_secret: &str,
) -> Result<()> {
    let Some(current_task) = repo.find_by_task_id(&task.task_id).await? else {
        warn!("Task {} disappeared before worker execution", task.task_id);
        return Ok(());
    };
    if current_task.worker_id.as_deref() != Some(worker_id.as_str())
        || !matches!(
            current_task.status,
            TaskStatus::Assigned | TaskStatus::Running
        )
    {
        info!(
            "Skipping worker execution for task {} because it is no longer assigned to worker {}",
            task.task_id, worker_id
        );
        return Ok(());
    }

    let mut client = hivemind_proto::worker_node_service_client::WorkerNodeServiceClient::connect(
        worker_endpoint(&worker_addr)?,
    )
    .await?;
    let token = worker_execution_token(jwt_secret, &current_task)?;
    match client
        .execute_task(build_execute_task_request_with_token(&current_task, token))
        .await
    {
        Ok(response) => {
            let response = response.into_inner();
            if response.success {
                if current_task.runtime.as_deref() == Some("managed-function-v0")
                    && !response.managed_receipt_json.trim().is_empty()
                {
                    repo.complete_for_worker_with_managed_receipt(
                        &task.task_id,
                        &worker_id,
                        Some(&response.status_message),
                        response.managed_executed_ops,
                        response.managed_output_bytes,
                        &response.managed_receipt_json,
                    )
                    .await?;
                } else {
                    repo.complete_for_worker(
                        &task.task_id,
                        &worker_id,
                        None,
                        Some(&response.status_message),
                    )
                    .await?;
                }
                info!("Task {} completed by worker {}", task.task_id, worker_id);
            } else {
                repo.fail_for_worker(&task.task_id, &worker_id, &response.status_message)
                    .await?;
                warn!(
                    "Task {} failed on worker {}: {}",
                    task.task_id, worker_id, response.status_message
                );
            }
        }
        Err(e) => {
            repo.reset_to_pending_for_worker(&task.task_id, &worker_id)
                .await?;
            warn!(
                "Task {} could not be sent to worker {}: {}",
                task.task_id, worker_id, e
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hivemind_models::{TaskStatus, WorkerStatus};
    use hivemind_proto::{
        worker_node_service_server::{WorkerNodeService, WorkerNodeServiceServer},
        ExecuteTaskRequest, ExecuteTaskResponse, StopTaskExecutionRequest,
        StopTaskExecutionResponse, TaskOutputRequest, TaskOutputResponse, TaskOutputUploadRequest,
        TaskOutputUploadResponse, TaskResultUploadRequest, TaskResultUploadResponse,
        TaskUsageRequest, TaskUsageResponse,
    };
    use std::net::SocketAddr;
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request, Response, Status};

    fn dispatcher_db_lock() -> Arc<tokio::sync::Mutex<()>> {
        static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
        LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    async fn test_db(
        test_name: &str,
    ) -> Option<(
        hivemind_database::DatabaseManager,
        hivemind_database::postgres::IsolatedTestPool,
    )> {
        let fixture = hivemind_database::postgres::create_isolated_test_pool(test_name)
            .await
            .ok()?;
        if hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .is_err()
        {
            fixture.cleanup().await.ok();
            return None;
        }
        let db = hivemind_database::DatabaseManager {
            pool: fixture.pool.clone(),
        };
        Some((db, fixture))
    }

    fn make_task(id: &str, status: TaskStatus, retry_count: i32) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            task_id: id.into(),
            owner: "example-user".into(),
            worker_id: None,
            worker_ip: None,
            status,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some("example-btih".into()),
            runtime: None,
            task_source: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 100,
            req_gpu_score: 0,
            req_memory_gb: 4,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
            retry_count,
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
            created_at: Utc::now(),
            last_update: Utc::now(),
            completed_at: None,
        }
    }

    #[test]
    fn build_execute_task_request_forwards_managed_runtime_source() {
        let mut task = make_task("managed-dispatch", TaskStatus::Pending, 0);
        task.runtime = Some("managed-function-v0".into());
        task.task_source = Some("return get(input, \"value\") + 1;".into());
        task.torrent_source = Some("{\"value\": 41}".into());

        let request = build_execute_task_request(&task);

        assert_eq!(request.runtime, "managed-function-v0");
        assert_eq!(request.task_source, "return get(input, \"value\") + 1;");
        assert_eq!(request.torrent, "{\"value\": 41}");
    }

    fn make_worker(id: &str, cpu: i32, mem: i32, status: WorkerStatus) -> WorkerNode {
        WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: id.into(),
            username: "test".into(),
            ip: format!("192.168.1.{}", &id[1..]),
            virtual_ip: None,
            hostname: None,
            cpu_cores: cpu,
            memory_gb: mem,
            cpu_score: cpu * 100,
            gpu_score: 0,
            gpu_memory_gb: 0,
            gpu_name: None,
            vram_mb: 0,
            storage_total_gb: 500,
            storage_available_gb: 200,
            provider_enabled: true,
            cpu_cores_limit: 0,
            memory_gb_limit: 0,
            gpu_memory_gb_limit: 0,
            storage_gb_limit: 0,
            min_cpt_per_hour: 0,
            location: "local".into(),
            status,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: mem,
            queue_capacity: cpu,
            last_heartbeat: Utc::now(),
            registered_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_dispatcher_creation() {
        assert_eq!(30u64, 30);
    }

    #[test]
    fn test_worker_endpoint_adds_scheme_and_rejects_unspecified_host() {
        let error = worker_endpoint("0.0.0.0:50053").unwrap_err().to_string();
        assert!(error.contains("WORKER_ADVERTISE_ADDR"));
        assert_eq!(
            worker_endpoint("127.0.0.1:50053").unwrap(),
            "http://127.0.0.1:50053"
        );
        assert_eq!(
            worker_endpoint("http://worker:50053").unwrap(),
            "http://worker:50053"
        );
    }

    #[test]
    fn test_build_execute_task_request_uses_task_requirements() {
        let task = make_task("execute-request-1", TaskStatus::Pending, 0);
        let request = build_execute_task_request(&task);
        let limits = request.resource_limits.unwrap();

        assert_eq!(request.task_id, "execute-request-1");
        assert_eq!(request.torrent, "example-btih");
        assert_eq!(limits.cpu_score, 100);
        assert_eq!(limits.memory_mb, 4096);
        assert_eq!(limits.storage_total_gb, 10);
    }

    #[tokio::test]
    async fn dispatcher_db_tests_use_isolated_schema() {
        let (db, fixture) = match test_db("dispatcher_schema_canary").await {
            Some(parts) => parts,
            None => return,
        };

        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&db.pool)
            .await
            .unwrap();

        assert!(
            schema.starts_with("hm_test_"),
            "dispatcher DB tests must use an isolated schema, got {schema}"
        );
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_one_no_workers() {
        let (db, fixture) = match test_db("dispatcher_no_workers").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db, 30, 2);
        let task = make_task("dispatch-test-1", TaskStatus::Pending, 0);
        let result = dispatcher.dispatch_one(&task, &[]).await;
        assert!(result.is_none());
        sqlx::query("DELETE FROM tasks WHERE task_id = 'dispatch-test-1'")
            .execute(&dispatcher.db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_one_with_worker() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_with_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let task = make_task("dispatch-test-2", TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.ok();
        let workers = vec![make_worker("w1", 4, 16, WorkerStatus::Idle)];
        let result = dispatcher.dispatch_one(&task, &workers).await;
        assert!(result.is_some());
        let (wid, wip) = result.unwrap();
        assert_eq!(wid, "w1");
        assert!(wip.contains("192.168"));
        let updated = dispatcher
            .repo
            .find_by_task_id("dispatch-test-2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, TaskStatus::Assigned);
        sqlx::query("DELETE FROM tasks WHERE task_id = 'dispatch-test-2'")
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_one_does_not_overwrite_stale_assignment() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_stale_assignment").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let task_id = "dispatch-stale-assignment";
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .execute(&db.pool)
            .await
            .ok();

        let stale_task = make_task(task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&stale_task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(task_id, "w1", "192.168.1.1")
            .await
            .unwrap();

        let workers = vec![make_worker("w2", 4, 16, WorkerStatus::Idle)];
        let result = dispatcher.dispatch_one(&stale_task, &workers).await;
        assert!(result.is_none());

        let stored = dispatcher
            .repo
            .find_by_task_id(task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.worker_id.as_deref(), Some("w1"));
        assert_eq!(stored.worker_ip.as_deref(), Some("192.168.1.1"));

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(task_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_execute_on_worker_skips_task_cancelled_after_assignment() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_cancelled_after_assignment").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-cancel-race-{unique}");
        let worker_id = format!("dispatch-cancel-race-w-{unique}");
        let (worker_addr, mut execute_rx) = match fake_worker_execute_server().await {
            Some(parts) => parts,
            None => return,
        };

        let task = make_task(&task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();
        dispatcher.repo.cancel(&task_id).await.unwrap();

        let result = execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            "test-secret",
        )
        .await;

        assert!(
            result.is_ok(),
            "execution skip should not be an error: {result:?}"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(250), execute_rx.recv())
                .await
                .is_err(),
            "cancelled task should not be sent to worker"
        );
        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Cancelled);

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_execute_on_worker_settles_managed_task_from_worker_receipt() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_managed_worker_receipt").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let username = format!("dispatch-managed-owner-{unique}");
        let worker_id = format!("dispatch-managed-worker-{unique}");
        let task_id = format!("dispatch-managed-task-{unique}");
        let receipt_json = "{\"executed_ops\":2500,\"output_bytes\":2049}".to_string();
        let response = ExecuteTaskResponse {
            success: true,
            status_message: "7".into(),
            managed_executed_ops: 2_500,
            managed_output_bytes: 2_049,
            managed_receipt_json: receipt_json.clone(),
        };
        let (worker_addr, mut execute_rx) =
            match fake_worker_execute_server_with_response(response).await {
                Some(parts) => parts,
                None => return,
            };

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&username)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '10.0.0.2', 4, 16)",
        )
        .bind(&worker_id)
        .bind(format!("provider-{unique}"))
        .execute(&db.pool)
        .await
        .unwrap();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.owner = username.clone();
        task.runtime = Some("managed-function-v0".into());
        task.task_source = Some("return 7;".into());
        task.max_cpt = 25;
        dispatcher.repo.create(&task).await.unwrap();
        dispatcher
            .repo
            .assign_to_worker(&task_id, &worker_id, &worker_addr.to_string())
            .await
            .unwrap();

        execute_on_worker(
            dispatcher.repo.clone(),
            task,
            worker_id.clone(),
            worker_addr.to_string(),
            "test-secret",
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), execute_rx.recv())
                .await
                .unwrap()
                .as_deref(),
            Some(task_id.as_str())
        );
        let stored = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, TaskStatus::Completed);
        assert_eq!(stored.billed_amount, 7);
        assert_eq!(stored.managed_executed_ops, 2_500);
        assert_eq!(stored.managed_output_bytes, 2_049);
        assert_eq!(
            stored.managed_receipt_json.as_deref(),
            Some(receipt_json.as_str())
        );

        sqlx::query("DELETE FROM ledger_entries WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM task_attestations WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&username)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_pending_from_registered_workers() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_registered_workers").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-registered-{}", unique);
        let worker_id = format!("dispatch-registered-w-{}", unique);

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();

        let task = make_task(&task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.unwrap();
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb,
             cpu_score, gpu_score, gpu_memory_gb, gpu_name, vram_mb,
             storage_total_gb, storage_available_gb, location, status, available_memory_gb, queue_capacity)
             VALUES ($1,'test','127.0.0.1:50053',4,16,400,0,0,NULL,0,500,200,'local','IDLE',16,4)",
        )
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ($1, 10, 0, 100, false)",
        )
        .bind(&worker_id)
        .execute(&db.pool)
        .await
        .unwrap();

        let count = dispatcher
            .dispatch_pending_from_registered_workers()
            .await
            .unwrap();
        assert!(count >= 1);

        let updated = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, TaskStatus::Assigned);
        assert_eq!(updated.worker_id.as_deref(), Some(worker_id.as_str()));

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_pending_multiple() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_pending_multiple").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        sqlx::query("DELETE FROM tasks WHERE task_id LIKE 'dispatch-multi-%'")
            .execute(&db.pool)
            .await
            .ok();
        for i in 0..3 {
            let task = make_task(&format!("dispatch-multi-{}", i), TaskStatus::Pending, 0);
            dispatcher.repo.create(&task).await.ok();
        }
        let workers = vec![
            make_worker("wm1", 8, 32, WorkerStatus::Idle),
            make_worker("wm2", 8, 32, WorkerStatus::Idle),
        ];
        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES ('wm1', 10, 0, 100, false), ('wm2', 10, 0, 100, false)
             ON CONFLICT (worker_id) DO UPDATE SET score = EXCLUDED.score, banned = EXCLUDED.banned",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        let count = dispatcher.dispatch_pending(&workers).await.unwrap();
        assert!(count >= 1);
        for i in 0..3 {
            sqlx::query("DELETE FROM tasks WHERE task_id = $1")
                .bind(format!("dispatch-multi-{}", i))
                .execute(&db.pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ('wm1', 'wm2')")
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_dispatch_pending_excludes_banned_worker_by_trust() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_excludes_banned_worker").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);
        let unique = uuid::Uuid::new_v4().to_string();
        let task_id = format!("dispatch-trust-banned-{}", unique);
        let banned_worker_id = format!("dispatch-trust-banned-w-{}", unique);
        let trusted_worker_id = format!("dispatch-trust-ok-w-{}", unique);

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
            .bind(&banned_worker_id)
            .bind(&trusted_worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
            .bind(&banned_worker_id)
            .bind(&trusted_worker_id)
            .execute(&db.pool)
            .await
            .ok();

        let task = make_task(&task_id, TaskStatus::Pending, 0);
        dispatcher.repo.create(&task).await.unwrap();

        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb,
             cpu_score, gpu_score, gpu_memory_gb, gpu_name, vram_mb,
             storage_total_gb, storage_available_gb, location, status, available_memory_gb, queue_capacity)
             VALUES
             ($1,'test','127.0.0.1:50053',4,16,400,0,0,NULL,0,500,200,'local','IDLE',16,4),
             ($2,'test','127.0.0.1:50054',4,16,400,0,0,NULL,0,500,200,'local','IDLE',16,4)",
        )
        .bind(&banned_worker_id)
        .bind(&trusted_worker_id)
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO worker_reputation (worker_id, successful_tasks, failed_tasks, score, banned)
             VALUES
             ($1, 10, 0, 200, true),
             ($2, 10, 0, 200, false)",
        )
        .bind(&banned_worker_id)
        .bind(&trusted_worker_id)
        .execute(&db.pool)
        .await
        .unwrap();

        let count = dispatcher
            .dispatch_pending_from_registered_workers()
            .await
            .unwrap();
        assert!(count >= 1);

        let updated = dispatcher
            .repo
            .find_by_task_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, TaskStatus::Assigned);
        assert_eq!(
            updated.worker_id.as_deref(),
            Some(trusted_worker_id.as_str())
        );

        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_reputation WHERE worker_id IN ($1, $2)")
            .bind(&banned_worker_id)
            .bind(&trusted_worker_id)
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id IN ($1, $2)")
            .bind(&banned_worker_id)
            .bind(&trusted_worker_id)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn test_rank_workers_prefers_worker_with_cache_affinity() {
        let lock = dispatcher_db_lock();
        let _guard = lock.lock().await;
        let (db, fixture) = match test_db("dispatcher_cache_affinity").await {
            Some(parts) => parts,
            None => return,
        };
        let dispatcher = Dispatcher::new(db.clone(), 30, 2);

        let task_id = format!("dispatch-cache-target-{}", uuid::Uuid::new_v4());
        let hist_task_id = format!("dispatch-cache-hist-{}", uuid::Uuid::new_v4());
        let worker_a = format!("dispatch-cache-a-{}", uuid::Uuid::new_v4());
        let worker_b = format!("dispatch-cache-b-{}", uuid::Uuid::new_v4());
        let torrent = "magnet:?xt=urn:btih:cache-affinity";

        sqlx::query("DELETE FROM tasks WHERE task_id IN ($1, $2)")
            .bind(&task_id)
            .bind(&hist_task_id)
            .execute(&db.pool)
            .await
            .ok();

        let mut task = make_task(&task_id, TaskStatus::Pending, 0);
        task.torrent_source = Some(torrent.into());
        dispatcher.repo.create(&task).await.unwrap();

        sqlx::query(
            "INSERT INTO tasks (
                task_id, owner, worker_id, worker_ip, status, torrent_source,
                req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
                host_count, max_cpt, billing_settled, billed_amount, max_retries,
                deterministic, side_effects, priority, cache_hits, created_at, last_update, completed_at
             ) VALUES (
                $1, 'example-user', $2, '127.0.0.1', 'COMPLETED', $3,
                100, 0, 4, 0, 10,
                1, 1000, true, 1000, 3,
                false, false, 0, 0, NOW() - INTERVAL '30 days', NOW() - INTERVAL '30 days', NOW() - INTERVAL '30 days'
             )",
        )
        .bind(&hist_task_id)
        .bind(&worker_a)
        .bind(torrent)
        .execute(&db.pool)
        .await
        .unwrap();

        let hist_task_recent = format!("dispatch-cache-hist-recent-{}", uuid::Uuid::new_v4());
        sqlx::query(
            "INSERT INTO tasks (
                task_id, owner, worker_id, worker_ip, status, torrent_source,
                req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, req_storage_gb,
                host_count, max_cpt, billing_settled, billed_amount, max_retries,
                deterministic, side_effects, priority, cache_hits, created_at, last_update, completed_at
             ) VALUES (
                $1, 'example-user', $2, '127.0.0.1', 'COMPLETED', $3,
                100, 0, 4, 0, 10,
                1, 1000, true, 1000, 3,
                false, false, 0, 1, NOW(), NOW(), NOW()
             )",
        )
        .bind(&hist_task_recent)
        .bind(&worker_b)
        .bind(torrent)
        .execute(&db.pool)
        .await
        .unwrap();

        let workers = vec![
            make_worker(&worker_a, 4, 16, WorkerStatus::Idle),
            make_worker(&worker_b, 4, 16, WorkerStatus::Idle),
        ];

        let ranked = dispatcher
            .rank_workers_by_cache_affinity(&task, &workers)
            .await
            .unwrap();
        assert_eq!(
            ranked.first().map(|w| w.worker_id.as_str()),
            Some(worker_b.as_str())
        );

        sqlx::query("DELETE FROM tasks WHERE task_id IN ($1, $2, $3)")
            .bind(&task_id)
            .bind(&hist_task_id)
            .bind(&hist_task_recent)
            .execute(&db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    async fn fake_worker_execute_server(
    ) -> Option<(SocketAddr, tokio::sync::mpsc::Receiver<String>)> {
        fake_worker_execute_server_with_response(ExecuteTaskResponse {
            success: true,
            status_message: "executed".into(),
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: String::new(),
        })
        .await
    }

    async fn fake_worker_execute_server_with_response(
        response: ExecuteTaskResponse,
    ) -> Option<(SocketAddr, tokio::sync::mpsc::Receiver<String>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
        let addr = listener.local_addr().ok()?;
        let (execute_tx, execute_rx) = tokio::sync::mpsc::channel(1);
        let service = WorkerNodeServiceServer::new(FakeWorkerExecuteService {
            execute_tx,
            response,
        });
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        for _ in 0..30 {
            if hivemind_proto::worker_node_service_client::WorkerNodeServiceClient::connect(
                format!("http://{addr}"),
            )
            .await
            .is_ok()
            {
                return Some((addr, execute_rx));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    struct FakeWorkerExecuteService {
        execute_tx: tokio::sync::mpsc::Sender<String>,
        response: ExecuteTaskResponse,
    }

    #[tonic::async_trait]
    impl WorkerNodeService for FakeWorkerExecuteService {
        async fn execute_task(
            &self,
            request: Request<ExecuteTaskRequest>,
        ) -> Result<Response<ExecuteTaskResponse>, Status> {
            let task_id = request.into_inner().task_id;
            let _ = self.execute_tx.send(task_id).await;
            Ok(Response::new(self.response.clone()))
        }

        async fn task_output_upload(
            &self,
            _request: Request<TaskOutputUploadRequest>,
        ) -> Result<Response<TaskOutputUploadResponse>, Status> {
            Err(Status::unimplemented("fake worker does not upload output"))
        }

        async fn task_result_upload(
            &self,
            _request: Request<TaskResultUploadRequest>,
        ) -> Result<Response<TaskResultUploadResponse>, Status> {
            Err(Status::unimplemented("fake worker does not upload results"))
        }

        async fn task_output(
            &self,
            _request: Request<TaskOutputRequest>,
        ) -> Result<Response<TaskOutputResponse>, Status> {
            Err(Status::unimplemented("fake worker has no output"))
        }

        async fn stop_task_execution(
            &self,
            _request: Request<StopTaskExecutionRequest>,
        ) -> Result<Response<StopTaskExecutionResponse>, Status> {
            Ok(Response::new(StopTaskExecutionResponse {
                success: true,
                status_message: "Stop requested".into(),
            }))
        }

        async fn task_usage(
            &self,
            _request: Request<TaskUsageRequest>,
        ) -> Result<Response<TaskUsageResponse>, Status> {
            Err(Status::unimplemented("fake worker does not report usage"))
        }
    }
}

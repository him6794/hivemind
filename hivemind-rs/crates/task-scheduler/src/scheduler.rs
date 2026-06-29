use hivemind_models::{Task, WorkerNode, WorkerStatus};

/// Find the best worker for a given task based on resource requirements and availability.
/// Considers CPU score, GPU score, RAM, VRAM, and storage.
pub async fn find_best_worker(task: &Task, workers: &[WorkerNode]) -> Option<WorkerNode> {
    fn effective_i32(hardware: i32, limit: i32) -> i32 {
        if limit > 0 {
            hardware.min(limit)
        } else {
            hardware
        }
    }

    fn effective_i64(hardware: i64, limit: i64) -> i64 {
        if limit > 0 {
            hardware.min(limit)
        } else {
            hardware
        }
    }

    fn effective_cpu_score(w: &WorkerNode) -> i32 {
        if w.cpu_cores_limit > 0 && w.cpu_cores > 0 {
            let limited_cores = w.cpu_cores.min(w.cpu_cores_limit).max(0);
            ((i64::from(w.cpu_score) * i64::from(limited_cores)) / i64::from(w.cpu_cores)) as i32
        } else {
            w.cpu_score
        }
    }

    fn effective_available_memory_gb(w: &WorkerNode) -> i32 {
        w.available_memory_gb
            .min(effective_i32(w.memory_gb, w.memory_gb_limit))
    }

    fn effective_available_storage_gb(w: &WorkerNode) -> i64 {
        w.storage_available_gb
            .min(effective_i64(w.storage_available_gb, w.storage_gb_limit))
    }

    fn accepts_task(w: &WorkerNode, task: &Task, allow_busy: bool) -> bool {
        let status_ok = if allow_busy {
            matches!(
                w.status,
                WorkerStatus::Active | WorkerStatus::Idle | WorkerStatus::Busy
            )
        } else {
            matches!(w.status, WorkerStatus::Active | WorkerStatus::Idle)
        };

        status_ok
            && w.provider_enabled
            && effective_cpu_score(w) >= task.req_cpu_score
            && w.gpu_score >= task.req_gpu_score
            && effective_available_memory_gb(w) >= task.req_memory_gb
            && effective_i32(w.gpu_memory_gb, w.gpu_memory_gb_limit) >= task.req_gpu_memory_gb
            && effective_available_storage_gb(w) >= task.req_storage_gb
            && w.min_cpt_per_hour <= task.max_cpt
    }

    let mut candidates: Vec<&WorkerNode> = workers
        .iter()
        .filter(|w| accepts_task(w, task, false))
        .collect();

    if candidates.is_empty() {
        candidates = workers
            .iter()
            .filter(|w| accepts_task(w, task, true))
            .collect();
    }

    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|a, b| {
        fn status_priority(s: &WorkerStatus) -> u8 {
            match s {
                WorkerStatus::Idle => 0,
                WorkerStatus::Active => 1,
                WorkerStatus::Busy => 2,
                _ => 3,
            }
        }
        status_priority(&a.status)
            .cmp(&status_priority(&b.status))
            .then_with(|| {
                a.cpu_usage
                    .partial_cmp(&b.cpu_usage)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| effective_available_memory_gb(b).cmp(&effective_available_memory_gb(a)))
            .then_with(|| effective_available_storage_gb(b).cmp(&effective_available_storage_gb(a)))
            .then_with(|| b.queue_capacity.cmp(&a.queue_capacity))
    });

    candidates.first().map(|w| (*w).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_models::WorkerStatus;

    fn make_worker(
        id: &str,
        cpu: i32,
        mem: i32,
        cpu_usage: f64,
        status: WorkerStatus,
    ) -> WorkerNode {
        WorkerNode {
            id: uuid::Uuid::new_v4(),
            worker_id: id.into(),
            username: "test".into(),
            ip: "127.0.0.1".into(),
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
            cpu_usage,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            available_memory_gb: mem,
            queue_capacity: cpu,
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_find_best_worker_prefers_idle() {
        let workers = vec![
            make_worker("w1", 4, 16, 80.0, WorkerStatus::Active),
            make_worker("w2", 8, 32, 10.0, WorkerStatus::Idle),
            make_worker("w3", 4, 8, 5.0, WorkerStatus::Active),
        ];

        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: "t1".into(),
            owner: "u1".into(),
            worker_id: None,
            worker_ip: None,
            status: hivemind_models::TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some("btih".into()),
            runtime: None,
            task_source: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 300,
            req_gpu_score: 0,
            req_memory_gb: 8,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1,
            max_cpt: 1000,
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
            created_at: chrono::Utc::now(),
            last_update: chrono::Utc::now(),
            completed_at: None,
        };

        let best = find_best_worker(&task, &workers).await;
        assert!(best.is_some());
        assert_eq!(best.unwrap().worker_id, "w2");
    }

    #[tokio::test]
    async fn test_no_suitable_worker() {
        let workers = vec![make_worker("w1", 1, 2, 0.0, WorkerStatus::Active)];
        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: "t2".into(),
            owner: "u1".into(),
            worker_id: None,
            worker_ip: None,
            status: hivemind_models::TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some("btih".into()),
            runtime: None,
            task_source: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 500,
            req_gpu_score: 0,
            req_memory_gb: 32,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
            host_count: 1,
            max_cpt: 1000,
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
            created_at: chrono::Utc::now(),
            last_update: chrono::Utc::now(),
            completed_at: None,
        };
        let best = find_best_worker(&task, &workers).await;
        assert!(best.is_none());
    }

    #[tokio::test]
    async fn test_provider_settings_filter_disabled_capped_and_overpriced_workers() {
        let mut disabled = make_worker("disabled", 8, 32, 0.0, WorkerStatus::Idle);
        disabled.provider_enabled = false;

        let mut cpu_capped = make_worker("cpu-capped", 8, 32, 0.0, WorkerStatus::Idle);
        cpu_capped.cpu_cores_limit = 2;

        let mut memory_capped = make_worker("memory-capped", 8, 32, 0.0, WorkerStatus::Idle);
        memory_capped.memory_gb_limit = 4;

        let mut storage_capped = make_worker("storage-capped", 8, 32, 0.0, WorkerStatus::Idle);
        storage_capped.storage_gb_limit = 5;

        let mut overpriced = make_worker("overpriced", 8, 32, 0.0, WorkerStatus::Idle);
        overpriced.min_cpt_per_hour = 200;

        let affordable = make_worker("affordable", 8, 32, 0.0, WorkerStatus::Active);

        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: "t-provider-settings".into(),
            owner: "u1".into(),
            worker_id: None,
            worker_ip: None,
            status: hivemind_models::TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some("btih".into()),
            runtime: None,
            task_source: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 300,
            req_gpu_score: 0,
            req_memory_gb: 8,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
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
            created_at: chrono::Utc::now(),
            last_update: chrono::Utc::now(),
            completed_at: None,
        };

        let workers = vec![
            disabled,
            cpu_capped,
            memory_capped,
            storage_capped,
            overpriced,
            affordable,
        ];

        let best = find_best_worker(&task, &workers).await;
        assert!(best.is_some());
        assert_eq!(best.unwrap().worker_id, "affordable");
    }

    #[tokio::test]
    async fn test_provider_settings_use_available_and_effective_resources_for_selection() {
        let mut unavailable_memory =
            make_worker("unavailable-memory", 8, 64, 0.0, WorkerStatus::Idle);
        unavailable_memory.available_memory_gb = 4;

        let mut capped_sort_winner =
            make_worker("effective-winner", 8, 64, 0.0, WorkerStatus::Idle);
        capped_sort_winner.available_memory_gb = 64;
        capped_sort_winner.memory_gb_limit = 16;
        capped_sort_winner.storage_available_gb = 500;
        capped_sort_winner.storage_gb_limit = 50;

        let mut capped_sort_loser = make_worker("effective-loser", 8, 64, 0.0, WorkerStatus::Idle);
        capped_sort_loser.available_memory_gb = 64;
        capped_sort_loser.memory_gb_limit = 12;
        capped_sort_loser.storage_available_gb = 1000;
        capped_sort_loser.storage_gb_limit = 20;

        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: "t-effective-resources".into(),
            owner: "u1".into(),
            worker_id: None,
            worker_ip: None,
            status: hivemind_models::TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some("btih".into()),
            runtime: None,
            task_source: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 300,
            req_gpu_score: 0,
            req_memory_gb: 8,
            req_gpu_memory_gb: 0,
            req_storage_gb: 10,
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
            created_at: chrono::Utc::now(),
            last_update: chrono::Utc::now(),
            completed_at: None,
        };

        let workers = vec![unavailable_memory, capped_sort_loser, capped_sort_winner];

        let best = find_best_worker(&task, &workers).await;
        assert!(best.is_some());
        assert_eq!(best.unwrap().worker_id, "effective-winner");
    }
}

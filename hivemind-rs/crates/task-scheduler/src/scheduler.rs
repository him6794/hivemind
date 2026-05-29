use hivemind_models::{Task, WorkerNode, WorkerStatus};

/// Find the best worker for a given task based on resource requirements and availability
pub async fn find_best_worker(
    task: &Task,
    workers: &[WorkerNode],
) -> Option<WorkerNode> {
    let mut candidates: Vec<&WorkerNode> = workers
        .iter()
        .filter(|w| {
            matches!(w.status, WorkerStatus::Active | WorkerStatus::Idle)
                && w.cpu_score >= task.req_cpu_score
                && w.gpu_score >= task.req_gpu_score
                && w.memory_gb >= task.req_memory_gb
                && w.gpu_memory_gb >= task.req_gpu_memory_gb
        })
        .collect();

    if candidates.is_empty() {
        // Fallback: include BUSY workers if no idle ones match
        candidates = workers
            .iter()
            .filter(|w| {
                matches!(w.status, WorkerStatus::Active | WorkerStatus::Idle | WorkerStatus::Busy)
                    && w.cpu_score >= task.req_cpu_score
                    && w.gpu_score >= task.req_gpu_score
                    && w.memory_gb >= task.req_memory_gb
                    && w.gpu_memory_gb >= task.req_gpu_memory_gb
            })
            .collect();
    }

    if candidates.is_empty() {
        return None;
    }

    // Sort by: lowest current CPU usage, then highest available memory, then highest queue capacity
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
            .then_with(|| b.available_memory_gb.cmp(&a.available_memory_gb))
            .then_with(|| b.queue_capacity.cmp(&a.queue_capacity))
    });

    candidates.first().map(|w| (*w).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_models::WorkerStatus;

    fn make_worker(id: &str, cpu: i32, mem: i32, cpu_usage: f64, status: WorkerStatus) -> WorkerNode {
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
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 300,
            req_gpu_score: 0,
            req_memory_gb: 8,
            req_gpu_memory_gb: 0,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
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
        assert_eq!(best.unwrap().worker_id, "w2"); // idle, low cpu, highest spec
    }

    #[tokio::test]
    async fn test_no_suitable_worker() {
        let workers = vec![
            make_worker("w1", 1, 2, 0.0, WorkerStatus::Active),
        ];

        let mut task = Task {
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
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 500,
            req_gpu_score: 0,
            req_memory_gb: 32,
            req_gpu_memory_gb: 0,
            host_count: 1,
            max_cpt: 1000,
            billing_settled: false,
            billed_amount: 0,
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
}
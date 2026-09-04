use general_compute_runtime::production::ManagedDslBackendRegistration;
use general_compute_runtime::{
    managed_gpu::{ManagedGpuRequest, MANAGED_GPU_RUNTIME_VERSION},
    CapabilityMatrix, GeneralComputeRequest, TrustedWorkerCapabilityRegistration,
    GENERAL_COMPUTE_RUNTIME_VERSION, MANAGED_DSL_RUNTIME_VERSION,
};

use hivemind_models::{
    Task, WorkerCapabilityReport, WorkerNode, WorkerRuntimeCapability, WorkerStatus,
    PRIVATE_STATIC_ADMISSION_MODE, PUBLIC_DYNAMIC_ADMISSION_MODE, WORKER_CAPABILITY_REPORT_VERSION,
};
use sha2::{Digest, Sha256};

pub const PUBLIC_DYNAMIC_CAPABILITY_MAX_AGE_SECS: i64 = 30;

/// Check a Nodepool-persisted, operator-approved managed DSL capability snapshot
/// against the exact task identity that will be executed.
///
/// `managed-function-v0` predates the explicit backend identity fields, so an
/// approved registration for that runtime is sufficient. The
/// `production_sandboxed_dsl` runtime must bind both the selected backend and
/// the canonical semantics manifest to the approved registration.
pub fn worker_supports_managed_dsl_request(
    persisted_capabilities_json: Option<&str>,
    runtime: &str,
    requested_backend_id: Option<&str>,
    requested_semantics_manifest_sha256: Option<&str>,
    requested_budget_units: i64,
) -> bool {
    let Some(persisted_capabilities_json) = persisted_capabilities_json else {
        return false;
    };
    if requested_budget_units <= 0 {
        return false;
    }
    if let Ok(registrations) =
        serde_json::from_str::<Vec<ManagedDslBackendRegistration>>(persisted_capabilities_json)
    {
        return registrations.iter().any(|registration| {
            registration.validate().is_ok()
                && registration.runtime_version == MANAGED_DSL_RUNTIME_VERSION
                && requested_budget_units as u64 <= registration.max_usage_units
                && match runtime {
                    "managed-function-v0" => true,
                    "production_sandboxed_dsl" => {
                        requested_backend_id.is_some_and(|backend_id| {
                            !backend_id.trim().is_empty() && backend_id == registration.backend_id
                        }) && requested_semantics_manifest_sha256.is_some_and(|semantics| {
                            semantics == registration.semantics_manifest_sha256
                        })
                    }
                    _ => false,
                }
        });
    }

    let Ok(capabilities) =
        serde_json::from_str::<Vec<WorkerRuntimeCapability>>(persisted_capabilities_json)
    else {
        return false;
    };
    let report = WorkerCapabilityReport {
        protocol_version: WORKER_CAPABILITY_REPORT_VERSION,
        capabilities,
        ready: true,
        readiness_reason: String::new(),
    };
    if report.validate_public_dynamic().is_err() {
        return false;
    }
    report.capabilities.iter().any(|capability| {
        capability.runtime == runtime
            && requested_budget_units as u64 <= capability.max_usage_units
            && match runtime {
                "managed-function-v0" => {
                    requested_backend_id.is_none_or(str::is_empty)
                        && requested_semantics_manifest_sha256.is_none_or(str::is_empty)
                }
                "production_sandboxed_dsl" => {
                    requested_backend_id.is_some_and(|backend_id| {
                        backend_id == capability.backend_id && !backend_id.trim().is_empty()
                    }) && requested_semantics_manifest_sha256
                        .is_some_and(|semantics| semantics == capability.semantics_manifest_sha256)
                }
                _ => false,
            }
    })
}

pub fn worker_supports_general_compute_request(
    request: &GeneralComputeRequest,
    persisted_capabilities_json: Option<&str>,
) -> bool {
    let Some(persisted_capabilities_json) = persisted_capabilities_json else {
        return false;
    };
    let Ok(registration) =
        serde_json::from_str::<TrustedWorkerCapabilityRegistration>(persisted_capabilities_json)
    else {
        return false;
    };
    if CapabilityMatrix::new(registration.backends.clone())
        .validate_request(request, &registration.worker)
        .is_err()
    {
        return false;
    }

    // The boolean `gpu_available` field is only a coarse scheduling hint. A
    // typed GPU request must also resolve against the Nodepool-persisted,
    // operator-approved capability identities before dispatch.
    registration.select_gpu_for_request(request).is_ok()
}

pub fn worker_supports_managed_gpu_request(
    request: &ManagedGpuRequest,
    persisted_capabilities_json: Option<&str>,
) -> bool {
    if request.validate().is_err() || request.reservation_cpt == 0 {
        return false;
    }
    let Some(persisted_capabilities_json) = persisted_capabilities_json else {
        return false;
    };
    let Ok(registration) =
        serde_json::from_str::<TrustedWorkerCapabilityRegistration>(persisted_capabilities_json)
    else {
        return false;
    };
    registration.select_managed_gpu_for_request(request).is_ok()
}

pub fn public_dynamic_managed_dsl_snapshot(worker: &WorkerNode) -> Option<&str> {
    if worker.admission_mode != PUBLIC_DYNAMIC_ADMISSION_MODE || !worker.dynamic_admission_ready {
        return None;
    }
    let observed_at = worker.dynamic_observed_at?;
    let age = chrono::Utc::now().signed_duration_since(observed_at);
    if age < chrono::Duration::zero()
        || age > chrono::Duration::seconds(PUBLIC_DYNAMIC_CAPABILITY_MAX_AGE_SECS)
    {
        return None;
    }
    let capabilities_json = worker.dynamic_capabilities_json.as_deref()?;
    let expected_digest = format!("sha256:{:x}", Sha256::digest(capabilities_json.as_bytes()));
    if worker.dynamic_capabilities_digest.as_deref() != Some(expected_digest.as_str()) {
        return None;
    }
    Some(capabilities_json)
}

pub fn general_compute_snapshot_for_worker(worker: &WorkerNode) -> Option<&str> {
    (worker.admission_mode == PRIVATE_STATIC_ADMISSION_MODE)
        .then_some(worker.general_compute_capabilities_json.as_deref())
        .flatten()
}

pub fn managed_dsl_snapshot_for_worker(worker: &WorkerNode) -> Option<&str> {
    if worker.admission_mode == PUBLIC_DYNAMIC_ADMISSION_MODE {
        public_dynamic_managed_dsl_snapshot(worker)
    } else if worker.admission_mode == PRIVATE_STATIC_ADMISSION_MODE {
        worker.managed_dsl_capabilities_json.as_deref()
    } else {
        None
    }
}

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

        let general_compute_compatible = match task.runtime.as_deref().map(str::trim) {
            Some("managed-function-v0") | Some("production_sandboxed_dsl") => {
                worker_supports_managed_dsl_request(
                    managed_dsl_snapshot_for_worker(w),
                    task.runtime.as_deref().unwrap_or_default(),
                    task.managed_dsl_backend_id.as_deref(),
                    task.managed_dsl_semantics_manifest_sha256.as_deref(),
                    task.max_cpt,
                )
            }
            Some(GENERAL_COMPUTE_RUNTIME_VERSION) => task
                .general_compute_manifest_json
                .as_deref()
                .and_then(|manifest| serde_json::from_slice::<GeneralComputeRequest>(manifest).ok())
                .is_some_and(|request| {
                    worker_supports_general_compute_request(
                        &request,
                        general_compute_snapshot_for_worker(w),
                    )
                }),
            Some(MANAGED_GPU_RUNTIME_VERSION) => task
                .managed_gpu_manifest_json
                .as_deref()
                .and_then(|manifest| serde_json::from_slice::<ManagedGpuRequest>(manifest).ok())
                .is_some_and(|request| {
                    u64::try_from(task.max_cpt).ok() == Some(request.reservation_cpt)
                        && worker_supports_managed_gpu_request(
                            &request,
                            general_compute_snapshot_for_worker(w),
                        )
                }),
            Some(_) | None => true,
        };

        status_ok
            && w.provider_enabled
            && general_compute_compatible
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
    use general_compute_runtime::{
        ArtifactManifest, ArtifactRole, DeterminismPolicy, ExecutionPolicy, GeneralComputeRequest,
        GENERAL_COMPUTE_RUNTIME_VERSION,
    };
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
            general_compute_capabilities_json: None,
            managed_dsl_capabilities_json: None,
            admission_mode: hivemind_models::PRIVATE_STATIC_ADMISSION_MODE.into(),
            dynamic_capabilities_json: None,
            dynamic_capabilities_digest: None,
            dynamic_admission_ready: false,
            dynamic_readiness_reason: None,
            dynamic_observed_at: None,
            last_heartbeat: chrono::Utc::now(),
            registered_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn managed_dsl_matching_uses_the_separate_snapshot() {
        let registration = general_compute_runtime::production::ManagedDslBackendRegistration {
            backend_id: "dsl-default".into(),
            runtime_version: "managed-function-v0".into(),
            semantics_manifest_sha256:
                general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256.into(),
            max_usage_units: 10,
            max_output_bytes: 1024,
        };
        let snapshot = serde_json::to_string(&vec![registration]).unwrap();
        assert!(worker_supports_managed_dsl_request(
            Some(&snapshot),
            "production_sandboxed_dsl",
            Some("dsl-default"),
            Some(general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256),
            10,
        ));
        assert!(!worker_supports_managed_dsl_request(
            None,
            "production_sandboxed_dsl",
            Some("dsl-default"),
            Some(general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256),
            10,
        ));
    }

    #[test]
    fn managed_dsl_matching_rejects_wrong_backend_and_semantics() {
        let registration = ManagedDslBackendRegistration {
            backend_id: "dsl-default".into(),
            runtime_version: MANAGED_DSL_RUNTIME_VERSION.into(),
            semantics_manifest_sha256:
                general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256.into(),
            max_usage_units: 10,
            max_output_bytes: 1024,
        };
        let snapshot = serde_json::to_string(&vec![registration]).unwrap();
        let semantics = general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256;

        assert!(!worker_supports_managed_dsl_request(
            Some(&snapshot),
            "production_sandboxed_dsl",
            Some("other-backend"),
            Some(semantics),
            10,
        ));
        assert!(!worker_supports_managed_dsl_request(
            Some(&snapshot),
            "production_sandboxed_dsl",
            Some("dsl-default"),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            10,
        ));
    }

    #[test]
    fn managed_dsl_matching_accepts_one_of_multiple_approved_backends() {
        let semantics = general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256;
        let registrations = vec![
            ManagedDslBackendRegistration {
                backend_id: "dsl-first".into(),
                runtime_version: MANAGED_DSL_RUNTIME_VERSION.into(),
                semantics_manifest_sha256: semantics.into(),
                max_usage_units: 10,
                max_output_bytes: 1024,
            },
            ManagedDslBackendRegistration {
                backend_id: "dsl-second".into(),
                runtime_version: MANAGED_DSL_RUNTIME_VERSION.into(),
                semantics_manifest_sha256: semantics.into(),
                max_usage_units: 20,
                max_output_bytes: 1024,
            },
        ];
        let snapshot = serde_json::to_string(&registrations).unwrap();

        assert!(worker_supports_managed_dsl_request(
            Some(&snapshot),
            "production_sandboxed_dsl",
            Some("dsl-second"),
            Some(semantics),
            20,
        ));
    }

    #[test]
    fn legacy_managed_dsl_matching_uses_an_approved_registration() {
        let registration = ManagedDslBackendRegistration {
            backend_id: "dsl-default".into(),
            runtime_version: MANAGED_DSL_RUNTIME_VERSION.into(),
            semantics_manifest_sha256:
                general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256.into(),
            max_usage_units: 10,
            max_output_bytes: 1024,
        };
        let snapshot = serde_json::to_string(&vec![registration]).unwrap();

        assert!(worker_supports_managed_dsl_request(
            Some(&snapshot),
            "managed-function-v0",
            None,
            None,
            10,
        ));
        assert!(!worker_supports_managed_dsl_request(
            Some(&snapshot),
            "managed-function-v0",
            None,
            None,
            11,
        ));
    }

    #[test]
    fn public_dynamic_snapshot_requires_fresh_matching_digest() {
        let mut worker = make_worker("public-worker", 4, 16, 0.0, WorkerStatus::Idle);
        let report = WorkerCapabilityReport::public_managed_dsl();
        let capabilities_json = report.capabilities_json().unwrap();
        let digest = format!("sha256:{:x}", Sha256::digest(capabilities_json.as_bytes()));
        worker.admission_mode = PUBLIC_DYNAMIC_ADMISSION_MODE.into();
        worker.dynamic_capabilities_json = Some(capabilities_json.clone());
        worker.dynamic_capabilities_digest = Some(digest);
        worker.dynamic_admission_ready = true;
        worker.dynamic_observed_at = Some(chrono::Utc::now());

        assert_eq!(
            public_dynamic_managed_dsl_snapshot(&worker),
            Some(capabilities_json.as_str())
        );
        assert!(worker_supports_managed_dsl_request(
            managed_dsl_snapshot_for_worker(&worker),
            "managed-function-v0",
            None,
            None,
            1,
        ));

        worker.dynamic_capabilities_digest = Some("sha256:tampered".into());
        assert!(public_dynamic_managed_dsl_snapshot(&worker).is_none());

        worker.dynamic_capabilities_digest = Some(format!(
            "sha256:{:x}",
            Sha256::digest(capabilities_json.as_bytes())
        ));
        worker.dynamic_observed_at = Some(
            chrono::Utc::now()
                - chrono::Duration::seconds(PUBLIC_DYNAMIC_CAPABILITY_MAX_AGE_SECS + 1),
        );
        assert!(public_dynamic_managed_dsl_snapshot(&worker).is_none());

        worker.dynamic_observed_at = Some(chrono::Utc::now());
        worker.dynamic_admission_ready = false;
        assert!(public_dynamic_managed_dsl_snapshot(&worker).is_none());
    }

    #[test]
    fn private_static_snapshot_remains_separate_from_dynamic_observation() {
        let mut worker = make_worker("private-worker", 4, 16, 0.0, WorkerStatus::Idle);
        worker.managed_dsl_capabilities_json = Some(
            serde_json::to_string(&vec![ManagedDslBackendRegistration {
                backend_id: "private-backend".into(),
                runtime_version: MANAGED_DSL_RUNTIME_VERSION.into(),
                semantics_manifest_sha256:
                    general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256.into(),
                max_usage_units: 10,
                max_output_bytes: 1024,
            }])
            .unwrap(),
        );
        worker.dynamic_capabilities_json = Some("[]".into());
        worker.dynamic_capabilities_digest = Some("sha256:ignored".into());
        worker.dynamic_admission_ready = true;
        worker.dynamic_observed_at = Some(chrono::Utc::now());

        assert_eq!(
            managed_dsl_snapshot_for_worker(&worker),
            worker.managed_dsl_capabilities_json.as_deref()
        );
    }

    #[test]
    fn public_workers_never_use_general_compute_static_snapshot() {
        let mut worker = make_worker("public-worker", 4, 16, 0.0, WorkerStatus::Idle);
        worker.admission_mode = PUBLIC_DYNAMIC_ADMISSION_MODE.into();
        worker.general_compute_capabilities_json = Some("operator-snapshot".into());

        assert!(general_compute_snapshot_for_worker(&worker).is_none());

        worker.admission_mode = PRIVATE_STATIC_ADMISSION_MODE.into();
        assert_eq!(
            general_compute_snapshot_for_worker(&worker),
            Some("operator-snapshot")
        );
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
            general_compute_manifest_json: None,
            managed_gpu_manifest_json: None,
            managed_dsl_backend_id: None,
            managed_dsl_semantics_manifest_sha256: None,
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
            general_compute_manifest_json: None,
            managed_gpu_manifest_json: None,
            managed_dsl_backend_id: None,
            managed_dsl_semantics_manifest_sha256: None,
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

    #[test]
    fn alpha_manifest_requires_nodepool_persisted_worker_capabilities() {
        let request = alpha_request();

        assert!(!worker_supports_general_compute_request(&request, None));
    }

    #[test]
    fn alpha_manifest_requires_a_matching_persisted_worker_capability_record() {
        let request = alpha_request();
        let matching = r#"{
            "worker":{
                "guest_image_digests":["sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
                "capabilities":["cpu"],
                "max_threads":4,
                "gpu_available":false
            },
            "backends":[{
                "backend_id":"python-cpython-312",
                "execution_mode":"reference_direct",
                "guest_image_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "capabilities":["cpu"],
                "max_threads":4,
                "network_allowed":false,
                "filesystem_read_only":true,
                "gpu_allowed":false
            }]
        }"#;
        let wrong_image = r#"{
            "worker":{
                "guest_image_digests":["sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"],
                "capabilities":["cpu"],
                "max_threads":4,
                "gpu_available":false
            },
            "backends":[{
                "backend_id":"python-cpython-312",
                "execution_mode":"reference_direct",
                "guest_image_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "capabilities":["cpu"],
                "max_threads":4,
                "network_allowed":false,
                "filesystem_read_only":true,
                "gpu_allowed":false
            }]
        }"#;

        assert!(worker_supports_general_compute_request(
            &request,
            Some(matching)
        ));
        assert!(!worker_supports_general_compute_request(
            &request,
            Some(wrong_image)
        ));
    }

    #[test]
    fn alpha_gpu_admission_requires_a_typed_persisted_identity() {
        use general_compute_runtime::gpu::{GpuCapability, GpuRequirement, GpuRuntime, GpuVendor};
        use general_compute_runtime::TrustedWorkerCapabilityRegistration;

        let image = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let requirement = GpuRequirement::new(
            GpuVendor::Nvidia,
            "sm_80",
            GpuRuntime::Cuda,
            "550.54",
            8 * 1024 * 1024 * 1024,
            4,
            image,
            false,
        )
        .unwrap();
        let capability = GpuCapability::new(
            GpuVendor::Nvidia,
            "gpu-0",
            "sm_80",
            GpuRuntime::Cuda,
            "12.4",
            "550.54",
            16 * 1024 * 1024 * 1024,
            8,
            image,
        )
        .unwrap();
        let mut request = alpha_request();
        request.execution_policy.gpu_required = true;
        request.execution_policy.gpu_requirement = Some(requirement);
        request.request_digest = request.canonical_request_digest();

        let mut registration: TrustedWorkerCapabilityRegistration =
            serde_json::from_str(&matching_capability_snapshot()).unwrap();
        registration.worker.gpu_available = true;
        registration.backends[0].gpu_allowed = true;
        registration.gpu_capabilities = vec![capability];
        let snapshot = serde_json::to_string(&registration).unwrap();
        assert!(worker_supports_general_compute_request(
            &request,
            Some(&snapshot)
        ));

        registration.gpu_capabilities.clear();
        let snapshot = serde_json::to_string(&registration).unwrap();
        assert!(!worker_supports_general_compute_request(
            &request,
            Some(&snapshot)
        ));
    }

    #[tokio::test]
    async fn alpha_task_is_scheduled_only_to_a_worker_with_the_nodepool_snapshot() {
        let request = alpha_request();
        let mut task = task_with_alpha_manifest(&request);
        task.req_cpu_score = 0;
        let unregistered = make_worker("unregistered", 4, 16, 0.0, WorkerStatus::Idle);
        let mut registered = make_worker("registered", 4, 16, 50.0, WorkerStatus::Active);
        registered.general_compute_capabilities_json = Some(matching_capability_snapshot());

        let selected = find_best_worker(&task, &[unregistered, registered]).await;

        assert_eq!(
            selected.map(|worker| worker.worker_id),
            Some("registered".into())
        );
    }

    fn alpha_request() -> GeneralComputeRequest {
        let mut request = GeneralComputeRequest {
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest: String::new(),
            runtime_version: GENERAL_COMPUTE_RUNTIME_VERSION.into(),
            guest_image_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            backend_id: "python-cpython-312".into(),
            entrypoint: "main".into(),
            source_artifact: ArtifactManifest::inline_json(
                "source",
                ArtifactRole::Source,
                b"source",
            ),
            input_artifacts: vec![],
            execution_policy: ExecutionPolicy::default(),
            determinism: DeterminismPolicy::default(),
            billing_version: "billing-v1".into(),
            cost_model_version: "cost-v1".into(),
        };
        request.request_digest = request.canonical_request_digest();
        request
    }

    fn task_with_alpha_manifest(request: &GeneralComputeRequest) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            task_id: "alpha-capability-task".into(),
            owner: "owner".into(),
            worker_id: None,
            worker_ip: None,
            status: hivemind_models::TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: None,
            runtime: Some(GENERAL_COMPUTE_RUNTIME_VERSION.into()),
            task_source: None,
            general_compute_manifest_json: Some(serde_json::to_vec(request).unwrap()),
            managed_gpu_manifest_json: None,
            managed_dsl_backend_id: None,
            managed_dsl_semantics_manifest_sha256: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 0,
            req_gpu_score: 0,
            req_memory_gb: 0,
            req_gpu_memory_gb: 0,
            req_storage_gb: 0,
            host_count: 1,
            max_cpt: 1,
            billing_settled: false,
            billed_amount: 0,
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
            retry_count: 0,
            max_retries: 0,
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
        }
    }

    fn matching_capability_snapshot() -> String {
        r#"{
            "worker":{
                "guest_image_digests":["sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
                "capabilities":["cpu"],
                "max_threads":4,
                "gpu_available":false
            },
            "backends":[{
                "backend_id":"python-cpython-312",
                "execution_mode":"reference_direct",
                "guest_image_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "capabilities":["cpu"],
                "max_threads":4,
                "network_allowed":false,
                "filesystem_read_only":true,
                "gpu_allowed":false
            }]
        }"#
        .into()
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
            general_compute_manifest_json: None,
            managed_gpu_manifest_json: None,
            managed_dsl_backend_id: None,
            managed_dsl_semantics_manifest_sha256: None,
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
            general_compute_manifest_json: None,
            managed_gpu_manifest_json: None,
            managed_dsl_backend_id: None,
            managed_dsl_semantics_manifest_sha256: None,
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

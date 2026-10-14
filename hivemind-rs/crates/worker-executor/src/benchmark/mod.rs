pub mod cpu;
pub mod gpu;
mod supervisor;
pub mod workload;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use hivemind_models::observations::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::RwLock;

pub const HELPER_MODE: &str = "--hivemind-hardware-v1";

/// Dispatch before service configuration or GUI initialization. The helper only
/// accepts this binary's fixed, versioned workloads.
pub fn dispatch_helper(args: &[String]) -> Option<Result<()>> {
    if args.get(1).map(String::as_str) != Some(HELPER_MODE) {
        return None;
    }
    Some((|| {
        let bytes = match (args.get(2).map(String::as_str), args.get(3), args.len()) {
            (Some("cpu"), Some(value), 4) => {
                let threads = value.parse::<u32>().context("invalid CPU thread budget")?;
                if !(1..=BENCHMARK_MAX_THREADS).contains(&threads) {
                    bail!("CPU thread budget out of range");
                }
                serde_json::to_vec(&cpu::run(threads)?)?
            }
            (Some("inventory"), None, 3) => serde_json::to_vec(&gpu::inventory()?)?,
            (Some("gpu"), Some(value), 4) if value.len() <= 4096 => {
                let identity: BenchmarkGpuIdentity = serde_json::from_str(value)?;
                identity.validate().map_err(anyhow::Error::msg)?;
                serde_json::to_vec(&gpu::run(&identity)?)?
            }
            _ => bail!("invalid fixed benchmark selector"),
        };
        if bytes.len() > BENCHMARK_MAX_BYTES {
            bail!("benchmark output exceeded bound");
        }
        std::io::stdout().lock().write_all(&bytes)?;
        Ok(())
    })())
}

pub fn executable_build_id() -> Result<String> {
    static BUILD_ID: OnceLock<String> = OnceLock::new();
    if let Some(value) = BUILD_ID.get() {
        return Ok(value.clone());
    }
    let mut file = std::fs::File::open(std::env::current_exe()?)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16384];
    loop {
        let len = file.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        digest.update(&buffer[..len]);
    }
    let id = format!("{:x}", digest.finalize());
    let _ = BUILD_ID.set(id.clone());
    Ok(id)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cache {
    cpu_identity: String,
    report: HardwareBenchmarkReport,
}

pub struct BenchmarkCoordinator {
    report: Mutex<HardwareBenchmarkReport>,
    cache: Option<PathBuf>,
    cpu_identity: String,
    threads: AtomicU32,
    busy: AtomicBool,
    pub(crate) admission: Arc<RwLock<()>>,
}

impl BenchmarkCoordinator {
    pub fn new(state_root: Option<PathBuf>) -> Result<Arc<Self>> {
        let build_id = executable_build_id()?;
        let identity = workload::cpu_identity();
        let threads = available_threads();
        let cache = state_root.map(|root| root.join("hardware-benchmark-v1.json"));
        let mut report = HardwareBenchmarkReport::empty(build_id.clone(), !cfg!(debug_assertions));
        if let Some(path) = &cache {
            if let Ok(metadata) = std::fs::symlink_metadata(path) {
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= BENCHMARK_MAX_BYTES as u64
                {
                    if let Ok(bytes) = std::fs::read(path) {
                        if let Ok(saved) = serde_json::from_slice::<Cache>(&bytes) {
                            if saved.cpu_identity == identity
                                && saved.report.build_id == build_id
                                && saved.report.release_build == report.release_build
                                && saved.report.validate().is_ok()
                                && saved.report.cpu.threads == threads
                                && saved.report.cpu.fresh(Utc::now())
                            {
                                report.cpu = saved.report.cpu;
                            }
                        }
                    }
                }
            }
        }
        Ok(Arc::new(Self {
            report: Mutex::new(report),
            cache,
            cpu_identity: identity,
            threads: AtomicU32::new(threads),
            busy: AtomicBool::new(false),
            admission: Arc::new(RwLock::new(())),
        }))
    }

    pub fn snapshot(&self) -> HardwareBenchmarkReport {
        let mut report = self
            .report
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        expire(&mut report.cpu);
        for gpu in &mut report.gpus {
            expire(&mut gpu.measurement);
        }
        if report.gpu_state == BenchmarkState::Ready
            && report
                .gpus
                .iter()
                .any(|gpu| gpu.measurement.state == BenchmarkState::Stale)
        {
            report.gpu_state = BenchmarkState::Stale;
        }
        report
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    pub fn threads(&self) -> u32 {
        self.threads.load(Ordering::Acquire)
    }

    pub fn update_budget(self: &Arc<Self>, budget: u32) -> Result<()> {
        if !(1..=BENCHMARK_MAX_THREADS).contains(&budget) || budget > available_threads() {
            bail!("Nodepool CPU budget is outside local thread capacity");
        }
        let old = self.threads.swap(budget, Ordering::AcqRel);
        if old != budget {
            invalidate(
                &mut self
                    .report
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .cpu,
                BenchmarkState::Stale,
            );
            self.start("cpu")?;
        }
        Ok(())
    }

    pub fn start(self: &Arc<Self>, selector: &str) -> Result<bool> {
        if !matches!(selector, "auto" | "all" | "cpu" | "gpu") {
            bail!("invalid benchmark component");
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(false);
        }
        let cpu =
            selector != "gpu" && (selector != "auto" || !self.snapshot().cpu.fresh(Utc::now()));
        let gpu = selector != "cpu";
        {
            let mut report = self
                .report
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if cpu {
                invalidate(&mut report.cpu, BenchmarkState::Queued);
            }
            if gpu {
                report.gpu_state = BenchmarkState::Queued;
                report.gpu_reason = None;
                report.gpus.clear();
            }
        }
        let coordinator = self.clone();
        tokio::spawn(async move {
            let _busy = BusyGuard(coordinator.clone());
            let _admission = coordinator.admission.write().await;
            // Keep the admission guard until every helper has been terminated
            // and reaped; cancelling only the async wait would release it early.
            coordinator.run(cpu, gpu).await;
            coordinator.persist_cache();
        });
        Ok(true)
    }

    async fn run(&self, cpu_run: bool, gpu_run: bool) {
        if cpu_run {
            self.report
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .cpu
                .state = BenchmarkState::Running;
            let budget = self.threads();
            let result: Result<BenchmarkMeasurement> =
                helper(vec!["cpu".into(), budget.to_string()], 30).await;
            let mut report = self
                .report
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match result {
                // Compare against the budget captured at spawn, not the current
                // atomic: an in-flight run was launched under `budget`, so a
                // valid measurement taken under it must not be rejected just
                // because a budget update landed while it was running. The
                // helper clamps to available parallelism at spawn, so a
                // measurement still short of `budget` (a real clamp) fails here.
                Ok(value)
                    if value.threads == budget
                        && value.score(false).is_ok()
                        && value.state == BenchmarkState::Ready =>
                {
                    report.cpu = value
                }
                Ok(_) => fail(
                    &mut report.cpu,
                    "CPU benchmark report failed validation".into(),
                    BenchmarkState::Failed,
                ),
                Err(error) => fail(&mut report.cpu, error.to_string(), failure_state(&error)),
            }
        }
        if !gpu_run {
            return;
        }
        self.report
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .gpu_state = BenchmarkState::Running;
        let devices: Result<Vec<BenchmarkGpuIdentity>> = helper(vec!["inventory".into()], 5).await;
        let devices = match devices {
            Ok(devices)
                if devices.len() <= 4 && devices.iter().all(|device| device.validate().is_ok()) =>
            {
                devices
            }
            Ok(_) => {
                self.gpu_failure(
                    "GPU inventory failed validation".into(),
                    BenchmarkState::Failed,
                );
                return;
            }
            Err(error) => {
                self.gpu_failure(error.to_string(), failure_state(&error));
                return;
            }
        };
        if devices.is_empty() {
            self.gpu_failure(
                "no supported hardware GPU adapter".into(),
                BenchmarkState::NoDevice,
            );
            return;
        }
        for device in devices {
            let encoded = match serde_json::to_string(&device) {
                Ok(value) => value,
                Err(error) => {
                    self.gpu_failure(error.to_string(), BenchmarkState::Failed);
                    return;
                }
            };
            let result: Result<GpuBenchmark> = helper(vec!["gpu".into(), encoded], 20).await;
            let measured = match result {
                Ok(value)
                    if value.device == device
                        && value.measurement.score(true).is_ok()
                        && value.measurement.state == BenchmarkState::Ready =>
                {
                    value
                }
                Ok(_) => GpuBenchmark {
                    device,
                    measurement: failure(
                        "GPU report failed validation".into(),
                        BenchmarkState::Failed,
                    ),
                },
                Err(error) => GpuBenchmark {
                    device,
                    measurement: failure(error.to_string(), failure_state(&error)),
                },
            };
            self.report
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .gpus
                .push(measured);
        }
        let mut report = self
            .report
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        report.gpu_state = if report
            .gpus
            .iter()
            .all(|gpu| gpu.measurement.state == BenchmarkState::Ready)
        {
            BenchmarkState::Ready
        } else {
            BenchmarkState::Failed
        };
    }

    fn gpu_failure(&self, reason: String, state: BenchmarkState) {
        let mut report = self
            .report
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        report.gpu_state = state;
        report.gpu_reason = Some(bounded_reason(reason));
    }

    fn persist_cache(&self) {
        if let Err(error) = self.write_cache() {
            tracing::warn!(error = %error, "hardware benchmark cache unavailable");
        }
    }

    fn write_cache(&self) -> Result<()> {
        let Some(path) = &self.cache else {
            return Ok(());
        };
        let report = self.snapshot();
        report.validate().map_err(anyhow::Error::msg)?;
        // GPU results are re-enumerated and measured after restart. wgpu model
        // identifiers are not stable hardware UUIDs.
        let saved = Cache {
            cpu_identity: self.cpu_identity.clone(),
            report,
        };
        let bytes = serde_json::to_vec(&saved)?;
        if bytes.len() > BENCHMARK_MAX_BYTES {
            bail!("benchmark cache exceeded bound");
        }
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if let Ok(metadata) = std::fs::symlink_metadata(path) {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                let _ = std::fs::remove_file(&temporary);
                bail!("benchmark cache is not a regular file");
            }
            #[cfg(windows)]
            std::fs::remove_file(path)?;
        }
        let result = std::fs::rename(&temporary, path);
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.context("commit benchmark cache")
    }
}

struct BusyGuard(Arc<BenchmarkCoordinator>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

async fn helper<T: DeserializeOwned>(args: Vec<String>, seconds: u64) -> Result<T> {
    let signal = Arc::new(AtomicBool::new(false));
    let _cancel = supervisor::CancellationGuard(signal.clone());
    let bytes = tokio::task::spawn_blocking(move || {
        supervisor::run(&args, Duration::from_secs(seconds), &signal)
    })
    .await??;
    serde_json::from_slice(&bytes).context("invalid benchmark helper JSON")
}

pub fn available_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get().min(BENCHMARK_MAX_THREADS as usize) as u32)
        .unwrap_or(1)
}

fn invalidate(value: &mut BenchmarkMeasurement, state: BenchmarkState) {
    value.state = state;
    value.cases.clear();
    value.correctness_passed = false;
    value.reason = None;
}
fn expire(value: &mut BenchmarkMeasurement) {
    if value.state == BenchmarkState::Ready && !value.fresh(Utc::now()) {
        invalidate(value, BenchmarkState::Stale);
    }
}
fn bounded_reason(mut text: String) -> String {
    if text.len() > 255 {
        let mut end = 255;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}
fn failure_state(error: &anyhow::Error) -> BenchmarkState {
    if error.to_string().contains("timed out") {
        BenchmarkState::Timedout
    } else {
        BenchmarkState::Failed
    }
}
fn failure(reason: String, state: BenchmarkState) -> BenchmarkMeasurement {
    BenchmarkMeasurement {
        state,
        reason: Some(bounded_reason(reason)),
        ..Default::default()
    }
}
fn fail(value: &mut BenchmarkMeasurement, reason: String, state: BenchmarkState) {
    *value = failure(reason, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_rejects_arbitrary_workloads_before_dispatch() {
        let args = vec![
            "worker".into(),
            HELPER_MODE.into(),
            "command".into(),
            "not-a-command".into(),
        ];
        assert!(dispatch_helper(&args).unwrap().is_err());
        assert!(dispatch_helper(&["worker".into(), "normal-service".into()]).is_none());
        assert!(dispatch_helper(&[
            "worker".into(),
            HELPER_MODE.into(),
            "cpu".into(),
            "33".into()
        ])
        .unwrap()
        .is_err());
    }
    #[test]
    fn failure_does_not_invent_measurements() {
        let failed = failure("missing driver".into(), BenchmarkState::Unsupported);
        assert_eq!(failed.score(true), Ok(None));
        assert!(failed.cases.is_empty());
        assert_eq!(bounded_reason("界".repeat(100)).len(), 255);
    }
}

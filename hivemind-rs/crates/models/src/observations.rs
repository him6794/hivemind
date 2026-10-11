use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const HARDWARE_SUITE: &str = "hivemind-hardware-v1";
pub const OBSERVATION_VERSION: u32 = 1;
pub const BENCHMARK_MAX_BYTES: usize = 64 * 1024;
pub const BENCHMARK_TTL_SECONDS: i64 = 24 * 60 * 60;
pub const RESOURCE_TTL_SECONDS: i64 = 30;
pub const BENCHMARK_MAX_THREADS: u32 = 32;
pub const BENCHMARK_BATCHES: usize = 5;
/// Maximum serialized size accepted for a resource or execution observation.
/// Matches the observation envelope bound on the wire.
pub const RESOURCE_OBSERVATION_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkState {
    #[default]
    NotRun,
    Queued,
    Running,
    Ready,
    Unsupported,
    NoDevice,
    Failed,
    Timedout,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkBatch {
    pub repetitions: u32,
    pub elapsed_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkCase {
    pub name: String,
    pub dimension: u32,
    pub threads: u32,
    pub batches: Vec<BenchmarkBatch>,
}

impl BenchmarkCase {
    pub fn median_gflops(&self) -> Result<f64, &'static str> {
        if self.batches.len() != BENCHMARK_BATCHES
            || !(1..=BENCHMARK_MAX_THREADS).contains(&self.threads)
        {
            return Err("invalid benchmark sample count or thread budget");
        }
        let mut rates = Vec::with_capacity(BENCHMARK_BATCHES);
        let mut repetitions = None;
        for batch in &self.batches {
            if batch.repetitions == 0
                || batch.repetitions > 128
                || !batch.repetitions.is_power_of_two()
                || batch.elapsed_ns == 0
                || batch.elapsed_ns > 30_000_000_000
                || repetitions.is_some_and(|value| value != batch.repetitions)
            {
                return Err("invalid benchmark repetitions or elapsed time");
            }
            repetitions = Some(batch.repetitions);
            let operations = 2.0
                * f64::from(self.dimension).powi(3)
                * f64::from(batch.repetitions)
                * f64::from(self.threads);
            let rate = operations / batch.elapsed_ns as f64;
            if !rate.is_finite() || rate <= 0.0 {
                return Err("invalid benchmark throughput");
            }
            rates.push(rate);
        }
        rates.sort_by(f64::total_cmp);
        Ok(rates[BENCHMARK_BATCHES / 2])
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkMeasurement {
    pub measurement_id: Option<Uuid>,
    pub state: BenchmarkState,
    pub measured_at: Option<DateTime<Utc>>,
    pub identity: String,
    pub threads: u32,
    pub correctness_passed: bool,
    pub cases: Vec<BenchmarkCase>,
    pub reason: Option<String>,
}

impl BenchmarkMeasurement {
    pub fn fresh(&self, now: DateTime<Utc>) -> bool {
        self.state == BenchmarkState::Ready
            && self.measured_at.is_some_and(|time| {
                time <= now && now.signed_duration_since(time).num_seconds() < BENCHMARK_TTL_SECONDS
            })
    }

    pub fn score(&self, gpu: bool) -> Result<Option<i32>, &'static str> {
        if self.identity.len() > 1024 || self.reason.as_ref().is_some_and(|value| value.len() > 255)
        {
            return Err("benchmark identity or reason too long");
        }
        if self.state != BenchmarkState::Ready {
            if !self.cases.is_empty() || self.correctness_passed {
                return Err("unready benchmark contains measurements");
            }
            return Ok(None);
        }
        if self.measurement_id.is_none()
            || self.measured_at.is_none()
            || self.identity.trim().is_empty()
            || !self.correctness_passed
            || !(1..=BENCHMARK_MAX_THREADS).contains(&self.threads)
            || self.reason.is_some()
        {
            return Err("incomplete ready benchmark");
        }
        let expected: Vec<(&str, u32, u32)> = if gpu {
            if self.threads != 1 {
                return Err("invalid GPU workload multiplier");
            }
            vec![("gpu-f32-512", 512, 1), ("gpu-f32-1024", 1024, 1)]
        } else {
            vec![
                ("cpu-f32-single", 128, 1),
                ("cpu-f64-single", 128, 1),
                ("cpu-f32-allocated", 128, self.threads),
                ("cpu-f64-allocated", 128, self.threads),
            ]
        };
        if self.cases.len() != expected.len() {
            return Err("invalid benchmark cases");
        }
        let mut rates = Vec::with_capacity(expected.len());
        for (case, (name, dimension, threads)) in self.cases.iter().zip(expected) {
            if case.name != name || case.dimension != dimension || case.threads != threads {
                return Err("benchmark workload does not match suite");
            }
            rates.push(case.median_gflops()?);
        }
        let selected = if gpu { &rates[..] } else { &rates[2..] };
        let score = (100.0 * selected.len() as f64
            / selected.iter().map(|rate| 1.0 / rate).sum::<f64>())
        .floor();
        if !score.is_finite() || score < 0.0 || score > f64::from(i32::MAX) {
            return Err("benchmark score out of range");
        }
        Ok(Some(score as i32))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkGpuIdentity {
    pub backend: String,
    pub vendor: u32,
    pub device: u32,
    pub name: String,
    pub driver: String,
    pub driver_info: String,
    pub device_type: String,
    pub pci_bus_id: Option<String>,
}

impl BenchmarkGpuIdentity {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !matches!(self.backend.as_str(), "dx12" | "vulkan")
            || !matches!(self.device_type.as_str(), "discrete-gpu" | "integrated-gpu")
            || self.name.trim().is_empty()
            || [&self.name, &self.driver, &self.driver_info]
                .iter()
                .any(|text| text.len() > 255)
            || self
                .pci_bus_id
                .as_ref()
                .is_some_and(|text| text.is_empty() || text.len() > 64)
        {
            return Err("unsupported GPU identity");
        }
        let text =
            format!("{} {} {}", self.name, self.driver, self.driver_info).to_ascii_lowercase();
        if [
            "llvmpipe",
            "lavapipe",
            "swiftshader",
            "microsoft basic render",
            "warp",
        ]
        .iter()
        .any(|software| text.contains(software))
        {
            return Err("software GPU adapters are not accepted");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuBenchmark {
    pub device: BenchmarkGpuIdentity,
    pub measurement: BenchmarkMeasurement,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareBenchmarkReport {
    pub version: u32,
    pub suite: String,
    pub build_id: String,
    pub release_build: bool,
    pub cpu: BenchmarkMeasurement,
    pub gpu_state: BenchmarkState,
    pub gpu_reason: Option<String>,
    pub gpus: Vec<GpuBenchmark>,
}

impl HardwareBenchmarkReport {
    pub fn empty(build_id: String, release_build: bool) -> Self {
        Self {
            version: OBSERVATION_VERSION,
            suite: HARDWARE_SUITE.into(),
            build_id,
            release_build,
            cpu: BenchmarkMeasurement::default(),
            gpu_state: BenchmarkState::NotRun,
            gpu_reason: None,
            gpus: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != OBSERVATION_VERSION
            || self.suite != HARDWARE_SUITE
            || self.build_id.len() != 64
            || !self.build_id.bytes().all(|value| value.is_ascii_hexdigit())
            || self.gpus.len() > 4
            || self
                .gpu_reason
                .as_ref()
                .is_some_and(|value| value.len() > 255)
        {
            return Err("invalid benchmark report version or bounds");
        }
        self.cpu.score(false)?;
        if self.gpu_state == BenchmarkState::NoDevice && !self.gpus.is_empty() {
            return Err("no-device report contains GPUs");
        }
        if self.gpu_state == BenchmarkState::Ready && self.gpus.is_empty() {
            return Err("ready GPU report has no devices");
        }
        for (index, gpu) in self.gpus.iter().enumerate() {
            gpu.device.validate()?;
            gpu.measurement.score(true)?;
            if self.gpus[..index]
                .iter()
                .any(|previous| previous.device == gpu.device)
            {
                return Err("ambiguous duplicate GPU identity");
            }
            if self.gpu_state == BenchmarkState::Ready
                && gpu.measurement.state != BenchmarkState::Ready
            {
                return Err("ready GPU report contains unready measurement");
            }
        }
        Ok(())
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > BENCHMARK_MAX_BYTES {
            return Err("benchmark report too large");
        }
        let report: Self = serde_json::from_slice(bytes).map_err(|_| "invalid benchmark JSON")?;
        report.validate()?;
        Ok(report)
    }

    pub fn cpu_score(&self, threads: u32, now: DateTime<Utc>) -> Option<i32> {
        if !self.release_build || self.cpu.threads != threads || !self.cpu.fresh(now) {
            return None;
        }
        self.cpu.score(false).ok().flatten()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceObservation {
    pub version: u32,
    pub sampled_at: Option<DateTime<Utc>>,
    pub cpu_percent: Option<f64>,
    pub memory_percent: Option<f64>,
    pub gpu_percent: Option<f64>,
    pub vram_percent: Option<f64>,
    pub storage_percent: Option<f64>,
    pub cpu_cores: Option<i32>,
    pub total_memory_mb: Option<i64>,
    pub available_memory_mb: Option<i64>,
    pub gpu_count: Option<i32>,
    pub storage_total_gb: Option<i64>,
    pub storage_available_gb: Option<i64>,
    pub queue_capacity: Option<i32>,
}

impl ResourceObservation {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != OBSERVATION_VERSION || self.sampled_at.is_none() {
            return Err("invalid resource observation version or timestamp");
        }
        for value in [
            self.cpu_percent,
            self.memory_percent,
            self.gpu_percent,
            self.vram_percent,
            self.storage_percent,
        ]
        .into_iter()
        .flatten()
        {
            if !value.is_finite() || !(0.0..=100.0).contains(&value) {
                return Err("invalid resource percentage");
            }
        }
        if [self.cpu_cores, self.gpu_count, self.queue_capacity]
            .into_iter()
            .flatten()
            .any(|value| value < 0)
            || [
                self.total_memory_mb,
                self.available_memory_mb,
                self.storage_total_gb,
                self.storage_available_gb,
            ]
            .into_iter()
            .flatten()
            .any(|value| value < 0)
            || matches!((self.available_memory_mb, self.total_memory_mb), (Some(available), Some(total)) if available > total)
            || matches!((self.storage_available_gb, self.storage_total_gb), (Some(available), Some(total)) if available > total)
        {
            return Err("invalid resource capacity");
        }
        Ok(())
    }

    pub fn fresh(&self, now: DateTime<Utc>) -> bool {
        self.sampled_at.is_some_and(|time| {
            time <= now && now.signed_duration_since(time).num_seconds() <= RESOURCE_TTL_SECONDS
        })
    }

    /// Parse and validate a resource observation from a canonical JSON payload.
    /// Mirrors the benchmark report's `parse`: a size bound, well-formed JSON,
    /// and the full field checks. Nodepool stores the report only after this
    /// passes, so a malformed observation is never persisted.
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > RESOURCE_OBSERVATION_MAX_BYTES {
            return Err("resource observation too large");
        }
        let observation: Self =
            serde_json::from_slice(bytes).map_err(|_| "invalid resource observation JSON")?;
        observation.validate()?;
        Ok(observation)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionObservation {
    pub version: u32,
    pub cpu_time_ms: Option<i64>,
    pub wall_time_ms: Option<i64>,
    pub peak_memory_mb: Option<i64>,
    pub download_bytes: Option<i64>,
    pub cache_hits: Option<i64>,
    pub cpu_percent: Option<f64>,
    pub memory_percent: Option<f64>,
    pub gpu_percent: Option<f64>,
    pub vram_percent: Option<f64>,
}

impl ExecutionObservation {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != OBSERVATION_VERSION
            || [
                self.cpu_time_ms,
                self.wall_time_ms,
                self.peak_memory_mb,
                self.download_bytes,
                self.cache_hits,
            ]
            .into_iter()
            .flatten()
            .any(|value| value < 0)
            || [
                self.cpu_percent,
                self.memory_percent,
                self.gpu_percent,
                self.vram_percent,
            ]
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite() || !(0.0..=100.0).contains(&value))
        {
            return Err("invalid execution observation");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu() -> BenchmarkMeasurement {
        BenchmarkMeasurement {
            measurement_id: Some(Uuid::new_v4()),
            state: BenchmarkState::Ready,
            measured_at: Some(Utc::now()),
            identity: "test-only CPU".into(),
            threads: 2,
            correctness_passed: true,
            reason: None,
            cases: [
                ("cpu-f32-single", 1),
                ("cpu-f64-single", 1),
                ("cpu-f32-allocated", 2),
                ("cpu-f64-allocated", 2),
            ]
            .into_iter()
            .map(|(name, threads)| BenchmarkCase {
                name: name.into(),
                dimension: 128,
                threads,
                batches: vec![
                    BenchmarkBatch {
                        repetitions: 1,
                        elapsed_ns: 4_194_304
                    };
                    BENCHMARK_BATCHES
                ],
            })
            .collect(),
        }
    }

    #[test]
    fn scores_are_recomputed_from_completed_work_and_time() {
        assert_eq!(cpu().score(false), Ok(Some(200)));
        let mut bad = cpu();
        bad.cases[0].batches[0].elapsed_ns = 0;
        assert!(bad.score(false).is_err());
        bad = cpu();
        bad.cases[0].dimension = 129;
        assert!(bad.score(false).is_err());
        bad = cpu();
        bad.correctness_passed = false;
        assert!(bad.score(false).is_err());
    }

    #[test]
    fn missing_and_explicit_zero_remain_distinct() {
        let missing = ExecutionObservation {
            version: OBSERVATION_VERSION,
            ..Default::default()
        };
        let zero = ExecutionObservation {
            wall_time_ms: Some(0),
            ..missing.clone()
        };
        assert!(missing.validate().is_ok());
        assert!(zero.validate().is_ok());
        assert_ne!(
            serde_json::to_value(&missing).unwrap(),
            serde_json::to_value(&zero).unwrap()
        );
        let roundtrip: ExecutionObservation =
            serde_json::from_slice(&serde_json::to_vec(&zero).unwrap()).unwrap();
        assert_eq!(roundtrip.wall_time_ms, Some(0));
        assert_eq!(roundtrip.cpu_time_ms, None);
    }

    #[test]
    fn cache_requires_matching_budget_release_and_freshness() {
        let mut report = HardwareBenchmarkReport::empty("a".repeat(64), true);
        report.cpu = cpu();
        let now = Utc::now();
        assert_eq!(report.cpu_score(2, now), Some(200));
        assert_eq!(report.cpu_score(1, now), None);
        report.release_build = false;
        assert_eq!(report.cpu_score(2, now), None);
        report.release_build = true;
        report.cpu.measured_at = Some(now - chrono::Duration::seconds(BENCHMARK_TTL_SECONDS));
        assert_eq!(report.cpu_score(2, now), None);
    }

    #[test]
    fn unsupported_is_not_a_successful_zero_score() {
        assert_eq!(BenchmarkMeasurement::default().score(false), Ok(None));
        let mut result = cpu();
        result.state = BenchmarkState::Unsupported;
        assert!(result.score(false).is_err());
        let mut report = HardwareBenchmarkReport::empty("a".repeat(64), true);
        report.suite = "unknown-suite".into();
        assert!(report.validate().is_err());
        assert!(HardwareBenchmarkReport::parse(&vec![0; BENCHMARK_MAX_BYTES + 1]).is_err());
    }
}

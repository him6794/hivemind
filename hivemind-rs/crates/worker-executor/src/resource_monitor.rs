use super::{GpuInfo, SystemResources};
use hivemind_models::{ResourceSpec, ResourceUsage};
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Run a bounded inventory probe with a hard deadline. Resource collection
/// happens on the heartbeat path, so a hung `nvidia-smi` or `powershell`
/// subprocess must be killed rather than left to block the worker. Reader
/// threads drain the pipes so a chatty probe cannot deadlock on a full
/// buffer; the caller's deadline bounds the whole call. Returns `None` if the
/// process could not start or the deadline passed.
fn run_bounded(command: &mut Command, deadline: Duration) -> Option<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let stdout_reader = child
        .stdout
        .take()
        .map(|mut pipe| std::thread::spawn(move || read_pipe_to_end(&mut pipe)));
    let stderr_reader = child
        .stderr
        .take()
        .map(|mut pipe| std::thread::spawn(move || read_pipe_to_end(&mut pipe)));
    let started = Instant::now();
    loop {
        if child.try_wait().ok().flatten().is_some() {
            let stdout = stdout_reader
                .and_then(|t| t.join().ok())
                .unwrap_or_default();
            let stderr = stderr_reader
                .and_then(|t| t.join().ok())
                .unwrap_or_default();
            return Some(Output {
                status: child.wait().ok()?,
                stdout,
                stderr,
            });
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            drop(stdout_reader);
            drop(stderr_reader);
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn read_pipe_to_end(pipe: &mut dyn Read) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = pipe.read_to_end(&mut buffer);
    buffer
}

struct SystemProbe {
    system: sysinfo::System,
    last_cpu_refresh: Option<Instant>,
}

pub fn collect_resources() -> SystemResources {
    static SYSTEM_PROBE: OnceLock<Mutex<SystemProbe>> = OnceLock::new();
    let probe = SYSTEM_PROBE.get_or_init(|| {
        Mutex::new(SystemProbe {
            system: sysinfo::System::new_all(),
            last_cpu_refresh: None,
        })
    });
    let mut probe = probe.lock().unwrap_or_else(|error| error.into_inner());
    let now = Instant::now();
    let cpu_usage_supported = probe
        .last_cpu_refresh
        .is_some_and(|last| now.duration_since(last) >= Duration::from_millis(200));
    probe.system.refresh_all();
    probe.last_cpu_refresh = Some(now);

    let cpu_cores = probe.system.cpus().len() as i32;
    let total_memory_bytes = probe.system.total_memory();
    let available_memory_bytes = probe.system.available_memory();
    let total_memory_gb = (total_memory_bytes / (1024 * 1024 * 1024)) as i32;
    let available_memory_gb = (available_memory_bytes / (1024 * 1024 * 1024)) as i32;
    let memory_supported = total_memory_bytes > 0 && available_memory_bytes <= total_memory_bytes;

    let cpu_usage = probe
        .system
        .cpus()
        .iter()
        .map(|cpu| cpu.cpu_usage() as f64)
        .sum::<f64>()
        / cpu_cores.max(1) as f64;
    let cpu_usage_supported = cpu_usage_supported
        && cpu_cores > 0
        && cpu_usage.is_finite()
        && (0.0..=100.0).contains(&cpu_usage);

    let memory_usage_percent = if memory_supported {
        ((total_memory_bytes - available_memory_bytes) as f64 / total_memory_bytes as f64) * 100.0
    } else {
        0.0
    };
    drop(probe);

    let (
        gpu_infos,
        gpu_inventory_supported,
        vram_total_supported,
        gpu_utilization_supported,
        vram_available_supported,
    ) = detect_gpus();
    let gpu_count = gpu_infos.len() as i32;
    let (storage_total_gb, storage_available_gb, storage_supported) = detect_storage();

    SystemResources {
        cpu_cores,
        total_memory_gb,
        available_memory_gb,
        cpu_usage_percent: cpu_usage,
        cpu_usage_supported,
        memory_usage_percent,
        memory_supported,
        gpu_count,
        gpu_infos,
        gpu_inventory_supported,
        gpu_utilization_supported,
        vram_total_supported,
        vram_available_supported,
        storage_supported,
        storage_total_gb,
        storage_available_gb,
    }
}

/// Derive a ResourceSpec from collected system resources
pub fn to_resource_spec(resources: &SystemResources) -> ResourceSpec {
    let gpu_name = resources
        .gpu_infos
        .first()
        .map(|g| g.name.clone())
        .unwrap_or_default();
    let gpu_count = resources.gpu_count;
    let vram_mb = resources
        .gpu_infos
        .first()
        .map(|g| g.vram_total_mb)
        .unwrap_or(0);

    // This profile carries identity and capacity. The measured performance
    // score is reported separately in the presence-aware benchmark observation
    // (absent when unmeasured), not fabricated here from core count and idle
    // time, so the profile scores read as "not scored in this profile".
    ResourceSpec {
        cpu_cores: resources.cpu_cores,
        memory_mb: resources.total_memory_gb as i64 * 1024,
        gpu_count,
        gpu_name,
        vram_mb,
        cpu_score: 0,
        gpu_score: 0,
        storage_total_gb: resources.storage_total_gb,
        storage_available_gb: resources.storage_available_gb,
    }
}

/// Derive a ResourceUsage from collected system resources
pub fn to_resource_usage(resources: &SystemResources) -> ResourceUsage {
    let gpu_util = resources
        .gpu_infos
        .iter()
        .map(|g| g.gpu_utilization_percent)
        .fold(0.0_f64, |a, b| a.max(b));

    let vram_percent = resources
        .gpu_infos
        .first()
        .map(|g| {
            if g.vram_total_mb > 0 {
                (g.vram_used_mb as f64 / g.vram_total_mb as f64) * 100.0
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    let storage_percent = if resources.storage_total_gb > 0 {
        ((resources.storage_total_gb - resources.storage_available_gb) as f64
            / resources.storage_total_gb as f64)
            * 100.0
    } else {
        0.0
    };

    ResourceUsage {
        cpu_percent: resources.cpu_usage_percent,
        memory_percent: resources.memory_usage_percent,
        gpu_percent: gpu_util,
        vram_percent,
        storage_percent,
    }
}

/// Detect NVIDIA GPUs via nvidia-smi
fn detect_gpus() -> (Vec<GpuInfo>, bool, bool, bool, bool) {
    let output = run_bounded(
        Command::new("nvidia-smi").args([
            "--query-gpu=index,name,memory.total,memory.used,memory.free,utilization.gpu",
            "--format=csv,noheader,nounits",
        ]),
        Duration::from_secs(5),
    );

    match output {
        Some(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            parse_nvidia_smi_output(&stdout)
        }
        _ => {
            #[cfg(windows)]
            {
                detect_gpus_windows()
            }
            #[cfg(not(windows))]
            {
                (Vec::new(), false, false, false, false)
            }
        }
    }
}

fn parse_nvidia_smi_output(stdout: &str) -> (Vec<GpuInfo>, bool, bool, bool, bool) {
    let mut infos = Vec::new();
    let mut inventory_supported = true;
    let mut vram_total_supported = true;
    let mut gpu_utilization_supported = true;
    let mut vram_available_supported = true;

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let parts: Vec<&str> = line.split(", ").map(str::trim).collect();
        let Some(index) = parts.first().and_then(|value| value.parse::<i32>().ok()) else {
            inventory_supported = false;
            vram_total_supported = false;
            gpu_utilization_supported = false;
            vram_available_supported = false;
            continue;
        };
        if parts.len() < 6 || parts[1].is_empty() {
            inventory_supported = false;
            vram_total_supported = false;
            gpu_utilization_supported = false;
            vram_available_supported = false;
            continue;
        }

        let vram_total = parts[2].parse::<i64>().ok().filter(|value| *value >= 0);
        let vram_used = parts[3].parse::<i64>().ok().filter(|value| *value >= 0);
        let vram_available = parts[4].parse::<i64>().ok().filter(|value| *value >= 0);
        let gpu_utilization = parts[5]
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && (0.0..=100.0).contains(value));

        vram_total_supported &= vram_total.is_some();
        vram_available_supported &= vram_available.is_some();
        gpu_utilization_supported &= gpu_utilization.is_some();

        infos.push(GpuInfo {
            index,
            name: parts[1].to_string(),
            vram_total_mb: vram_total.unwrap_or(0),
            vram_used_mb: vram_used.unwrap_or(0),
            vram_available_mb: vram_available.unwrap_or(0),
            gpu_utilization_percent: gpu_utilization.unwrap_or(0.0),
        });
    }

    (
        infos,
        inventory_supported,
        vram_total_supported,
        gpu_utilization_supported,
        vram_available_supported,
    )
}

#[cfg(windows)]
fn detect_gpus_windows() -> (Vec<GpuInfo>, bool, bool, bool, bool) {
    // PowerShell query for GPU info via WMI
    let output = run_bounded(
        Command::new("powershell").args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance -ClassName Win32_VideoController | Where-Object { $_.AdapterRAM -gt 0 } | Select-Object Name, AdapterRAM, DriverVersion | ConvertTo-Csv -NoTypeInformation",
        ]),
        Duration::from_secs(5),
    );

    match output {
        Some(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let infos: Vec<GpuInfo> = stdout
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let line = line.trim_matches('"');
                    let parts: Vec<&str> = line.split("\",\"").collect();
                    if parts.len() >= 2 {
                        let vram_total = parts
                            .get(1)
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);
                        Some(GpuInfo {
                            index: 0,
                            name: parts.first().unwrap_or(&"Unknown GPU").to_string(),
                            vram_total_mb: vram_total / (1024 * 1024),
                            vram_used_mb: 0,
                            vram_available_mb: vram_total / (1024 * 1024),
                            gpu_utilization_percent: 0.0,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            // WMI only identifies adapters and their advertised capacity; it
            // does not report live utilization or available VRAM.
            let vram_total_supported = infos.iter().all(|gpu| gpu.vram_total_mb > 0);
            (infos, true, vram_total_supported, false, false)
        }
        _ => {
            tracing::info!("GPU inventory is unavailable via WMI");
            (Vec::new(), false, false, false, false)
        }
    }
}

/// Detect storage space using sysinfo
fn detect_storage() -> (i64, i64, bool) {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let total: u64 = disks.iter().map(|d| d.total_space()).sum();
    let available: u64 = disks.iter().map(|d| d.available_space()).sum();
    (
        (total / (1024 * 1024 * 1024)) as i64,
        (available / (1024 * 1024 * 1024)) as i64,
        !disks.is_empty() && total > 0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resources_are_positive() {
        let r = collect_resources();
        assert!(r.cpu_cores > 0, "CPU cores must be positive");
        assert!(r.total_memory_gb > 0, "Total memory must be positive");
        assert!(r.cpu_usage_percent >= 0.0, "CPU usage must be non-negative");
        assert!(r.storage_total_gb > 0, "Storage total must be positive");
    }

    #[test]
    fn test_resource_spec_conversion() {
        let r = collect_resources();
        let spec = to_resource_spec(&r);
        assert_eq!(spec.cpu_cores, r.cpu_cores);
        assert!(spec.memory_mb > 0);
        assert!(spec.storage_total_gb > 0);
    }

    #[test]
    fn capacity_profile_does_not_fabricate_a_performance_score() {
        // The capacity profile reports identity and capacity only. A measured
        // score is carried in the separate presence-aware benchmark observation,
        // so this profile must not synthesize one from cores or VRAM idle time.
        let r = collect_resources();
        let spec = to_resource_spec(&r);
        assert_eq!(spec.cpu_score, 0);
        assert_eq!(spec.gpu_score, 0);
        // Capacity fields are still reported independently of the scores.
        assert_eq!(spec.cpu_cores, r.cpu_cores);
        assert_eq!(spec.gpu_count, r.gpu_count);
        assert!(spec.vram_mb >= 0);
    }

    #[test]
    fn test_resource_usage_conversion() {
        let r = collect_resources();
        let usage = to_resource_usage(&r);
        assert!(usage.cpu_percent >= 0.0);
        assert!(usage.memory_percent >= 0.0);
        assert!(usage.storage_percent >= 0.0);
    }

    #[test]
    fn test_gpu_detection_does_not_panic() {
        let (
            gpus,
            _inventory_supported,
            _vram_total_supported,
            _utilization_supported,
            _vram_available_supported,
        ) = detect_gpus();
        for gpu in &gpus {
            assert!(!gpu.name.is_empty(), "GPU name must not be empty");
            assert!(gpu.vram_total_mb >= 0, "VRAM must be non-negative");
        }
    }

    #[test]
    fn nvidia_smi_keeps_unavailable_measurements_unknown() {
        let (gpus, inventory, vram_total, utilization, vram_available) = parse_nvidia_smi_output(
            "0, NVIDIA Example, N/A, N/A, N/A, N/A\n1, NVIDIA Other, 8192, 1024, 7168, 25\n",
        );

        assert_eq!(gpus.len(), 2);
        assert!(inventory);
        assert!(!vram_total);
        assert!(!utilization);
        assert!(!vram_available);
        assert_eq!(gpus[0].index, 0);
        assert_eq!(gpus[0].name, "NVIDIA Example");
        assert_eq!(gpus[0].vram_total_mb, 0);
        assert_eq!(gpus[0].vram_available_mb, 0);
        assert_eq!(gpus[0].gpu_utilization_percent, 0.0);
        assert_eq!(gpus[1].vram_total_mb, 8192);
        assert_eq!(gpus[1].vram_available_mb, 7168);
        assert_eq!(gpus[1].gpu_utilization_percent, 25.0);
    }

    #[test]
    fn nvidia_smi_preserves_supported_readings_in_a_valid_row() {
        let (gpus, inventory, vram_total, utilization, vram_available) =
            parse_nvidia_smi_output("0, NVIDIA Example, 8192, 2048, 6144, 25\n");

        assert_eq!(gpus.len(), 1);
        assert!(inventory);
        assert!(vram_total);
        assert!(utilization);
        assert!(vram_available);
        assert_eq!(gpus[0].vram_total_mb, 8192);
        assert_eq!(gpus[0].vram_available_mb, 6144);
        assert_eq!(gpus[0].gpu_utilization_percent, 25.0);
    }

    #[test]
    fn test_storage_detection() {
        let (total, available, _supported) = detect_storage();
        assert!(total > 0, "Storage total must be positive, got {}", total);
        assert!(available >= 0, "Storage available must be non-negative");
        assert!(available <= total, "Available cannot exceed total");
    }
}

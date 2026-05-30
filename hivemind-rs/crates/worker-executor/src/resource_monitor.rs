use hivemind_models::{ResourceSpec, ResourceUsage};
use super::{SystemResources, GpuInfo};
use std::process::Command;

pub fn collect_resources() -> SystemResources {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_cores = sys.cpus().len() as i32;
    let total_memory_kb = sys.total_memory();
    let available_memory_kb = sys.available_memory();
    let total_memory_gb = (total_memory_kb / (1024 * 1024)) as i32;
    let available_memory_gb = (available_memory_kb / (1024 * 1024)) as i32;

    let cpu_usage = sys.cpus().iter()
        .map(|cpu| cpu.cpu_usage() as f64)
        .sum::<f64>()
        / cpu_cores.max(1) as f64;

    let memory_usage_percent = if total_memory_kb > 0 {
        ((total_memory_kb - available_memory_kb) as f64 / total_memory_kb as f64) * 100.0
    } else {
        0.0
    };

    let gpu_infos = detect_gpus();
    let gpu_count = gpu_infos.len() as i32;
    let (storage_total_gb, storage_available_gb) = detect_storage();

    SystemResources {
        cpu_cores,
        total_memory_gb,
        available_memory_gb,
        cpu_usage_percent: cpu_usage,
        memory_usage_percent,
        gpu_count,
        gpu_infos,
        storage_total_gb,
        storage_available_gb,
    }
}

/// Derive a ResourceSpec from collected system resources
pub fn to_resource_spec(resources: &SystemResources) -> ResourceSpec {
    let gpu_name = resources.gpu_infos.first()
        .map(|g| g.name.clone())
        .unwrap_or_default();
    let vram_mb = resources.gpu_infos.first()
        .map(|g| g.vram_total_mb)
        .unwrap_or(0);
    let gpu_count = resources.gpu_count;

    ResourceSpec {
        cpu_cores: resources.cpu_cores,
        memory_mb: resources.total_memory_gb as i64 * 1024,
        gpu_count,
        gpu_name,
        vram_mb,
        cpu_score: resources.cpu_cores * 100,
        gpu_score: if gpu_count > 0 { (vram_mb / 1024) as i32 * 100 } else { 0 },
        storage_total_gb: resources.storage_total_gb,
        storage_available_gb: resources.storage_available_gb,
    }
}

/// Derive a ResourceUsage from collected system resources
pub fn to_resource_usage(resources: &SystemResources) -> ResourceUsage {
    let gpu_util = resources.gpu_infos.iter()
        .map(|g| g.gpu_utilization_percent)
        .fold(0.0_f64, |a, b| a.max(b));

    let vram_percent = resources.gpu_infos.first()
        .map(|g| {
            if g.vram_total_mb > 0 {
                (g.vram_used_mb as f64 / g.vram_total_mb as f64) * 100.0
            } else { 0.0 }
        })
        .unwrap_or(0.0);

    let storage_percent = if resources.storage_total_gb > 0 {
        ((resources.storage_total_gb - resources.storage_available_gb) as f64
            / resources.storage_total_gb as f64) * 100.0
    } else { 0.0 };

    ResourceUsage {
        cpu_percent: resources.cpu_usage_percent,
        memory_percent: resources.memory_usage_percent,
        gpu_percent: gpu_util,
        vram_percent,
        storage_percent,
    }
}

/// Detect NVIDIA GPUs via nvidia-smi
fn detect_gpus() -> Vec<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=index,name,memory.total,memory.used,memory.free,utilization.gpu", "--format=csv,noheader,nounits"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().filter_map(|line| {
                let parts: Vec<&str> = line.split(", ").map(|s| s.trim()).collect();
                if parts.len() >= 6 {
                    Some(GpuInfo {
                        index: parts[0].parse().unwrap_or(0),
                        name: parts[1].to_string(),
                        vram_total_mb: parts[2].parse().unwrap_or(0),
                        vram_used_mb: parts[3].parse().unwrap_or(0),
                        vram_available_mb: parts[4].parse().unwrap_or(0),
                        gpu_utilization_percent: parts[5].parse().unwrap_or(0.0),
                    })
                } else { None }
            }).collect()
        }
        _ => {
            // Try AMD/Intel GPU via lspci or dxgi as fallback (platform-dependent)
            if cfg!(windows) { detect_gpus_windows() }
            else { Vec::new() }
        }
    }
}

#[cfg(windows)]
fn detect_gpus_windows() -> Vec<GpuInfo> {
    // PowerShell query for GPU info via WMI
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command",
            "Get-CimInstance -ClassName Win32_VideoController | Where-Object { .AdapterRAM -gt 0 } | Select-Object Name, AdapterRAM, DriverVersion | ConvertTo-Csv -NoTypeInformation"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().skip(1).filter_map(|line| {
                let line = line.trim_matches('"');
                let parts: Vec<&str> = line.split("\",\"").collect();
                if parts.len() >= 2 {
                    let vram_total = parts.get(1)
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(0);
                    Some(GpuInfo {
                        index: 0,
                        name: parts.get(0).unwrap_or(&"Unknown GPU").to_string(),
                        vram_total_mb: vram_total / (1024 * 1024),
                        vram_used_mb: 0,
                        vram_available_mb: vram_total / (1024 * 1024),
                        gpu_utilization_percent: 0.0,
                    })
                } else { None }
            }).collect()
        }
        _ => {
            tracing::info!("No GPU detected via WMI");
            Vec::new()
        }
    }
}

#[cfg(not(windows))]
fn detect_gpus_linux() -> Vec<GpuInfo> {
    tracing::info!("GPU detection: nvidia-smi not found, no other backend available");
    Vec::new()
}

/// Detect storage space using sysinfo
fn detect_storage() -> (i64, i64) {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let total: u64 = disks.iter().map(|d| d.total_space()).sum();
    let available: u64 = disks.iter().map(|d| d.available_space()).sum();
    ((total / (1024 * 1024 * 1024)) as i64, (available / (1024 * 1024 * 1024)) as i64)
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
    fn test_resource_usage_conversion() {
        let r = collect_resources();
        let usage = to_resource_usage(&r);
        assert!(usage.cpu_percent >= 0.0);
        assert!(usage.memory_percent >= 0.0);
        assert!(usage.storage_percent >= 0.0);
    }

    #[test]
    fn test_gpu_detection_does_not_panic() {
        let gpus = detect_gpus();
        for gpu in &gpus {
            assert!(!gpu.name.is_empty(), "GPU name must not be empty");
            assert!(gpu.vram_total_mb >= 0, "VRAM must be non-negative");
        }
    }

    #[test]
    fn test_storage_detection() {
        let (total, available) = detect_storage();
        assert!(total > 0, "Storage total must be positive, got {}", total);
        assert!(available >= 0, "Storage available must be non-negative");
        assert!(available <= total, "Available cannot exceed total");
    }
}

use super::SystemResources;

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
        / cpu_cores as f64;

    let memory_usage_percent = if total_memory_kb > 0 {
        ((total_memory_kb - available_memory_kb) as f64 / total_memory_kb as f64) * 100.0
    } else {
        0.0
    };

    SystemResources {
        cpu_cores,
        total_memory_gb,
        available_memory_gb,
        cpu_usage_percent: cpu_usage,
        memory_usage_percent,
    }
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
    }
}
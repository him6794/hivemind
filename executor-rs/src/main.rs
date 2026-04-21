use std::env;
use std::process;
use executor::{run_task, run_task_with_monty, ExecutionConfig, MontyConfig, OutputFormat};

fn env_bool(name: &str, fallback: bool) -> bool {
    match env::var(name) {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "y" | "on" => true,
            "0" | "false" | "no" | "n" | "off" => false,
            _ => fallback,
        },
        Err(_) => fallback,
    }
}

fn env_u32(name: &str, fallback: u32) -> u32 {
    match env::var(name) {
        Ok(v) => v.trim().parse::<u32>().unwrap_or(fallback),
        Err(_) => fallback,
    }
}

fn env_output_format() -> OutputFormat {
    match env::var("EXECUTOR_OUTPUT_FORMAT") {
        Ok(v) if v.trim().eq_ignore_ascii_case("magnet") => OutputFormat::Magnet,
        _ => OutputFormat::ResultUrl,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxBackend {
    Native,
    Monty,
}

fn env_sandbox_backend() -> SandboxBackend {
    match env::var("EXECUTOR_SANDBOX_BACKEND") {
        Ok(v) if v.trim().eq_ignore_ascii_case("monty") => SandboxBackend::Monty,
        _ => SandboxBackend::Native,
    }
}

fn main() {
    let mut args = env::args().skip(1);
    let task_id = match args.next() {
        Some(v) if !v.trim().is_empty() => v,
        _ => {
            eprintln!("usage: executor-cli <task_id> <torrent_source>");
            process::exit(2);
        }
    };
    let source = match args.next() {
        Some(v) if !v.trim().is_empty() => v,
        _ => {
            eprintln!("usage: executor-cli <task_id> <torrent_source>");
            process::exit(2);
        }
    };

    if env_bool("EXECUTOR_FORCE_FAIL", false) {
        eprintln!("executor-rs failed: forced failure (EXECUTOR_FORCE_FAIL)");
        process::exit(1);
    }

    let config = ExecutionConfig {
        steps: env_u32("EXECUTOR_STEPS", 5_000),
        step_log_interval: env_u32("EXECUTOR_STEP_LOG_INTERVAL", 1_000),
        strict_source: env_bool("EXECUTOR_STRICT_SOURCE", true),
        output_format: env_output_format(),
    };

    let backend = env_sandbox_backend();
    let fallback_native = env_bool("EXECUTOR_MONTY_FALLBACK_NATIVE", true);
    let run_result = match backend {
        SandboxBackend::Native => run_task(&task_id, &source, &config, |line| println!("{line}")),
        SandboxBackend::Monty => {
            let monty = MontyConfig {
                python_cmd: env::var("EXECUTOR_MONTY_PYTHON_CMD").unwrap_or_else(|_| "python".to_string()),
            };
            match run_task_with_monty(&task_id, &source, &config, &monty, |line| println!("{line}")) {
                Ok(r) => Ok(r),
                Err(e) if fallback_native => {
                    eprintln!("executor-rs monty failed, fallback native: {e}");
                    run_task(&task_id, &source, &config, |line| println!("{line}"))
                }
                Err(e) => Err(e),
            }
        }
    };

    match run_result {
        Ok(report) => {
            println!(
                "executor-rs completed task={} source_btih={} result_btih={} digest={}",
                task_id, report.source_btih, report.result_btih, report.final_digest
            );
            println!("RESULT_TORRENT={}", report.result_torrent);
        }
        Err(e) => {
            eprintln!("executor-rs failed: {e}");
            process::exit(1);
        }
    }
}

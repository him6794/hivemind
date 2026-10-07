use super::workload::{self, BenchmarkFloat, CPU_DIMENSION};
use anyhow::{Context, Result};
use chrono::Utc;
use general_compute_runtime::numeric::DenseTensor;
use hivemind_models::observations::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// CPU thread budget used for the allocated cases: the smallest of local
/// parallelism, the caller's requested budget, and the suite cap.
fn effective_threads(requested: u32) -> u32 {
    requested.clamp(
        1,
        workload_available_parallelism().min(BENCHMARK_MAX_THREADS),
    )
}

fn workload_available_parallelism() -> u32 {
    std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(1)
}

/// Run the fixed CPU suite: single-thread and allocated FP32/FP64 matmul
/// cases, each verified against the analytical reference, timed over warm-up
/// calibration plus five measured batches.
pub fn run(requested_threads: u32) -> Result<BenchmarkMeasurement> {
    let threads = effective_threads(requested_threads);
    let identity = workload::cpu_identity();

    let cases = [
        ("cpu-f32-single", 1u32, false),
        ("cpu-f64-single", 1u32, true),
        ("cpu-f32-allocated", threads, false),
        ("cpu-f64-allocated", threads, true),
    ]
    .into_iter()
    .map(|(name, case_threads, is_f64)| {
        if is_f64 {
            run_case::<f64>(name, case_threads)
        } else {
            run_case::<f32>(name, case_threads)
        }
    })
    .collect::<Result<Vec<BenchmarkCase>>>()?;

    Ok(BenchmarkMeasurement {
        measurement_id: Some(uuid::Uuid::new_v4()),
        state: BenchmarkState::Ready,
        measured_at: Some(Utc::now()),
        identity,
        threads,
        correctness_passed: true,
        cases,
        reason: None,
    })
}

fn run_case<T: BenchmarkFloat>(name: &str, threads: u32) -> Result<BenchmarkCase> {
    let left = Arc::new(workload::matrix_inputs::<T>(CPU_DIMENSION)?.0);
    let right = Arc::new(workload::matrix_inputs::<T>(CPU_DIMENSION)?.1);
    verify::<T>(&left, &right)?;

    // Warm up once: the first batch warms caches and the allocator.
    timed_once(&left, &right, threads, 1)?;

    // Calibrate repetitions to roughly the target window, then measure five
    // batches at that fixed repetition count.
    let repetitions = calibrate(&left, &right, threads)?;
    let mut batches = Vec::with_capacity(BENCHMARK_BATCHES);
    for _ in 0..BENCHMARK_BATCHES {
        let elapsed_ns = timed_once(&left, &right, threads, repetitions)?;
        batches.push(BenchmarkBatch {
            repetitions,
            elapsed_ns,
        });
    }

    Ok(BenchmarkCase {
        name: name.to_owned(),
        dimension: CPU_DIMENSION,
        threads,
        batches,
    })
}

/// Verify the fixed inputs produce the analytical product before any timing.
fn verify<T: BenchmarkFloat>(
    left: &Arc<DenseTensor<T>>,
    right: &Arc<DenseTensor<T>>,
) -> Result<()> {
    let output = left
        .matmul(right)
        .context("fixed reference matmul failed to run")?;
    workload::validate_values::<T>(CPU_DIMENSION, output.values())
        .context("reference output mismatch")?;
    Ok(())
}

fn calibrate<T: BenchmarkFloat>(
    left: &Arc<DenseTensor<T>>,
    right: &Arc<DenseTensor<T>>,
    threads: u32,
) -> Result<u32> {
    let mut repetitions = 1u32;
    for _ in 0..8 {
        let elapsed_ns = timed_once(left, right, threads, repetitions)?;
        if elapsed_ns >= 200_000_000 {
            return Ok(repetitions);
        }
        repetitions = repetitions.saturating_mul(2).min(128);
    }
    Ok(repetitions)
}

/// Time `repetitions` independent matmuls across `threads` workers. The timer
/// spans allocation plus compute for every worker; thread and input setup are
/// excluded. The barrier releases all workers so the wall clock reflects the
/// whole participating set.
fn timed_once<T: BenchmarkFloat>(
    left: &Arc<DenseTensor<T>>,
    right: &Arc<DenseTensor<T>>,
    threads: u32,
    repetitions: u32,
) -> Result<u64> {
    let left = Arc::clone(left);
    let right = Arc::clone(right);
    let start = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::with_capacity(threads as usize);
    for _ in 0..threads {
        let left = Arc::clone(&left);
        let right = Arc::clone(&right);
        let start = Arc::clone(&start);
        handles.push(std::thread::spawn(move || {
            while !start.load(Ordering::Acquire) {}
            for _ in 0..repetitions {
                let _ = left.matmul(&right);
            }
        }));
    }
    let started = Instant::now();
    start.store(true, Ordering::Release);
    for handle in handles {
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("benchmark worker thread panicked"))?;
    }
    Ok(started.elapsed().as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_thread_suite_is_valid_and_scoring() {
        let result = run(1).expect("single-thread CPU suite completes");
        assert_eq!(result.threads, 1);
        assert_eq!(result.state, BenchmarkState::Ready);
        assert!(result.correctness_passed);
        let score = result.score(false).expect("suite scores");
        assert!(score.is_some(), "ready suite produces a positive score");
    }

    #[test]
    fn thread_budget_is_capped_by_local_parallelism() {
        let result = run(1024).expect("over-large budget is clamped");
        assert!(result.threads >= 1);
        assert!(result.threads <= BENCHMARK_MAX_THREADS);
        assert!(result.threads <= workload_available_parallelism());
    }
}

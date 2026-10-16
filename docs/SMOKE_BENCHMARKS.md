# Hivemind Smoke Benchmarks

This document defines the first practical benchmark gate recommended by
`UTILITY_PERFORMANCE_EVALUATION.md`.

## Scope

The smoke benchmark validates that a controlled worker pool can accept,
dispatch, execute, and complete active `hcode` tasks. It does not
validate public marketplace pricing, fiat conversion, or untrusted provider
operation.

V1 enforced consensus needs at least three eligible Workers by default. The
script fails before submission when a scenario has fewer registered Workers than
its target, rather than silently measuring a one-Worker path.

Default scenarios:

| Scenario | Worker target | Task count |
|---|---:|---:|
| Consensus | 3 | 10 |
| Pool | 5 | 100 |

The script records:

- `submit_latency_ms` — time spent in the submission call; missing measurements
  are `null`.
- `terminal_latency_ms` — per-task monotonic elapsed time from immediately
  before submission until an API terminal status is observed. The clock starts
  separately for each task.
- `wait_duration_ms` — elapsed wait for a task that did not reach an API
  terminal status before the local timeout. This is a censored observation, not
  terminal or completion latency.
- final task `status`, including API-reported `TIMED_OUT` separately from the
  local `TIMEOUT_WAITING_FOR_TERMINAL_STATUS` outcome.
- `redispatched`, `wall_time_ms`, and `peak_memory_mb`; unavailable measurements
  are `null`, while an explicitly measured zero remains `0`.
- observed worker count at scenario start.

A submission rejected by the API or failing with a request error is recorded as
`SUBMIT_FAILED` and is not added to the pending-task set. Its measured submit
attempt duration may be present, but failed submissions are excluded from the
submission percentile.

The summary reports nearest-rank p95 values. `p95_submit_latency_ms` includes
known submit durations only for accepted submissions. `p95_terminal_latency_ms`
includes known `terminal_latency_ms` values only for `COMPLETED` tasks; failed,
cancelled, timed-out, censored, and unknown-latency rows do not enter that
percentile. If there are no eligible measured samples, the percentile is `null`.
The aggregate `redispatched` value is also `null` if every row's count is
unknown.

## Command

Run from the repository root after the master/nodepool/worker services are
running and after obtaining a requestor token:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\hivemind-smoke-benchmark.ps1 `
  -MasterUrl http://127.0.0.1:8082 `
  -Token "<requestor-token>" `
  -TaskSourcePath .\templates\hcode\03_batch_sum.hmf `
  -TaskInputPath .\templates\hcode\03_batch_sum.input.json
```

Outputs are written to `test_logs/smoke-benchmark/` as CSV and JSON.

The deterministic offline checks can be run without a Master, Workers, or a
PowerShell test framework:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\hivemind-smoke-benchmark.Tests.ps1
```

## Pass criteria

For a release candidate, record the exact commit and require:

- all submitted tasks reach an API terminal status before the local wait timeout;
- at least 95% of tasks complete successfully in the 3-worker/10-task run;
- at least 90% of tasks complete successfully in the 5-worker/100-task run;
- redispatches are explained by intentional worker churn or known worker
  failure;
- p95 `submit_latency_ms` and p95 `terminal_latency_ms` are included in the
  release note, with the terminal percentile limited to known completed tasks.

## Notes

- The benchmark script uses the current Master HTTP API and submits
  `hcode` tasks via JSON `POST /api/tasks`.
- The benchmark does not scale workers by itself; `WorkerCounts` are minimum
  registered-worker requirements and the script records the observed worker
  count from `/api/workers`.
- CPT-to-fiat conversion is intentionally out of scope.

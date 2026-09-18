# Hivemind Smoke Benchmarks

This document defines the first practical benchmark gate recommended by
`UTILITY_PERFORMANCE_EVALUATION.md`.

## Scope

The smoke benchmark validates that a controlled worker pool can accept,
dispatch, execute, and complete active `managed-function-v1` tasks. It does not
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

- `submit_latency_ms`
- `terminal_latency_ms`
- final task `status`
- `redispatched`
- `wall_time_ms`
- `peak_memory_mb`
- observed worker count at scenario start

## Command

Run from the repository root after the master/nodepool/worker services are
running and after obtaining a requestor token:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\hivemind-smoke-benchmark.ps1 `
  -MasterUrl http://127.0.0.1:8082 `
  -Token "<requestor-token>" `
  -TaskSourcePath .\templates\managed-function-v1\03_batch_sum.hmf `
  -TaskInputPath .\templates\managed-function-v1\03_batch_sum.input.json
```

Outputs are written to `test_logs/smoke-benchmark/` as CSV and JSON.

## Pass criteria

For a release candidate, record the exact commit and require:

- all submitted tasks reach a terminal status before timeout;
- at least 95% of tasks complete successfully in the 3-worker/10-task run;
- at least 90% of tasks complete successfully in the 5-worker/100-task run;
- redispatches are explained by intentional worker churn or known worker
  failure;
- p95 `submit_latency_ms` and p95 `terminal_latency_ms` are included in the
  release note.

## Notes

- The benchmark script uses the current Master HTTP API and submits
  `managed-function-v1` tasks via JSON `POST /api/tasks`.
- The benchmark does not scale workers by itself; `WorkerCounts` are minimum
  registered-worker requirements and the script records the observed worker
  count from `/api/workers`.
- CPT-to-fiat conversion is intentionally out of scope.

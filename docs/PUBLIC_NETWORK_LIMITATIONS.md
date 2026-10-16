# Public Network Limitations

Version: `10d3c92e67034cfda345fb889029069dd40e0b4a` · 2026-10-05

See the [project manual](HIVEMIND_MANUAL.zh-TW.md) for configuration and the source index.

## Enrollment and admission

Hivemind supports authenticated enrollment, Worker identity binding,
capability/readiness checks, bounded managed execution, replicated result
agreement, and Nodepool-owned CPT accounting.

Public onboarding uses authenticated dynamic admission. Ordinary managed-DSL
users do not maintain a per-Worker cryptographic allowlist. Operators can restrict
pilot deployments; general-compute beta admission has separate policy gates.
Authentication does not attest hardware resources or establish provider
independence. The trust model is defined in [AGENT.md](../AGENT.md).

## Managed execution

- New CPU managed work uses `hcode`, a closed Rust-owned interpreter
  with no file, network, import, subprocess, DLL, or arbitrary native API access.
- `managed-function-v0` is retained for historical reads. New v0 submissions are
  rejected and unfinished historical work is retired.
- Default enforced consensus requires three distinct registered Workers and two
  matching canonical results. It does not fall back to single-Worker completion.
- Worker IDs do not establish distinct owners. A colluding majority or a shared
  interpreter bug can produce matching, incorrect output.
- A quorum certificate records authenticated agreement, not independently
  verified correctness, hardware resources, or actual resource usage.
- New v1 `max_cpt` is the fee-inclusive **task-wide** cap across all configured
  replicas. Nodepool derives equal integer execution budgets and validates
  attempt-bound usage before settlement.
- Valid partial, failed, or divergent replica work can be billable even without
  a successful task result. Missing or invalid receipts are not billable;
  observe mode does not settle.
- Automatic paid retries are disabled pending a funded platform treasury.
  The task-wide cap does not retroactively change historical billing.

The full contract is in [hcode Runtime](HCODE_RUNTIME.md)
and [managed consensus](managed-consensus-state.md).

## General compute, GPU, and ONNX

These routes have separate capabilities and deployment requirements. The managed
DSL does not execute arbitrary Python, shell, containers, CUDA kernels, or
NumPy/SciPy programs.

- `general-compute-v1alpha1` is a gated pre-release route, separate from the
  default managed contract.
- Linux general compute requires the approved rootless OCI provider, pinned
  operator-owned assets, namespaces, cgroup v2, seccomp, read-only root, explicit
  artifact mounts, and deny-all networking.
- Native Windows general compute requires Windows Containers/HCS and its
  validated policy. Job Objects or direct host processes do not provide the
  required isolation; Docker Desktop, WSL, and Linux VMs are not native Windows
  providers.
- The closed Windows DSL does not require HCS. Missing general-compute
  prerequisites block only that backend, with no weaker execution fallback.
- GPU and ONNX routes require compatible typed capabilities and approved backend
  assets. Managed GPU operations are fixed; user-supplied native code is not accepted.
- ONNX output transport requires verifiable, bounded inline artifact bytes.
  Referencing an untransferred `/work/output` file is insufficient. See the
  [ONNX guide](GENERAL_COMPUTE_ONNX.md).

## CPT and service limits

Treat CPT as an internal quota/budget unit only. Account transfers and earnings
displays record internal CPT balances; they do not provide fiat/token redemption
or withdrawable income.

The current service contract does not include:

- a public bidding marketplace with independently attested provider resources;
- guaranteed monetary value or fiat/token redemption for CPT;
- guaranteed operator diversity based on Worker count;
- dispute resolution, public SLAs, or audited large-scale capacity and latency;
- hostile-host hardening for every backend and operating system;
- universal macOS/Android packages or an operational production signed-update channel.

## Rollout validation

Validate the release artifact with its deployed backend configuration:

1. Exercise Website API enrollment through the external Headscale path.
2. Register enough eligible Workers and test attempt-bound execution, quorum,
   cancellation/fencing, result retrieval, valid usage, holds/refunds, and durable
   Nodepool settlement.
3. Run settlement tests against a dedicated PostgreSQL database and record any
   skipped cases.
4. Run general-compute workloads on the selected provider, including hostile-workload
   tests. `-CheckOnly`, mocks, and HCS prerequisite checks cover setup, not execution.
5. Measure throughput, p95 latency, cost, and availability for the actual workload
   and topology before publishing performance comparisons.

Testing layers are described in the [platform validation guide](platform-validation-state.md).
[Smoke Benchmarks](SMOKE_BENCHMARKS.md) provides the measurement procedure.

# Public Network Limitations

Version: `2026-11-19`

See the [project manual](HIVEMIND_MANUAL.zh-TW.md) for configuration and the source index.

## First public version scope

The core managed compute path covers managed `hcode` CPU execution, quorum consensus,
and Nodepool settlement. Additional modules for OpenAI-compatible LLM inference
(`model-catalog`, `model-swarm`, `llm-runtime`, `petals`, `llm-verifier`), bounded
WebAssembly (`wasm-runtime`), multi-format model selection, and cross-platform C ABI
(`hivemind-ffi`) are implemented in-tree with automated unit and database tests,
while production public-internet multi-host rollouts, signed desktop/mobile installers,
physical NPU driver certification, and the signed auto-update channel remain gated on
real deployment validation.
Lazy dispatch (`MANAGED_CONSENSUS_INITIAL_REPLICAS`) and hedging
(`MANAGED_CONSENSUS_HEDGE_AFTER_SECS`) are off by default. The drain window
(`MANAGED_CONSENSUS_DRAIN_SECS`) defaults to 5 seconds.

## Enrollment and admission

Hivemind supports authenticated enrollment, Worker identity binding,
capability/readiness checks, bounded managed execution, replicated result
agreement, and Nodepool-owned CPT accounting.

Public onboarding uses authenticated dynamic admission. Ordinary managed-DSL
users do not maintain a per-Worker cryptographic allowlist. Operators can restrict
pilot deployments; general-compute beta admission has separate policy gates.
Authentication does not attest hardware resources or establish provider
independence. The trust model is defined in [AGENT.md](../AGENT.md).

## LLM inference, privacy tiers, and distributed execution

- **Tier 0 (Private Own-Device Mesh)**: Prompts and tasks are routed strictly to
  devices owned by the requesting account (`StrictPrivate` / `X-Hivemind-Own-Devices-Only`).
  Same-LAN private peers are preferred before WAN overlay hops.
- **Tier 1 (Public Mutual-Aid Pool)**: Prompts sent to third-party public Workers are
  **visible in plaintext to the executing Worker node**. Master UI displays an explicit
  privacy warning and every OpenAI HTTP response emits `X-Hivemind-Data-Privacy: public-unencrypted-to-worker`.
  Do not send regulated secrets or confidential personal data to Tier 1 public Workers.
- **Tier 2 (Confidential Computing / TEE)**: Control-plane traits and routing tiers are
  reserved (`tier2_tee`), but hardware remote attestation (NVIDIA CC / AMD SEV / Intel TDX)
  is not active in the public pool until verified TEE hardware nodes are deployed.
- **LLM runtime isolation**: `hivemind-llm-runtime` enforces Linux rootless OCI sandboxing
  (namespaces, cgroup v2, seccomp, read-only root, deny-all network) when `RequiredOci` is
  configured on Linux, and fails closed on non-Linux hosts. Native Windows does not claim
  Linux rootless OCI isolation for `llama-server`.
- **LLM quality verification**: Deterministic `hcode` and bounded `wasm-runtime` tasks can
  use exact output consensus, whereas floating-point LLM inference varies slightly across
  GPU/CPU backends. `hivemind-llm-verifier` provides teacher-forcing logprob/top-k spot
  checks and node reputation quarantine, which detect gross tampering or model substitution
  rather than providing cryptographic proof of honest execution.
- **Distributed inference (`hivemind-petals`)**: Layer-sharded multi-hop execution, activation
  compression, and KV-cache replay failover are verified in-process via async multi-node tests;
  public-internet latency and stability depend on participating WAN links.

## Managed execution & WebAssembly

- New CPU managed work uses `hcode`, a closed Rust-owned interpreter
  with no file, network, import, subprocess, DLL, or arbitrary native API access.
- `hivemind-wasm-runtime` provides a pure-Rust bounded WebAssembly / WASI-subset sandbox
  enforcing fuel budgets, linear memory page caps, wall-clock timeouts, output byte caps,
  and an explicit import allowlist (rejecting filesystem and network syscalls at load time).
- `managed-function-v0` is retained for historical reads. New v0 submissions are
  rejected and unfinished historical work is retired.
- Default enforced consensus dispatches three Workers from distinct owners. It
  requires two matching canonical results and does not fall back to single-Worker
  completion. With `MANAGED_CONSENSUS_INITIAL_REPLICAS=2`, two Workers are
  dispatched first and a third is added only after disagreement or timeout.
- Worker IDs do not establish distinct owners. A colluding majority or a shared
  interpreter bug can produce matching, incorrect output.
- A quorum certificate records authenticated agreement, not independently
  verified correctness, hardware resources, or actual resource usage.
- New v1 `max_cpt` is the fee-inclusive **task-wide** cap across all configured
  replicas. Nodepool derives equal integer execution budgets and validates
  attempt-bound usage before settlement.
- Under the current billing identity (`hcode-billing-v3-failed-no-charge`), a task
  that ends without a quorum certificate is not charged, whether it failed,
  timed out, or had divergent replicas. A task with a certificate is charged for
  valid replica work within `max_cpt`. Holds created under earlier identities keep
  the rule they were created with. Missing or invalid receipts are not billable;
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
- Native Windows general compute is not provided. Job Objects, direct host processes,
  Docker Desktop, WSL, and Linux VMs are not isolation for it.
- The closed Windows DSL needs no Windows container feature. Missing general-compute
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
- signed/notarized macOS/iOS/Android app-store packages or an operational production signed-update channel.

## Rollout validation

Validate the release artifact with its deployed backend configuration:

1. Exercise Website API enrollment through the external Headscale path.
2. Register enough eligible Workers and test attempt-bound execution, quorum,
   cancellation/fencing, result retrieval, valid usage, holds/refunds, and durable
   Nodepool settlement.
3. Run settlement tests against a dedicated PostgreSQL database and record any
   skipped cases.
4. Run general-compute workloads on the selected provider, including hostile-workload
   tests. `-CheckOnly` and mocks cover setup, not execution.
5. Measure throughput, p95 latency, cost, and availability for the actual workload
   and topology before publishing performance comparisons.

Testing layers are described in the [platform validation guide](platform-validation-state.md).
[Smoke Benchmarks](SMOKE_BENCHMARKS.md) provides the measurement procedure.

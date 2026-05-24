# HiveMind Architecture

HiveMind is a batch-oriented distributed compute runtime for public-network
workers. It is not a Python sandbox product, Ray-like RPC runtime, actor system,
or DNS task-routing system.

## Core Constraints

- Public network workers are high-latency and may disconnect.
- Task execution must be coarse-grained, usually seconds to minutes or longer.
- One network call should lease or complete a batch of tasks, not one micro-task.
- DNS may be used for node discovery only. It must not map tasks to workers.
- Python is only a DSL/compiler surface. It must not become the execution
  runtime or scheduler control path.

Explicitly forbidden runtime directions:

- Fine-grained RPC orchestration.
- Actor-based runtime behavior.
- Distributed shared memory or distributed object calls.
- Micro-task execution.
- DNS-based task routing.
- Dynamic Python runtime control flow that affects scheduling.

## Layers

- Python DSL: declares tasks, DAG dependencies, and resource annotations only.
  The DSL must not execute user code, perform networking, interact with the
  scheduler, or depend on mutable global state.
- Compiler / Task Graph Builder: converts DSL declarations to DAG IR and
  ExecutionPackage metadata. It does not execute Python or inspect workers.
- Nodepool / Scheduler: leases batches, tracks task ownership, retry state,
  lease deadlines, and minimal worker liveness. It does not execute tasks,
  parse Python, or maintain full worker cache state.
- Rust Worker Runtime: owns local queue, artifact cache, batch execution,
  sandbox/resource enforcement, and result upload. It does not schedule across
  workers or parse DAGs.
- Network Layer: uses VPN overlay for node connectivity and pull-based task
  acquisition.
- Artifact Store: a core subsystem for manifests, chunk integrity, resumable
  transfer, deduplication, corruption recovery, and cache consistency.

## Runtime Contracts

- DAG IR includes `job_id`, `nodes`, `edges`, resource requirements, artifact
  inputs, `max_retries`, `deadline`, `deterministic`, `side_effects`, and
  `priority`.
- ExecutionPackage sits between DAG IR and worker execution. It pins
  `runtime_version`, `task_code_ref`, `artifact_refs`, and constraints.
- ArtifactManifest is required for every artifact. SHA256 CAS is only the
  storage primitive; manifests carry chunks, size, compression, content type,
  and producer task metadata.
- PullBatchRequest carries real-time worker capacity and a cache summary:
  `max_inflight_batches`, available memory, queue capacity, cache epoch, Bloom
  filter, partial digest, and summary counters.
- Nodepool must not persist full worker cache manifests or build a central
  artifact cache index. Locality decisions use the current PullBatch request
  only.
- CompleteBatchRequest returns per-task status, result artifact references, and
  execution metrics such as CPU time, wall time, peak memory, downloaded bytes,
  and cache hits.

## Current Implementation Status

- The existing `UploadTask` / `ExecuteTask` push path remains as a legacy
  compatibility path.
- `BatchRuntimeService.PullBatch`, `CompleteBatch`, and `Heartbeat` define the
  new pull-based batch path in `proto/hivemind.proto`.
- Nodepool has a weak-state batch lease implementation that applies
  backpressure and records completion metrics without storing heavy worker
  cache state.
- `services/nodepool/internal/dag` provides the Go DAG builder/compiler
  surface for `DAGIR` and `ExecutionPackage`.
- `services/nodepool/internal/artifact` provides the initial in-memory
  manifest-based CAS contract for tests and future storage backends.

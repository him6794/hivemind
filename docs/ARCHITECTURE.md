# HiveMind Architecture

HiveMind is a Rust-based distributed compute runtime built around a single
workspace, `hivemind-rs`, and a coordinating binary, `hivemind-bin`.

## Workspace Layout

```text
hivemind-rs/
  crates/common           shared tracing, errors, helpers
  crates/config           environment and file-based configuration
  crates/proto            generated gRPC contracts
  crates/models           shared domain types
  crates/database         PostgreSQL and Redis access
  crates/auth             registration, login, token handling
  crates/node-manager     worker registration, heartbeat, trust, cleanup
  crates/task-scheduler   dispatch, redispatch, and timeout handling
  crates/master-api       HTTP API and proxy layer
  crates/worker-executor  sandboxed task execution and worker control API
  crates/vpn-service      VPN peer management
  crates/torrent-service  artifact and torrent handling
  crates/hivemind-bin     runtime entry point
```

The repository also contains React frontends under `frontend/master-ui` and
`frontend/worker-ui`.

## Runtime Topology

```text
Client / UI
  -> Master HTTP API
  -> Nodepool gRPC services
  -> Worker gRPC service and worker control HTTP API

Nodepool composes:
  - auth
  - node-manager
  - task-scheduler
  - database
  - vpn-service
  - proto contracts

Worker execution composes:
  - worker-executor
  - torrent-service
  - config
  - proto contracts
```

## Main Service Roles

### Master API
- Exposes the external HTTP API on `MASTER_HTTP_ADDR`.
- Proxies master-side requests to the nodepool gRPC service.
- Serves `/health` and task/worker administration endpoints.

### Node Manager
- Registers workers and tracks heartbeats.
- Maintains worker trust and liveness state.
- Owns cleanup and status reporting for worker nodes.

### Task Scheduler
- Selects workers for pending work.
- Handles redispatch and timeout loops.
- Keeps the task lifecycle moving without embedding execution logic.

### Worker Executor
- Runs tasks in a sandboxed environment.
- Tracks local resource usage.
- Exposes worker gRPC and worker control HTTP endpoints.

### VPN Service
- Manages secure worker connectivity.
- Handles peer lifecycle and virtual addressing.

### Torrent Service
- Handles artifact/torrent-oriented payloads used by task execution.
- Lives alongside the worker runtime and master proxying path.

## Current Contracts

- `proto/hivemind.proto` defines the shared gRPC surface.
- The current workspace includes both legacy task upload/result paths and the
  newer batch-runtime messages in the proto file.
- `hivemind-bin` can run `master`, `nodepool`, `worker`, or `all`.
- The binary also exposes `submit`, `status`, and `result` CLI helpers.

## Default Addresses

- Nodepool gRPC: `0.0.0.0:50051`
- Master HTTP: `0.0.0.0:8082`
- Worker gRPC: `0.0.0.0:50053`
- Worker control HTTP: `127.0.0.1:18080`

These defaults come from `crates/config` and can be overridden with
environment variables or a JSON config file.

## Data Flow

1. Client or UI submits work to the Master API.
2. Master API forwards the request to nodepool over gRPC.
3. Nodepool coordinates worker registration, task dispatch, and trust checks.
4. Workers execute tasks locally and report progress or results back through
   the gRPC/control APIs.
5. The database and Redis back the shared runtime state.

## Current Status

- The Rust workspace is the authoritative implementation.
- The older Python-era architecture notes in `docs_backup_20260611_202024/`
  are historical reference only.
- The architecture here reflects the current crate boundaries and runtime
  entry points rather than the archived pre-Rust layout.

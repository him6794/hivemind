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
  crates/worker-executor  managed-function task execution and worker control API
  crates/vpn-service      VPN peer management
  crates/hivemind-bin     runtime entry point
```

The repository also contains the Official Site at `frontend/`, Master UI at
`frontend/master-ui`, and Worker UI at `frontend/worker-ui`.

## Runtime Topology

```text
Browser
  -> Official Site (8080; public site and account center)
       -> website server -> Nodepool gRPC (50051)
  -> Master UI (3000) -> Master API (8082) -> Nodepool gRPC (50051)
  -> Worker UI (3001) -> Master API (8082)
                      -> local worker control HTTP (18080)

Nodepool gRPC (50051)
  -> Worker gRPC (50053) for dispatch and result reporting

Nodepool composes:
  - auth
  - node-manager
  - task-scheduler
  - database
  - vpn-service
  - proto contracts

Worker execution composes:
  - worker-executor
  - config
  - proto contracts
```

## Main Service Roles

### Official Site
- Serves the public product experience and authenticated account center on
  port 8080.
- Owns registration, login, balance visibility, and handoff documentation.
- Does not submit tasks or control workers.

### Master UI
- Serves the task operator application on port 3000.
- Calls the Master API for authentication, managed-function task submission,
  task state, cancellation, logs, results, and artifact downloads.

### Worker UI
- Serves the provider application on port 3001.
- Reads local capacity/profile data from the worker control API and uses the
  Master API to authenticate and register the worker.

### Master API
- Exposes the external HTTP API on `MASTER_HTTP_ADDR` (port 8082 in the release
  stack).
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
- Runs `managed-function-v0` tasks in the managed-function runtime.
- Tracks local resource usage.
- Exposes worker gRPC and worker control HTTP endpoints; the release stack
  publishes worker control on port 18080.

### VPN Service
- Manages secure worker connectivity.
- Handles peer lifecycle and virtual addressing.

## Current Contracts

- `proto/hivemind.proto` defines the shared gRPC surface.
- Tasks use the `managed-function-v0` runtime: a source function plus a JSON
  input payload carried in the batch-runtime messages of the proto file.
- `hivemind-bin` can run `master`, `nodepool`, `worker`, or `all`.
- The binary also exposes `submit`, `status`, and `result` CLI helpers.

## Default Addresses

- Official Site: `0.0.0.0:8080` (Compose host mapping)
- Master UI: `0.0.0.0:3000` (Compose host mapping)
- Worker UI: `0.0.0.0:3001` (Compose host mapping)
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

## Trust and Product Boundaries

Nodepool is the only trusted authority. It owns account state, balances, task
state, worker registration, scheduling, and billing; Master and Worker
deployments remain untrusted callers whose tokens are validated by nodepool.
Worker results, usage, and billing values are claims that nodepool verifies
server-side.

The Official Site is deliberately limited to public content and the account
center. Task submission and result operations belong to Master UI, while local
worker discovery and registration belong to Worker UI. Browser code must never
connect directly to nodepool. The Official Site browser calls its own website
backend, which uses `WEBSITE_NODEPOOL_GRPC_ADDR` server-side; the nodepool gRPC
endpoint is not a public browser API.

## Current Status

- The Rust workspace is the authoritative implementation.
- The older Python-era architecture notes in `docs_backup_20260611_202024/`
  are historical reference only.
- The architecture here reflects the current crate boundaries and runtime
  entry points rather than the archived pre-Rust layout.

# Backend Service Progress

## Goal
Deliver the backend portion of the centralized compute marketplace, including authentication, worker ingress, task dispatch, persistence, transfer accounting, and observability.

## Status
In progress. The workspace verification is currently green, but several backend areas still rely on simplified or in-memory implementations.

## Current Step
The current focus is worker ingress and trust enforcement. The worker pull path now blocks banned workers; the next slice is coverage for low-score claim blocking and alignment with dispatcher trust thresholds.

## Completed Workstreams

### 1. Authentication and registration flow
- NodePool HTTP exposes `POST /api/register`.
- Master HTTP exposes `POST /api/register` and forwards to NodePool HTTP.
- The registration flow uses username/password credentials.
- Passwords are stored with bcrypt.
- The reworked flow avoids relying on a separate shell script for password bootstrapping.

### 2. Worker ingress authentication and authorization
- NodePool `TaskOutputUpload`, `TaskResultUpload`, and `TaskUsage` now require a token.
- Token validation is driven by `NODEPOOL_WORKER_INGRESS_AUTH`.
- The token can be a JWT.
- The token username must match the worker identity, and the worker identity must be trusted.
- Worker clients use `WORKER_PASSWORD` for the NodePool token, with a default value of `worker123`.
- Worker uploads for output, result, and usage are now gated by token checks.

### 3. `host_count` fanout across workers
- `UploadTask` uses `host_count` to fan out shards across multiple workers.
- Partial dispatch is logged explicitly in the task log with a `[HOST_FANOUT]` marker.
- The current design still needs a clearer mapping between `task_id`, worker sharding, and subtask routing.

### 4. Worker removal and executor cleanup
- Worker removal now goes through the executor cleanup path.
- The cleanup path ensures that removing a worker also shuts down the related executor if it is active.
- This prevents stale executors from continuing to run after a worker is removed.

### 5. Rust Python sandbox support
- The executor currently supports a Python sandbox backend.
- `executor-cli` can use a `pydantic/monty`-based backend via `EXECUTOR_SANDBOX_BACKEND=monty`.
- Monty can fall back to the native backend with `EXECUTOR_MONTY_FALLBACK_NATIVE=true`.
- `EXECUTOR_MONTY_PYTHON_CMD` controls the Python executable used by Monty.
- A follow-up cleanup is still needed around the helper script location and packaging flow.

### 6. Worker task persistence
- Worker `TaskService` still uses an in-memory repository in the current implementation.
- `WORKER_TASK_PERSIST_PATH` is the proposed persistence location for durable task state.
- The next step is to move this path from a placeholder into a real persistence layer, with restart recovery and durability guarantees.

### 7. CPT transfer query API
- NodePool HTTP exposes `GET /api/transfers`.
- The endpoint supports `limit` and `task_id`.
- The query reads from `cpt_transfers`, which stores payer/payee accounting data.
- Master HTTP proxies `GET /api/transfers` to NodePool HTTP.

### 8. Dispatch failure telemetry and observability
- Dispatch failures should be classified into `NO_WORKER`, `PROBE_FAIL`, `DIAL_FAIL`, `EXEC_FAIL`, and `REJECTED`.
- Failure reasons should be written into the task log.
- Pre-dispatch probe output should be captured with request parameters.
- HTTP request IDs and latency should be recorded across:
  - NodePool HTTP
  - Master HTTP
  - Worker control HTTP
- Observability still needs a fuller implementation for logs, traces, and deployment-level pipeline visibility.

## Configuration and Runtime Defaults
- `NODEPOOL_WORKER_INGRESS_AUTH`: `true`
- `NODEPOOL_PRE_DISPATCH_PROBE`: `true`
- `NODEPOOL_TRANSFERS_LIMIT`: `100`
- `NODEPOOL_HTTP_BASE`: Master-facing NodePool HTTP base URL
- `WORKER_PASSWORD`: token used by worker ingress
- `WORKER_TASK_PERSIST_PATH`: worker task persistence path
- `EXECUTOR_SANDBOX_BACKEND`: `native` or `monty`
- `EXECUTOR_MONTY_PYTHON_CMD`: defaults to `python`
- `EXECUTOR_MONTY_FALLBACK_NATIVE`: `true` when Monty may fall back to native execution

## Verification
- Workspace status: `passed=18, failed=0`
- No new schema or migration failures were introduced by the recent progress work.

## Next Steps
1. Split `host_count` fanout into a clearer subtask model.
2. Replace the current password flow with bcrypt or argon2-backed registration and gRPC-friendly auth flows.
3. Add transfer queries for task-specific and user-specific views.
4. Move worker task persistence to SQLite plus migrations.
5. Add observability for metrics and tracing, then wire the result into CI.

## Risks
- The current worker task store is still in-memory.
- The sandbox backend still has a fallback path that must not be allowed to mask deployment issues.
- Transfer accounting, dispatch telemetry, and persistence are still incomplete compared with the target backend design.

## Reference Files
- [proto/hivemind.proto](../proto/hivemind.proto)
- [services/nodepool/cmd/server/main.go](../services/nodepool/cmd/server/main.go)
- [services/master/cmd/server/main.go](../services/master/cmd/server/main.go)
- [frontend/master-ui/src/App.jsx](../frontend/master-ui/src/App.jsx)
- [frontend/worker-ui/src/App.jsx](../frontend/worker-ui/src/App.jsx)

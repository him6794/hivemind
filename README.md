# Hivemind - Distributed Compute Runtime

A distributed compute runtime for public-network workers, now rewritten in Rust.

## Quick Start

```bash
# Build
make build

# Run tests
make test

# Start development environment
make dev
```

## User Guide

If you want to submit a task and write the task program, start here:

- [docs/MANAGED_FUNCTION_RUNTIME.md](docs/MANAGED_FUNCTION_RUNTIME.md) — the
  `managed-function-v0` syntax, metering model, and billing formula
- [docs/PUBLIC_NETWORK_LIMITATIONS.md](docs/PUBLIC_NETWORK_LIMITATIONS.md) —
  what the network does not do yet, including that CPT is an internal quota unit
- The Docs and Usage rules pages on the official site render the same reference
  from `frontend/src/lib/hivemind-site-data.mjs`; `frontend/site-contract.test.mjs`
  checks the limits and failure codes it publishes against the enforcing source

## Architecture

Hivemind is a batch-oriented distributed compute runtime. The system consists of:

- **Hivemind Binary** (`hivemind-rs/`) - Unified Rust binary containing all services
- **Official Site** (`frontend/`) - Public-facing product site, account center, and documentation
- **Master UI** (`frontend/master-ui/`) - React surface for users: task submission, API keys, dashboard
- **Worker UI** (`frontend/worker-ui/`) - React surface for workers: node status, task queue, earnings
- **Infrastructure** - Docker Compose for Redis and PostgreSQL

### Services

All services run in a single binary (`hivemind-bin`):

| Service | Port | Protocol | Description |
|---------|------|----------|-------------|
| Master API | 8082 | HTTP | User authentication, task management |
| Nodepool | 50051 | gRPC | Worker registration, task scheduling |
| Worker | 50053 | gRPC | Task execution, result reporting |

### Rust Crates

```
hivemind-rs/
├── crates/
│   ├── auth/           - JWT authentication
│   ├── common/         - Shared utilities
│   ├── config/         - Configuration management
│   ├── database/       - PostgreSQL & Redis integration
│   ├── hivemind-bin/   - Main binary entry point
│   ├── master-api/     - HTTP API handlers
│   ├── models/         - Data models
│   ├── node-manager/   - Worker management
│   ├── proto/          - gRPC protobuf definitions
│   ├── task-scheduler/ - Task dispatch & scheduling
│   ├── vpn-service/    - VPN management
│   └── worker-executor/- Task execution engine
```

## Development

### Prerequisites

- Rust 1.70+
- Docker (for Redis & PostgreSQL)
- Node.js 18+ (for frontend)

### Build

```bash
# Build Rust binary
make build

# Build frontend
make build-frontend

# Release smoke all frontend surfaces
make smoke-frontend
```

The shared frontend release harness also runs directly on Windows PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-release-frontends.ps1

$env:VITE_API_BASE = "http://127.0.0.1:8082"
$env:VITE_WORKER_CONTROL_BASE = "http://127.0.0.1:18080"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-frontend-smoke.ps1
```

### Test

```bash
# Run all tests
make test

# Run tests with verbose output
make test-verbose
```

### Lint & Format

```bash
# Run linter
make lint

# Format code
make fmt
```

## Deployment

### Exposed Ports

| Service       | Port  | Description                |
|---------------|-------|----------------------------|
| Official Site | 8080  | Public product site and account center |
| Master UI     | 3000  | User-facing task dashboard |
| Worker UI     | 3001  | Worker node control panel  |
| Master API    | 8082  | HTTP API (auth, tasks)     |
| Nodepool gRPC | 50051 | Worker registration        |
| Worker gRPC   | 50053 | Task execution             |
| Worker HTTP   | 18080 | Local worker control API   |
| Redis         | 6379  | Session & cache            |
| PostgreSQL    | 5432  | Persistent data            |

### Docker Compose

```bash
# Start all services
make docker-up

# View logs
make docker-logs

# Stop all services
make docker-down
```

### Three-surface release

The release candidate consists of the public Official Site/account center on
port 8080, the task-oriented Master UI on port 3000, and the provider-oriented
Worker UI on port 3001. The detailed operator runbook, environment contract,
trust boundaries, smoke checks, browser proof, and troubleshooting steps are in
[docs/GETTING_STARTED.md](docs/GETTING_STARTED.md).

From the repository root on Windows, build and leave the complete stack running:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-stack-smoke.ps1 -KeepRunning
```

Then run the real cross-surface browser journey:

```powershell
cd frontend; npm run test:e2e
```

When verification is complete, return to the repository root and run
`docker compose down`. See the runbook before using raw Compose in production:
unlike the smoke harness, raw Compose requires secrets and the matching worker
execution key pair to be supplied.

### Managed-function proving

`managed-function-v0` tasks are settled only from a RISC Zero proof that the
Nodepool verifies itself — a Worker's own usage numbers are never trusted. The
Worker produces that proof by spawning an isolated prover sidecar, so a Worker
that is meant to run managed tasks needs the sidecar binary present.

#### Supported proving hosts

| Proving host | Supported | Notes |
|---|---|---|
| Linux (`x86_64`) | Yes | The host the released sidecar is built on |
| macOS | Yes | Supported by RISC Zero 3.0.6 |
| WSL | Yes | Reports `Linux`, so it takes the supported Linux path |
| Native Windows (MINGW/MSYS/Cygwin) | No | RISC Zero 3.0.6 ships no Windows prover |

There is no native Windows proving path, and Hivemind does not emulate one.
`scripts/build-managed-prover.sh` refuses to run under a native Windows shell up
front, rather than failing deep inside a RISC Zero build script.

A native Windows Worker therefore has no prover sidecar to spawn. It can still
run ordinary worker workloads, but under the default `enforce` rollout mode every
managed task it is handed **fails closed** — never settled from unverified
numbers. Managed tasks must run on a worker image or runtime that contains the
Linux prover sidecar.

`scripts/package-worker-windows.ps1` packages a native Windows worker and so
stages no prover sidecar. The README it generates states that, rather than
leaving a provider to infer it from managed tasks failing.

Build the sidecar once on a supported host and stage it, then build the worker
image:

```bash
bash scripts/build-managed-prover.sh   # writes packaging/managed-prover/
docker compose build worker            # bakes it into /app/prover/
```

From a Windows checkout, run the same build through WSL:

```powershell
wsl bash scripts/build-managed-prover.sh
```

#### Building without access to the RISC Zero artifact bucket

The recursion circuit build downloads `recursion_zkr.zip` from
`risc0-artifacts.s3.us-west-2.amazonaws.com`. Where network policy blocks that
bucket, use `RECURSION_SRC_PATH` — the official upstream offline escape hatch —
rather than patching anything in the RISC Zero registry sources:

```bash
RECURSION_SRC_PATH=/path/to/recursion_zkr.zip bash scripts/build-managed-prover.sh
```

`scripts/build-managed-prover.sh` verifies that artifact's SHA-256 against
`744b999f0a35b3c86753311c7efb2a0054be21727095cf105af6ee7d3f4d8849` before
handing it to Cargo, and aborts on a mismatch. This reuses a checked artifact —
it does not skip the check. With `RECURSION_SRC_PATH` unset the script reuses a
`recursion_zkr.zip` already present in the Cargo target tree under the same
digest check, and otherwise leaves RISC Zero to its normal network download.

#### Settlement behaviour

`MANAGED_PROVER_EXECUTABLE` defaults to `/app/prover/hivemind-managed-proof-prover`
under Compose. If the sidecar is missing, or its proof fails verification, the
task fails — it is never settled from unverified numbers. Proving a single
managed function currently takes roughly 570–580 seconds, which is why
`MANAGED_PROVER_TIMEOUT_SECS` defaults to 900.

`MANAGED_PROOF_ROLLOUT_MODE` controls the settlement policy and defaults to the
fail-closed `enforce`. `observe` verifies proofs and records the outcome but
still settles from the legacy path, which is useful for a monitored migration;
`off` skips proof handling entirely and is an emergency rollback only. Both
non-default modes settle from Worker-reported numbers, so neither is a
trust-preserving configuration — watch
`/api/admin/managed-proof/metrics` and the `managed_proof_verification` audit
entries while either is active.

Note which service takes which setting. `MANAGED_PROOF_ROLLOUT_MODE` belongs to
the **nodepool**, because the nodepool owns the dispatcher that decides how a
task settles; setting it on a worker has no effect at all. The prover settings
belong to the **worker**, because that is where proving happens.

### Manual

```bash
# Start Redis
docker run -d -p 6379:6379 redis:7-alpine

# Start PostgreSQL
export POSTGRES_PASSWORD='replace-with-a-strong-password'
docker run -d -p 5432:5432 \
  -e POSTGRES_DB=hivemind \
  -e POSTGRES_USER=hivemind \
  -e POSTGRES_PASSWORD="$POSTGRES_PASSWORD" \
  postgres:16-alpine

# Run Hivemind
DATABASE_URL="postgres://hivemind:${POSTGRES_PASSWORD}@localhost:5432/hivemind" \
REDIS_URL=redis://localhost:6379 \
./target/release/hivemind-bin all
```

## Configuration

Configuration is via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | - | PostgreSQL connection string |
| `REDIS_URL` | - | Redis connection string |
| `JWT_SECRET` | - | JWT signing secret |
| `HIVEMIND_ADMIN_USERS` | unset | Comma-separated usernames allowed to access `/api/admin/*` endpoints |
| `HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE` | `60` | Per-user task submission rate limit for a rolling 1-minute window (`0` disables limiting) |
| `WEBSITE_NODEPOOL_GRPC_ADDR` | `localhost:50051` | Server-side nodepool target used only by the official website backend for account endpoints |
| `MASTER_HTTP_ADDR` | `0.0.0.0:8082` | Master HTTP listen address |
| `NODEPOOL_GRPC_ADDR` | `0.0.0.0:50051` | Nodepool gRPC listen/connect address |
| `WORKER_GRPC_ADDR` | `0.0.0.0:50053` | Worker gRPC listen address |
| `WORKER_ADVERTISE_ADDR` | - | Worker address registered with nodepool |
| `EXECUTOR_SANDBOX_DIR` | `./sandbox` | Per-task working directory root |
| `MANAGED_PROOF_ROLLOUT_MODE` | `enforce` | Managed-proof settlement policy: `off`, `observe`, or `enforce`; production default is fail-closed `enforce` |
| `MANAGED_PROVER_EXECUTABLE` | - | Absolute path to the managed-proof prover sidecar, built on a Linux/macOS/WSL host; required for managed tasks in `enforce`. Unset on native Windows, where managed tasks fail closed |
| `MANAGED_PROVER_TIMEOUT_SECS` | `900` | Bounded prover sidecar execution timeout |
| `LOG_LEVEL` | `info` | Log level (debug, info, warn, error) |

## API Reference

### Authentication

```bash
# Login
curl -X POST http://localhost:8082/api/login \
  -H "Content-Type: application/json" \
  -d '{"username": "<username>", "password": "<password>"}'
```

### Tasks

```bash
# Create task
curl -X POST http://localhost:8082/api/tasks \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "task_id": "task-1",
    "runtime": "managed-function-v0",
    "task_source": "fn sum(values) {\n  return get(values, \"a\") + get(values, \"b\");\n}\nsum(input);\n",
    "torrent": "{\"a\": 1, \"b\": 2}",
    "memory_gb": 4,
    "cpu_score": 100,
    "storage_gb": 10,
    "max_cpt": 25
  }'

# List tasks
curl http://localhost:8082/api/tasks \
  -H "Authorization: Bearer <token>"
```

### Admin Observability

```bash
# Cache alert (with thresholds)
curl "http://localhost:8082/api/admin/scheduling/cache-alert?low=0.5&high=2.0" \
  -H "Authorization: Bearer <admin-token>"

# Cache anomaly history (persisted low/high alerts)
curl "http://localhost:8082/api/admin/scheduling/cache-anomalies?limit=100" \
  -H "Authorization: Bearer <admin-token>"

# Admin audit logs (trust-control / artifact cleanup / etc.)
curl "http://localhost:8082/api/admin/audit/logs?limit=100" \
  -H "Authorization: Bearer <admin-token>"

# Managed-proof verification counters and active rollout mode (Nodepool-owned)
curl http://localhost:8082/api/admin/managed-proof/metrics \
  -H "Authorization: Bearer <admin-token>"
```

Managed-proof verification and observe-mode fallback decisions are also written
as `managed_proof_verification` entries in the admin audit log. The Nodepool is
the only authority for these counters; a Master or Worker cannot edit them.

### Health Check

```bash
curl http://localhost:8082/health
```

## License

MIT

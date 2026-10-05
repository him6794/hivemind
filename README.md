# Hivemind - Distributed Compute Runtime

A batch-oriented distributed compute runtime for public-network workers, built in Rust.

## Quick Start

From the repository root, build and run the Rust workspace tests:

```powershell
cargo build --release --manifest-path hivemind-rs/Cargo.toml
cargo test --manifest-path hivemind-rs/Cargo.toml
```

Database and settlement tests require a dedicated test database. To start
services, prepare the role-specific configuration and execution keys; enforced
consensus also needs enough eligible Workers. See the
[project manual (繁體中文)](docs/HIVEMIND_MANUAL.zh-TW.md)
or [Getting Started](docs/GETTING_STARTED.md) for setup.

The root `Makefile` provides convenience targets when GNU Make and the required
platform tools are installed; there is no separate `hivemind-rs/Makefile`.

## User Guide

The [project manual (繁體中文)](docs/HIVEMIND_MANUAL.zh-TW.md) covers architecture,
installation, operations, APIs, billing, testing, and troubleshooting. See the
[documentation index](docs/README.md) for runtime contracts, product plans, and
historical evaluations.

For task submission and programming:

- [docs/MANAGED_FUNCTION_RUNTIME.md](docs/MANAGED_FUNCTION_RUNTIME.md) — active
  `managed-function-v1` syntax, metering, and settlement
- [docs/PUBLIC_NETWORK_LIMITATIONS.md](docs/PUBLIC_NETWORK_LIMITATIONS.md) —
  execution, security, service, and CPT limits
- The official site's Docs and Usage rules pages share the reference in
  `frontend/src/lib/hivemind-site-data.mjs`. `frontend/site-contract.test.mjs`
  checks the published limits and failure codes against the runtime implementation.

## Architecture

The system consists of:

- **Hivemind Binary** (`hivemind-rs/`) - Unified Rust binary containing all services
- **Official Site** (`frontend/`) - Product site, account center, and documentation
- **Master UI** (`frontend/master-ui/`) - Task submission, progress, logs, results, and history
- **Worker UI** (`frontend/worker-ui/`) - Local sign-in, registration, status, and earnings
- **Infrastructure** - Redis and PostgreSQL, with a local Docker Compose stack

### Services

Service roles are selected through `hivemind-bin`:

| Service | Port | Protocol | Description |
|---------|------|----------|-------------|
| Master API | 8082 | HTTP | User authentication, task management |
| Nodepool | 50051 | gRPC | Worker registration, task scheduling |
| Worker | 50053 | gRPC | Task execution, result reporting |

### Rust Crates

```text
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

- Rust 1.98.1, used by Hivemind CI; the workspace does not declare an MSRV
- Native build tools for the chosen target; see the [project manual](docs/HIVEMIND_MANUAL.zh-TW.md)
- Docker with Compose for the local service stack
- Node.js 20.9+ for the Next.js official site (CI uses Node.js 20)
- Windows PowerShell 5.1 or PowerShell 7 for frontend/release scripts

### Build

```bash
make build
make build-frontend
make smoke-frontend
```

The frontend release harness also runs directly on Windows PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-release-frontends.ps1

$env:VITE_API_BASE = "http://127.0.0.1:8082"
$env:VITE_WORKER_CONTROL_BASE = "http://127.0.0.1:18080"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-frontend-smoke.ps1
```

### Test

```bash
make test
make test-verbose
```

### Lint & Format

```bash
make lint
make fmt
```

## Deployment

### Exposed Ports

| Service       | Port  | Description                |
|---------------|-------|----------------------------|
| Official Site | 8080  | Public product site and account center |
| Master UI     | 3000  | Task dashboard |
| Worker UI     | 3001  | Local worker dashboard |
| Master API    | 8082  | HTTP API (auth, tasks) |
| Nodepool gRPC | 50051 | Worker registration |
| Worker gRPC   | 50053 | Task execution |
| Worker HTTP   | 18080 | Local worker control API |
| Redis         | 6379  | Session & cache |
| PostgreSQL    | 5432  | Persistent data |

### Docker Compose

```bash
make docker-up
make docker-logs
make docker-down
```

### Three-surface release

The release includes the Official Site/account center on port 8080, Master UI
on port 3000, and Worker UI on port 3001. Configuration, smoke checks, browser
tests, and troubleshooting are in [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md).

Use a disposable Windows environment. The smoke harness does not force a unique
Compose project; normal cleanup runs `docker compose down -v` and deletes project
volumes. Check project and volume ownership before running it. From the repository
root, build and leave the stack running:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-stack-smoke.ps1 -KeepRunning
```

In the browser-test shell, set `HIVEMIND_SITE_URL`, `HIVEMIND_MASTER_UI_URL`, and
`HIVEMIND_WORKER_UI_URL` to the URLs printed by the harness, then run:

```powershell
cd frontend; npm run test:e2e
```

The harness can allocate ephemeral host ports. The root stack contains one
Worker; default enforced task execution and settlement require three eligible
Workers in a separate test.

After verification, return to the repository root and run `docker compose down`.
Raw Compose requires supplied secrets and a matching Worker execution key pair;
see the runbook for configuration and cleanup details.

### Managed consensus

Nodepool assigns each deterministic CPU `managed-function-v1` task to three
distinct registered Workers by default. Two must return matching canonical results. Nodepool
persists a quorum certificate before publishing a successful task result and
does not fall back to single-Worker completion. Valid attempt-bound v1 usage
can still be billable for a failed or no-quorum task.

Consensus records agreement, not independently verified correctness. A colluding
or commonly compromised Worker majority can agree on an incorrect result. Only
deterministic, side-effect-free managed tasks are eligible.

For new v1 tasks, `max_cpt` is the total charge cap, including the 10% platform
fee and all replicas. Nodepool derives equal integer execution budgets from the
fee-exclusive cap. With a 100 CPT cap and three replicas, each receives 30 CPT
of execution budget. The hold is 99 CPT (90 usage + 9 fee); the remaining 1 CPT
stays with the owner. Settlement aggregates valid replica usage plus the fee,
never exceeds `max_cpt`, and refunds unused held CPT.

Automatic paid retries are disabled pending a funded platform treasury; retry
work is not charged to task owners. Historical per-replica billing remains as
recorded, without retroactive re-capping or refunds.

The default rollout is `enforce`. `observe` dispatches replicas and records shadow
state without publishing user output or settling. `disabled` turns off the
consensus path.

```text
MANAGED_CONSENSUS_ROLLOUT_MODE=enforce
MANAGED_CONSENSUS_REPLICA_COUNT=3
MANAGED_CONSENSUS_QUORUM=2
MANAGED_CONSENSUS_MAX_RESULT_BYTES=262144
```

### Manual

Prepare PostgreSQL, Redis, the platform JWT secret, execution keys, and the
selected role's configuration. When starting services, the `hivemind-bin` CLI
searches the working directory and its parents for `.env`; standalone Windows Master/Worker packages
load only the file beside the executable. Role-specific settings are in the
[project manual](docs/HIVEMIND_MANUAL.zh-TW.md).

With Nodepool configured, run from the repository root:

```powershell
cargo run --release --manifest-path hivemind-rs/Cargo.toml --bin hivemind-bin -- nodepool
```

This starts Nodepool only. `all` combines services for local use; production
separates the official control plane from user-deployed Master and Worker nodes.

## Configuration

These settings are for service deployments. Ordinary packaged Master and Worker
clients do not need the platform `JWT_SECRET` or a fixed Nodepool IP: authenticated
login handles VPN enrollment and Nodepool discovery through the overlay.

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | - | PostgreSQL connection string |
| `REDIS_URL` | - | Redis connection string |
| `JWT_SECRET` | - | JWT signing secret |
| `HIVEMIND_ADMIN_USERS` | unset | Comma-separated usernames allowed to access `/api/admin/*` endpoints |
| `HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE` | `60` | Master process-local per-owner 60-second submission window; `0` or a negative value blocks submissions |
| `WEBSITE_NODEPOOL_GRPC_ADDR` | `localhost:50051` | Server-side Nodepool target for the official website's account endpoints |
| `MASTER_HTTP_ADDR` | `0.0.0.0:8082` | Master HTTP listen address |
| `NODEPOOL_GRPC_ADDR` | `0.0.0.0:50051` | Nodepool gRPC listen/connect address |
| `WORKER_GRPC_ADDR` | `0.0.0.0:50053` | Worker gRPC listen address |
| `WORKER_ADVERTISE_ADDR` | - | Worker address registered with Nodepool |
| `EXECUTOR_SANDBOX_DIR` | `./sandbox` | Per-task working directory root |
| `MANAGED_CONSENSUS_ROLLOUT_MODE` | `enforce` | `disabled`, `observe`, or `enforce`; successful output requires a strict-majority certificate; valid v1 usage is validated separately for payment |
| `MANAGED_CONSENSUS_REPLICA_COUNT` | `3` | Distinct Worker replicas per managed task |
| `MANAGED_CONSENSUS_QUORUM` | `2` | Matching replicas required; must be a strict majority |
| `MANAGED_CONSENSUS_TIMEOUT_SECS` | `120` | Deadline for one consensus round |
| `MANAGED_CONSENSUS_MAX_RESULT_BYTES` | `262144` | Nodepool/Worker cap for one consensus result |
| `LOG_LEVEL` | `info` | Log level (debug, info, warn, error) |

## API Reference

### Authentication

```bash
curl -X POST http://localhost:8082/api/login \
  -H "Content-Type: application/json" \
  -d '{"username": "<username>", "password": "<password>"}'
```

### Tasks

```bash
curl -X POST http://localhost:8082/api/tasks \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "task_id": "task-1",
    "runtime": "managed-function-v1",
    "task_source": "fn sum(values) {\n  return get(values, \"a\") + get(values, \"b\");\n}\nsum(input);\n",
    "torrent": "{\"a\": 1, \"b\": 2}",
    "memory_gb": 4,
    "cpu_score": 100,
    "storage_gb": 10,
    "max_cpt": 1000
  }'

curl http://localhost:8082/api/tasks \
  -H "Authorization: Bearer <token>"
```

### Admin Observability

```bash
curl "http://localhost:8082/api/admin/scheduling/cache-alert?low=0.5&high=2.0" \
  -H "Authorization: Bearer <admin-token>"

curl "http://localhost:8082/api/admin/scheduling/cache-anomalies?limit=100" \
  -H "Authorization: Bearer <admin-token>"

curl "http://localhost:8082/api/admin/audit/logs?limit=100" \
  -H "Authorization: Bearer <admin-token>"
```

### Health Check

```bash
curl http://localhost:8082/health
```

## License

Hivemind is licensed under the MIT [LICENSE](LICENSE). Third-party dependencies,
vendored components, and packaged images retain their own licenses and required
notices.

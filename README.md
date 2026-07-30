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

If you want to upload a task and write the task program, start here:

- [docs/user-task-guide.md](docs/user-task-guide.md)

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
    "task_source": "def main(input):\n    return {\"sum\": input[\"a\"] + input[\"b\"]}\n",
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
```

### Health Check

```bash
curl http://localhost:8082/health
```

## License

MIT

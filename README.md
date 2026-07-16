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
- **Frontend** (`frontend/`) - React UIs for master and worker management
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
│   ├── torrent-service/- ZIP to BitTorrent metainfo conversion and swarm helpers
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


## Multi-Host Role Binaries

Build feature-gated role binaries from the Rust workspace:

```bash
cd hivemind-rs
cargo build --release --no-default-features --features worker --bin hivemind-worker
cargo build --release --no-default-features --features master --bin hivemind-master
cargo build --release --no-default-features --features nodepool --bin hivemind-nodepool
```

For multi-host deployments, set bind and advertise endpoints separately (`WORKER_ADVERTISE_ADDR`, `NODEPOOL_GRPC_ENDPOINT`, `TORRENT_SEED_ADVERTISE_HOST`). Backend authorization, not UI checks, defines privilege. Workers must receive `WORKER_EXECUTION_SECRET` and must not receive control-plane `JWT_SECRET`. Prefer `docker-compose.yml` for the canonical multi-service topology.


### Docker Compose

Copy `.env.example` to `.env`, replace every `replace-me` value, and provision
the worker registration account before starting the worker. The worker image
contains the pinned Monty runtime and serves its UI from the same process; the
master image likewise serves the master UI.

```bash
# Start all services
make docker-up

# View logs
make docker-logs

# Stop all services
make docker-down
```

`JWT_SECRET` is the control-plane signing secret shared by master and nodepool.
`WORKER_EXECUTION_SECRET` is a distinct secret shared only by nodepool and
workers for worker RPC tokens. Both must be explicit, non-default values of at
least 32 bytes. The Compose worker gRPC port binds to loopback by
default. For a multi-host deployment, set `WORKER_GRPC_BIND_HOST` to the
specific host interface that should accept worker gRPC traffic and set
`WORKER_ADVERTISE_ADDR` to the same worker's routable `host:port` endpoint.
Also set `NODEPOOL_GRPC_ENDPOINT`, `TORRENT_ANNOUNCE_URL`, and
`TORRENT_SEED_ADVERTISE_HOST` to routable addresses. Keep nodepool/worker
traffic on a private network or VPN; public Internet TLS/mTLS termination is
still an operator responsibility.

### Manual

```bash
# Start Redis
docker run -d -p 6379:6379 redis:7-alpine

# Start PostgreSQL
docker run -d -p 5432:5432 \
  -e POSTGRES_DB=hivemind \
  -e POSTGRES_USER=hivemind \
  -e POSTGRES_PASSWORD=hivemind \
  postgres:16-alpine

# Run Hivemind
DATABASE_URL=postgres://hivemind:hivemind@localhost:5432/hivemind \
REDIS_URL=redis://localhost:6379 \
./target/release/hivemind-bin all
```

## Configuration

Configuration is via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | - | PostgreSQL connection string |
| `REDIS_URL` | - | Redis connection string |
| `JWT_SECRET` | - | Master/user control-plane JWT signing secret |
| `WORKER_EXECUTION_SECRET` | - | Nodepool/worker execution-token signing secret |
| `HIVEMIND_ADMIN_USERS` | unset | Comma-separated usernames allowed to access `/api/admin/*`; configured names are reserved from public registration |
| `HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE` | `60` | Per-user task submission rate limit for a rolling 1-minute window (`0` disables limiting) |
| `MASTER_HTTP_ADDR` | `0.0.0.0:8082` | Master HTTP listen address |
| `NODEPOOL_GRPC_ADDR` | `0.0.0.0:50051` | Nodepool gRPC listen/connect address |
| `WORKER_GRPC_ADDR` | `0.0.0.0:50053` | Worker gRPC listen address |
| `WORKER_ADVERTISE_ADDR` | - | Worker address registered with nodepool |
| `TORRENT_API_DIR` | `./api/torrents` | Nodepool seed directory for uploaded task packages |
| `TORRENT_BT_DIR` | `./bt_torrents` | Generated `.torrent` output directory |
| `TORRENT_ANNOUNCE_URL` | `http://localhost:6969/announce` | Tracker announce URL embedded in magnets/torrents (workers must reach this) |
| `TORRENT_TRACKER_LISTEN_ADDR` | `0.0.0.0:6969` | Nodepool HTTP tracker listen address |
| `TORRENT_SEED_LISTEN_ADDR` | `0.0.0.0:6881` | Nodepool BitTorrent seed listen address |
| `TORRENT_SEED_ADVERTISE_HOST` | unset | Optional host/IP advertised to workers for the nodepool seeder |
| `TORRENT_TASK_ARTIFACT_BASE_URL` | unset | Legacy optional HTTP artifact base URL; primary package distribution is now nodepool BT seeding |
| `EXECUTOR_SANDBOX_DIR` | `./sandbox` | Per-task sandbox root |
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
    "torrent": "magnet:?xt=urn:btih:<info-hash>",
    "memory_gb": 4,
    "cpu_score": 100,
    "storage_gb": 10,
    "max_cpt": 25
  }'

# Upload a local ZIP as bytes (JSON task creation rejects server-local paths)
curl -X POST http://localhost:8082/api/tasks/upload \
  -H "Authorization: Bearer <token>" \
  -F "task_id=task-zip-1" \
  -F "file=@./task/windows_dist/out/task.zip" \
  -F "memory_gb=4" \
  -F "cpu_score=100" \
  -F "storage_gb=10" \
  -F "max_cpt=25"

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


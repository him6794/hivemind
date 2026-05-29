# HiveMind-RS — Rust Rewrite

Complete Rust rewrite of the HiveMind distributed compute runtime.

## Architecture

```
hivemind-rs/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── common/             # Tracing, error types, utilities
│   ├── proto/              # gRPC protobuf definitions (tonic)
│   ├── models/             # Domain models (User, Task, WorkerNode, etc.)
│   ├── config/             # Configuration loading (env + file)
│   ├── database/           # PostgreSQL + Redis persistence layer
│   ├── auth/               # JWT authentication (bcrypt + jsonwebtoken)
│   ├── node-manager/       # Worker registration, heartbeat, cleanup
│   ├── task-scheduler/     # Task dispatch, redispatch, timeout handling
│   ├── master-api/         # HTTP API server (axum)
│   ├── worker-executor/    # Sandbox execution, resource monitoring
│   ├── vpn-service/        # WireGuard VPN peer management
│   └── hivemind-bin/       # Binary entry point (wires all services)
```

## Quick Start

```bash
# Build
cargo build

# Run all services
cargo run --bin hivemind-bin -- all

# Run specific service
cargo run --bin hivemind-bin -- master
cargo run --bin hivemind-bin -- nodepool
cargo run --bin hivemind-bin -- worker
```

## Testing

```bash
# All tests (43 tests)
cargo test

# Specific crate
cargo test -p hivemind-master-api
cargo test -p hivemind-task-scheduler
cargo test -p hivemind-node-manager
```

## Test Coverage

| Crate | Tests | Coverage |
|-------|-------|----------|
| common | 0 | (utility only) |
| proto | 0 | (generated code) |
| models | 0 | (data structures) |
| config | 0 | (deserialization) |
| database | 6 | postgres migrations, redis keys, pool creation |
| auth | 2 | JWT roundtrip, invalid login |
| node-manager | 7 | heartbeat validation, worker upsert, status normalization |
| task-scheduler | 7 | dispatch, redispatch, worker selection, task CRUD |
| master-api | 6 | health, login, tasks CRUD, authorization, integration |
| worker-executor | 7 | sandbox lifecycle, resource monitoring |
| vpn-service | 8 | WireGuard config, IP allocation, peer management |
| **Total** | **43** | |

## Services

### Master API (HTTP :8082)
- `POST /api/login` — User authentication
- `GET /api/balance` — Check user balance
- `POST /api/tasks` — Upload task
- `GET /api/tasks` — List user tasks
- `GET /api/tasks/{id}/result` — Get task result
- `POST /api/tasks/{id}/stop` — Cancel task
- `GET /health` — Health check

### Nodepool (gRPC :50051)
- Worker registration and heartbeat
- Task dispatch loop (assigns pending tasks to workers)
- Timeout monitor (redispatches or fails timed-out tasks)
- Stale worker cleanup (marks offline workers)

### Worker (gRPC :50053)
- Task execution in sandboxed environment
- Resource monitoring (CPU, memory, GPU)
- Result upload

## Key Design Decisions

1. **All state in PostgreSQL** — No in-memory task maps. Tasks, workers, VPN peers all persisted.
2. **Resource lifecycle** — Sandbox dirs created per-task and cleaned up. DB connections pooled.
3. **Graceful shutdown** — All background loops accept `watch::Sender<bool>` shutdown signals.
4. **Input validation** — Heartbeat validates CPU/memory ranges, worker_id non-empty.
5. **Pull-based scheduling** — Workers pull tasks; nodepool dispatches based on capacity.

## Configuration

Environment variables:
- `DATABASE_URL` — PostgreSQL connection string
- `REDIS_URL` — Redis connection string
- `JWT_SECRET` — JWT signing secret
- `MASTER_HTTP_ADDR` — Master API bind address
- `NODEPOOL_GRPC_ADDR` — Nodepool gRPC bind address

## Dependencies

- **tokio** — Async runtime
- **axum** — HTTP framework
- **tonic** — gRPC framework
- **sqlx** — PostgreSQL driver
- **deadpool-redis** — Redis pool
- **jsonwebtoken** — JWT
- **bcrypt** — Password hashing
- **sysinfo** — System resource monitoring

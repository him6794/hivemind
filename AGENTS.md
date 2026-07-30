# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## What is Hivemind?

Hivemind is a distributed compute runtime for public-network workers. Users submit batch tasks as `managed-function-v0` source functions with a JSON input payload; the system schedules them across a pool of worker nodes and returns results. The backend is a single Rust binary (`hivemind-bin`) that can run as `master`, `nodepool`, `worker`, or `all` (colocated). Frontends are React SPAs.

## Common Commands

### Backend (Rust)

All Rust commands run from `hivemind-rs/`:

```bash
make build              # cargo build --release
make build-debug        # cargo build (debug)
make test               # cargo test
make test-verbose       # cargo test -- --nocapture
make lint               # cargo clippy -- -D warnings
make fmt                # cargo fmt
make dev                # build + start redis/postgres + cargo run --bin hivemind-bin -- all
make proto              # regenerate protobuf (cargo build -p hivemind-proto)
```

Run a single test:
```bash
cd hivemind-rs && cargo test <test_name>
```

### Frontend

Three separate frontend surfaces, each with its own `package.json`:

```bash
make build-frontend     # builds all three via scripts/build-release-frontends.ps1
make smoke-frontend     # smoke-test release artifacts
```

Individual frontends:
```bash
cd frontend && npm install && npm run build          # Official site (Next.js)
cd frontend/master-ui && npm install && npm run build # Master UI (Vite + React 18)
cd frontend/worker-ui && npm install && npm run build # Worker UI (Vite + React 18)
```

Dev servers:
```bash
cd frontend && npm run dev            # port 3000
cd frontend/master-ui && npm run dev  # port 3000
cd frontend/worker-ui && npm run dev  # port 3001
```

### Docker

```bash
make docker-up          # start full stack
make docker-down        # stop
make docker-logs        # tail logs
make docker-build       # build images
make db-reset           # destroy and recreate postgres + redis volumes
```

### Python build helper

```bash
python scripts/build_local.py --all          # build backend + frontends without Docker
python scripts/build_local.py --debug        # debug mode, skip release optimizations
```

## Architecture

### Runtime Topology

```
Client / UI
  → Master HTTP API (8082)      — HTTP-to-gRPC proxy, no direct DB access
    → Nodepool gRPC (50051)     — owns DB, auth, scheduling, node management
  → Worker gRPC (50053)         — task execution, result reporting
  → Worker Control HTTP (18080) — local worker status/config endpoint
```

`hivemind-bin` runs one or more of these roles based on the CLI argument (`master`, `nodepool`, `worker`, `all`). In `all` mode everything is colocated in one process.

### Crate Layout (`hivemind-rs/crates/`)

| Crate | Responsibility |
|---|---|
| `hivemind-bin` | Entry point, CLI parsing, service wiring |
| `config` | `HivemindConfig` — loads from env vars or JSON file (`HIVEMIND_CONFIG`) |
| `proto` | Generated gRPC code from `proto/hivemind.proto` |
| `models` | Shared domain types |
| `database` | PostgreSQL (sqlx) + Redis (deadpool-redis) access, migrations |
| `auth` | JWT service, registration, login, bcrypt password handling |
| `node-manager` | Worker registration, heartbeat tracking, trust/liveness state |
| `task-scheduler` | Task dispatch, redispatch, timeout loops |
| `master-api` | Axum HTTP handlers, proxies to nodepool gRPC |
| `worker-executor` | Managed-function task execution, resource tracking, worker control API |
| `vpn-service` | Headscale-based VPN peer management |
| `common` | Tracing init, shared error types, helpers |

### Frontend Structure (`frontend/`)

| Directory | Tech | Purpose |
|---|---|---|
| `frontend/` (root) | Next.js 16, React 19, Tailwind v4 | Official site, account center, docs |
| `frontend/master-ui/` | Vite, React 18 | Task submission, API keys, dashboard |
| `frontend/worker-ui/` | Vite, React 18 | Worker node status, task queue, earnings |

### Key Data Flow

1. User submits a task (HTTP POST or CLI `submit`) → Master API → Nodepool gRPC
2. Nodepool persists task to Postgres, dispatches to an available worker via gRPC
3. Worker runs the `managed-function-v0` source with its JSON input in the managed-function runtime
4. Worker reports results back via gRPC → Nodepool marks task complete
5. Frontend polls Master API for task status/results

### Configuration

`HivemindConfig` (in `crates/config`) loads from:
1. JSON file if `HIVEMIND_CONFIG` env var points to one
2. Otherwise, individual env vars (see `.env.example` for the full list)

Key env vars: `DATABASE_URL`, `REDIS_URL`, `JWT_SECRET`, `NODEPOOL_GRPC_ADDR`, `MASTER_HTTP_ADDR`, `WORKER_GRPC_ADDR`, `WORKER_CONTROL_HTTP_ADDR`

`JWT_SECRET` must not be the default `CHANGE_ME_IN_PRODUCTION` — the binary refuses to start with that value.

### Proto Contracts

- `proto/hivemind.proto` — main gRPC surface (user, node, master, batch-runtime, VPN, worker services)
- `proto/vpn.proto` — VPN-specific messages
- Generated code is included via `tonic::include_proto!("nodepool")` in `crates/proto/src/lib.rs`
- Regenerate with `make proto`

### Testing

- Rust tests: `make test` (runs `cargo test` across the workspace)
- Frontend tests: `cd frontend/master-ui && npm test` or `cd frontend/worker-ui && npm test`
- Test config uses `HivemindConfig::for_test()` which defaults to `hivemind_test` database
- Set `HIVEMIND_TEST_DATABASE_URL` to override the test database connection string

## Trust Model (from AGENT.md — load-bearing)

- **Nodepool** is the only trusted authority. It owns account state, balances, task state, worker registration, scheduling, and billing.
- **Master** and **Worker** nodes are user-deployed and therefore untrusted. Nodepool identifies callers solely by validating tokens it issued itself.
- Worker results, resource usage, and billing figures are *claims*, not facts — nodepool verifies them server-side.
- The official website must not become a task/worker operations console. Task submission and worker control belong in Master UI and Worker UI respectively.
- The website backend calls nodepool server-side via `WEBSITE_NODEPOOL_GRPC_ADDR`; nodepool must never be exposed directly to browsers.

See `AGENT.md` for the full trust model, deployment model, and product boundary rules.

## Workflow Conventions

- Commit per feature, not in bulk. Run tests before committing.
- Commit to local repo only — do not push unless explicitly asked.
- Use Conventional Commits: `feat(scope):`, `fix(scope):`, `docs(scope):`, `test(scope):`, `refactor(scope):`, etc.
- Keep commits focused enough to revert individually.

## Platform Notes

- Primary development is on Windows (PowerShell); Makefile targets use `powershell -NoProfile -ExecutionPolicy Bypass` for frontend build scripts
- Docker Compose runs the full production-like stack
- The `scripts/build_local.py` helper works on both Windows and Linux for building without Docker

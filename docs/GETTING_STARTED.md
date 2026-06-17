# HiveMind Getting Started

## Prerequisites

- Rust stable toolchain
- PostgreSQL
- Redis
- Node.js 18+ for the React frontends

## Configure

Set the core runtime variables before starting the services:

- `DATABASE_URL`
- `REDIS_URL`
- `JWT_SECRET`
- `NODEPOOL_GRPC_ADDR`
- `MASTER_HTTP_ADDR`
- `WORKER_GRPC_ADDR`
- `WORKER_CONTROL_HTTP_ADDR`

## Build

```bash
cd hivemind-rs
cargo build --release
```

```bash
cd frontend/master-ui
npm install
npm run build

cd ../worker-ui
npm install
npm run build
```

## Run

The repository root `README.md` documents the current development and runtime
entry points.

```bash
make dev
```

For a manual run, set the database, Redis, and JWT environment variables, then
start `hivemind-bin` in the mode you need (`all`, `master`, `nodepool`, or
`worker`).

## Verify

```bash
curl http://localhost:8082/health
```

## Next Steps

- Read `docs/ARCHITECTURE.md` for the current workspace layout
- Use `make test` for the main Rust workspace test pass
- Use `make build-frontend` to build both React apps

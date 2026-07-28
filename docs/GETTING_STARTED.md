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
cd frontend
npm install
npm run build

cd master-ui
npm install
npm run build

cd ../worker-ui
npm install
npm run build
```

If you prefer repository-level commands, the root `Makefile` also provides:

```bash
make build-frontend
```

This builds all three release surfaces:

- `frontend/` - official site
- `frontend/master-ui/` - customer dashboard
- `frontend/worker-ui/` - worker console

## Run

The repository root `README.md` documents the current development and runtime
entry points.

```bash
make dev
```

For a manual run, set the database, Redis, and JWT environment variables, then
start `hivemind-bin` in the mode you need (`all`, `master`, `nodepool`, or
`worker`).

To run the full local release stack, including the official site and both app
surfaces:

```bash
docker compose up --build
```

The default local ports are:

- `http://localhost:8080` - official site
- `http://localhost:3000` - master UI
- `http://localhost:3001` - worker UI
- `http://localhost:8082` - master API
- `http://localhost:18080` - worker control API

## Verify

```bash
curl http://localhost:8082/health
```

For frontend release verification:

```bash
$env:WEBSITE_NODEPOOL_GRPC_ADDR = "127.0.0.1:50051"
$env:VITE_API_BASE = "http://127.0.0.1:8082"
$env:VITE_WORKER_CONTROL_BASE = "http://127.0.0.1:18080"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-frontend-smoke.ps1
```

## Next Steps

- Read `docs/ARCHITECTURE.md` for the current workspace layout
- Use `make test` for the main Rust workspace test pass
- Use `make build-frontend` to build all three frontend surfaces

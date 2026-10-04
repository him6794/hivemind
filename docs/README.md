# HiveMind Docs

This directory contains the current documentation for the Rust workspace under
`hivemind-rs`.

## Start Here

- [Architecture](ARCHITECTURE.md)
- [Getting Started](GETTING_STARTED.md)
- [Managed Function Runtime](MANAGED_FUNCTION_RUNTIME.md)
- [Utility and Performance Evaluation](UTILITY_PERFORMANCE_EVALUATION.md)
- [Smoke Benchmarks](SMOKE_BENCHMARKS.md)
- [Public Network Limitations](PUBLIC_NETWORK_LIMITATIONS.md)

## Workspace Snapshot

HiveMind is organized around these runtime pieces:

- `hivemind-bin` for process startup and service composition
- `master-api` for the external HTTP API
- `node-manager` and `task-scheduler` for worker state and dispatch
- `worker-executor` for managed-function execution and worker control
- `vpn-service` for worker connectivity
- `database`, `auth`, `config`, `models`, `common`, and `proto` as shared
  support crates

## Build Entry Points

```powershell
cd hivemind-rs
cargo build
cargo test
```

Frontend builds live under:

- `frontend` for the official site and account center
- `frontend/master-ui`
- `frontend/worker-ui`

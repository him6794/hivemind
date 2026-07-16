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
- `worker-executor` for sandboxed execution and worker control
- `vpn-service` and `torrent-service` for connectivity and artifact support
- `database`, `auth`, `config`, `models`, `common`, and `proto` as shared
  support crates

## Build Entry Points

```bash
cd hivemind-rs
cargo build
cargo test
```

Frontend builds live under:

- `frontend/master-ui`
- `frontend/worker-ui`

## Historical Notes

Older mixed-language notes were moved to
`docs_backup_20260611_202024/`. They are kept as archive material and are not
the source of truth for the current Rust workspace.

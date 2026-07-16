# PROJECT KNOWLEDGE BASE

## OVERVIEW
HiveMind is a Rust distributed task runtime with nodepool coordination, master API, worker execution, React/Vite UIs, and an optional Cloudflare artifact service.

## STRUCTURE
- `hivemind-rs/` — authoritative Rust workspace and role binaries
- `proto/` — protobuf contracts
- `executor-rs/` — managed-function runtime dependency
- `frontend/` — master and worker React/Vite applications
- `scripts/` — packaging and operational tooling
- `docker-compose.yml` — canonical Rust deployment topology
- `infra/` — legacy deployment definitions; do not assume current
- `cloudflare/` — separate update/artifact service

## WHERE TO LOOK
| Task | Location |
|---|---|
| Role startup | `hivemind-rs/crates/hivemind-bin/src/lib.rs` |
| Protocol | `proto/*.proto`, `hivemind-rs/crates/proto` |
| API/auth | `hivemind-rs/crates/master-api`, `node-manager` |
| Worker execution | `hivemind-rs/crates/worker-executor` |
| Deployment | `docker-compose.yml`, `.env.example`, `hivemind-rs/Dockerfile` |
| UI | `frontend/master-ui`, `frontend/worker-ui` |
| Release | `Makefile`, `docker-compose.yml`, feature-gated `cargo build` |

## CONVENTIONS
- Rust is the backend source of truth; do not revive the missing `services/` legacy tree.
- Keep feature-gated role builds independently compilable: `master`, `nodepool`, `worker`.
- Bind and advertised endpoints are distinct; remote deployments require routable endpoint configuration.
- Backend authorization, not UI checks or CORS, defines privilege.
- Preserve unrelated dirty-worktree changes; never reset or checkout wholesale.

## ANTI-PATTERNS
- No unauthenticated public worker control endpoint.
- No wildcard CORS or public database/Redis exposure in production.
- No path joins/deletes from unvalidated task IDs.
- No secrets, tokens, or weak credentials in release artifacts.
- No claim of release readiness from compilation alone.
- Do not use `infra/` or `task/windows_dist/` as Rust sources without reconciliation.

## COMMANDS
```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo build --release --no-default-features --features worker --bin hivemind-worker
cargo build --release --no-default-features --features master --bin hivemind-master
cargo build --release --no-default-features --features nodepool --bin hivemind-nodepool
npm ci; npm run build; npm test
```

## NOTES
- Env overrides apply after `HIVEMIND_CONFIG` JSON load; invalid numeric env values fail closed.
- Workers need the managed runtime and reachable nodepool/torrent topology.
- UI assets must be version-compatible with the backend serving them.
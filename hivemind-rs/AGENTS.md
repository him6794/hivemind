# RUST WORKSPACE KNOWLEDGE

## OVERVIEW
Feature-gated Rust services share protobuf models, config, database/auth, scheduling, node management, torrent/VPN services, and worker execution.

## WHERE TO LOOK
- `crates/hivemind-bin` — binaries and startup wiring
- `crates/config` — env/JSON configuration and production validation
- `crates/master-api` — HTTP routes, auth middleware, gRPC client
- `crates/node-manager` — nodepool gRPC and owner/admin checks
- `crates/worker-executor` — sandbox, executor, worker control API
- `crates/proto` — generated tonic/prost bindings

## CONVENTIONS
- Run format/check/tests for the workspace after cross-crate changes.
- Keep public API errors contextual and typed; do not hide startup failures.
- Treat bind and advertise addresses as separate contracts.
- Add a focused unit test before broad integration tests for each behavior.

## ANTI-PATTERNS
- Never weaken JWT, owner, admin, path, or egress checks to make tests pass.
- Do not expose control or gRPC services without an explicit security boundary.
- Do not add dependencies that break role-isolated builds.
- Do not use `unwrap`/`expect` in library production paths.

## COMMANDS
```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
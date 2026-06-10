# Agent Guidelines

These instructions apply to the entire repository unless a more specific
`AGENTS.md` exists in a subdirectory.

## Project Overview

Hivemind is a Rust-based distributed compute runtime with React frontends.
The main system is implemented under `hivemind-rs/` as a Cargo workspace and
ships a single `hivemind-bin` binary that can run the master API, nodepool, and
worker services.

Important areas:

- `hivemind-rs/`: Rust workspace for services, domain models, gRPC, scheduling,
  persistence, worker execution, VPN, and the binary entry point.
- `proto/`: Protobuf contracts shared by services.
- `frontend/master-ui/`: React UI for master/admin workflows.
- `frontend/worker-ui/`: React UI for worker/provider workflows.
- `scripts/`: Release and packaging scripts.
- `docs/`: Durable project documentation.
- `test_logs/`, `dist/`, `target/`, `node_modules/`: generated or local
  artifacts; avoid editing manually unless the task explicitly requires it.

## Development Commands

Run commands from the repository root unless noted otherwise.

```powershell
make build
make test
make lint
make fmt
make build-frontend
```

Rust-only commands:

```powershell
cd hivemind-rs
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt
```

Frontend commands:

```powershell
cd frontend/master-ui
npm run build

cd ../worker-ui
npm run build
```

Windows worker packaging:

```powershell
.\scripts\package-worker-windows.ps1
```

## Change Guidelines

- Prefer small, focused changes that follow the existing crate and module
  boundaries.
- Keep protobuf changes synchronized between `proto/hivemind.proto` and the
  Rust code that consumes generated types.
- Do not commit secrets or local credentials. Treat `.env` as local-only.
- Do not hand-edit dependency folders such as `node_modules/`.
- Do not hand-edit Rust build output under `target/`.
- Do not remove tracked files unless the task is explicitly cleanup or the file
  is demonstrably obsolete.
- Preserve user work in the working tree. If unrelated changes exist, leave
  them untouched.
- Use ASCII for new files unless existing project content requires otherwise.

## Rust Conventions

- Use async Rust patterns already present in the workspace.
- Keep shared domain types in `crates/models`.
- Keep HTTP API behavior in `crates/master-api`.
- Keep gRPC worker/nodepool integration in `crates/node-manager`,
  `crates/task-scheduler`, and `crates/proto` as appropriate.
- Keep executor-specific sandbox, resource, and task execution behavior in
  `crates/worker-executor`.
- Add or update focused tests for behavioral changes.
- Run `cargo fmt` after Rust edits.

## Frontend Conventions

- Follow the existing React structure in each UI package.
- Keep master/admin behavior in `frontend/master-ui`.
- Keep provider/worker behavior in `frontend/worker-ui`.
- If changing built frontend output is required by the repository workflow,
  rebuild with `npm run build` instead of manually editing `dist/` assets.
- Avoid broad restyling unless the task is specifically about UI design.

## Testing Expectations

Choose verification based on the touched surface:

- Rust API, scheduler, nodepool, worker, or model changes: run
  `cd hivemind-rs; cargo test`.
- Rust lint-sensitive refactors: run `cd hivemind-rs; cargo clippy -- -D warnings`.
- Frontend changes: run the package build for the changed UI.
- Packaging changes: run or dry-review `scripts/package-worker-windows.ps1`
  and document any command that could not be executed.

If a test cannot be run because required services or tools are unavailable,
state that clearly in the final response.

## Git Hygiene

- Use Conventional Commit style when committing:
  `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, or `chore:`.
- Before committing, check `git status --short` and review the staged diff.
- Keep generated logs, temporary scripts, and local artifacts out of commits.
- Do not rewrite history or discard changes unless explicitly requested.

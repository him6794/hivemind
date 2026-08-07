# Platform validation state

## Goal

Validate the managed-function-only Hivemind platform end to end, fix discovered regressions, and commit focused changes locally without pushing.

## Status

complete

## Completed validation

- Managed function runtime: 15 passed, 0 failed.
- GNU backend workspace: 243 passed, 0 failed, including doc tests.
- Site: 13 tests passed and Next.js production build passed.
- Master UI: 14 tests passed and Vite production build passed.
- Worker UI: 10 tests passed and Vite production build passed.
- PowerShell release contracts: all 8 `scripts/*.Tests.ps1` files passed.
- Release frontend preview smoke: official site, Master UI, and Worker UI passed; cleanup releases ports 4173-4175.
- Release Docker stack smoke: official site, Master UI, Worker UI, Master API, and Worker Control passed on collision-free host ports.
- Playwright release flow: 2 passed, covering account registration/login/logout, worker registration, task cancellation/completion, log/result inspection, artifact download, and controlled failure surfaces.
- Rust gates passed: `cargo fmt --all -- --check`, GNU workspace `cargo check`, and GNU all-target/all-feature `cargo clippy -D warnings`.

## Regressions fixed

- Managed task cancellation now remains responsive and cooperatively stops blocking managed execution.
- Release stack host ports and named volumes are configurable; smoke runs use isolated volumes and collision-free ports.
- Dynamic Master/Worker UI ports are reflected in API bases and CORS allowlists.
- Billing-aware E2E fixtures now submit affordable quoted tasks while retaining an unschedulable cancellation case.
- Windows frontend smoke cleanup terminates the full npm/Node preview process tree.

## Cleanup

- All `hivemind-smoke-20260807-*` validation containers and isolated Docker volumes were removed.
- The native validation PostgreSQL server on `127.0.0.1:3240` was stopped.
- `D:\hivemind-validation-postgres-20260807` remains as an inactive data directory because the command safety policy rejected recursive removal. It contains validation-only data and can be deleted manually.

## Constraints

- Windows Rust validation uses target `x86_64-pc-windows-gnu` because the vendored tailscale archive is MinGW-compatible.
- No changes were pushed and no pull request was created.

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
- Windows ARM64 cross-target check: `cargo check --target aarch64-pc-windows-msvc --workspace` passed under the VS arm64 dev environment (2026-08-23), proving the whole workspace compiles for ARM64 Windows.
- Linux target check: `cargo check --target x86_64-unknown-linux-gnu -p hivemind-client-core` passes; full-workspace Linux/macOS checks stay blocked in this environment because no `x86_64-linux-gnu-gcc` toolchain exists for the `ring` build script. This is a local toolchain blocker, not a source-compatibility failure.
- PowerShell release contracts: all 11 `scripts/*.Tests.ps1` files pass, including zero-config package assertions (no required endpoint/Worker-ID/token settings, session-only default documented) and the zh-tw architecture doc contract.

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

- Windows Rust builds now keep the MinGW static archive on `x86_64-pc-windows-gnu` and use an ABI-neutral dynamically loaded `libtailscale.dll` on `x86_64-pc-windows-msvc`. The MSVC package ships the DLL beside the executable and fails closed when it is absent or missing required exports.
- The MSVC build was verified locally with `cargo build --release --locked --target x86_64-pc-windows-msvc -p hivemind-bin --bins`; this proves compilation/linking and CLI startup, not a live VPN or Windows HCS isolation run.
- An ARM64 `libtailscale.dll` and `aarch64-pc-windows-msvc` worker executable were built and validated as `IMAGE_FILE_MACHINE_ARM64`. The package includes required native exports, no undeployed MinGW DLL dependencies, provenance, and checksums. This proves compilation and static package validation, not live VPN or Windows HCS isolation.

## Authenticated local enrollment slice

The current Windows Master/Worker startup path now supports both operator and
interactive enrollment without weakening the trust boundary:

- `MASTER_VPN_AUTHKEY` and `WORKER_VPN_AUTHKEY` remain optional explicit
  operator-provisioned paths and fail closed when VPN or Nodepool readiness does
  not complete.
- Without a role auth key, the local UI/control surface remains available. An
  authenticated local session calls the protected Rust Website API
  `POST /api/vpn/config` through `WEBSITE_API_BASE` (or the role-specific
  override), consumes the one-time Headscale key in process memory, and waits
  for the Nodepool gRPC protocol probe before enabling remote operations or
  registration.
- Persisted libtailscale state is attempted before issuing another enrollment
  key. The nonsecret device marker is role-scoped; passwords, `HEADSCALE_API_KEY`,
  reusable Headscale keys, and raw one-time keys are not persisted or returned by
  local status routes.
- The Website API deployment used by downloaded clients must be the Rust API
  exposing `/api/login` and protected `/api/vpn/config`; the official Next BFF
  must not be assumed to provide the VPN route.

## Remaining formal gates

- Formal external Headscale evidence still requires protected Website API
  enrollment credentials and an online overlay peer. Local Compose, Docker,
  WSL, SSH, socat, or direct-host reachability are not substitutes for that
  evidence.
- The required external flow remains to be demonstrated with Master and Worker
  on a suitable host separate from Orange Pi: enrollment, worker registration,
  quote, task execution, proof verification where the provider is supported,
  result/log retrieval, usage, billing, settlement, and audit evidence.
- Native Windows managed proving remains fail-closed because the approved RISC
  Zero prover is Linux x86_64 and no validated native Windows or ARM64 provider
  has been accepted yet.
- Automatic client update/download remains deferred.

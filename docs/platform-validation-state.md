# Platform validation state

## Goal

Validate the managed-function-only Hivemind platform end to end, fix discovered regressions, and commit focused changes locally without pushing.

## Status

running

## Completed validation

- Managed function runtime: 15 passed, 0 failed.
- GNU backend workspace: 243 passed, 0 failed, including doc tests.
- Site: 13 tests passed and Next.js production build passed.
- Master UI: 14 tests passed and Vite production build passed.
- Worker UI: 10 tests passed and Vite production build passed.
- PowerShell release contracts: all 8 `scripts/*.Tests.ps1` files passed.
- Release frontend preview smoke: official site, Master UI, and Worker UI passed; cleanup releases ports 4173-4175.
- Release Docker stack smoke: official site, Master UI, Worker UI, Master API, and Worker Control passed on collision-free host ports.
- Frontend browser smoke: hosted smoke passed 3/3. The release flow account registration/login/logout, Worker registration, task cancellation, managed completion, log inspection, artifact download, and controlled missing-artifact checks passed. The first single-Worker run stayed PENDING for 120 seconds as required by enforce mode; a separate isolated run with three distinct Workers then settled through quorum without a single-Worker fallback.
- Rust gates passed: `cargo fmt --all -- --check`, GNU workspace `cargo check`, and GNU all-target/all-feature `cargo clippy -D warnings`.
- Windows ARM64 cross-target check: `cargo check --target aarch64-pc-windows-msvc --workspace` passed under the VS arm64 dev environment (2026-08-23), proving the whole workspace compiles for ARM64 Windows.
- Linux target check: `cargo check --target x86_64-unknown-linux-gnu -p hivemind-client-core` passes; full-workspace Linux/macOS checks stay blocked in this environment because no `x86_64-linux-gnu-gcc` toolchain exists for the `ring` build script. This is a local toolchain blocker, not a source-compatibility failure.
- PowerShell release contracts: all 11 `scripts/*.Tests.ps1` files pass, including zero-config package assertions (no required endpoint/Worker-ID/token settings, session-only default documented) and the zh-tw architecture doc contract.
- Managed consensus settlement is covered by local dispatcher and repository tests for replica fan-out, quorum certificates, persistence, and fail-closed no-quorum behavior. A live local Docker deployment also settled a managed task with three distinct Workers, a 2-of-3 quorum certificate, replicated evidence, and nodepool-authorized billing.

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
- The Windows package now accepts an operator-built signed HCS runtime bundle beside the Worker executable. The package copies its manifest, guest image, and custom runner into a fixed relative path and includes them in the release inventory; the public template does not require a backend registry, image path, runner path, or execution setting. Missing or invalid bundle material leaves Windows general-compute unavailable.

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
  on a suitable host separate from Orange Pi: enrollment, Worker registration,
  quote, task execution, multi-Worker quorum certificate, result/log retrieval,
  usage, billing, settlement, and audit evidence.
- Native Windows Worker packaging, login-driven registration, managed execution,
  and local multi-Worker quorum settlement are now proven on this host. A
  clean-host run over the real external Headscale/VPN path is still required;
  missing readiness or quorum must continue to fail closed.
- Real Windows HCS guest execution and restart/recovery remain unproven.
- Automatic client update/download remains deferred.

## Historical release-gate recovery — 2026-09-11

This section records the release-gate state before the operator-owned OCI
registry was supplied. The current OCI result is recorded below.

The current dirty-tree recovery has fixed several local correctness contracts:

- Managed tasks without a persisted consensus policy remain held across
  `disabled`, `observe`, and `enforce` rollout transitions; persisted consensus
  tasks never silently downgrade to single-Worker completion.
- Direct Node Manager gRPC replica/quorum overrides use checked `u32`→`u16`
  conversion, and ingress runtime values are trimmed before persistence and
  classification.
- Artifact/chunk identity fields reject values above the 255-byte persistent
  bound before database or CAS use.
- Windows Worker packaging re-verifies the final executable, runtime files,
  checksums, and provenance before the package is written.

Current local evidence includes passing workspace-scoped rustfmt checks for both
Rust workspaces, the affected-crate suites (`hivemind-config` 29 passed,
`hivemind-proto` 23 passed, `hivemind-task-scheduler` 195 passed and 1
intentional ignored, `hivemind-worker-executor` 140 passed), the focused
consensus/readiness regressions, the general-compute runtime suite, and the
Windows packaging/verifier contracts. `git diff --check` also passes. The main
workspace dependency audit now exits successfully under the narrowly scoped
`hivemind-rs/.cargo/audit.toml` policy. Remaining warnings are recorded by the
dependency-audit document and are not vulnerabilities. A secret-shaped
content scan found no matches in untracked files; matches in tracked files are
limited to test/configuration fixtures and example names.

The following release gates remain blocked or not-run, and are not represented
as passing evidence:

- The OCI `-CheckOnly` harness fails closed because the operator-owned backend
  registry is not configured. Real `-Run` execution therefore was not run;
  rootless namespaces, cgroup v2, seccomp, deny-all networking, hostile
  workload behavior, and multi-process OCI settlement remain unproven.
- Native Windows live Worker execution remains not run against a clean host;
  login-driven enrollment, registration, managed execution, and quorum
  settlement still need direct end-to-end evidence on Windows itself.

## Local release validation — 2026-09-13

- `scripts/release-stack-smoke.Tests.ps1` passed after the smoke harness began
  creating its stack without starting services, seeding an empty registry only in
  harness-owned Worker volumes, and fixing mutable state-volume ownership.
- `scripts/release-stack-smoke.ps1 -CheckOnly` passed.
- A clean isolated Docker release stack started successfully on fixed browser
  ports. Official site, Master UI, Worker UI, Master API, and Worker Control all
  passed their health checks.
- `frontend` contract tests passed (20/20), hosted Playwright smoke passed (3/3),
  and the full release browser journey passed (2/2), including registration,
  login/logout, Worker registration, cancellation, managed completion, log
  inspection, artifact download, and controlled missing-artifact handling.
- The first single-Worker release attempt remained `PENDING` for 120 seconds;
  enforce mode correctly refused to settle without the configured replica set.
  A separate isolated validation topology then ran three distinct Workers
  (`release-consensus-a`, `release-consensus-b`, and `release-ui-worker`) with
  separate mutable volumes and completed the same managed flow without a
  single-Worker fallback.
- Direct task evidence for `qa-complete-mtzoe8au` reported `COMPLETED` with
  `replica_count=3`, `required_quorum=2`, `votes_received=2`,
  `evidence_level=replicated`, `mode=enforce`, and both certificate and
  nodepool settlement authorization present. The task output, log, and artifact
  download were verified by the browser journey.
- The Docker PostgreSQL-backed integration suite exited 0 with 525 passed and
  0 failed tests, including the managed consensus repository, node-manager,
  master API, and binary suites.
- `cargo test --workspace --all-targets --all-features -- --test-threads=1`
  exited 0 for the unified Rust workspace; no test failures were reported.

## Native Windows and OCI release-gate evidence — 2026-09-13

- The packaged native Windows Worker (`dist/windows-worker/hivemind-worker.exe`)
  started on this Windows host and served `/api/worker-info` from its local
  control API. The native `hivemind-bin.exe master` binary also started against
  the Docker PostgreSQL/Redis services and returned `OK` from `/health`.
- A user created through the official site logged in successfully through the
  native Master API; an authenticated native-Master balance request also passed.
- The native Worker completed the local login-driven Nodepool registration path
  with a matching account and execution public key. This local run used
  `WORKER_DISABLE_WEBSITE_VPN=1` and the Docker-published Nodepool endpoint, so
  it proves native startup, local login, and registration only; it is not
  evidence of Headscale/VPN enrollment.
- In task `native-win-consensus-918797bd5fae`, the native Worker was present in
  the three-replica assignment alongside two Docker Workers. The task completed
  with a 2-of-3 replicated certificate, but the two Docker Workers reached
  quorum before the native replica reported and the native replica was cancelled
  with `quorum reached`; that mixed topology run proved registration and
  assignment, but not a native vote.
- A fresh native-only local validation then ran three packaged native Windows
  Workers (`native-win-a-efb24a566a`, `native-win-b-efb24a566a`, and
  `native-win-c-efb24a566a`) against the local Nodepool. All three Workers
  received task `native-only-quorum-efb24a566a-5` and returned the same `3`
  result. The task completed with `replica_count=3`, `required_quorum=2`,
  `votes_received=2`, `matching_votes=2`, `evidence_level=replicated`, a
  certificate, `settlement_authorized=true`, and `billing_settled=true`.
  This proves native Worker consensus contribution and local settlement, but
  used `WORKER_DISABLE_WEBSITE_VPN=1` and the Docker-published Nodepool endpoint;
  it is not evidence of external Headscale/VPN enrollment or Windows HCS guest
  execution.
- An earlier native HCS gate was executed without Docker, WSL, a Linux VM, or
  direct process fallback. That run failed closed with exit code 2 because the
  Windows Containers optional feature was `Disabled` while `vmcompute` was
  running. A fresh host check on 2026-09-13 now reports Containers `Enabled`,
  `vmcompute` and HNS `Running`, and `hcsdiag.exe` present; the required signed
  package bundle with operator-owned Windows image and custom runner, plus an
  executing HCS harness, are still absent, so this is prerequisite evidence only
  and not HCS execution or recovery evidence.
- The OCI production `-Run` harness was invoked and failed closed with exit code
  1 because no operator-owned production backend registry was configured. No
  direct-host substitute was used; rootless OCI namespaces, cgroup v2, seccomp,
  deny-all networking, and hostile-workload cases remain unproven.
- A current release-stack browser registration/login smoke passed 1/1. The
  complete release browser journey remains covered by the earlier 2/2 evidence
  above; the current single-Worker stack was not represented as a new successful
  managed-task journey because enforce mode requires the configured replica set.
- The reviewed OCI production harness passed with the operator-owned registry,
  rootless policy, pinned runner/rootfs and seccomp material, and the staged
  Nodepool readiness gate. Evidence was written to
  `test_logs/general-compute-oci-e2e-final-192fa15aa85347c3a56eec30b869a4b3.json`
  with schema `general-compute-oci-e2e-v1` and status `passed`.
- The OCI evidence validates Worker registration, primary production execution,
  PostgreSQL typed-result persistence, and settlement. The primary result is a
  `general-compute-result-v1` envelope with execution `primary-execution`, the
  expected request digest, and a completed status.
- The same run passed the running-task cancellation case, including the
  `CANCELLED` task state and Nodepool-owned `task_cancelled` result envelope,
  plus the deny-all network and read-only filesystem hostile-workload cases,
  which returned the reviewed `FAILED`/`backend_failed` outcomes.
- The isolated OCI Compose project was cleaned up after the run; no host-process
  substitute or unsafe execution fallback was used.

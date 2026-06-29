## 2026-06-27 Final Verification Snapshot

### Hivemind-rs

- cargo fmt --check: PASS
- cargo clippy --workspace --all-targets --all-features -- -D warnings: PASS (clean)
- cargo audit: 1 new vuln: quinn-proto v0.11.14 (RUSTSEC-2026-0185, CVSS 7.5). Fix: >=0.11.15.
- cargo test --workspace --all-targets --all-features: 155 passed, 13 failed (all env-driven)

#### 13 Environment-Driven Failures (worker-executor)

6 storage: C: drive ~567MB free (< 1024MB needed). Sandbox uses C:\Users\user\AppData\Local\Temp.
4 assertion-mismatch: uncommitted executor.rs changes shift error messages.
3 stop-task timeouts: process timing on this Windows machine. Passed in prior rounds.

### Frontend

- master-ui: build PASS, 5 tests PASS, audit 0 vulns
- worker-ui: build PASS, 7 tests PASS, audit 0 vulns

### Executor-rs

- fmt PASS, clippy PASS, audit 6 allowed warnings, test all PASS

## 2026-06-28 Live Runtime Verification

- Built `monty-cli` and confirmed `monty.exe` at `executor-rs/target/debug/monty.exe`.
- Started a fresh all-mode instance on isolated ports `18082`, `15051`, `15053`, and `18180` with the local database/Redis, a non-default JWT secret, worker registration token, and `TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS=true`.
- Confirmed `GET /health` on master returned `200 OK`.
- Confirmed `GET /api/worker-info` on worker returned a live hardware profile and CORS did not reflect an arbitrary evil origin.
- Submitted three live ZIP tasks with distinct payloads: `live-hello-2`, `live-math-2`, and `live-text-2`.
- Retrieved all three task statuses as `COMPLETED`.
- Retrieved all three task results successfully.
- Found that the repository sample ZIP tasks `01_hello_world.zip`, `02_math_compute.zip`, and `03_text_processing.zip` fail at runtime because `__name__` is not predefined in Monty script globals.
- Re-ran the same kinds of tasks as minimal top-level scripts without `if __name__ == "__main__"` and confirmed successful execution with outputs `hello-live`, `10`, and `HIVEMIND LIVE TEST / 3`.
- Ran security probes: unsafe task ids were rejected client-side, and the worker control API did not enable wildcard CORS for an arbitrary origin.
- Re-ran `cargo fmt --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`; both passed.

## 2026-06-29 Finding 27 Repair

- Root cause confirmed with RED test: `cargo test -p monty --test main script_name_is_main_for_entrypoint_guard -- --nocapture` failed with `NameError: name '__name__' is not defined`.
- Fixed Monty runtime namespace initialization so script-level `__name__` defaults to `"__main__"` when referenced and remains overrideable by an explicit input.
- Added regression coverage in `executor-rs/crates/monty/tests/main.rs`.
- Rebuilt `monty-cli` and verified the three repository sample task scripts:
  - `test_tasks/01_hello_world/main.py`: exit 0, printed `Hello from Hivemind sample task`
  - `test_tasks/02_math_compute/main.py`: exit 0, printed `fib(20)=6765` and the prime list
  - `test_tasks/03_text_processing/main.py`: exit 0, printed `word_count=4` and uppercase text
- Verification passed:
  - `cargo test -p monty --test main`
  - `cargo test -p monty`
  - `cargo build -p monty-cli`
  - `cargo fmt --check` (exit 0; stable rustfmt still warns about nightly-only import options)
  - `cargo clippy -p monty --tests -- -D warnings`

## 2026-06-29 Finding 25 Repair

- Updated the Hivemind Rust lockfile from `quinn-proto 0.11.14` to patched `quinn-proto 0.11.15` for RUSTSEC-2026-0185.
- Verification passed:
  - `cd hivemind-rs; cargo audit`
  - `cd hivemind-rs; cargo build --workspace`

## 2026-06-29 Finding 26 Repair

- Updated the executor Rust lockfile from `memmap2 0.9.9` to `memmap2 0.9.11` for RUSTSEC-2026-0186.
- Verification passed:
  - `cd executor-rs; cargo audit` exits 0 and no longer reports the `memmap2` advisory; only the existing allowed `unic-*` unmaintained warnings remain.
  - `cd executor-rs; cargo build --workspace`

## 2026-06-29 Post-Commit Focused Cargo Verification

- Re-ran the focused Cargo tests that previously timed out while the disk was busy.
- Verification passed:
  - `cd hivemind-rs; cargo test -p hivemind-master-api zip_distribution_uses_remote_artifact_base_url_without_local_opt_in -- --nocapture`
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor resolve_task_source_downloads_http_artifact -- --nocapture`

## 2026-06-29 Managed Function Runtime Planning And MVP

- Added `docs/MANAGED_FUNCTION_RUNTIME.md` with the initial managed serverless-style executor plan, supported syntax v0, metering table, and billing direction.
- Added a new Rust crate `executor-rs/crates/managed-function-runtime` to the executor workspace.
- Followed RED/GREEN for the runtime API:
  - RED: `cargo test -p managed-function-runtime --test runtime` failed because `ExecutionLimits`, `ManagedExecutor`, `Status`, and `Value` did not exist.
  - GREEN: implemented a restricted Rust lexer/parser/evaluator with integer, boolean, string, `let`, `fn`, `return`, `print`, `if/else`, function calls, arithmetic, comparison, operation metering, output limits, and execution receipts.
- Verification passed:
  - `cd executor-rs; cargo test -p managed-function-runtime -- --nocapture`
  - `cd executor-rs; cargo clippy -p managed-function-runtime --all-targets -- -D warnings`
  - `cd executor-rs; cargo fmt` (exit 0; stable rustfmt still warns about nightly-only import options)

## 2026-06-29 Managed Runtime Expansion And Transpiler

- Continued the managed runtime delivery goal with `worklog/managed-function-runtime-state.md` as durable state.
- Used subagents for parallel work:
  - Mencius completed a read-only task-pipeline integration survey.
  - Boyle completed a read-only receipt-billing integration survey.
  - Carver produced a converter prototype; the main agent re-created and verified it in the real `D:\hivemind` workspace.
- Extended `managed-function-runtime` with JSON input, list/dict values, bounded `for`, stdlib functions `len/get/contains`, and source-location access on parse errors.
- Added `managed-function-transpiler` with conservative Python/C++ subset conversion and explicit unsupported-construct errors.
- Verification passed:
  - `cd executor-rs; cargo test -p managed-function-runtime -- --nocapture`
  - `cd executor-rs; cargo clippy -p managed-function-runtime --all-targets -- -D warnings`
  - `cd executor-rs; cargo test -p managed-function-transpiler -- --nocapture`
  - `cd executor-rs; cargo clippy -p managed-function-transpiler --all-targets -- -D warnings`

## 2026-06-29 Stale Finding Status Reconciliation

- Reconciled confirmed-finding statuses for repairs already present in the current worktree:
  - Findings 1-17: synced the Confirmed Findings section with the existing repair-stream evidence for default-account gating, VPN auth/scope, CORS allowlists, task-list mapping, billing aggregation, worker RPC/stop implementation, Rust lint gates, JWT defaults, Rust/JS/frontend audits, provider ownership, Monty JS smoke testing, sample task packaging, executor audit vulnerabilities, and executor all-target test gating.
  - Finding 18: static search of frontend source found no remaining `localStorage` / `hivemind_token` token persistence.
  - Finding 20: `scripts/package-worker-windows.Tests.ps1` passed and the generated launcher now uses `Import-DotEnv` with malformed/duplicate key rejection.
  - Finding 21: current task-scheduler code default-denies missing reputation rows and retains regression tests for trusted-worker filtering and claim blocking.
  - Finding 22: current worker executor invokes Monty with `--max-duration`, `--max-memory`, and the local script path for trusted local artifacts.
  - Findings 23 and 24: `cd frontend/worker-ui; npm test` passed with registration payload coverage for fresh worker profile data and local `worker_id`.
- Additional verification passed:
  - `cd frontend/worker-ui; npm run build`

# Full Test And Review Progress

## Current Snapshot

- Date: 2026-06-17
- Documentation has been updated to match the current Rust workspace layout
  and the real developer entry points.
- Verified this turn:
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`
  - `cd frontend/master-ui; npm test`
  - `cd frontend/worker-ui; npm test`
- Also verified this turn:
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cd hivemind-rs; cargo fmt --check`
  - `cd hivemind-rs; cargo build --release`
  - `cd frontend/master-ui; npm run build`
  - `cd frontend/worker-ui; npm run build`
  - `docker compose -f docker-compose.test.yml up --build --abort-on-container-exit tests`
- Wrapper note:
  - `make` is not installed in this shell, so the equivalent direct commands
    were run instead.

## Current Focus

- Keep the workspace docs aligned with the Rust implementation.
- Keep the verification record current when new gates are run.
- Preserve the archived Python-era notes under `docs_backup_20260611_202024/`
  as reference material only.

# 2026-06-10 Repair42 With Master API Task Resource Validation

- Continued after repair42 read-only subagents timed out without markdown reports by doing a narrower direct boundary review.
- Root cause confirmed: Master API task quote/create/upload paths parsed task resource fields as integers but did not reject negative values or invalid counts before calling node-manager. Node-manager quote pricing clamps negative resources with `.max(0)`, so malformed task requirements could be priced as zero and forwarded toward upload.
- Added RED coverage before production changes:
  - `cd hivemind-rs; cargo test -p hivemind-master-api task_submission_routes_reject_invalid_resource_values_before_grpc -- --nocapture` first failed because `/api/tasks/quote` returned HTTP 200 for `memory_gb:-1` instead of HTTP 400.
- Hardened Master API task resource handling:
  - Added shared `validate_task_resources` for `CreateTaskBody`.
  - `/api/tasks/quote` rejects invalid task resources before gRPC quote forwarding.
  - `/api/tasks` and `/api/tasks/upload` reject invalid task resources before gRPC upload forwarding.
  - Rejected values include negative CPU/GPU score, memory, GPU memory, storage, negative `max_cpt`, and `host_count < 1`.
- Verification completed after this round:
  - Focused RED/GREEN integration test failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`: passed with 14 tests.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed after `cargo fmt` formatted the new test.
  - `cd hivemind-rs; cargo audit`: passed.
- Residuals after this round:
  - Continue with another narrower read-only direct Codex subagent pass or direct review for the next concrete issue.
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Repair41 With Worker UI Capacity Validation

- Continued from repair41 Worker UI child report `worklog/subagent-codex-worker-ui-residual-repair41.md`.
- Root cause confirmed: Worker UI converted capacity/resource fields with `toNumber()` and forwarded all finite numbers directly, so negative CPU/memory/GPU/storage values, fractional `cpu_cores`, and `storage_available_gb > storage_total_gb` could be sent to `/api/register-worker`.
- Added RED coverage before production changes:
  - `cd frontend/worker-ui; npm test` first failed with missing expected exceptions for negative worker capacity values, fractional CPU cores, and impossible storage availability.
- Hardened Worker UI registration payload construction:
  - `buildRegisterWorkerBody` now normalizes capacity values once, rejects any negative capacity/resource value, requires integer `cpu_cores`, and rejects `storage_available_gb` values greater than `storage_total_gb`.
- Verification completed after this round:
  - `cd frontend/worker-ui; npm test`: failed first, then passed with 7 tests.
  - `cd frontend/worker-ui; npm run build`: passed.
  - `git diff --check` on touched Worker UI files passed.
- Additional direct Codex subagent attempt from this round:
  - Repair42 read-only direct Codex subagents for Master UI residuals, Worker UI residuals, and Rust boundary validation were launched through the real Codex CLI path. They produced no markdown reports within the checkpoint and were stopped; no repair42 child processes remained after cleanup.
- Residuals after this round:
  - Continue with a narrower read-only direct Codex subagent pass for the next concrete issue.
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Thirty-Seventh Repair Round With Worker UI Endpoint Validation

- Continued from repair39 Worker UI child report `worklog/subagent-codex-worker-ui-followup-repair39.md`.
- Root cause confirmed: Worker UI passed the endpoint input directly into `buildRegisterWorkerBody`, so whitespace-padded endpoints were submitted as-is and blank endpoints could reach `/api/register-worker` as an empty/whitespace `ip`.
- Added RED coverage before production changes:
  - `cd frontend/worker-ui; npm test` first failed because `buildRegisterWorkerBody(..., '  localhost:50053  ')` returned the whitespace-padded endpoint instead of `localhost:50053`.
- Hardened Worker UI endpoint handling:
  - `buildRegisterWorkerBody` now trims `endpoint` before placing it in the registration payload.
  - Blank endpoints now throw `worker endpoint is required` before any registration request body is built.
  - Existing `App.jsx` registration catch path surfaces the local validation error without calling `authedFetch`.
- Verification completed after this round:
  - `cd frontend/worker-ui; npm test`: failed first, then passed with 4 tests.
  - `cd frontend/worker-ui; npm run build`: passed.
- Residuals after this round:
  - Continue with another small read-only direct Codex subagent pass for the next concrete issue.
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Thirty-Sixth Repair Round With Worker UI Authenticated Username Stability

- Continued from repair37 Worker UI child report `worklog/subagent-codex-worker-ui-residual-repair37.md`.
- Root cause confirmed: Worker UI kept the login username as editable form state after authentication. The authenticated `Register again` action reused the current form username, so a user could log in as one account, edit the field, and send a registration payload with a different `username` than the bearer-token subject.
- Added RED coverage before production changes:
  - `cd frontend/worker-ui; npm test` first failed because `registrationOwnerUsername` was imported by the new test but not exported by `workerProfile.mjs`.
- Hardened Worker UI registration identity:
  - Added `registrationOwnerUsername(authenticatedUsername, formUsername)` to prefer the stored authenticated username and only fall back to the form username before authentication state exists.
  - `frontend/worker-ui/src/App.jsx` now stores `authenticatedUsername` after successful login, clears it on login retry/logout, and uses it for `/api/register-worker` payload construction and fallback worker display id.
- Verification completed after this round:
  - `cd frontend/worker-ui; npm test`: failed first, then passed with 3 tests.
  - `cd frontend/worker-ui; npm run build`: passed.
- Additional direct Codex subagent report from this round:
  - `worklog/subagent-codex-worker-ui-followup-repair39.md` reported Worker UI endpoint input is not trimmed or validated before registration. This is the next small frontend follow-up.
  - `worklog/subagent-codex-worker-identity-backend-repair39.md` timed out without a report file.
- Residuals after this round:
  - Worker UI endpoint trim/validation remains queued.
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Thirty-Fifth Repair Round With Direct gRPC Trust-Control Worker-ID Hardening

- Continued from repair37 ID-policy child report `worklog/subagent-codex-id-policy-residual-repair37.md`.
- Root cause confirmed: HTTP admin trust-control routes now reject unsafe `worker_id` values, but direct gRPC `UpdateWorkerTrustControl` authorized admins and then used `req.worker_id` directly in `worker_reputation` queries/inserts.
- Added RED coverage before production changes:
  - `test_update_worker_trust_control_rejects_unsafe_worker_id_before_insert` first failed because direct gRPC `worker_id="."` returned `success=true` and could insert an unsafe `worker_reputation` row.
- Hardened direct gRPC trust-control:
  - `GrpcMasterNodeService::update_worker_trust_control` now trims and validates `worker_id` with `is_safe_worker_id` immediately after admin authorization.
  - Invalid worker ids return `success=false`, `Invalid worker_id`, empty response worker id, and no SQL lookup/insert/update.
  - Valid worker ids use the normalized trimmed value for existence checks, update/insert, and response payloads.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_update_worker_trust_control_rejects_unsafe_worker_id_before_insert -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture`: passed with 43 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: initially failed on the new test formatting; after `cargo fmt`, repeated fmt check passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
- Residuals after this round:
  - Worker UI authenticated username stability from `worklog/subagent-codex-worker-ui-residual-repair37.md` remains the next small frontend follow-up.
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Thirty-Fourth Repair Round With Master UI Task-ID Policy Alignment

- Continued from `worklog/subagent-codex-client-task-paths-repair35.md` and `worklog/subagent-codex-master-ui-task-id-repair37.md`.
- Root cause confirmed: Master UI only normalized ZIP-derived task id defaults. Manually entered task ids were only trimmed before upload, and task log/result/cancel/artifact actions built URLs from task ids without applying the same policy as server/CLI/node-manager validation.
- Added RED coverage before production changes:
  - `cd frontend/master-ui; npm test` first failed with `ERR_MODULE_NOT_FOUND` because `src/taskIdPolicy.mjs` did not exist while the new test imported the expected helper.
- Hardened Master UI task-id handling:
  - Added `frontend/master-ui/src/taskIdPolicy.mjs` with `isSafeTaskId`, `validateTaskId`, and `taskIdFromFileName` mirroring the current Rust policy: trimmed non-empty ASCII alphanumeric plus `.`, `_`, `-`, reject exact `.` and any `..` substring.
  - Added `frontend/master-ui/src/taskIdPolicy.test.mjs` and a package `npm test` script using Node's built-in test runner.
  - `frontend/master-ui/src/App.jsx` now validates manual and filename-derived task ids before upload and validates task ids before constructing log, result, cancel, or artifact-download URLs.
- Verification completed after this round:
  - `cd frontend/master-ui; npm test`: failed first, then passed with 3 tests.
  - `cd frontend/master-ui; npm run build`: passed with Vite production build.
  - `git diff --check` on repair37 files passed with LF/CRLF warnings only.
- Additional read-only direct Codex subagent reports from this round:
  - `worklog/subagent-codex-id-policy-residual-repair37.md` reported direct gRPC admin trust-control can still accept unsafe `worker_id` values.
  - `worklog/subagent-codex-worker-ui-residual-repair37.md` reported Worker UI re-registration can send an edited username that differs from the bearer-token subject.
- Residuals after this round:
  - Direct gRPC admin trust-control worker-id validation is the next small server-side repair target.
  - Worker UI authenticated username stability remains a frontend follow-up.
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Thirty-Third Repair Round With Batch Completion Task-ID Prevalidation

- Continued from repair35's node-manager child report `worklog/subagent-codex-node-task-worker-ids-repair35.md`.
- Root cause confirmed: `CompleteBatch` authorized the reporting worker and then iterated reported tasks directly, so a malformed later `CompletedTask.task_id` could be discovered only after earlier valid tasks had already been completed/failed.
- Added RED coverage before production changes:
  - `test_batch_runtime_complete_batch_rejects_bad_task_id_before_mutating_any_task` first failed because the RPC did not return `InvalidArgument` for `task_id="."` and could mutate the valid first task before encountering the malformed second report.
- Hardened batch completion:
  - `GrpcBatchRuntimeService::complete_batch` now prevalidates every reported task id with the existing `is_safe_task_id` policy immediately after worker authorization and before any scheduler/artifact mutation.
  - A malformed batch task id now returns `Status::invalid_argument("Invalid task_id")` and leaves all tasks unchanged.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_batch_runtime_complete_batch_rejects_bad_task_id_before_mutating_any_task -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture`: passed with 42 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched repair36 files passed with line-ending warnings only.
- Residuals after this round:
  - Master UI client-side task-id policy remains a client hardening target even though server HTTP path routes now reject unsafe ids.
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Thirty-Second Repair Round With Task Path Route ID Hardening

- Spawned repair35 read-only direct Codex subagents for master API task path routes, node-manager task/worker id usage, and client task path construction.
  - `worklog/subagent-codex-master-task-paths-repair35.md` reported that master API task path routes bypassed `is_safe_task_id` and forwarded unsafe path ids directly to gRPC.
  - `worklog/subagent-codex-client-task-paths-repair35.md` reported the master UI does not enforce the same task-id policy before submit/log/result/stop/artifact path construction; this remains a follow-up client-side hardening target now that the HTTP boundary is fixed.
  - `worklog/subagent-codex-node-task-worker-ids-repair35.md` reported a separate node-manager `CompleteBatch` prevalidation/partial-commit risk; this remains the next likely server-side repair target.
  - Recovery checks found no repair35 child agent processes and no cargo/rustc/link processes after verification.
- Added RED coverage before production changes:
  - `task_path_routes_reject_unsafe_task_ids_before_grpc` first failed because `GET /api/tasks/task..path/log` returned HTTP 200 after reaching gRPC instead of rejecting the malformed path id at the HTTP boundary.
  - The same test covers log, result, artifact download, and stop task path routes.
- Hardened master API task path route validation:
  - Added `normalized_task_id()` using the same `is_safe_task_id` policy as create/upload.
  - `get_task_log`, `get_task_result`, `stop_task`, and `download_task_artifact` now reject unsafe path `task_id` values with `400 Invalid task_id` before any gRPC call.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-master-api task_path_routes_reject_unsafe_task_ids_before_grpc -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`: passed with 13 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched repair35 files passed with line-ending warnings only.
- Residuals after this round:
  - Master UI client-side task-id policy remains a client hardening target even though the server now rejects unsafe path ids.
  - Node-manager `CompleteBatch` task-id prevalidation/partial-commit risk remains queued as a likely next repair.
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Thirty-First Repair Round With Worker Path Route ID Hardening

- Spawned repair34 read-only direct Codex subagents for master API worker routes, node-manager worker-id usage, and Worker UI id handling.
  - `worklog/subagent-codex-master-worker-routes-repair34.md` reported that provider/admin `worker_id` path routes bypassed the worker-id validation already added to registration and removal.
  - `worklog/subagent-codex-worker-ui-id-repair34.md` reported a possible blank local profile display mismatch; main-agent review treated this as non-actionable for now because the master API intentionally falls back omitted `worker_id` to the authenticated subject, matching the normal UI username/token flow.
  - The node-manager worker-id subagent timed out without a report. Recovery checks found no repair34 child agent processes and no cargo/rustc/link processes after verification.
- Added RED coverage before production changes:
  - `worker_path_routes_reject_unsafe_worker_ids_before_grpc` first failed because `GET /api/provider/workers/worker..path/settings` returned HTTP 200 after reaching gRPC instead of rejecting the malformed path id at the HTTP boundary.
  - The same test covers provider settings GET/PUT, provider trust GET, and admin trust-control PUT path routes.
- Hardened master API worker path route validation:
  - Added `normalized_worker_id()` using the same `is_safe_worker_id` policy as registration/removal.
  - `get_provider_worker_settings`, `update_provider_worker_settings`, `get_provider_worker_trust_profile`, and `update_worker_trust_control` now reject unsafe path `worker_id` values with `400 Invalid worker_id` before any gRPC call.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-master-api worker_path_routes_reject_unsafe_worker_ids_before_grpc -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`: passed with 12 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `cargo fmt --check` initially failed on import formatting after the new test; `cargo fmt` was run and the repeated fmt check passed.
  - `git diff --check` on touched repair34 files passed with line-ending warnings only.
- Residuals after this round:
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Thirtieth Repair Round With Remove-Worker ID Path-Safety Hardening

- Attempted repair33 read-only direct Codex subagents for master API worker route validation, node-manager worker route validation, and Worker UI profile id handling.
  - All repair33 subagent attempts timed out without writing usable report files.
  - Recovery found and stopped one lingering repair33 child-agent process pair; final process checks found no repair33 child agent, cargo, rustc, or linker processes.
- Main-agent direct review found the remove-worker path still accepted unsafe worker ids even after registration hardening:
  - `GrpcNodeManagerService::remove_worker` used `req.worker_id` directly, so `worker_id="."` returned a normal not-found response instead of rejecting a malformed id.
  - Master API `remove_worker` forwarded `body.worker_id` directly to gRPC without applying the worker-id safety policy.
- Added RED coverage before production changes:
  - `test_remove_worker_rejects_single_dot_worker_id` first failed because node-manager remove-worker did not report an invalid `worker_id` for `.`.
- Hardened remove-worker id validation:
  - `hivemind-rs/crates/node-manager/src/grpc.rs` now trims and validates `RemoveWorkerRequest.worker_id` before worker lookup/removal.
  - `hivemind-rs/crates/master-api/src/handlers.rs` now trims and validates the remove-worker request body before calling node-manager, returning `400 Invalid worker_id` for unsafe ids.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_remove_worker_rejects_single_dot_worker_id -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture`: passed with 41 tests.
  - `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`: passed with 11 tests.
  - Targeted clippy passed for `hivemind-node-manager` and `hivemind-master-api`.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched Rust files passed with line-ending warnings only.
- Residuals after this round:
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-10 Twenty-Ninth Repair Round With Worker-ID Path-Safety Hardening

- Attempted repair32 read-only direct Codex subagents for master API worker id validation, node-manager worker registration, and provider worker route identifiers.
  - All repair32 subagent attempts timed out without writing report files.
  - Recovery checks found no repair32 child agent processes and no cargo/rustc/link processes before implementation.
- Main-agent direct review found direct node-manager `RegisterWorkerNode` and master API `POST /api/register-worker` only trimmed worker ids. Values such as `.` could be persisted as `worker_nodes.worker_id` and later used in provider routes such as `/api/provider/workers/:worker_id/settings`, creating the same path-normalization class previously fixed for task ids.
- Added RED coverage before production changes:
  - `test_register_worker_node_rejects_single_dot_worker_id` first failed because node-manager registration returned success for `worker_id="."`.
  - `worker_id_safety_rejects_path_normalizing_values` covers the master API helper for `.`, `../worker`, and slash-containing ids.
- Hardened worker id validation:
  - `hivemind-rs/crates/node-manager/src/grpc.rs` now rejects unsafe registered worker ids before inserting worker rows or reputation rows.
  - `hivemind-rs/crates/master-api/src/handlers.rs` now rejects unsafe provider registration worker ids with `400 Invalid worker_id` before calling gRPC.
  - Empty explicit worker ids still fall back to the authenticated owner / username path as before.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_register_worker_node_rejects_single_dot_worker_id -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api worker_id_safety_rejects_path_normalizing_values -- --nocapture`: passed after master API defense-in-depth was added.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture`: passed with 40 tests.
  - `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`: passed with 11 tests.
  - Targeted clippy passed for `hivemind-node-manager` and `hivemind-master-api`.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched files passed with line-ending warnings only; process recovery found no remaining repair32 child agent or cargo/rustc/link processes.
- Residuals after this round:
  - Remote artifact materialization remains product/design scope.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-09 Twenty-Eighth Repair Round With Worker UI Capacity Registration Fix

- Attempted repair31 read-only direct Codex subagents for worker UI registration, remote artifact scope, executor audit residual, dotenv parser, worker profile payload, and Monty CLI contract.
  - All repair31 subagent attempts timed out without writing report files.
  - Recovery checks found and stopped lingering repair31 child processes; no repair31 child agent, cargo, rustc, or linker process remained before verification.
- Main-agent direct review of the already-fixed Worker UI registration flow found a remaining data-loss bug: `/api/worker-info` exposed `gpu_name`, `storage_total_gb`, and `storage_available_gb`, but `buildRegisterWorkerBody()` did not send them and master API converted provider UI registrations to a `ProtoResourceSpec` with `gpu_name = ""`, `gpu_count = 0`, and both storage fields set to 0.
- Added RED coverage before production changes:
  - `frontend/worker-ui npm test` first failed because the registration payload omitted `gpu_name`, `storage_total_gb`, and `storage_available_gb`.
  - `cargo test -p hivemind-master-api worker_registration_resources_preserve_ui_capacity_fields -- --nocapture` first failed because `RegisterWorkerBody` lacked those fields and no resource-mapping helper existed.
- Hardened Worker UI provider registration capacity mapping:
  - `frontend/worker-ui/src/workerProfile.mjs` now sends `gpu_name`, `storage_total_gb`, and `storage_available_gb` from the freshly fetched local worker profile.
  - `hivemind-rs/crates/master-api/src/handlers.rs` now accepts those optional fields and maps them through `worker_registration_resources()` into `ProtoResourceSpec` for node-manager registration.
  - Existing older clients remain compatible because omitted optional fields still default to empty/zero values.
- Verification completed after this round:
  - `cd frontend/worker-ui; npm test`: failed first, then passed.
  - `cd frontend/worker-ui; npm run build`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api worker_registration_resources_preserve_ui_capacity_fields -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`: passed with 10 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched source files passed with line-ending warnings only; process recovery found no remaining repair31 child agent or cargo/rustc/link processes.
- Residuals after this round:
  - Remote artifact materialization remains product/design scope because worker-executor explicitly rejects non-local task sources and node-manager only registers locally resolvable artifact refs as downloadable bytes.
  - Executor `unic-*` audit warnings remain an allowed Ruff/Ty/salsa migration residual.

# 2026-06-09 Twenty-Seventh Repair Round With HTTP Artifact Selector Blank-Key Hardening

- Continued from repair29 and inspected the master API HTTP artifact download handler directly.
- Found that `/api/tasks/:task_id/artifact/download?artifact_key=%20%20%20` was trimmed and filtered to `None` before the gRPC call, bypassing node-manager's new malformed-selector rejection and falling back to latest-ready artifact selection.
- Added RED coverage before production changes:
  - `artifact_key_normalization_rejects_blank_explicit_selector` first failed because `normalized_artifact_key` could not represent invalid blank explicit selectors.
- Hardened master API artifact selector normalization:
  - Added `normalized_artifact_key` returning `Ok(None)` for omitted or truly empty selectors.
  - Non-empty but whitespace-only selectors now return `Err(())` and the HTTP handler responds with `400 Invalid artifact key`.
  - Valid selectors are trimmed before forwarding to node-manager, and existing max-length validation remains in place.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-master-api artifact_key_normalization_rejects_blank_explicit_selector -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`: passed with 9 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched files passed with line-ending warnings only; process recovery found no remaining repair30 child agent or cargo/rustc/link processes.

# 2026-06-09 Twenty-Sixth Repair Round With Artifact Selector Blank-Key Hardening

- Attempted one very narrow read-only direct Codex subagent for repair29:
  - `worklog/subagent-codex-artifact-selector-repair29.md` was requested for node-manager artifact selector validation, but the child timed out without writing a report and left no lingering repair29 process.
- Continued the same scope directly and found that `DownloadTaskArtifactRequest.artifact_key = "   "` was trimmed to an empty selector and silently fell back to downloading the latest ready artifact.
- Added RED coverage before production changes:
  - `test_download_task_artifact_can_select_specific_artifact_key` was extended with a whitespace-only selector and failed first because the response succeeded with latest artifact bytes.
- Hardened node-manager artifact selector validation:
  - Truly empty `artifact_key` still means the documented latest-ready fallback.
  - Non-empty but whitespace-only `artifact_key` now returns `Invalid artifact key` and no bytes.
  - Existing max-length validation remains in place.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_download_task_artifact_can_select_specific_artifact_key -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture`: passed with 39 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched files passed with line-ending warnings only; process recovery found no remaining repair29 child agent or cargo/rustc/link processes.

# 2026-06-09 Twenty-Fifth Repair Round With Node-Manager gRPC Task-ID Hardening

- Attempted one very narrow read-only direct Codex subagent for repair28:
  - `worklog/subagent-codex-node-manager-rpc-validation-repair28.md` was requested for node-manager `upload_task` / artifact validation, but the child timed out without writing a report and left no lingering repair28 process.
- Continued the same scope directly and found that direct node-manager gRPC `UploadTask` accepted exact single-dot task ids even after CLI and master API HTTP validation were hardened.
- Added RED coverage before production changes:
  - `test_upload_task_rejects_single_dot_task_id` failed first because `upload_task` returned success for `task_id="."`.
- Hardened node-manager gRPC task submission:
  - Added local `is_safe_task_id` validation for `UploadTaskRequest.task_id`.
  - `upload_task` now rejects empty, `..`-containing, invalid-character, and exact single-dot task ids before creating a `Task` row.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_upload_task_rejects_single_dot_task_id -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture`: passed with 39 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched files passed with line-ending warnings only; process recovery found no remaining repair28 child agent or cargo/rustc/link processes.

# 2026-06-09 Twenty-Fourth Repair Round With Server-Side Dot Task-ID Hardening

- Attempted three narrow read-only direct Codex subagents for repair27:
  - Server-side task id validation, artifact selector validation, and frontend/API task-id assumptions were dispatched.
  - All three timed out without writing report files; recovery stopped lingering server-task-id and frontend-task-id child processes and found no remaining repair27 child process.
- Continued the same scope directly and found that `hivemind-master-api` still allowed exact single-dot task ids in its create/upload validation helper even after the CLI was hardened.
- Added RED coverage before production changes:
  - `task_id_safety_rejects_single_dot_segment` failed first because `is_safe_task_id(".")` returned true in `hivemind-rs/crates/master-api/src/handlers.rs`.
- Hardened master API task id validation:
  - `is_safe_task_id` now rejects exact single-dot task ids before accepting `POST /api/tasks` or multipart upload task ids.
  - Existing rejection of empty ids and `..` substrings remains in place.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-master-api task_id_safety_rejects_single_dot_segment -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`: passed with 8 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched files passed with line-ending warnings only; process recovery found no remaining repair27 child agent or cargo/rustc/link processes.

# 2026-06-09 Twenty-Third Repair Round With Direct Subagent CLI Dot Task-ID Hardening

- Spawned three narrow read-only direct Codex subagents for repair26:
  - `worklog/subagent-codex-cli-lookup-edge-repair26.md` reported that `hivemind result . --download` accepted a single-dot task id and built `/api/tasks/./artifact/download`.
  - The worker report edge and node artifact edge subagents timed out without reports; recovery checks found and stopped their lingering child processes.
- Added RED coverage before production changes:
  - `result_download_rejects_dot_task_id` failed first because `parse_cli_args` accepted `.` as a valid result/download task id.
- Hardened CLI task id validation in `hivemind-rs/crates/hivemind-bin/src/cli.rs`:
  - `is_safe_task_id` now rejects exact single-dot task ids before URL path construction.
  - Existing rejection of empty task ids and `..` substrings remains in place.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-bin result_download_rejects_dot_task_id -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-bin -- --nocapture`: passed with 12 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-bin --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed after the final test cleanup: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - Process recovery found no remaining repair26 child agent processes and no cargo/rustc/link processes after cleanup.

# 2026-06-09 Twenty-Second Repair Round With Direct Subagent CLI Task-ID Validation

- Spawned three narrow read-only direct Codex subagents for repair25:
  - `worklog/subagent-codex-cli-parser-edge-repair25.md` reported that `hivemind submit --task-id ../bad` accepted an unsafe explicit task id even though `status` and `result` reject unsafe task ids.
  - The worker report edge and node artifact edge subagents timed out without writing reports; recovery checks stopped the timed-out children and found no lingering repair25 child processes.
- Added RED coverage before production changes:
  - `submit_rejects_unsafe_explicit_task_id` failed first because the submit parser assigned the explicit `--task-id` value directly.
- Hardened CLI submit parsing in `hivemind-rs/crates/hivemind-bin/src/cli.rs`:
  - Added `parse_task_id_flag` using the same `is_safe_task_id` policy already used by task lookup commands.
  - `submit --task-id <id>` now rejects empty or unsafe values instead of accepting ids that later CLI commands refuse to query.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-bin submit_rejects_unsafe_explicit_task_id -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-bin -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy -p hivemind-bin --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - Process recovery found no remaining repair25 child agent processes and no cargo/rustc/link processes after cleanup.

# 2026-06-09 Twenty-First Repair Round With Direct Subagents For Artifact Filename Edge Cases

- Spawned three narrow read-only direct Codex subagents for repair24:
  - `worklog/subagent-codex-master-artifact-response-repair24.md` reported that master API `safe_download_filename` could emit dot-only filenames such as `.` or `..` in `Content-Disposition` after sanitization.
  - `worklog/subagent-codex-cli-result-edge-repair24.md` reported that CLI `artifact_filename_from_content_disposition` split parameters on every semicolon, so a valid quoted filename like `"report;final.zip"` was rejected.
  - The worker report edge subagent timed out without writing `worklog/subagent-codex-worker-report-edge-repair24.md`; recovery checks found no lingering repair24 child process.
- Added RED tests before production changes:
  - `safe_download_filename_falls_back_for_dot_only_names` failed first because `safe_download_filename(".")` returned `"."`.
  - `artifact_download_filename_accepts_quoted_semicolon` failed first because the CLI rejected `attachment; filename="report;final.zip"` as unsafe.
- Hardened master API artifact response filenames in `hivemind-rs/crates/master-api/src/handlers.rs`:
  - `safe_download_filename` now falls back to `artifact.bin` when the sanitized filename is empty or consists only of dots.
- Hardened CLI Content-Disposition parsing in `hivemind-rs/crates/hivemind-bin/src/cli.rs`:
  - Replaced naive `header.split(';')` parsing with a small parameter scanner that respects quoted strings.
  - Existing filename safety checks remain in place after parsing.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-master-api safe_download_filename_falls_back_for_dot_only_names -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-bin artifact_download_filename_accepts_quoted_semicolon -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-bin -p hivemind-master-api --lib --bins -- --nocapture`: passed with 10 bin tests and 7 master-api tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-bin -p hivemind-master-api --all-targets --all-features -- -D warnings`: passed.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on touched repair24/state files passed with line-ending warnings only.
- Process recovery found no remaining repair24 child agent processes and no cargo/rustc/link processes after cleanup.

# 2026-06-09 Twentieth Repair Round With Direct Subagents For Artifact Download Edge Cases

- Spawned three narrow read-only direct Codex subagents for repair23:
  - `worklog/subagent-codex-cli-download-edge-repair23.md` reported that `hivemind result --download` trusted the server `Content-Disposition` filename and passed it directly to `File::create`, allowing unsafe relative/absolute paths from a malicious or malformed response.
  - `worklog/subagent-codex-master-artifact-http-repair23.md` reported that the master API artifact download handler mapped every gRPC `Status` error to HTTP 500 instead of preserving expected client-facing statuses such as 404, 401, 403, or 400.
  - The remote artifact ref subagent timed out without writing `worklog/subagent-codex-remote-artifact-ref-repair23.md`; recovery checks found and stopped the lingering wrapper/child processes.
- Added RED tests before production changes:
  - `artifact_download_filename_rejects_paths_from_content_disposition` first failed because the CLI had no safe filename extraction helper and the current download path accepted the raw header filename.
  - `artifact_download_grpc_errors_map_to_http_statuses` first failed because the master API had no artifact-specific gRPC status mapping helper.
- Hardened CLI artifact download filename handling in `hivemind-rs/crates/hivemind-bin/src/cli.rs`:
  - `download_task_artifact` now routes `Content-Disposition` parsing through `artifact_filename_from_content_disposition`.
  - Missing or filename-less headers still fall back to `artifact.bin`.
  - Unsafe filenames are rejected before writing: empty names, `.`/`..`, path separators, Windows drive/path characters, control characters, and Windows reserved device names.
- Hardened master API artifact error mapping in `hivemind-rs/crates/master-api/src/handlers.rs`:
  - Artifact gRPC errors now map `NotFound` to 404, `Unauthenticated` to 401, `PermissionDenied` to 403, `InvalidArgument` to 400, and `Unavailable` to 503; unknown/internal errors still return 500.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-bin artifact_download_filename_rejects_paths_from_content_disposition -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api artifact_download_grpc_errors_map_to_http_statuses -- --nocapture`: failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-bin -p hivemind-master-api --lib --bins -- --nocapture`: passed with 9 bin tests and 6 master-api tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-bin -p hivemind-master-api --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed after formatting.
  - Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.
  - `git diff --check` on the touched repair23/state files passed with line-ending warnings only.
- Process recovery found no remaining repair23 child agent processes and no cargo/rustc/link processes after cleanup.

# 2026-06-09 Nineteenth Repair Round With Direct Subagent CLI Artifact-Key Fix

- Recalibrated direct Codex subagent invocation after the previous repair20/21 wrapper issues. A stdin-piped smoke child agent successfully wrote `worklog/subagent-smoke-direct-repair22.md`, confirming the direct child-agent path works when using `codex exec -`.
- Attempted three broader read-only repair22 child agents for artifact API, Docker/Rust, and frontend/CLI review. They hit the outer timeout; recovery checks found only the artifact API process still running and no reports written. The remaining process was stopped cleanly.
- Narrowed the task and spawned a focused direct read-only child agent for the CLI artifact-key surface. `worklog/subagent-codex-cli-artifact-key-repair22.md` confirmed the bug: backend/proto supports `DownloadTaskArtifactRequest.artifact_key`, but `hivemind result <task-id> --download` had no `--artifact-key` flag and always downloaded the latest ready artifact.
- Fixed the CLI gap in `hivemind-rs/crates/hivemind-bin/src/cli.rs`:
  - Added optional `artifact_key` to `TaskLookupCommand`.
  - Added `--artifact-key <key>` parsing for task lookup commands, intended for `result --download`.
  - Added `artifact_download_url()` and a local percent-encoding helper so artifact keys are sent as `?artifact_key=...` without adding a new dependency.
  - Extended CLI tests to prove `result --download --artifact-key "stdout artifact"` parses and emits `artifact_key=stdout%20artifact` in the download URL.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-bin -- --nocapture`: passed with 8 bin tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-bin --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo audit`: passed.
- Process check found no remaining repair22 child agent or cargo/rustc processes after cleanup.

# 2026-06-09 Eighteenth Repair Round With Artifact Selection, Docker Gate, And Executor Audit Decision

- Attempted three additional direct read-only Codex subagents for executor `unic-*` feasibility, artifact selector review, and Docker integration health. The wrappers/logs showed the Codex CLI path attempted to read stdin and hit model/provider issues before writing markdown reports; process checks found no repair20/repair21 child processes left running. The main agent completed the bounded reviews directly.
- Rechecked the executor Rust audit residual:
  - `cd executor-rs; cargo audit` exits successfully with 5 allowed informational unmaintained warnings: `unic-char-property`, `unic-char-range`, `unic-common`, `unic-ucd-category`, and `unic-ucd-version`.
  - `cargo tree -i unic-char-property` / `cargo tree -i unic-ucd-category` confirmed the path is `ruff_python_literal -> ty_python_semantic -> monty_type_checking -> monty-cli` through the pinned Ruff rev `6ded4bed1651e30b34dd04cdaa50c763036abb0d`.
  - Ruff upstream HEAD is `267af002fb9431f6f2db5ab849c1dcd6d7752733` and has moved away from `unic-*`, but a temporary Ruff HEAD probe failed in `monty_type_checking` because the Ruff/Ty/salsa APIs changed substantially. The temporary `worklog/executor-rs-ruff-head-probe*` directories were verified under `D:\hivemind\worklog` and removed.
  - Decision: do not attempt a narrow dependency bump for `unic-*`; clearing it requires a larger Ruff/Ty/salsa integration migration.
- Added precise artifact download selection:
  - `DownloadTaskArtifactRequest` now carries optional `artifact_key`.
  - Master API accepts `/api/tasks/:task_id/artifact/download?artifact_key=...`, rejects keys over 255 bytes, and forwards the selector to node-manager.
  - Node-manager queries by `task_id + artifact_key` when a selector is provided; without a selector it preserves the old latest-ready behavior.
  - RED/GREEN test `test_download_task_artifact_can_select_specific_artifact_key` proves a task owner can download an older named artifact, a missing key does not fall back to another ready artifact, and the old no-key path still downloads the latest ready artifact.
- Docker integration recovered and was exercised for real:
  - `docker version` and `docker compose version` passed.
  - `docker compose -f docker-compose.yml config --quiet` and `docker compose -f docker-compose.test.yml config --quiet` passed.
  - `docker compose -f docker-compose.test.yml build` passed.
  - First one-shot compose test run failed in `hivemind-config` because `REDIS_URL=redis://redis:6379` from compose leaked into `env_loading_keeps_defaults_for_unspecified_values`. Reproduced locally with `$env:REDIS_URL='redis://redis:6379'; cargo test -p hivemind-config env_loading_keeps_defaults_for_unspecified_values -- --nocapture`, then fixed the test to save/remove/restore `REDIS_URL`; polluted-env focused test passed.
  - Second compose test run failed in Linux container on `stop_task_execution_kills_wrapper_spawned_child_process`; reproduced with `docker compose -f docker-compose.test.yml run --rm --no-deps tests sh -lc "/usr/local/cargo/bin/cargo test -p hivemind-worker-executor stop_task_execution_kills_wrapper_spawned_child_process -- --nocapture --test-threads=1"`. Root cause was Unix `kill` argument ambiguity for negative process-group ids. Fixed Unix termination to call `kill <signal> -- -PGID`; Docker focused test passed.
  - Final `docker compose -f docker-compose.test.yml up --abort-on-container-exit --exit-code-from tests tests` passed, covering Postgres/Redis health plus auth/config/database/worker-executor/node-manager/master-api/bin tests in containers. Compose containers were removed with `docker compose -f docker-compose.test.yml down`.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_download_task_artifact_can_select_specific_artifact_key -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager grpc::tests -- --nocapture`: passed with 31 gRPC tests.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager -p hivemind-master-api --lib -- --nocapture`: passed with 38 node-manager tests and 5 master-api tests.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor --lib -- --nocapture`: passed with 58 worker-executor tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-master-api -p hivemind-bin --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo audit`: passed.
  - Docker compose test gate passed as described above.
- Residual after this round:
  - Local artifact downloads now support precise `artifact_key` selection, but remote artifact refs such as `btih:...` are still references, not materialized bytes.
  - Executor `unic-*` warnings remain allowed informational audit warnings until the Ruff/Ty/salsa migration is planned and implemented.

# 2026-06-09 Seventeenth Repair Round With Artifact Download Hardening

- Attempted three direct read-only Codex subagents for artifact review across node-manager, master-api, and worker/scheduler reporting. The broad repair19 wrappers timed out before writing reports; the remaining node-manager wrapper process was identified and stopped. A shorter retry produced `worklog/subagent-codex-artifact-download-short-repair19b.md`; the insert/write-path retry timed out without a report. Process checks found no remaining repair19/repair19b subagent processes.
- Root cause investigation found two concrete artifact-path gaps:
  - `DownloadTaskArtifact` authorized owner/admin access, but trusted `artifacts.storage_path` directly and read the whole file into memory with no canonical artifact-root check or size cap.
  - Production code queried and cleaned `artifacts`, but did not insert rows for worker/batch-reported stdout/stderr/result artifact refs, so task logs could show `artifact://...` refs while `/api/tasks/:task_id/artifact/download` returned no downloadable row.
- Added RED tests before production changes:
  - `test_download_task_artifact_rejects_storage_path_outside_artifact_root` failed because a task owner could download bytes from a DB row pointing outside the artifact root.
  - `test_download_task_artifact_rejects_oversized_file_before_reading` failed because the download path returned success for a sparse file over the intended download limit.
  - `test_batch_runtime_complete_batch_registers_local_stdout_artifact_for_download` failed because `CompleteBatch` persisted the stdout ref text but left `artifacts` row count at zero.
- Hardened artifact behavior:
  - `NodepoolState` now carries an artifact root, derived by `artifact_root_for_config()` from `HIVEMIND_ARTIFACT_ROOT` or `<torrent.api_dir>/artifacts`.
  - `DownloadTaskArtifact` canonicalizes DB storage paths, rejects paths outside the configured artifact root, rejects non-files, and refuses files over 16 MiB before reading into memory.
  - Node-manager report handling now registers downloadable metadata for local artifact refs when possible. `artifact://relative/path` and root-contained absolute paths are canonicalized, size-checked, assigned stable metadata keys for the existing schema, and upserted into `artifacts`. Remote/non-local refs such as `btih:...` remain task result/log references and are not falsely registered as local downloadable files.
- Verification completed after this round:
  - The three RED tests listed above failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager grpc::tests -- --nocapture`: passed with 30 gRPC tests.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager -p hivemind-master-api --lib -- --nocapture`: passed with 37 node-manager tests and 5 master-api tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-node-manager -p hivemind-master-api -p hivemind-bin --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo audit`: passed.
  - `git diff --check` on the touched artifact files and state files passed with line-ending warnings only.
- Residual after this round:
  - Addressed in the eighteenth repair round: local artifact downloads now support precise `artifact_key` selection, and Docker compose integration recovered and passed the one-shot test gate.
  - Remote artifact refs are still references, not materialized bytes. Executor `unic-*` audit warnings remain a larger Ruff/Ty/salsa migration residual.

# 2026-06-09 Sixteenth Repair Round With Provider Installer Signed Release Verification

- Attempted three direct read-only Codex subagents for provider installer signing design: PowerShell scripts, Bash scripts, and package/docs. All three repair18 wrappers timed out before writing reports and were stopped; process checks found no remaining repair18 subagent processes beyond the process-check command matching its own command line.
- Reproduced the signed-release gap with RED script checks:
  - PowerShell `update-worker.ps1` accepted an unsigned artifact when `SHA256SUMS` matched and copied it into `bin/worker-executor.exe`.
  - Bash `update-worker.sh` accepted an unsigned artifact when `SHA256SUMS` matched and copied it into `bin/worker-executor`.
- Hardened provider install/update scripts:
  - PowerShell and Bash install/update paths now require `release/SHA256SUMS` plus detached OpenSSL signature `release/SHA256SUMS.sig` before trusting any checksum entry.
  - Trusted public keys must come from `HIVEMIND_RELEASE_PUBLIC_KEY`, `release-public-key.pem` next to the installer script, or `release-public-key.pem` in the install root. Keys inside `release/` are intentionally ignored because that directory is the untrusted artifact source.
  - Standalone `.sha256` sidecars are no longer accepted as sufficient provenance for install/update.
  - Scripts verify the manifest signature first, then verify source artifact hash, copy the artifact, and verify the copied hash.
- Updated `subagents/provider-installer/README.md` with the signed manifest requirement, trusted public key placement, and OpenSSL signing examples.
- Verification completed after this round:
  - Unsigned PowerShell and Bash update checks now fail and do not copy binaries.
  - Signed PowerShell and Bash update checks pass with a trusted public key.
  - Signed PowerShell and Bash install checks pass with a trusted public key.
  - Tampered `SHA256SUMS` checks fail for both PowerShell and Bash after signature generation.
  - Release-directory public key substitution checks fail for both PowerShell and Bash, proving the untrusted artifact directory cannot supply its own verification key.
  - PowerShell parser checks passed for provider installer scripts and `scripts/package-worker-windows.ps1`.
  - `bash -n` passed for provider installer shell scripts.
  - `git diff --check` on touched provider installer/package files passed with line-ending warnings only.
- Residual after this round:
  - The provider installer no longer trusts unsigned or attacker-supplied checksum manifests. Later artifact repair added local downloadable artifact-table integration; the eighteenth repair round added precise artifact selection and passed Docker compose integration. Remaining hardening candidates are remote artifact materialization and executor `unic-*` warning cleanup through a Ruff/Ty/salsa migration.

# 2026-06-09 Fifteenth Repair Round With Direct Codex Subagents And Remaining DB Fixture Isolation

- Spawned three direct read-only Codex subagents for the remaining shared Postgres fixture review:
  - `worklog/subagent-codex-task-repository-fixture-repair17.md` completed and identified the shared `pool()` helper plus affected task repository tests.
  - Node-manager gRPC and dispatcher subagent wrappers timed out before writing reports and were stopped; process checks found no remaining repair17 subagent processes other than the process-check command matching its own command line.
- Added RED schema canaries proving the remaining helpers still used the shared `public` schema:
  - `hivemind-task-scheduler::task_repository_pool_uses_isolated_schema` failed with `current_schema() = public`.
  - `hivemind-task-scheduler::dispatcher_db_tests_use_isolated_schema` failed with `current_schema() = public`.
  - `hivemind-node-manager::test_service_uses_isolated_schema` failed with `current_schema() = public`.
- Migrated the remaining shared DB-backed test fixtures to isolated Postgres schemas:
  - `hivemind-rs/crates/task-scheduler/src/task_repository.rs` now routes the local test `pool(test_name)` helper through `create_isolated_test_pool`, runs migrations in the helper, keeps a fixture handle alive for each test, and explicitly cleans up the schema after each DB-backed repository test.
  - `hivemind-rs/crates/task-scheduler/src/dispatcher.rs` now uses a `test_db(test_name)` helper backed by `IsolatedTestPool` for all DB-backed dispatcher tests, with a schema canary and explicit cleanup.
  - `hivemind-rs/crates/node-manager/src/grpc.rs` now builds `test_service()` on `create_isolated_test_pool_with_config`; the shared test cleanup drops `hm_test_*` schemas after row cleanup while leaving non-isolated schemas untouched.
- Verification completed after this round:
  - RED schema canaries failed first, then passed after migration.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler task_repository::tests -- --nocapture`: passed with 22 tests.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler dispatcher::tests -- --nocapture`: passed with 12 tests.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager grpc::tests -- --nocapture`: passed with 27 tests.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler --lib -- --nocapture`: passed with 38 tests.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture`: passed with 34 tests.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo audit`: passed.
  - `git diff --check` on touched files passed with line-ending warnings only.
- Residual after this round:
  - The previously identified shared Postgres test fixture migration set is now complete for auth, master API, VPN service, node-manager heartbeat/gRPC/worker repository, and task-scheduler dispatcher/repository DB tests.
  - Provider signed-release verification was completed in the next repair round, later artifact repair added local downloadable artifact-table integration and precise artifact selection, and Docker compose integration later passed. Remaining hardening candidates are remote artifact materialization and executor `unic-*` warning cleanup through a Ruff/Ty/salsa migration.

# 2026-06-09 Fourteenth Repair Round With Direct Codex Subagents And More DB Fixture Isolation

- Verified a reliable direct Codex subagent pattern with `codex exec --output-last-message`: a tiny smoke child agent wrote `worklog/subagent-smoke-direct-codex.md`; the parent can poll for the report and stop the wrapper process if the CLI does not exit promptly.
- Spawned two direct read-only Codex subagents for bounded DB fixture review:
  - `worklog/subagent-codex-db-fixtures-auth-master-vpn-repair16.md` reviewed auth, master API, and VPN DB-backed tests.
  - `worklog/subagent-codex-db-fixtures-node-scheduler-repair16.md` reviewed remaining node-manager and task-scheduler DB-backed tests.
- Used the auth/master/vpn report to drive implementation, including the subagent's warning that the master API in-process gRPC server needs explicit shutdown before dropping an isolated schema.
- Added RED schema assertions proving shared-DB helpers still used `public`:
  - `hivemind-auth::setup_test_db_uses_isolated_schema` failed with `current_schema() = public`.
  - `hivemind-master-api::grpc_client_talks_to_nodepool_test_fixture_for_provider_flow` failed with `current_schema() = public`.
  - `hivemind-vpn-service::join_vpn_rejects_invalid_auth_token_before_creating_peer` failed with `current_schema() = public`.
  - `hivemind-node-manager::test_process_heartbeat_valid` failed with `current_schema() = public`.
- Migrated additional fixtures to `hivemind_database::postgres::create_isolated_test_pool`:
  - Auth test helper now returns the DB plus `IsolatedTestPool` and cleans up after DB-backed tests.
  - Master API nodepool integration fixture now uses isolated schema, `serve_with_shutdown`, a shutdown channel, and an awaited server join handle before schema cleanup.
  - VPN test service now uses isolated schema fixtures and cleans up after both VPN DB tests.
  - Node-manager heartbeat setup now uses isolated schema fixtures and cleans up after DB-backed heartbeat tests.
- Verification completed after this round:
  - Focused RED/GREEN tests listed above failed first, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-auth --lib -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-master-api grpc_client_talks_to_nodepool_test_fixture_for_provider_flow -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-vpn-service --lib -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager heartbeat::tests -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-auth -p hivemind-master-api -p hivemind-vpn-service -p hivemind-node-manager --lib -- --nocapture`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo audit`: passed.
- Process checks found no remaining repair16 subagent processes.
- Residual after this round:
  - The second subagent report recommends migrating remaining node-manager gRPC, task-scheduler dispatcher, and broader task repository DB tests onto the isolated fixture next.

# 2026-06-09 Thirteenth Repair Round With Postgres Schema Isolation Fixture

- Continued the residual Postgres test-isolation track after the earlier `HivemindConfig::for_test()` hardening.
- Added a RED database test, `isolated_test_pool_runs_migrations_in_unique_schema`, that initially failed because `create_isolated_test_pool` did not exist.
- Implemented a reusable isolated Postgres schema fixture in `hivemind-rs/crates/database/src/postgres.rs`:
  - Creates unique `hm_test_*` schema names from sanitized test names plus UUID suffixes.
  - Uses a normal admin pool to create/drop schemas and a fixture pool connected with `search_path=<schema>,public` so the existing unqualified migration SQL lands in the isolated schema.
  - Caps fixture pool connections to avoid multiplying the default production-sized pool across parallel tests.
  - Exposes explicit async `cleanup()` that closes the fixture pool and drops the schema with `CASCADE`.
  - Keeps the helper available for test/dev builds without relying on ignored crate `Cargo.toml` changes.
- Added fixture behavior coverage proving:
  - `run_migrations` creates `users` in the unique schema.
  - Ad-hoc unqualified DDL through the fixture pool also lands in the unique schema, not `public`.
  - Explicit cleanup removes the schema.
- Converted high-risk DB tests to the new fixture:
  - All `hivemind-node-manager/src/worker_repository.rs` DB tests now use isolated schemas.
  - `hivemind-task-scheduler/src/task_repository.rs::test_claim_pending_for_worker_does_not_overlap_between_repositories` now runs in its own schema while preserving the concurrent `SKIP LOCKED` overlap assertion.
- Attempted repair15 read-only Codex subagents for DB fixture review, artifact visibility review, and residual security/performance review. Multiple `codex exec` invocations either timed out or failed to write reports; process checks found no remaining repair15 subagent processes. Main agent completed the implementation and verification directly.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-database isolated_test_pool_runs_migrations_in_unique_schema -- --nocapture`: failed first on missing helper, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager worker_repository::tests -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler test_claim_pending_for_worker_does_not_overlap_between_repositories -- --nocapture`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo audit`: passed.
- Residual after this round:
  - Per-test schema isolation is now available and adopted for the most direct global worker/pending-queue tests, but the remaining DB-backed tests should be migrated incrementally.
  - Docker integration was blocked by Docker Desktop API 500/timeouts at this checkpoint, but later recovered and passed compose config/build plus the one-shot compose test gate in the eighteenth repair round.
  - Signed provider release verification and local downloadable artifact-table integration were completed in later repair rounds. Precise artifact selection/remote artifact materialization and executor `unic-*` audit warning cleanup remain future hardening candidates.

## 2026-06-09 Eleventh Repair Round With Provider Installer Provenance Hardening

- Used `worklog/subagent-codex-provider-installer-provenance-repair14b.md` as the residual-risk review input.
- Reproduced the provider installer provenance gap with RED script checks:
  - `subagents/provider-installer/update-worker.ps1` copied `release/worker-executor.exe` even when `worker-executor.exe.sha256` contained an all-zero, mismatching checksum.
  - The first bash RED attempt exposed a Windows path quoting issue in the test harness; rerunning with a bash-created temp directory confirmed `update-worker.sh` also accepted a mismatched artifact before the fix.
- Hardened provider installer/update scripts:
  - PowerShell `install-worker.ps1` and `update-worker.ps1` now require checksum metadata from either `worker-executor.exe.sha256` or `SHA256SUMS`, verify source hash before copy, and verify destination hash after copy.
  - Bash `install-worker.sh` and `update-worker.sh` now require checksum metadata from either `worker-executor.sha256` or `SHA256SUMS`, verify source hash before copy, and verify destination hash after copy.
  - Install scripts now fail on missing artifacts instead of finishing a partial scaffold with only a warning.
- Hardened Windows package provenance:
  - `scripts/package-worker-windows.ps1` now emits `SHA256SUMS` and `manifest.json` with package name, configuration, UTC generation time, git commit, dirty-tree flag, and `hivemind-bin.exe` SHA256.
  - Provider installer README now documents the required checksum metadata and states that signed release manifests/signatures remain needed for untrusted distribution.
- Verification completed after this round:
  - PowerShell update mismatch check rejected the artifact and left no copied binary.
  - PowerShell update valid-checksum check copied the artifact and destination hash matched.
  - Bash update mismatch check rejected the artifact and left no copied binary.
  - Bash update valid-checksum check copied the artifact, preserved executable bit, and file contents matched.
  - PowerShell install missing-checksum check rejected the artifact instead of producing a partial install.
  - PowerShell install valid-checksum check installed the artifact.
  - Bash install missing-checksum check rejected the artifact instead of producing a partial install.
  - Bash install valid-checksum check installed the artifact.
  - `bash -n subagents/provider-installer/install-worker.sh` and `bash -n subagents/provider-installer/update-worker.sh` passed.
  - PowerShell parser checks passed for `subagents/provider-installer/install-worker.ps1`, `subagents/provider-installer/update-worker.ps1`, and `scripts/package-worker-windows.ps1`.
  - `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package-worker-windows.ps1 -Configuration debug -OutputDir dist\windows-worker-provenance-smoke` passed and generated `SHA256SUMS` / `manifest.json`; manifest hash matched the packaged binary hash.
  - `git diff --check` on the provider installer/package files passed with line-ending warnings only.
- Residual after this round:
  - Checksums now protect the local artifact handoff path, but they are not a substitute for a trusted release channel or signed manifest. Authenticode/detached signature verification remains future hardening for untrusted distribution.
- Completed a first Postgres test-isolation hardening step using `worklog/subagent-codex-postgres-test-isolation-repair14b.md`:
  - Added `HivemindConfig::for_test()` so DB tests have an explicit test-database configuration path using `HIVEMIND_TEST_DATABASE_URL` or the `hivemind_test` fallback.
  - Added RED/GREEN config coverage proving the test config does not default to the production/dev `hivemind` database.
  - Updated DB test helpers in `hivemind-auth`, `hivemind-node-manager` gRPC/heartbeat tests, and `hivemind-task-scheduler` dispatcher tests to use `for_test()` instead of `HivemindConfig::default()`.
  - This removes the footgun where several DB tests could use the default non-test `hivemind` database. It does not yet implement per-test schema/database isolation.
- Verification after the test-config hardening:
  - `cd hivemind-rs; cargo test -p hivemind-config test_config_uses_dedicated_test_database_url -- --nocapture`: failed first because `for_test` did not exist, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-auth -p hivemind-node-manager -p hivemind-task-scheduler --lib -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed after formatting.
  - `cd hivemind-rs; cargo audit`: passed.

## 2026-06-09 Tenth Repair Round With Batch Runtime Auth Hardening

- Dispatched repair14/repair14b residual-risk subagents:
  - Initial read-only repair14 subagents for batch runtime auth, provider installer provenance, and Postgres test isolation timed out and were terminated without reports.
  - Retried shorter repair14b subagents for residual review. Reports were written to:
    - `worklog/subagent-codex-provider-installer-provenance-repair14b.md`
    - `worklog/subagent-codex-postgres-test-isolation-repair14b.md`
  - Temporary stdout/stderr logs from the retry were removed after the markdown reports were saved.
- Reproduced the batch runtime auth gap with RED tests:
  - `test_batch_runtime_pull_batch_rejects_missing_token`
  - `test_batch_runtime_complete_batch_rejects_missing_token`
  - `test_batch_runtime_heartbeat_rejects_missing_token`
  - `cd hivemind-rs; cargo test -p hivemind-node-manager batch_runtime_ -- --nocapture` failed because all three RPCs accepted missing tokens and mutated worker/task state.
- Fixed batch runtime worker identity/auth:
  - Added `token` fields to `PullBatchRequest`, `CompleteBatchRequest`, and `HeartbeatRequest`.
  - Reused the node-manager worker authorization policy for batch runtime: token subject must be the registered `worker_id`, the provider owner username for that worker, or an admin.
  - `PullBatch`, `CompleteBatch`, and `Heartbeat` now reject missing/invalid tokens before claiming tasks, completing tasks, or updating worker heartbeat state.
  - Missing worker ids return invalid argument, missing workers return not found, and wrong valid users return permission denied.
- Added positive and negative coverage:
  - Non-owner tokens cannot pull a batch for another provider's worker and leave pending tasks unclaimed.
  - Provider-owner tokens can pull a batch and assign the task to the registered worker.
  - Non-owner tokens cannot complete a task assigned to another provider's worker.
  - Provider-owner tokens can complete the assigned batch task.
  - Non-owner tokens cannot update heartbeat status for another provider's worker.
  - Provider-owner tokens can update heartbeat status.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager batch_runtime_ -- --nocapture`: passed with 6 batch runtime auth tests.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture`: passed with 32 tests.
  - `cd hivemind-rs; cargo fmt --check`: passed after formatting.
  - `cd hivemind-rs; cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo audit`: passed.
- Residual reports from repair14b:
  - Provider installer provenance remains a supply-chain hardening item: local `release/worker-executor(.exe)` artifacts are copied without trusted checksums/signatures, install scripts can still produce partial installs, and package artifact naming differs from the current `hivemind-bin.exe` worker package.
  - Postgres-backed tests still share physical database state in several areas. The next migration should introduce a shared per-test schema or per-test database fixture and convert the highest-risk scheduler, dispatcher, worker repository, heartbeat, and gRPC billing tests first.
- Final repair14b process check found no remaining subagent processes.
- Continued within the same repair scope to address the next batch runtime data-loss residual:
  - Added RED test `test_batch_runtime_complete_batch_persists_artifact_refs_and_metrics`; it failed because `CompleteBatch` stored only the first result artifact reference and left execution metrics at zero.
  - Added worker-guarded scheduler persistence for batch report details through `BatchTaskReport` and `record_batch_report_for_worker`.
  - `CompleteBatch` now persists stdout/stderr artifact refs into the task log surface and records `cpu_time_ms`, `wall_time_ms`, `peak_memory_mb`, `download_bytes`, and `cache_hits` after task completion/failure.
  - Added repository guard coverage proving a wrong worker cannot write batch report details for another worker's completed task.
- Additional verification after batch report persistence:
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_batch_runtime_complete_batch_persists_artifact_refs_and_metrics -- --nocapture`: failed first, then passed after the fix.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler record_batch_report -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager batch_runtime_complete_batch -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler -p hivemind-node-manager --lib -- --nocapture`: passed with 36 scheduler tests and 33 node-manager tests.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo clippy -p hivemind-task-scheduler -p hivemind-node-manager --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo audit`: passed.

## 2026-06-09 Ninth Repair Round With Direct Codex Subagents

- Dispatched and consumed six read-only direct Codex subagents for the worker report persistence gap:
  - `worklog/subagent-codex-worker-report-scope-repair13.md` reviewed auth and assignment scope risks.
  - `worklog/subagent-codex-worker-report-e2e-repair13.md` traced why worker-local report RPCs were not visible through master APIs.
  - `worklog/subagent-codex-worker-report-api-repair13.md` reviewed master/node-manager persistence boundaries.
  - `worklog/subagent-codex-worker-report-client-repair13b.md` reviewed worker client reporting options.
  - `worklog/subagent-codex-worker-report-proto-repair13b.md` reviewed the proto/service placement.
  - `worklog/subagent-codex-worker-report-scheduler-repair13b.md` reviewed guarded scheduler repository methods.
- Added nodepool-facing report ingestion on `NodeManagerService` while leaving worker-local `WorkerNodeService` report RPCs as legacy/local endpoints:
  - Added `worker_id` to `TaskOutputUploadRequest`, `TaskResultUploadRequest`, and `TaskUsageRequest`.
  - Added node-manager `TaskOutputUpload`, `TaskResultUpload`, and `TaskUsage` RPC handlers.
  - Added `authorize_worker_report(token, worker_id)` so provider owner, worker self identity, or admin can report for a registered worker.
  - Enforced task id, output size, result reference size, required usage, and finite usage validation before persistence.
- Added scheduler/report persistence methods guarded by both task id and worker id:
  - `complete_result_for_worker`
  - `record_output_for_worker`
  - `update_resource_usage_for_worker`
  - Result-only completion now preserves previously uploaded output with `COALESCE` rather than erasing it.
- Added worker nodepool-client helpers and fake node-manager tests proving worker-scoped report RPCs are sent:
  - `report_task_output_once`
  - `report_task_result_once`
  - `report_task_usage_once`
- Verified the new behavior with focused RED/GREEN tests and full Rust gates:
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler complete -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager worker_report_rpc -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor nodepool_client::tests -- --nocapture`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo clippy -p hivemind-task-scheduler -p hivemind-node-manager -p hivemind-worker-executor --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo audit`: passed.
- Final repair13 process recovery check found no active worker-report/codex repair13 subagent processes; earlier matches were the process-check command matching its own command line.
- Remaining open candidates after this round:
  - Batch runtime `PullBatch` / `CompleteBatch` / `Heartbeat` identity and authorization are still weak and should be the next security-hardening target.
  - Worker runtime execution still completes through the existing nodepool-initiated `ExecuteTaskResponse` path; the new worker-push report helpers are not wired into `execute_task` to avoid double completion without a broader execution-mode design.
  - Worker-local report RPCs remain in-memory only and should be treated as local/legacy service behavior, not durable master persistence.
  - Provider installer artifact signing/checksum, full per-test Postgres schema/database isolation, and executor unmaintained `unic-*` audit warnings remain residual hardening items.

## 2026-06-09 Eighth Repair Round With Direct Codex Subagents

- Dispatched three read-only direct Codex subagents for the SQLx 0.9 decision point:
  - `worklog/subagent-codex-sqlx-task-scheduler-failures-repair12.md` investigated the failing scheduler DB tests.
  - `worklog/subagent-codex-sqlx-dependency-decision-repair12.md` reviewed SQLx 0.9 versus SQLx 0.8.6/audit-exception tradeoffs.
  - `worklog/subagent-codex-sqlx-db-fixture-isolation-repair12.md` did not produce a report and was terminated after the main investigation completed.
- Reproduced the SQLx 0.9 scheduler failures systematically:
  - The three failing tests all passed individually.
  - `cargo test -p hivemind-task-scheduler --lib -- --nocapture` passed.
  - `cargo test -p hivemind-task-scheduler --lib -- --test-threads=1 --nocapture` passed.
  - A full `cargo test --workspace --all-targets --all-features -- --nocapture` passed before the isolation patch, showing the earlier failure was transient and not a deterministic SQLx 0.9 production regression.
- Fixed the concrete shared-DB hazard in `test_claim_pending_for_worker_does_not_overlap_between_repositories`:
  - The production `claim_pending_for_worker` intentionally claims from the global pending queue, but the test assumed the shared test database had only its four rows.
  - Added pre-cleanup for the test's own `claim-*` namespace to remove stale rows from prior failed runs.
  - Raised the four test rows' priority and changed the two concurrent claims to a limit of two each, preserving the disjointness assertion while avoiding unrelated pending rows under normal workspace tests.
  - Verified the focused test and full scheduler lib test pass.
- Kept SQLx 0.9 because full Postgres-backed tests and `cargo audit` now pass cleanly. The alternative SQLx 0.8.6 path would likely require documenting an inactive `sqlx-mysql`/`rsa` audit exception.
- Removed the temporary `#[allow(clippy::items_after_test_module)]` in `hivemind-worker-executor/src/grpc_server.rs` by moving the test module to EOF.
- Verification completed after this round:
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler test_claim_pending_for_worker_does_not_overlap_between_repositories -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler --lib -- --nocapture`: passed with 32 tests.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor grpc_server::tests -- --nocapture`: passed with 8 tests.
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd hivemind-rs; cargo audit`: passed with no vulnerabilities.
- Final process check found no remaining subagent PIDs from repair12 or earlier recorded process IDs.

## 2026-06-09 Seventh Repair Round With Direct Codex Subagents

- Dispatched repair11 read-only direct Codex subagents for Hivemind Rust dependency audit, executor Rust dependency audit, Monty JS dependency audit, and frontend dev dependency audit.
- Consumed completed reports for frontend dev dependencies, executor dependencies, and Hivemind Rust dependencies. The Monty JS subagent hung without a report and was terminated.
- Completed worker output/result/usage RPC minimal implementation:
  - Added an in-memory worker report store behind `WorkerGrpcState`.
  - Added JWT validation for worker report RPCs.
  - Implemented output upload/retrieval, result upload, and usage reporting with task id validation, output byte cap, result reference cap, missing usage rejection, and non-finite usage rejection.
  - Moved the worker gRPC test module to EOF in the next round to remove the temporary clippy allow.
- Cleared frontend dev dependency advisories:
  - Updated Vite/PostCSS-related dependency lockfiles in `frontend/`, `frontend/master-ui/`, and `frontend/worker-ui`.
  - Verified full `npm audit --audit-level=moderate` passes for all three frontend packages.
  - Verified all three frontend builds pass and `frontend/worker-ui npm test` passes.
- Cleared Monty JS npm audit advisories:
  - Ran a controlled `npm audit fix` in `executor-rs/crates/monty-js`.
  - Verified audit, debug build, lint, 293 AVA tests, and smoke test with 42 assertions pass.
- Cleared executor Rust cargo-audit vulnerabilities:
  - Updated `postcard` and vulnerable/unsound transitive dependencies.
  - Verified executor `cargo audit`, workspace all-target/all-feature tests, clippy, and fmt check pass. Audit still reports unmaintained `unic-*` warnings from pinned Ruff dependencies.
- Upgraded Hivemind Rust dependencies for audit hygiene:
  - Updated `tonic`, `prost`, `tonic-build`, `tower`, and `sqlx` to newer compatible versions.
  - Adjusted proto build code and SQLx model encode implementation for the newer APIs.
  - `cargo check --workspace`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit` passed after the upgrade.
  - Initial full workspace test exposed transient scheduler DB test failures, which became the eighth repair round.

## 2026-06-09 Sixth Repair Round With Direct Codex Subagents

- Dispatched two read-only direct Codex subagents in parallel for the next open worker tracks:
  - `worklog/subagent-codex-worker-process-tree-stop-repair9.md` for process-tree stop design and test review.
  - `worklog/subagent-codex-worker-rpc-output-usage-repair9.md` for worker output/result/usage RPC review.
- Completed the process-tree stop repair on the main line before the subagent reports returned:
  - Added a RED test, `stop_task_execution_kills_wrapper_spawned_child_process`, where a wrapper executor spawns a delayed child process that writes a survival marker.
  - Verified the RED failure: existing direct `child.kill()` stopped the wrapper but let the spawned child write `wrapper-child-survived.marker`.
  - Added process-tree termination in worker-executor: Unix execution starts in a new process group and stop sends TERM/KILL to that group; Windows stop calls `taskkill /PID <pid> /T /F` before falling back to direct child kill.
  - Verified the wrapper-spawned child no longer survives stop.
- Verification completed in this round:
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor stop_task_execution_kills_wrapper_spawned_child_process -- --nocapture`: failed before process-tree termination, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor stop_task_execution -- --nocapture`: passed with 5 stop tests.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor --lib`: passed with 49 tests.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler --lib`: passed with 100 tests.
  - `cd hivemind-rs; cargo clippy -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler -p hivemind-master-api -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
- Pending at this checkpoint:
  - The two repair9 read-only Codex subagent processes did not return reports after the main implementation and gates completed, so they were terminated to avoid leaving background work running. No repair9 report files were produced.
- Remaining open candidates after this round:
  - Worker output upload/result upload/task usage RPCs remain unimplemented.
  - Dependency audit vulnerabilities and frontend dev dependency advisories remain open from the broader review.

## 2026-06-09 Fifth Repair Round With Direct Codex Subagents

- Dispatched and consumed read-only direct Codex subagents for worker process registry, node-manager worker stop wiring, and dispatcher cancellation race analysis. Reports were written to:
  - `worklog/subagent-codex-worker-process-registry-repair7.md`
  - `worklog/subagent-codex-node-manager-stop-worker-repair7.md`
  - `worklog/subagent-codex-node-manager-stop-worker-repair8.md`
  - `worklog/subagent-codex-dispatcher-cancel-race-repair8.md`
- Completed worker-side real stop implementation:
  - Added `StopTaskOutcome` and an active task registry in `WorkerExecutor`.
  - Registered each active task before execution starts and rejects duplicate active task ids.
  - Added cancellable execution through `run_task_with_cancel`, `execute_sandboxed`, and `wait_for_process_output`.
  - `StopTaskExecution` now cancels the running task wait path, kills the direct executor child process, and returns a stopped failure result.
  - Worker gRPC `StopTaskExecution` now maps `StopRequested`, `AlreadyStopping`, and `NotRunning` to clear response messages.
- Completed node-manager to worker stop wiring:
  - Added a RED test with an in-process fake `WorkerNodeServiceServer` proving assigned-task cancellation must send `StopTaskExecutionRequest.task_id` to the worker.
  - Node-manager `StopTask` now records scheduler cancellation first, then calls the worker endpoint from the cancelled task row's `worker_ip`.
  - Worker stop connect/RPC failures do not roll back DB cancellation; response text explicitly says worker stop was not confirmed.
  - Stop tests use the existing DB lock to avoid shared Postgres test flakiness under default parallel test execution.
- Completed dispatcher cancellation race mitigation:
  - Added a RED test with a fake worker proving `execute_on_worker` previously attempted `ExecuteTask` after a task was cancelled post-assignment.
  - `execute_on_worker` now reloads the current task and skips the RPC unless the task is still assigned/running for the same worker.
- Completed ZIP conflict/no-partial-write hardening:
  - Added a RED test for `main.py` plus `main.py/child.py` file/directory conflict.
  - ZIP extraction now validates all entries before writing, including duplicate normalized paths, unsafe paths, total size, entry count, and file/directory conflicts.
  - Validation failures no longer leave partially extracted files in the sandbox destination.
- Verification completed in this round:
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor stop_task_execution -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor grpc_server::tests::stop_task_execution -- --nocapture`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor --lib`: passed with 48 tests after ZIP conflict hardening.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_stop_task_sends_stop_execution_to_assigned_worker -- --nocapture`: failed before node-manager stop dispatch, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager stop_task -- --nocapture`: passed with 4 stop tests.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler test_execute_on_worker_skips_task_cancelled_after_assignment -- --nocapture`: failed before dispatcher preflight, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor zip_extraction_rejects_file_directory_conflict_before_writing -- --nocapture`: failed before ZIP conflict preflight, then passed.
  - Final gate: `cargo test -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler --lib` passed with 99 tests; `cargo clippy -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler -p hivemind-master-api -- -D warnings` passed; `cargo fmt --check` passed.
- Remaining open candidates after this round:
  - Process-tree termination for wrapped executors on Windows is not guaranteed by the current direct-child kill path.
  - Worker output upload/result upload/task usage RPCs remain unimplemented.
  - Dependency audit vulnerabilities and frontend dev dependency advisories remain open from the broader review.

## 2026-06-09 Fourth Repair Round With Direct Codex Subagents

- Dispatched three read-only direct Codex subagents in parallel using `codex exec --ephemeral --sandbox read-only` for worker-executor package/Monty safety, Worker UI registration, and task stop/cancel behavior. Reports were written to:
  - `worklog/subagent-codex-worker-executor-package-repair5.md`
  - `worklog/subagent-codex-worker-ui-register-repair5.md`
  - `worklog/subagent-codex-task-stop-repair5.md`
- Dispatched a repair6 read-only direct Codex subagent batch for executor ZIP/storage caps, executor memory-limit mapping, and real stop-process design. Reports were written to:
  - `worklog/subagent-codex-executor-zip-storage-repair6.md`
  - `worklog/subagent-codex-executor-memory-limit-repair6.md`
  - `worklog/subagent-codex-real-stop-process-repair6.md`
- Completed the worker-executor/Monty repair:
  - Added tests proving real Monty-compatible argv shape for local `.py` and local `.zip` task artifacts.
  - Added safe ZIP materialization into the task sandbox and required top-level `main.py` to be a file.
  - Added ZIP unsafe-path and `main.py/` directory rejection tests.
  - Restricted user-controlled direct task paths to canonical paths under `config.torrent.api_dir`.
  - Added BTIH verification for magnet `dn` local seed resolution by recomputing the torrent info-hash before execution.
  - Switched ZIP dependency features away from `zip/deflate` zopfli bloat to `deflate-flate2` with an explicit `flate2/rust_backend` dependency.
- Completed the Worker UI first-login/provider registration repair:
  - Added `frontend/worker-ui/src/workerProfile.mjs` for profile normalization and register-worker payload construction.
  - Added `frontend/worker-ui/src/workerProfile.test.mjs` Node tests.
  - Updated `frontend/worker-ui/src/App.jsx` so login registration uses the freshly fetched local profile and includes local `worker_id` in the request body.
- Completed the small honest stop/cancel wording repair:
  - Added a RED node-manager test proving `StopTask` should report cancellation recording rather than process termination.
  - Changed node-manager success wording to `Task cancellation recorded`.
  - Changed master API `/api/tasks/:task_id/stop` to return the node-manager status message instead of hard-coded `Task stopped`.
  - Changed master UI action/status text from Stop/stopping to Cancel/cancellation request.
  - Real worker process termination remains open because `worker-executor` still has no active child-process registry and `StopTaskExecution` is still unimplemented.
- Completed the worker-executor memory limit mapping repair:
  - Added a RED test that captures the fake Monty argv and showed a 1GB task still received global `4096MB` before the fix.
  - Changed the executor memory limit passed to Monty to the lower of `EXECUTOR_MAX_MEMORY_MB` and `task.req_memory_gb * 1024` for positive task memory requests.
  - Preserved `req_memory_gb=0` as unspecified, so omitted task memory uses the global executor maximum instead of silently becoming a 1GB cap.
- Completed first-pass worker ZIP extraction hardening:
  - Added RED tests for duplicate normalized paths and excessive entry count.
  - Added a direct helper test for total uncompressed byte limits without creating a large archive.
  - `extract_zip_safely` now receives byte and entry caps, rejects too many entries before extraction, rejects duplicate normalized paths, sums uncompressed entry sizes with overflow checks, and rejects packages over the task storage budget before writing the oversized entry.
  - `prepare_task_script` now passes `SandboxLimits.max_storage_mb` into ZIP extraction, tying package expansion to the task storage request.
- Verification completed in this round:
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor local_python_task_uses_monty_cli_file_contract -- --nocapture`: passed earlier in the round.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor zip_task_rejects -- --nocapture`: first showed `main.py/` was accepted, then passed after the fix.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor local_task_source_rejects_paths_outside_artifact_dir -- --nocapture`: failed before artifact-root allowlisting, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor magnet_display_name_rejects_mismatched_btih -- --nocapture`: failed before BTIH verification, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor --lib`: passed with 38 tests.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor -p hivemind-task-scheduler --lib`: passed with 69 tests across both crates.
  - `cd hivemind-rs; cargo clippy -p hivemind-worker-executor -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd frontend/worker-ui; npm test`: passed with 2 tests.
  - `cd frontend/worker-ui; npm run build`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-node-manager test_stop_task_reports_cancellation_recorded -- --nocapture`: failed before wording changes, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-task-scheduler overwrite -- --nocapture`: passed.
  - `cd hivemind-rs; cargo clippy -p hivemind-node-manager -p hivemind-master-api -- -D warnings`: passed.
  - `cd frontend/master-ui; npm run build`: passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor task_requested_memory_caps_monty_memory_argument -- --nocapture`: failed before the memory mapping fix, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor memory -- --nocapture`: passed for positive requested memory and zero/unspecified memory behavior.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor zip_task_rejects_duplicate_paths -- --nocapture`: failed before duplicate-path validation, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor zip_task_rejects_too_many_entries -- --nocapture`: failed before entry-count validation, then passed.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor zip_extraction_rejects_uncompressed_size_over_limit -- --nocapture`: passed after byte-cap extraction validation.
  - `cd hivemind-rs; cargo test -p hivemind-worker-executor --lib`: passed with 43 tests after the memory and ZIP hardening fixes.
  - `cd hivemind-rs; cargo clippy -p hivemind-worker-executor -- -D warnings`: passed after the memory mapping fix.
  - `cd hivemind-rs; cargo fmt --check`: passed after formatting the ZIP helper changes.
  - Final spot gate for this round passed: `npm run build` in both `frontend/master-ui` and `frontend/worker-ui`, `npm test` in `frontend/worker-ui`, `cargo test -p hivemind-worker-executor -p hivemind-task-scheduler --lib`, `cargo clippy -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-master-api -- -D warnings`, and `cargo fmt --check`.
- New or still-open repair candidates from this round:
  - Real running-task stop still does not terminate a worker process; current stop path cancels scheduler DB state only. The honest wording fix is complete, but the larger correct fix still needs process-handle tracking and worker `StopTaskExecution` implementation.
  - ZIP extraction can still be hardened further with full directory/file conflict preflight and no-partial-extract cleanup on I/O failures.

## 2026-06-09 Third Repair Round With Direct Codex Subagents

- Dispatched four read-only direct Codex subagents in parallel for dependency audit hygiene, worker RPC implementation gaps, packaging hardening, and frontend/executor runtime behavior. Reports were written to:
  - `worklog/subagent-codex-deps-audit-repair3.md`
  - `worklog/subagent-codex-worker-rpc-repair3.md`
  - `worklog/subagent-codex-packaging-hardening-repair3.md`
  - `worklog/subagent-codex-frontend-executor-runtime-repair3.md`
- Reproduced the Monty JS smoke-test portability failure with `bash -n scripts/smoke-test.sh`; it failed on CRLF parsing before fixes.
- Fixed Monty JS smoke-test portability and package-state hygiene:
  - Added root `.gitattributes` rule so `*.sh` remains LF in the working tree.
  - Changed `smoke-test.sh` to `/usr/bin/env bash` and `set -euo pipefail`.
  - Added cleanup trap for generated `npm/`, tarballs, and `optionalDependencies` so failed smoke runs do not leave package state behind.
  - Changed smoke-test installation to use the tarballs produced by the current package run with `--no-save`.
  - Removed stale `pydantic-monty-1.0.0.tgz` fixture dependencies from `smoke-test/package.json`.
  - Updated `smoke-test/test.ts` for the current `MontyNameLookup` return union.
- Verified the Monty JS smoke path:
  - `bash -n scripts/smoke-test.sh`: passed.
  - `cd executor-rs/crates/monty-js/smoke-test; npm run type-check`: passed.
  - `cd executor-rs/crates/monty-js; npm run smoke-test`: passed after release native build, package directory creation, npm pack/install, type-check, and 42 runtime assertions.
  - Confirmed cleanup removed generated `npm/` and tarballs and did not leave `optionalDependencies` in tracked `package.json`; only ignored native `.node` build output remains.
- Fixed the generated Windows worker launcher dotenv handling:
  - `scripts/package-worker-windows.ps1` now emits strict `Import-DotEnv` and `Assert-RequiredEnv` functions.
  - Generated launcher rejects malformed dotenv keys, duplicate keys, blank required settings, and default/blank `JWT_SECRET` before launching `hivemind-bin.exe`.
  - Generated launcher requires `WORKER_ADVERTISE_ADDR` for the provider package to avoid registering an unreachable `127.0.0.1` fallback from a packaged remote worker.
- Verified Windows launcher behavior:
  - `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package-worker-windows.ps1 -Configuration debug -OutputDir dist\windows-worker-review-smoke3`: passed.
  - Blank template `.env.worker` failed before binary execution with missing `WORKER_ADVERTISE_ADDR`.
  - Valid `.env.worker` with quoted secret and spaces around `=` passed parser/validation and reached the intentionally missing binary execution step in the isolated test copy.
  - Invalid key, duplicate key, and default `JWT_SECRET` cases failed with explicit launcher errors.
  - PowerShell parser check for `scripts/package-worker-windows.ps1` passed.
  - `git diff --check` for third-batch changed files passed with line-ending warnings only.
- Additional confirmed open findings from subagent/runtime review were added to `findings.md`: worker executor/Monty CLI argument mismatch, Worker UI stale profile first-login registration, and Worker UI missing local `worker_id` in provider registration.

## 2026-06-09 Second Repair Round With Direct Codex Subagents

- Dispatched four read-only Codex subagents in parallel for auth/JWT defaults, CORS/token persistence, VPN auth/scope, and provider ownership/control behavior. All four returned actionable reports in `worklog/subagent-codex-*-repair2.md`; the main agent applied all edits to avoid write conflicts in the dirty worktree.
- Added RED tests and verified failures for the next high/medium defects before implementation:
  - JWT/default-user startup behavior.
  - Master and worker-control CORS allowlist behavior.
  - Provider registration with distinct worker id and provider owner.
  - Worker late completion after cancellation and owner cancellation after completion.
  - VPN invalid token rejection and task-scoped peer listing.
- Fixed default public account exposure: `hivemind-bin` now only calls `seed_default_user` when `HIVEMIND_SEED_DEFAULT_USER` is explicitly truthy, the database helper no longer logs the password, and `frontend/src/App.jsx` no longer advertises `testuser / testpass123`.
- Fixed weak JWT startup/deployment defaults: added `AuthConfig::validate_jwt_secret`, made master/nodepool/all startup fail on empty/default JWT secrets, changed Docker compose to require `JWT_SECRET`, and removed the weak JWT value from the Windows worker package template.
- Fixed permissive CORS and root frontend token persistence: added configured exact origin allowlists to `ServerConfig`, replaced wildcard CORS in master and worker control routers, passed worker control origins through service startup, and removed root frontend `localStorage` bearer token persistence.
- Fixed provider worker ownership mismatch for authenticated registration: `RegisterWorkerNodeRequest` now has a distinct `worker_id`; master API registration passes authenticated owner plus worker id; node-manager stores worker id separately from owner username with legacy fallback; worker auto-registration sends both fields as the worker id for unclaimed workers.
- Fixed terminal-state business logic: `complete_for_worker` only completes `ASSIGNED`/`RUNNING` tasks, and `cancel` only cancels active non-terminal tasks.
- Fixed VPN auth/scope: `VpnService` now validates JWTs, authorizes worker operations against `worker_nodes.username` or worker self identity, passes auth tokens through gRPC for join/get peers/leave/update, and scopes `GetTaskPeers` to the assigned task worker instead of returning all online peers.
- Verification completed after this batch:
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features`: passed.
  - `cd hivemind-rs; cargo clippy -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd executor-rs; cargo test --workspace --all-targets --all-features`: passed.
  - `cd executor-rs; cargo clippy --workspace --tests --all-features -- -D warnings`: passed.
  - `cd executor-rs; cargo fmt --check`: passed with existing stable-rustfmt warnings about nightly-only options.
  - `npm run build`: passed for root frontend, master UI, and worker UI.
  - Windows worker packaging and sample task packaging passed.
  - Static `rg` found no root frontend `localStorage` token use, public `testpass123` text, or weak JWT fallback in checked product surfaces.
  - `git diff --check`: passed with line-ending warnings only.
- Remaining open findings are dependency audit vulnerabilities, unimplemented worker control RPCs/output-upload paths, Monty JS smoke script portability, frontend dev dependency audits, provider installer artifact provenance, Windows dotenv parser fragility, and Docker integration being blocked by Docker engine API errors.

## 2026-06-09 Repair Round With Codex Subagents

- User clarified subagents should be directly spawned Codex agents, not external agents. Dispatched read-only `codex exec` subagents for executor all-target failures and backend business-logic fixes. The executor and backend subagents returned actionable patch plans; the lint/script subagent timed out before writing a report, so the main agent handled that track directly.
- Added RED tests for three backend business bugs, verified each failed for the expected reason, then implemented minimal fixes:
  - `test_get_all_user_tasks_preserves_persisted_task_fields` failed on empty `owner`; now passes after mapping all persisted `Task` fields into proto `TaskInfo`.
  - `test_admin_billing_overview_uses_payer_debit_and_failed_pending_tasks` failed because payer debit was sourced from `task_debit`; now passes after switching to `payer_debit` and `FAILED` pending status.
  - `test_trusted_workers_excludes_missing_reputation_rows` and `test_claim_pending_for_worker_blocks_missing_reputation_row` failed because missing reputation rows were trusted; now pass after default-deny trust handling.
- Fixed Hivemind Rust lint failure in `worker-executor/src/resource_monitor.rs`; `cargo clippy -- -D warnings` from `hivemind-rs/` now passes.
- Fixed executor Rust lint failures in Monty runtime and test targets; `cargo clippy --workspace --tests --all-features -- -D warnings` from `executor-rs/` now passes.
- Fixed executor Windows all-target/all-feature test gate by gating non-Windows `pprof` bench usage and excluding the cargo-fuzz crate from normal workspace target selection while keeping its manifest standalone-parseable.
- Restored `test_tasks/package_tasks.ps1` with valid ASCII PowerShell, `$PSScriptRoot` paths, and robust source-file enumeration. Added minimal sample task source directories for `01_hello_world`, `02_math_compute`, and `03_text_processing`; the packaging script now produces all three ZIP archives.
- Verification completed:
  - `cd hivemind-rs; cargo test --workspace --all-targets --all-features`: passed.
  - `cd hivemind-rs; cargo clippy -- -D warnings`: passed.
  - `cd hivemind-rs; cargo fmt --check`: passed.
  - `cd executor-rs; cargo test --workspace --all-targets --all-features`: passed.
  - `cd executor-rs; cargo clippy --workspace --tests --all-features -- -D warnings`: passed.
  - `cd executor-rs; cargo fmt --check`: passed with existing stable-rustfmt warnings about nightly-only options.
  - `powershell -NoProfile -ExecutionPolicy Bypass -File test_tasks\package_tasks.ps1`: passed and generated three ZIP archives.
- Remaining high/medium findings still open include default public account, VPN auth/scope, permissive CORS, weak JWT startup defaults, dependency audit vulnerabilities, provider ownership mismatch, unimplemented worker control RPCs, token storage, installer provenance, dotenv parsing, and dev dependency audits.

## 2026-06-08 Coordination Recovery

- Recovered existing durable state from `worklog/full-test-review-state.md`, `task_plan.md`, and `findings.md`.
- Confirmed no callable subagent API/tool is exposed in this Codex session. Local `subagents/` contains helper projects, not invocable review agents.
- To satisfy the delegation intent within available tooling, work is split into role-scoped parallel review tracks: backend/executor, frontend, config/infrastructure, security/data authenticity, and performance/business logic.
- Previous `cargo test` run timed out after 120 seconds during compilation; this is not a test failure. A longer full run is required.

## 2026-06-08 Executed Tests

- `cargo test` from `hivemind-rs/`: passed. Unit tests and doc-tests completed across the workspace; warning only for future incompatibility in `sqlx-postgres v0.7.4`.
- `cargo clippy -- -D warnings` from `hivemind-rs/`: failed on one lint error, `clippy::needless_return` in `crates/worker-executor/src/resource_monitor.rs`.
- `npm run build` from `frontend/master-ui/`: passed with Vite production build.
- `npm run build` from `frontend/worker-ui/`: passed with Vite production build.
- `npm run build` from `frontend/`: passed with Vite production build.
- `cargo build -p hivemind-bin` from `hivemind-rs/`: passed; warning only for future incompatibility in `sqlx-postgres v0.7.4`.
- `make build` from repository root: blocked because `make` is not installed in this Windows environment.
- `cargo build --release` from `hivemind-rs/`: passed as the equivalent of the Makefile build target; warning only for future incompatibility in `sqlx-postgres v0.7.4`.
- `cargo fmt --check` from `hivemind-rs/`: passed.
- `cargo test -- --ignored` from `hivemind-rs/`: passed with zero ignored tests discovered/executed.
- `npm audit --omit=dev` from `frontend/`, `frontend/master-ui/`, and `frontend/worker-ui/`: all passed with zero production dependency vulnerabilities.
- Installed and ran `cargo audit`: failed with 6 RustSec vulnerabilities and 2 unmaintained warnings; details recorded in `findings.md`.
- `cargo test --workspace` from `executor-rs/`: passed across the executor subproject.
- `cargo fmt --check` from `executor-rs/`: passed with warnings that stable rustfmt ignores nightly-only options `imports_granularity` and `group_imports`.
- `cargo clippy --workspace --tests --all-features -- -D warnings` from `executor-rs/`: failed on `clippy::uninlined_format_args` in `crates/monty/src/modules/time.rs`.
- `npm install` and `npm run build:debug` from `executor-rs/crates/monty-js/`: passed.
- `npm test` from `executor-rs/crates/monty-js/`: passed after build, with 293 AVA tests passing.
- `npm run lint` from `executor-rs/crates/monty-js/`: passed with zero oxlint warnings/errors.
- `npm audit --audit-level=moderate` from `executor-rs/crates/monty-js/`: failed with 5 vulnerabilities; details recorded in `findings.md`.
- `npm run smoke-test` from `executor-rs/crates/monty-js/`: failed before package validation because `scripts/smoke-test.sh` was parsed with CRLF line endings under bash.

## 2026-06-08 Runtime Checks

- `docker compose -f docker-compose.test.yml up --build --abort-on-container-exit tests`: could not execute because Docker Desktop returned a 500 error while resolving `redis:7-alpine`; earlier restricted attempt also failed on Docker engine/config permission. Recorded as an environment/tooling blocker, not a test assertion failure.
- Local Postgres and Redis ports were reachable on `localhost:5432` and `localhost:6379`.
- Started `hivemind-bin all` locally with test-only env vars and verified `8082`, `50051`, and `18080` were reachable.
- Exercised live HTTP API: `/health`, `/api/register`, `/api/login`, `/api/balance`, `/api/tasks/quote`, `/api/tasks`, `/api/tasks`, `/api/tasks/:task_id/result`, `/api/tasks/:task_id/stop`, `/api/workers?include_offline=true`, and worker control `/api/worker-info`.
- Runtime API returned `owner: "` and default billing/runtime values in task list, confirming the task-list data mapping issue.
- Runtime API confirmed `testuser` / `testpass123` can log in and receive a token after normal startup.
- Runtime CORS probe with `Origin: http://evil.example` returned `Access-Control-Allow-Origin: *` for master `/health` and worker `/api/worker-info`.
- Retried Docker on both `desktop-linux` and `default` contexts; both returned Docker API 500 at `/version`, so compose integration remains blocked by Docker engine state.
- Extended runtime flow exercised admin billing/artifacts/cache/trust/audit endpoints, provider earnings/trust endpoints, and CLI `submit`, `status`, and `result` with correct `--api` flag.
- Extended runtime flow confirmed provider settings GET/PUT returns `Not authorized` for a provider account against an auto-registered worker whose username is the worker id.
- Extended runtime flow showed admin billing totals with provider/platform credits but payer debit still zero, strengthening the confirmed billing aggregation finding.
- `scripts/package-worker-windows.ps1 -Configuration debug -OutputDir dist/windows-worker-review-smoke`: passed and produced the Windows worker package. The generated environment template includes `JWT_SECRET=change-me-in-production`, reinforcing the weak default secret finding.
- `test_tasks/package_tasks.ps1`: failed during PowerShell parsing with a missing string terminator at line 34, so sample task package generation is currently blocked by script syntax/content corruption.

## 2026-06-08 Continuation With External Subagents

- Confirmed non-interactive agent CLIs exist locally: `codex exec`, `claude -p`, `opencode run`, and `gemini -p`.
- External subagent attempts:
  - `codex exec` broad read-only review tasks were launched but timed out before writing reports.
  - `opencode run` failed because the configured Anthropic API key is invalid.
  - `claude -p` business/performance review was stopped after it stayed running without output.
  - `gemini -p` broad/small tasks often hit model capacity 429 or empty-response errors.
  - `gemini` backend-security small review completed and confirmed already-recorded default-account, VPN auth/scope, CORS, and weak JWT default findings.
  - `codex exec` tiny packaging review completed and added review notes for relative task package paths and a fragile `.env.worker` parser.
  - `gemini -m gemini-2.5-flash` quick frontend/business reviews produced supplemental notes; each item was cross-checked before adoption.
- `cargo test --workspace --all-targets --all-features` from `hivemind-rs/`: passed across unit and integration test targets with the same `sqlx-postgres v0.7.4` future-incompatibility warning.
- `cargo test --workspace --all-targets --all-features` from `executor-rs/`: failed before completing tests because the Windows all-target build cannot resolve bench-only `pprof` and tries to link cargo-fuzz binaries without normal entry points.
- `cargo audit` from `executor-rs/`: failed with 2 vulnerabilities (`thin-vec`, `time`) and 8 warnings (`atomic-polyfill`, `unic-*`, `rand` unsound advisories).
- Full `npm audit --audit-level=moderate` from `frontend/`, `frontend/master-ui/`, and `frontend/worker-ui/`: failed on dev dependency advisories. Production-only audits still pass.
- `subagents/sandbox-egress cargo test`: passed with 8 tests.
- `subagents/sandbox-egress cargo clippy -- -D warnings`: passed.
- `subagents/provider-installer` PowerShell scripts parsed successfully via PSParser.
- Docker remained unusable: `docker --context default version` returned Docker API 500 again, and a later retry timed out.
- `test_tasks/package_tasks.ps1` still fails under normal PowerShell execution with a missing string terminator, even though UTF-8 tokenization can parse it. This points to an execution encoding/path robustness problem rather than a pure static syntax issue.

## 2026-06-08 Review Findings

- Confirmed findings are recorded in `findings.md`: default public account, unauthenticated/unscoped VPN RPC behavior, permissive CORS and worker info exposure, task list data loss, admin billing aggregation mismatch, unimplemented worker control RPCs, Hivemind and executor lint failures, weak JWT defaults, Rust and JS dependency audit failures, provider worker ownership mismatch, and broken package helper scripts.

## 2026-06-29 Managed Function Runtime

- Added and locally committed the expanded managed runtime and conservative
  Python/C++ transpiler milestone in commit `bec9b01`.
- Integrated `runtime = managed-function-v0` and `task_source` through proto,
  master API, node-manager upload, task model/database persistence, dispatcher,
  worker gRPC, and worker execution.
- Worker executor now runs managed function tasks directly from source with JSON
  input, bypassing host artifacts and external sandbox process execution.
- Added managed receipt fields to task persistence and worker execution results:
  `managed_executed_ops`, `managed_output_bytes`, and `managed_receipt_json`.
- Billing now settles managed tasks from persisted receipt data using
  `1 + ceil(executed_ops / 1000) + ceil(output_bytes / 1024)`, capped by
  `max_cpt`; legacy tasks continue to settle at `max_cpt`.
- Verification passed:
  - `cargo test -p managed-function-runtime -p managed-function-transpiler -- --nocapture`
  - `cargo test -p hivemind-worker-executor -p hivemind-task-scheduler -- --nocapture`
  - `cargo test --workspace --no-run` from `hivemind-rs/`
  - `cargo clippy -p hivemind-task-scheduler -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-master-api --all-targets -- -D warnings`
  - `cargo clippy -p managed-function-runtime -p managed-function-transpiler --all-targets -- -D warnings`

## 2026-06-08

- Started full test and review pass.
- Confirmed the working tree contains many pre-existing modifications; they will be preserved.
- Root `AGENTS.md` applies repository-wide.
- `planning-with-files-zh` instructions were partially garbled by encoding, but the clear requirement to maintain `task_plan.md`, `findings.md`, and `progress.md` is being followed.

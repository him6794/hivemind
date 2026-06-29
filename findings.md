# Full Test And Review Findings

## Fixed In Current Repair Stream

### Thirty-Eighth Repair Batch

- Direct repair41 Worker UI report `worklog/subagent-codex-worker-ui-residual-repair41.md` found Worker UI registration accepted impossible negative capacity values because `toNumber()` returns any finite number and `buildRegisterWorkerBody()` forwarded the values directly.
- `frontend/worker-ui/src/workerProfile.mjs` now normalizes capacity/resource fields once before building the payload, rejects any negative capacity values, requires integer `cpu_cores`, and rejects `storage_available_gb` values larger than `storage_total_gb`.
- `frontend/worker-ui/src/workerProfile.test.mjs` covers negative capacity rejection, fractional CPU-core rejection, and impossible storage availability rejection.

Verification run after the thirty-eighth batch:

- RED Worker UI tests first failed with missing expected exceptions for negative capacity, fractional CPU cores, and impossible storage availability: `cd frontend/worker-ui; npm test`.
- After implementation, `cd frontend/worker-ui; npm test` passed with 7 tests.
- `cd frontend/worker-ui; npm run build` passed.
- `git diff --check` on touched Worker UI files passed.

Residual risk after this batch:

- Repair42 read-only direct Codex subagents for Master UI, Worker UI, and Rust boundary-validation residuals were launched, produced no markdown reports within the checkpoint, and were stopped with no remaining matching processes.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Thirty-Ninth Repair Batch

- Direct repair42 review found Master API task submission/quote routes accepted invalid task resource values. `/api/tasks/quote` could return HTTP 200 for negative resource values, and `/api/tasks` could forward negative resources or `host_count=0` toward node-manager.
- Root cause: `CreateTaskBody` resource fields were parsed as integers but were not range-validated at the HTTP boundary; node-manager quote pricing clamps some negative values with `.max(0)`, which can hide invalid input instead of rejecting it.
- `hivemind-rs/crates/master-api/src/handlers.rs` now validates task resources before quote or create/upload forwarding: CPU/GPU score, memory, GPU memory, and storage must be non-negative; `host_count` must be at least 1; `max_cpt` must be non-negative.
- `hivemind-rs/crates/master-api/src/integration_tests.rs` covers quote and create routes through the real Axum router and in-process nodepool fixture.

Verification run after the thirty-ninth batch:

- RED integration test first failed because `/api/tasks/quote` returned HTTP 200 for `memory_gb:-1`: `cd hivemind-rs; cargo test -p hivemind-master-api task_submission_routes_reject_invalid_resource_values_before_grpc -- --nocapture`.
- After implementation, the same focused test passed.
- `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture` passed with 14 tests.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

### Thirty-Seventh Repair Batch

- Direct repair39 Worker UI report `worklog/subagent-codex-worker-ui-followup-repair39.md` found Worker UI registration sent the endpoint input raw, allowing blank or whitespace-padded `ip` values to reach `/api/register-worker`.
- `frontend/worker-ui/src/workerProfile.mjs` now trims the endpoint before building the registration body and throws `worker endpoint is required` for blank endpoints.
- `frontend/worker-ui/src/workerProfile.test.mjs` covers trimmed endpoint payloads and blank endpoint rejection.

Verification run after the thirty-seventh batch:

- RED Worker UI test first failed because endpoint whitespace was preserved, then passed after implementation: `cd frontend/worker-ui; npm test`.
- `cd frontend/worker-ui; npm run build` passed.

Residual risk after this batch:

- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Thirty-Sixth Repair Batch

- Direct repair37 Worker UI report `worklog/subagent-codex-worker-ui-residual-repair37.md` found authenticated re-registration could send a mutable form username that differed from the bearer-token subject.
- `frontend/worker-ui/src/workerProfile.mjs` now exposes `registrationOwnerUsername`, which prefers the authenticated username and trims it.
- `frontend/worker-ui/src/App.jsx` stores the authenticated username after login, clears it on login retry/logout, and uses it for Worker UI registration payloads instead of the editable form value.

Verification run after the thirty-sixth batch:

- RED Worker UI test first failed with a missing `registrationOwnerUsername` export, then passed after implementation: `cd frontend/worker-ui; npm test`.
- `cd frontend/worker-ui; npm run build` passed.

Residual risk after this batch:

- `worklog/subagent-codex-worker-ui-followup-repair39.md` reported Worker UI endpoint input is not trimmed or validated before registration.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Thirty-Fifth Repair Batch

- Direct repair37 ID-policy report `worklog/subagent-codex-id-policy-residual-repair37.md` found that direct gRPC `UpdateWorkerTrustControl` could still accept unsafe `worker_id` values even though HTTP routes reject them.
- `hivemind-rs/crates/node-manager/src/grpc.rs` now trims and validates admin trust-control worker ids with `is_safe_worker_id` before any `worker_reputation` lookup, insert, or update.
- Unsafe direct gRPC worker ids now return `success=false` with `Invalid worker_id` and do not create reputation rows.

Verification run after the thirty-fifth batch:

- RED node-manager test first failed, then passed after implementation: `test_update_worker_trust_control_rejects_unsafe_worker_id_before_insert`.
- Focused gates passed: `cargo test -p hivemind-node-manager --lib -- --nocapture` and `cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- `worklog/subagent-codex-worker-ui-residual-repair37.md` reported Worker UI re-registration can send a username that differs from the authenticated token subject.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Thirty-Fourth Repair Batch

- Direct repair37 client reports found Master UI task-id policy drift. The UI normalized ZIP-derived defaults but accepted manually typed task ids after only trimming, and task log/result/cancel/artifact actions built URLs from task ids without client-side policy checks.
- `frontend/master-ui/src/taskIdPolicy.mjs` now centralizes the current Rust-compatible task-id policy for the Master UI: non-empty after trim, ASCII alphanumeric plus `.`, `_`, `-`, reject exact `.` and any `..` substring.
- `frontend/master-ui/src/App.jsx` now validates upload task ids before `FormData` submission and validates task ids before building log, result, stop, or artifact-download paths.
- `frontend/master-ui/src/taskIdPolicy.test.mjs` and the package `npm test` script cover the helper behavior.

Verification run after the thirty-fourth batch:

- RED frontend test first failed with `ERR_MODULE_NOT_FOUND` before the helper existed, then passed after implementation: `cd frontend/master-ui; npm test`.
- `cd frontend/master-ui; npm run build` passed.
- `git diff --check` on repair37 files passed with LF/CRLF warnings only.

Residual risk after this batch:

- `worklog/subagent-codex-id-policy-residual-repair37.md` reported direct gRPC admin trust-control still needs worker-id validation.
- `worklog/subagent-codex-worker-ui-residual-repair37.md` reported Worker UI re-registration can send a username that differs from the authenticated token subject.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Thirty-Third Repair Batch

- Direct repair35 Codex subagent report `worklog/subagent-codex-node-task-worker-ids-repair35.md` found `CompleteBatch` did not prevalidate all reported task ids before scheduler mutations. A later malformed `CompletedTask.task_id` could fail indirectly after earlier batch entries were already completed/failed.
- `hivemind-rs/crates/node-manager/src/grpc.rs` now validates all `CompleteBatchRequest.tasks[*].task_id` values with `is_safe_task_id` immediately after worker authorization and before any scheduler or artifact writes.
- Malformed batch task ids now return `InvalidArgument` and leave previously assigned tasks unchanged.

Verification run after the thirty-third batch:

- RED node-manager test first failed, then passed after implementation: `test_batch_runtime_complete_batch_rejects_bad_task_id_before_mutating_any_task`.
- Focused gates passed: `cargo test -p hivemind-node-manager --lib -- --nocapture` and `cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Master UI task-id validation remains a defense-in-depth/client UX follow-up from `worklog/subagent-codex-client-task-paths-repair35.md`.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Thirty-Second Repair Batch

- Direct repair35 Codex subagent report `worklog/subagent-codex-master-task-paths-repair35.md` found task path routes did not apply the task-id safety policy used by create/upload. Unsafe ids such as `task..path` could reach gRPC through task log, task result, artifact download, and stop routes.
- `hivemind-rs/crates/master-api/src/handlers.rs` now normalizes and validates path `task_id` values before forwarding those four task path requests.
- Unsafe path ids now return `400 Invalid task_id` at the HTTP boundary and do not reach gRPC.

Verification run after the thirty-second batch:

- RED integration test first failed, then passed after implementation: `task_path_routes_reject_unsafe_task_ids_before_grpc`.
- Focused gates passed: `cargo test -p hivemind-master-api --lib -- --nocapture` and `cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- `worklog/subagent-codex-client-task-paths-repair35.md` reported client-side master UI task-id policy drift; server-side HTTP rejection now protects the boundary, but client UX hardening remains useful.
- `worklog/subagent-codex-node-task-worker-ids-repair35.md` reported node-manager `CompleteBatch` task-id prevalidation and partial-commit risk; this is queued as a likely next server-side repair.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Thirty-First Repair Batch

- Direct repair34 Codex subagent report `worklog/subagent-codex-master-worker-routes-repair34.md` found provider/admin `worker_id` path routes did not apply the worker-id safety policy used by registration/removal. Unsafe ids such as `worker..path` could reach gRPC through provider settings, provider trust, and admin trust-control routes.
- `hivemind-rs/crates/master-api/src/handlers.rs` now normalizes and validates path `worker_id` values before forwarding provider settings GET/PUT, provider trust GET, or admin trust-control PUT requests.
- Unsafe path ids now return `400 Invalid worker_id` at the HTTP boundary and do not reach gRPC.

Verification run after the thirty-first batch:

- RED integration test first failed, then passed after implementation: `worker_path_routes_reject_unsafe_worker_ids_before_grpc`.
- Focused gates passed: `cargo test -p hivemind-master-api --lib -- --nocapture` and `cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- The repair34 node-manager worker-id subagent timed out without a report; the master API route finding was completed from the usable master-route subagent report and main-agent verification.
- The repair34 Worker UI report was reviewed but not fixed because omitted `worker_id` already falls back to the authenticated subject in master API registration, matching the normal username/token UI flow.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Thirtieth Repair Batch

- Main-agent direct review found the remove-worker paths were not covered by the worker-id safety hardening from the previous batch. Direct node-manager `RemoveWorkerRequest.worker_id = "."` reached the ordinary lookup path, and master API `remove_worker` forwarded the request body worker id without validation.
- `hivemind-rs/crates/node-manager/src/grpc.rs` now trims and rejects unsafe remove-worker ids before any lookup/removal.
- `hivemind-rs/crates/master-api/src/handlers.rs` now returns `400 Invalid worker_id` for unsafe remove-worker ids before gRPC forwarding.

Verification run after the thirtieth batch:

- RED test first failed, then passed after implementation: `test_remove_worker_rejects_single_dot_worker_id`.
- Focused gates passed: `cargo test -p hivemind-node-manager --lib -- --nocapture`, `cargo test -p hivemind-master-api --lib -- --nocapture`, and targeted clippy for both crates.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Repair33 direct Codex subagents timed out without usable reports; this finding was completed by main-agent direct inspection and verification.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-Ninth Repair Batch

- Main-agent direct review found worker ids were not protected with the same path-safe identifier policy already applied to task ids. Direct node-manager `RegisterWorkerNode` accepted `worker_id="."`, and master API `POST /api/register-worker` forwarded trimmed worker ids without validating path-normalizing values.
- `hivemind-rs/crates/node-manager/src/grpc.rs` now validates registered worker ids before creating worker and reputation rows.
- `hivemind-rs/crates/master-api/src/handlers.rs` now rejects unsafe provider registration worker ids with `400 Invalid worker_id` before calling node-manager.

Verification run after the twenty-ninth batch:

- RED test first failed, then passed after implementation: `test_register_worker_node_rejects_single_dot_worker_id`.
- Focused gates passed: `cargo test -p hivemind-node-manager --lib -- --nocapture`, `cargo test -p hivemind-master-api --lib -- --nocapture`, and targeted clippy for both crates.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Repair32 direct Codex subagents timed out without usable reports; this finding was completed by main-agent direct inspection.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-Eighth Repair Batch

- Main-agent direct review found Worker UI provider registration still lost local capacity fields even after the stale-state and local `worker_id` fixes: `/api/worker-info` returned `gpu_name`, `storage_total_gb`, and `storage_available_gb`, but the UI registration payload omitted them and master API converted registration resources with empty GPU name and zero storage.
- `frontend/worker-ui/src/workerProfile.mjs` now includes `gpu_name`, `storage_total_gb`, and `storage_available_gb` in the registration body.
- `hivemind-rs/crates/master-api/src/handlers.rs` now accepts those optional fields and maps provider UI registrations through `worker_registration_resources()` into the node-manager `ProtoResourceSpec`, preserving GPU name, GPU count, VRAM, and storage totals.

Verification run after the twenty-eighth batch:

- RED tests first failed, then passed after implementation: `frontend/worker-ui` registration payload test and `worker_registration_resources_preserve_ui_capacity_fields`.
- Focused gates passed: `cd frontend/worker-ui; npm test`, `npm run build`, `cd hivemind-rs; cargo test -p hivemind-master-api --lib -- --nocapture`, and `cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Repair31 direct Codex subagents timed out without usable reports; this finding was completed by main-agent direct inspection.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-Seventh Repair Batch

- Main-agent direct review found the master API HTTP artifact download handler still converted whitespace-only `artifact_key` query values into an omitted selector before calling node-manager, bypassing repair29's gRPC-level malformed-selector rejection.
- `hivemind-rs/crates/master-api/src/handlers.rs` now normalizes artifact selectors through `normalized_artifact_key`: omitted and truly empty selectors still mean latest-ready fallback, valid selectors are trimmed, and non-empty whitespace-only selectors return `400 Invalid artifact key`.

Verification run after the twenty-seventh batch:

- RED test first failed, then passed after implementation: `artifact_key_normalization_rejects_blank_explicit_selector`.
- Focused gates passed: `cargo test -p hivemind-master-api --lib -- --nocapture` and `cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-Sixth Repair Batch

- A very narrow repair29 direct Codex subagent was attempted for node-manager artifact selector validation, but it timed out without a report and left no lingering process.
- Main-agent direct review found `DownloadTaskArtifactRequest.artifact_key = "   "` was treated as an omitted selector after trimming, so it silently downloaded the latest ready artifact instead of rejecting the malformed explicit selector.
- `hivemind-rs/crates/node-manager/src/grpc.rs` now preserves the empty-string fallback but rejects non-empty whitespace-only artifact selectors with `Invalid artifact key` and no response bytes.

Verification run after the twenty-sixth batch:

- RED test first failed, then passed after implementation: the whitespace-only selector branch added to `test_download_task_artifact_can_select_specific_artifact_key`.
- Focused gates passed: `cargo test -p hivemind-node-manager --lib -- --nocapture` and `cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Repair29 child agent produced no report; this finding was completed by main-agent direct inspection.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-Fifth Repair Batch

- A very narrow repair28 direct Codex subagent was attempted for node-manager RPC validation, but it timed out without a report and left no lingering process.
- Main-agent direct review found direct node-manager gRPC `UploadTask` still accepted exact single-dot task ids, so non-HTTP clients could bypass the CLI/master API task-id hardening.
- `hivemind-rs/crates/node-manager/src/grpc.rs` now validates `UploadTaskRequest.task_id` before creating a task row, rejecting empty, `..`-containing, invalid-character, and exact single-dot task ids.

Verification run after the twenty-fifth batch:

- RED test first failed, then passed after implementation: `test_upload_task_rejects_single_dot_task_id`.
- Focused gates passed: `cargo test -p hivemind-node-manager --lib -- --nocapture` and `cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Repair28 child agent produced no report; this finding was completed by main-agent direct inspection.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-Fourth Repair Batch

- Repair27 direct Codex subagents were attempted for server-side task id validation, artifact selector validation, and frontend/API task-id assumptions, but all timed out without report files; recovery stopped lingering child processes.
- Main-agent direct review found the same single-dot task id class still present in `hivemind-rs/crates/master-api/src/handlers.rs`: create/upload task id validation allowed `.` even though such a value is path-normalizable in later route URLs.
- `hivemind-rs/crates/master-api/src/handlers.rs` now rejects exact single-dot task ids in `is_safe_task_id`, while preserving existing empty-id and `..` rejection.

Verification run after the twenty-fourth batch:

- RED test first failed, then passed after implementation: `task_id_safety_rejects_single_dot_segment`.
- Focused gates passed: `cargo test -p hivemind-master-api --lib -- --nocapture` and `cargo clippy -p hivemind-master-api --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Repair27 child agents produced no reports; this finding was completed by main-agent direct inspection.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-Third Repair Batch

- Direct repair26 Codex subagents reported one actionable CLI path-segment issue:
  - `worklog/subagent-codex-cli-lookup-edge-repair26.md`: `hivemind result . --download` accepted a single-dot task id and built `/api/tasks/./artifact/download`.
- `hivemind-rs/crates/hivemind-bin/src/cli.rs` now rejects exact single-dot task ids in `is_safe_task_id`, while preserving the existing empty-id and `..` rejection.
- This prevents a task id that can be normalized as a URL path segment from reaching CLI status/result/download paths.

Verification run after the twenty-third batch:

- RED test first failed, then passed after implementation: `result_download_rejects_dot_task_id`.
- Focused gates passed: `cargo test -p hivemind-bin -- --nocapture` and `cargo clippy -p hivemind-bin --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- The worker report edge and node artifact edge repair26 subagents timed out without reports; recovery stopped their lingering child processes.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-Second Repair Batch

- Direct repair25 Codex subagents reported one actionable CLI parser inconsistency:
  - `worklog/subagent-codex-cli-parser-edge-repair25.md`: `hivemind submit --task-id ../bad` accepted an unsafe explicit task id even though `status` and `result` reject unsafe task ids.
- `hivemind-rs/crates/hivemind-bin/src/cli.rs` now validates explicit submit task ids through `parse_task_id_flag`, reusing the same `is_safe_task_id` policy used by task lookup commands.
- This prevents users from creating CLI-submitted task ids that later CLI commands refuse to query or download artifacts for.

Verification run after the twenty-second batch:

- RED test first failed, then passed after implementation: `submit_rejects_unsafe_explicit_task_id`.
- Focused gates passed: `cargo test -p hivemind-bin -- --nocapture` and `cargo clippy -p hivemind-bin --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- The worker report edge and node artifact edge repair25 subagents timed out without reports; recovery stopped the timed-out children and found no lingering process.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twenty-First Repair Batch

- Direct repair24 Codex subagents reported two more artifact filename edge cases:
  - `worklog/subagent-codex-master-artifact-response-repair24.md`: master API filename sanitization allowed dot-only names such as `.` and `..` to reach `Content-Disposition`.
  - `worklog/subagent-codex-cli-result-edge-repair24.md`: CLI `Content-Disposition` parsing split on semicolons inside quoted filenames, causing safe names like `report;final.zip` to be rejected.
- `hivemind-rs/crates/master-api/src/handlers.rs` now falls back to `artifact.bin` when the sanitized artifact filename is empty or only dots.
- `hivemind-rs/crates/hivemind-bin/src/cli.rs` now scans `Content-Disposition` parameters while respecting quoted strings, then applies the existing filename safety check.

Verification run after the twenty-first batch:

- RED tests first failed, then passed after implementation: `safe_download_filename_falls_back_for_dot_only_names` and `artifact_download_filename_accepts_quoted_semicolon`.
- Focused gates passed: `cargo test -p hivemind-bin -p hivemind-master-api --lib --bins -- --nocapture` and `cargo clippy -p hivemind-bin -p hivemind-master-api --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- The worker report edge subagent timed out without a report but left no child process behind.
- Remote artifact materialization and executor `unic-*` warning cleanup remain the documented larger residuals.

### Twentieth Repair Batch

- Direct repair23 Codex subagents reported two additional artifact download edge cases:
  - `worklog/subagent-codex-cli-download-edge-repair23.md`: CLI artifact downloads trusted the server `Content-Disposition` filename and could write to unsafe relative/absolute paths.
  - `worklog/subagent-codex-master-artifact-http-repair23.md`: master API artifact download gRPC errors were collapsed to HTTP 500 instead of preserving client-relevant statuses.
- `hivemind-rs/crates/hivemind-bin/src/cli.rs` now parses artifact download filenames through `artifact_filename_from_content_disposition`, keeps the no-header fallback of `artifact.bin`, and rejects unsafe filenames before creating a local file.
- `hivemind-rs/crates/master-api/src/handlers.rs` now maps artifact download gRPC status codes to HTTP statuses: `NotFound -> 404`, `Unauthenticated -> 401`, `PermissionDenied -> 403`, `InvalidArgument -> 400`, `Unavailable -> 503`, and unknown/internal errors remain 500.

Verification run after the twentieth batch:

- RED tests first failed for missing helpers, then passed after implementation: `artifact_download_filename_rejects_paths_from_content_disposition` and `artifact_download_grpc_errors_map_to_http_statuses`.
- Focused gates passed: `cargo test -p hivemind-bin -p hivemind-master-api --lib --bins -- --nocapture` and `cargo clippy -p hivemind-bin -p hivemind-master-api --all-targets --all-features -- -D warnings`.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- Remote artifact refs such as `btih:...` are still references, not materialized downloadable bytes. The repair23 remote-artifact subagent timed out without a report and was stopped cleanly.
- Executor `unic-*` warnings remain the documented Ruff/Ty/salsa migration residual.

### Nineteenth Repair Batch

- CLI artifact download selection was aligned with the backend `artifact_key` selector added in the eighteenth batch.
- A focused direct read-only Codex subagent wrote `worklog/subagent-codex-cli-artifact-key-repair22.md` and confirmed that `hivemind result <task-id> --download` had no way to pass `artifact_key` even though `DownloadTaskArtifactRequest` supports it.
- `hivemind-bin` now parses `--artifact-key <key>` into `TaskLookupCommand.artifact_key` and appends it to the artifact download request as `?artifact_key=...` using local percent-encoding.
- Regression coverage proves `result --download --artifact-key "stdout artifact"` parses successfully and builds a URL ending in `artifact_key=stdout%20artifact`.

Verification run after the nineteenth batch:

- `cd hivemind-rs; cargo test -p hivemind-bin -- --nocapture` passed.
- `cd hivemind-rs; cargo clippy -p hivemind-bin --all-targets --all-features -- -D warnings` passed.
- Full Hivemind gates passed: `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `cargo audit`.

Residual risk after this batch:

- CLI now reaches local artifact-key selection. Remote artifact materialization and executor `unic-*` warning cleanup remain the only documented residuals from this audit stream.

### Eighteenth Repair Batch

- Precise local artifact download selection was added on top of the previous artifact-root hardening.
- `DownloadTaskArtifactRequest` now accepts `artifact_key`; master API forwards `?artifact_key=...` to node-manager and rejects oversized keys before gRPC.
- Node-manager now resolves a requested artifact through `task_id + artifact_key + status='ready'`. If no key is supplied, it preserves the existing latest-ready fallback for backward compatibility.
- The selector does not bypass owner/admin checks, artifact-root canonicalization, non-file rejection, or the 16 MiB download cap from the seventeenth repair batch.
- Docker integration is no longer only a static/config check. Docker Desktop recovered, compose config/build passed, and the one-shot compose test service ran real tests against Postgres and Redis.
- Docker testing exposed and fixed two real cross-environment issues:
  - `hivemind-config` env default coverage did not isolate `REDIS_URL`, so compose's Redis URL invalidated the default-value assertion. The test now saves/removes/restores `REDIS_URL` like the other env vars it controls.
  - Unix process-tree termination used `kill -TERM -PGID` / `kill -KILL -PGID`; inside Linux containers this was ambiguous enough that wrapper-spawned children could survive. The implementation now uses `kill <signal> -- -PGID`, and the Docker-focused worker-executor stop test passes.
- Executor `unic-*` warnings were re-audited and deliberately left as a documented migration residual, not a quick fix. `cargo audit` exits 0 with allowed informational warnings, and removing them requires a Ruff/Ty/salsa compatibility migration rather than a safe lockfile bump.

Verification run after the eighteenth batch:

- RED/GREEN artifact selector coverage: `test_download_task_artifact_can_select_specific_artifact_key` passed after implementation and proves selector hit, selector miss without fallback, and old latest-ready fallback.
- Docker RED/GREEN config coverage: polluted `REDIS_URL` focused test failed first, then passed after test env isolation.
- Docker RED/GREEN process-tree coverage: Linux container `stop_task_execution_kills_wrapper_spawned_child_process` failed first, then passed after the Unix `kill -- -PGID` fix.
- Focused and full gates passed: node-manager gRPC tests, node-manager/master-api lib tests, worker-executor lib tests, targeted clippy, full Hivemind workspace tests, full workspace clippy, `cargo fmt --check`, `cargo audit`, compose config/build, and `docker compose -f docker-compose.test.yml up --abort-on-container-exit --exit-code-from tests tests`.

Residual risk after this batch:

- Remote artifact refs such as `btih:...` are still not materialized into downloadable bytes by the master/nodepool API.
- Executor `unic-*` warnings remain allowed informational warnings until a planned Ruff/Ty/salsa migration is done.

### Seventeenth Repair Batch

- Artifact download security and local artifact-table integration were hardened.
- `DownloadTaskArtifact` now validates the caller as before, then canonicalizes `artifacts.storage_path` under a configured artifact root before reading. Paths outside the artifact root, non-files, missing files, and files over 16 MiB are rejected with empty response data instead of being read into memory.
- The nodepool artifact root is now derived from `HIVEMIND_ARTIFACT_ROOT` or `<torrent.api_dir>/artifacts` and is carried in `NodepoolState` for runtime and in-process test fixtures.
- Worker/batch report handling now registers local downloadable artifact metadata when refs are local and safe: `artifact://relative/path` and root-contained absolute paths are canonicalized, size-checked, assigned stable metadata keys for the existing schema, and upserted into `artifacts`. Remote refs such as `btih:...` remain references only and are not misrepresented as local bytes.
- Added regression coverage proving root-outside storage paths are rejected, oversized files are rejected before read, and a local batch stdout artifact ref creates an `artifacts` row that can be downloaded by the task owner.

Verification run after the seventeenth batch:

- RED tests first proved the unsafe behavior: outside-root DB paths were downloadable, oversized files returned success, and batch stdout `artifact://...` refs created zero artifact rows.
- After the fix, the three focused tests passed.
- `cd hivemind-rs; cargo test -p hivemind-node-manager grpc::tests -- --nocapture` passed.
- `cd hivemind-rs; cargo test -p hivemind-node-manager -p hivemind-master-api --lib -- --nocapture` passed.
- `cd hivemind-rs; cargo clippy -p hivemind-node-manager -p hivemind-master-api -p hivemind-bin --all-targets --all-features -- -D warnings` passed.
- Full Hivemind gates passed: `cargo fmt --check`, `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo audit`.
- `git diff --check` on touched artifact/state files passed with line-ending warnings only.

Residual risk after this batch:

- Addressed in the eighteenth repair batch: local artifact downloads now support precise `artifact_key` selection while preserving the old no-key latest-ready fallback.
- Remote artifact refs remain refs; this batch only registered locally resolvable artifacts. Executor `unic-*` audit warnings remain a documented migration residual.

### Sixteenth Repair Batch

- Provider installer artifact provenance was hardened from unsigned checksum validation to signed manifest verification.
- PowerShell and Bash install/update scripts now require `release/SHA256SUMS` and detached OpenSSL signature `release/SHA256SUMS.sig` before trusting artifact hashes.
- Trusted public keys must be provided via `HIVEMIND_RELEASE_PUBLIC_KEY`, beside the installer script, or in the install root as `release-public-key.pem`; public keys inside `release/` are ignored.
- Standalone `.sha256` sidecars are no longer sufficient for install/update because they can be replaced together with the artifact.
- `subagents/provider-installer/README.md` now documents the signed manifest format, key placement, and signing commands.

Verification run after the sixteenth batch:

- RED checks first showed unsigned PowerShell and Bash update flows accepted matching `SHA256SUMS`; after the fix both reject unsigned manifests and copy nothing.
- Signed PowerShell and Bash update/install checks passed using temporary OpenSSL keys and trusted public key paths.
- Tampered `SHA256SUMS` checks failed after signing for both PowerShell and Bash.
- Release-directory public-key substitution checks failed for both PowerShell and Bash, proving the untrusted artifact directory cannot bring its own trust root.
- PowerShell parser checks passed for provider installer scripts and `scripts/package-worker-windows.ps1`.
- `bash -n subagents/provider-installer/install-worker.sh` and `bash -n subagents/provider-installer/update-worker.sh` passed.
- `git diff --check` on touched provider installer/package files passed with line-ending warnings only.

Residual risk after this batch:

- Provider installer signed-release verification is now enforced locally. Later artifact repair added local downloadable artifact-table integration and precise artifact selection, and Docker compose integration later passed. Remaining non-provider residuals are remote artifact materialization and executor `unic-*` warnings.

### Fifteenth Repair Batch

- Direct Codex subagents were used for the remaining DB fixture isolation pass. The task repository subagent wrote `worklog/subagent-codex-task-repository-fixture-repair17.md`; node-manager gRPC and dispatcher subagent attempts timed out and were stopped cleanly.
- The remaining shared Postgres test fixture candidates from the repair16 reports were migrated to isolated schemas:
  - `hivemind-task-scheduler` task repository DB tests now use `create_isolated_test_pool` through a local `pool(test_name)` helper, with migrations and schema cleanup handled per test.
  - `hivemind-task-scheduler` dispatcher DB tests now use an isolated `test_db(test_name)` helper and verify `current_schema()` starts with `hm_test_`.
  - `hivemind-node-manager` gRPC test service now uses `create_isolated_test_pool_with_config`; cleanup drops `hm_test_*` schemas after existing row cleanup.

Verification run after the fifteenth batch:

- `cd hivemind-rs; cargo test -p hivemind-task-scheduler task_repository_pool_uses_isolated_schema -- --nocapture` failed first with `current_schema() = public`, then passed.
- `cd hivemind-rs; cargo test -p hivemind-task-scheduler dispatcher_db_tests_use_isolated_schema -- --nocapture` failed first with `current_schema() = public`, then passed.
- `cd hivemind-rs; cargo test -p hivemind-node-manager test_service_uses_isolated_schema -- --nocapture` failed first with `current_schema() = public`, then passed.
- Focused module gates passed: task repository tests, dispatcher tests, and node-manager gRPC tests.
- Crate/full gates passed: `cargo test -p hivemind-task-scheduler --lib -- --nocapture`, `cargo test -p hivemind-node-manager --lib -- --nocapture`, `cargo fmt --check`, `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo audit`.

Residual risk after this batch:

- The previously recorded shared Postgres DB fixture migration candidates are complete. Provider signed-release verification was addressed in the sixteenth repair batch; later artifact repair added local downloadable artifact-table integration and precise artifact selection, and Docker compose integration later passed. Remaining residuals are remote artifact materialization and executor `unic-*` warnings.

### Fourteenth Repair Batch

- Direct Codex subagents were spawned successfully with bounded read-only scopes and wrote reports under `worklog/`:
  - `worklog/subagent-codex-db-fixtures-auth-master-vpn-repair16.md`
  - `worklog/subagent-codex-db-fixtures-node-scheduler-repair16.md`
- Postgres test isolation was extended to additional DB-backed fixtures:
  - `hivemind-auth` test DB helper now uses `create_isolated_test_pool` and verifies `current_schema()` starts with `hm_test_`.
  - `hivemind-master-api` nodepool integration fixture now uses an isolated schema, runs its in-process gRPC server with explicit shutdown, awaits the server task, and drops the schema afterward.
  - `hivemind-vpn-service` test service now uses isolated schemas for VPN auth/scope DB tests and verifies the schema on the invalid-token test path.
  - `hivemind-node-manager` heartbeat DB fixture now uses an isolated schema and verifies it on the valid heartbeat path.

Verification run after the fourteenth batch:

- `cd hivemind-rs; cargo test -p hivemind-auth setup_test_db_uses_isolated_schema -- --nocapture` failed first with `current_schema() = public`, then passed after migration.
- `cd hivemind-rs; cargo test -p hivemind-master-api grpc_client_talks_to_nodepool_test_fixture_for_provider_flow -- --nocapture` failed first with `current_schema() = public`, then passed after isolated fixture and gRPC shutdown wiring.
- `cd hivemind-rs; cargo test -p hivemind-vpn-service join_vpn_rejects_invalid_auth_token_before_creating_peer -- --nocapture` failed first with `current_schema() = public`, then passed after migration.
- `cd hivemind-rs; cargo test -p hivemind-node-manager test_process_heartbeat_valid -- --nocapture` failed first with `current_schema() = public`, then passed after migration.
- Focused crate gate passed: `cargo test -p hivemind-auth -p hivemind-master-api -p hivemind-vpn-service -p hivemind-node-manager --lib -- --nocapture`.
- Full Hivemind gate passed: `cargo fmt --check`, `cargo test --workspace --all-targets --all-features -- --nocapture`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo audit`.

Residual risk after this batch:

- This residual was addressed in the fifteenth repair batch: node-manager gRPC, task-scheduler dispatcher, and the remaining task repository DB tests now use isolated schemas.

### Thirteenth Repair Batch

- Postgres test isolation advanced from "test database only" to a reusable per-test schema fixture in `hivemind-database`. `create_isolated_test_pool` now creates a unique `hm_test_*` schema, connects with `search_path=<schema>,public`, caps fixture pool connections, and provides explicit async cleanup that drops the schema with `CASCADE`.
- Added database coverage proving unqualified migrations and ad-hoc test DDL land in the isolated schema instead of `public`, and proving cleanup removes the schema.
- Converted all `hivemind-node-manager` worker repository DB tests to isolated schemas, covering worker upsert/list/stale-offline behavior without depending on global `worker_nodes` state.
- Converted the high-risk `hivemind-task-scheduler` concurrent `claim_pending_for_worker` test to the isolated schema fixture. This test previously required defensive cleanup and priority bias because it shared the global pending queue; it now runs against its own migrated schema while preserving the `FOR UPDATE SKIP LOCKED` overlap assertion.

Verification run after the thirteenth batch:

- `cd hivemind-rs; cargo test -p hivemind-database isolated_test_pool_runs_migrations_in_unique_schema -- --nocapture` first failed because the helper did not exist, then passed after implementation.
- `cd hivemind-rs; cargo test -p hivemind-node-manager worker_repository::tests -- --nocapture` passed.
- `cd hivemind-rs; cargo test -p hivemind-task-scheduler test_claim_pending_for_worker_does_not_overlap_between_repositories -- --nocapture` passed.
- `cd hivemind-rs; cargo fmt --check` passed.
- `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture` passed.
- `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings` passed.
- `cd hivemind-rs; cargo audit` passed.

Residual risk after this batch:

- The reusable fixture now exists and the most direct global worker/pending-queue tests were converted, but many Postgres-backed tests still use the shared `hivemind_test` database. Remaining conversion candidates include auth DB tests, node-manager gRPC/heartbeat fixtures, task-scheduler dispatcher and additional repository tests, master API integration tests, and VPN DB tests.
- Repair15 `codex exec` subagent attempts did not produce usable markdown reports before timeout; no repair15 subagent processes remained after process checks. The main agent completed the implementation and verification directly.

### Eleventh Repair Batch

- Provider installer local artifact provenance fixed for checksum verification. Windows and Linux install/update scripts now refuse to copy worker artifacts unless checksum metadata is present and matches.
- PowerShell installer/update scripts accept either `release/worker-executor.exe.sha256` or a `release/SHA256SUMS` entry, verify the source before copy, and verify the copied destination after copy.
- Bash installer/update scripts accept either `release/worker-executor.sha256` or a `release/SHA256SUMS` entry, verify the source before copy, verify the copied destination after copy, and preserve executable permissions.
- Provider install scripts now fail on missing executable artifacts instead of completing a partial scaffold with only a warning.
- Windows worker packaging now emits package provenance metadata: `SHA256SUMS` and `manifest.json` with package name, configuration, generation time, git commit, dirty flag, and `hivemind-bin.exe` SHA256.
- Provider installer README now documents the required checksum metadata and explicitly scopes remaining signature/trusted-release-channel work.

Verification run after the eleventh batch:

- PowerShell update mismatch check failed as expected and did not copy the binary.
- PowerShell update valid-checksum check succeeded and the copied hash matched.
- Bash update mismatch check failed as expected and did not copy the binary.
- Bash update valid-checksum check succeeded, preserved executable permission, and copied content matched.
- PowerShell install missing-checksum check failed as expected and did not silently finish a partial install.
- PowerShell install valid-checksum check succeeded.
- Bash install missing-checksum check failed as expected and did not silently finish a partial install.
- Bash install valid-checksum check succeeded.
- `bash -n subagents/provider-installer/install-worker.sh` and `bash -n subagents/provider-installer/update-worker.sh` passed.
- PowerShell parser checks passed for both provider installer PowerShell scripts and `scripts/package-worker-windows.ps1`.
- `scripts/package-worker-windows.ps1 -Configuration debug -OutputDir dist\windows-worker-provenance-smoke` passed and generated `SHA256SUMS` / `manifest.json`; manifest hash matched `hivemind-bin.exe`.
- `git diff --check` on the touched provider installer/package files passed with line-ending warnings only.

Residual risk after this batch:

- This residual was addressed in the sixteenth repair batch: install/update scripts now require a detached OpenSSL signature over `SHA256SUMS` and a trusted public key outside the untrusted `release/` directory.

### Twelfth Repair Batch

- Postgres test-database footgun reduced: added `HivemindConfig::for_test()` so DB tests can explicitly use `HIVEMIND_TEST_DATABASE_URL` or the `hivemind_test` fallback instead of the default non-test `hivemind` database URL.
- Updated DB-backed test helpers in `hivemind-auth`, `hivemind-node-manager` gRPC/heartbeat tests, and `hivemind-task-scheduler` dispatcher tests to use `HivemindConfig::for_test()`.
- Added config-level coverage proving the test config URL does not default to the production/dev database name.

Verification run after the twelfth batch:

- `cd hivemind-rs; cargo test -p hivemind-config test_config_uses_dedicated_test_database_url -- --nocapture` first failed because `for_test` did not exist, then passed.
- `cd hivemind-rs; cargo test -p hivemind-auth -p hivemind-node-manager -p hivemind-task-scheduler --lib -- --nocapture` passed.
- `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture` passed.
- `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings` passed.
- `cd hivemind-rs; cargo fmt --check` passed after formatting.
- `cd hivemind-rs; cargo audit` passed.

Residual risk after this batch:

- This fixes accidental use of the default non-test database by several DB test helpers. It does not yet provide full per-test schema/database isolation, so shared-state risks in global pending queues, worker lists, stale-worker cleanup, billing totals, and process-wide env mutation remain longer-term test architecture work.

### Tenth Repair Batch

- Batch runtime worker identity/auth fixed for `PullBatch`, `CompleteBatch`, and `Heartbeat`. These RPCs now carry a token and authorize it against the registered worker row before mutating task or worker state. Accepted identities are the worker id itself, the provider owner username for that worker, or an admin.
- `PullBatch` now rejects missing/invalid/wrong tokens before claiming pending tasks for the requested worker.
- `CompleteBatch` now rejects missing/invalid/wrong tokens before completing or failing tasks under the requested worker id.
- `Heartbeat` now rejects missing/invalid/wrong tokens before updating worker status/heartbeat fields.
- Added coverage for missing-token rejection, non-owner spoofing rejection, and provider-owner success paths for all three batch runtime RPCs.
- Batch runtime completion data loss fixed for the existing task-row surface: `CompleteBatch` now persists stdout/stderr artifact references to the task log output and writes execution metrics (`cpu_time_ms`, `wall_time_ms`, `peak_memory_mb`, `download_bytes`, `cache_hits`) through a worker-guarded scheduler method. A repository regression test proves a wrong worker cannot write these batch report details for another worker's completed task.

Verification run after the tenth batch:

- `cd hivemind-rs; cargo test -p hivemind-node-manager batch_runtime_ -- --nocapture` first failed with the RED missing-token tests, then passed with 6 batch runtime auth tests after the fix.
- `cd hivemind-rs; cargo test -p hivemind-node-manager test_batch_runtime_complete_batch_persists_artifact_refs_and_metrics -- --nocapture` first failed with metrics still zero, then passed after the batch report persistence fix.
- `cd hivemind-rs; cargo test -p hivemind-task-scheduler record_batch_report -- --nocapture` passed.
- `cd hivemind-rs; cargo test -p hivemind-node-manager batch_runtime_complete_batch -- --nocapture` passed.
- `cd hivemind-rs; cargo test -p hivemind-task-scheduler -p hivemind-node-manager --lib -- --nocapture` passed with 36 scheduler tests and 33 node-manager tests.
- `cd hivemind-rs; cargo test -p hivemind-node-manager --lib -- --nocapture` passed with 32 tests.
- `cd hivemind-rs; cargo fmt --check` passed after formatting.
- `cd hivemind-rs; cargo clippy -p hivemind-task-scheduler -p hivemind-node-manager --all-targets --all-features -- -D warnings` passed.
- `cd hivemind-rs; cargo clippy -p hivemind-node-manager --all-targets --all-features -- -D warnings` passed.
- `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture` passed.
- `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings` passed.
- `cd hivemind-rs; cargo audit` passed.
- Repair14b read-only Codex subagents produced residual-risk reports for provider installer provenance and Postgres test isolation; no repair14b processes remained after cleanup.

Residual risk after this batch:

- Batch runtime stdout/stderr refs and execution metrics now persist to the task row. A fuller artifact-system integration could still insert downloadable stdout/stderr/result rows into the `artifacts` table, but the previous master-visible data loss in task log/metrics is fixed.
- Provider installer checksum/manifest provenance was fixed in the eleventh repair batch. Signed-release verification remains future hardening for untrusted distribution.
- Postgres-backed tests still use shared physical databases in several crates. Some known flakes are fixed, but the durable improvement is a per-test schema/database fixture.

### Ninth Repair Batch

- Worker output/result/usage persistence fixed on the nodepool-facing path. `NodeManagerService` now exposes `TaskOutputUpload`, `TaskResultUpload`, and `TaskUsage`; each request carries `worker_id`, validates a JWT, checks that the token subject is the registered worker id, provider owner, or admin, and persists through scheduler methods guarded by `task_id + worker_id`.
- The scheduler now has report-specific guarded writes: `complete_result_for_worker`, `record_output_for_worker`, and `update_resource_usage_for_worker`. These reject stale or wrong workers and avoid exposing the previous task-id-only usage update to worker-supplied report paths.
- Result-only completion no longer erases already reported output: `complete_guarded` now preserves existing `tasks.output` when the completion call has no replacement output.
- Node-manager report ingestion now rejects empty task ids, oversized output over 1 MiB, empty or oversized result references over 4096 bytes, missing usage, and non-finite usage before writing/finishing the task.
- Worker nodepool-client report helpers were added and verified against a fake node-manager gRPC service so future worker-push paths send worker-scoped report RPCs.

Verification run after the ninth batch:

- `cd hivemind-rs; cargo test -p hivemind-task-scheduler complete -- --nocapture` passed.
- `cd hivemind-rs; cargo test -p hivemind-node-manager worker_report_rpc -- --nocapture` passed.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor nodepool_client::tests -- --nocapture` passed.
- `cd hivemind-rs; cargo fmt --check` passed.
- `cd hivemind-rs; cargo clippy -p hivemind-task-scheduler -p hivemind-node-manager -p hivemind-worker-executor --all-targets --all-features -- -D warnings` passed.
- `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture` passed.
- `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings` passed.
- `cd hivemind-rs; cargo audit` passed.
- Repair13 read-only Codex subagents all returned reports and no active repair13 subagent process remained after recovery checks.

Residual risk after this batch:

- The worker-local `WorkerNodeService` report RPCs still store in memory only. Durable persistence now exists through node-manager, so the worker-local service should be treated as local/legacy behavior rather than the authoritative reporting path.
- The new worker-push report helpers are not automatically wired into `execute_task`; the current runtime still completes through `ExecuteTaskResponse`. Wiring both paths without a new state machine could double-complete tasks, so that should be a separate execution-mode design.
- Batch runtime worker identity/auth and master-visible stdout/stderr/metrics persistence were fixed in the tenth repair batch. Local downloadable artifact-table integration was added in the seventeenth repair batch, and precise local artifact selection was added in the eighteenth repair batch. Remote artifact materialization remains future hardening.

### Eighth Repair Batch

- Finding 9 Hivemind Rust dependency audit fixed with a clean audit gate: the workspace now uses updated `tonic`, `prost`, `tonic-build`, `tower`, and `sqlx` dependencies, with generated proto build code adjusted for the newer tonic-build API and model SQLx encode implementations adjusted for the newer SQLx return type. `cargo audit` from `hivemind-rs/` exits successfully and the previous vulnerable `rsa`, `sqlx 0.7.4`, and old `rustls-webpki 0.102.8` paths are no longer present in the active lockfile audit result.
- SQLx 0.9 scheduler test failure investigated and reduced to a test-isolation issue, not a production query regression. The three previously failing scheduler tests passed individually, the scheduler crate passed under normal and single-threaded execution, and the full workspace gate passed after strengthening the shared-Postgres `test_claim_pending_for_worker_does_not_overlap_between_repositories` fixture. That test now pre-cleans its own `claim-*` namespace, gives its four owned rows high priority, and limits each concurrent claim to two tasks so it still exercises `FOR UPDATE SKIP LOCKED` disjointness without assuming the global pending queue is otherwise empty.
- Removed the temporary `clippy::items_after_test_module` allow in worker gRPC by moving the test module to the end of `grpc_server.rs`. This is structural only; focused worker gRPC tests passed after the move.

Verification run after the eighth batch:

- `cd hivemind-rs; cargo test -p hivemind-task-scheduler test_claim_pending_for_worker_does_not_overlap_between_repositories -- --nocapture` passed after the test-isolation fix.
- `cd hivemind-rs; cargo test -p hivemind-task-scheduler --lib -- --nocapture` passed with 32 tests.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor grpc_server::tests -- --nocapture` passed with 8 worker gRPC tests after the test-module move.
- `cd hivemind-rs; cargo test --workspace --all-targets --all-features -- --nocapture` passed across the workspace.
- `cd hivemind-rs; cargo clippy --workspace --all-targets --all-features -- -D warnings` passed.
- `cd hivemind-rs; cargo fmt --check` passed.
- `cd hivemind-rs; cargo audit` passed with no vulnerabilities reported.
- Repair12 read-only Codex subagents reported that the SQLx 0.9 scheduler failures were consistent with shared database test isolation and that keeping SQLx 0.9 is appropriate when a clean `cargo audit` gate is required and full Postgres-backed tests pass.

Residual risk after this batch:

- The scheduler repository tests still share the default `hivemind_test` database. The immediate flake was fixed for the known overlap test, but the stronger long-term improvement is per-test database/schema isolation for all Postgres-backed tests.

### Seventh Repair Batch

- Finding 6 worker output/result/usage RPC gap fixed at the worker-service boundary: worker gRPC now validates JWTs, accepts bounded task output upload, returns uploaded task output, accepts bounded result references, and records resource usage after rejecting missing or non-finite usage payloads. The implementation is intentionally local to the worker process and in memory.
- Finding 12 Monty JS dependency audit fixed: `executor-rs/crates/monty-js/package-lock.json` was updated by a controlled npm audit fix, clearing the `brace-expansion`, `lodash`, `minimatch`, `picomatch`, and `tar` advisories while preserving package build, lint, unit, and smoke-test behavior.
- Finding 15 frontend development dependency audit fixed: root `frontend`, `frontend/master-ui`, and `frontend/worker-ui` now use updated Vite/PostCSS lockfiles. Full `npm audit --audit-level=moderate` passes for all three frontend packages, not only production-only audits.
- Finding 16 executor Rust dependency audit vulnerabilities fixed: `postcard` was updated and vulnerable/unsound transitive versions for `thin-vec`, `time`, and `rand` were updated. `cargo audit` from `executor-rs/` no longer reports vulnerabilities, though it still reports unmaintained `unic-*` warnings from the pinned Ruff dependency stack.

Verification run after the seventh batch:

- `cd hivemind-rs; cargo test -p hivemind-worker-executor grpc_server::tests -- --nocapture` passed with 8 tests covering worker output/result/usage RPCs and worker stop RPC behavior.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler --lib` passed with 106 tests before the broader dependency-gate work.
- `cd hivemind-rs; cargo clippy -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler -p hivemind-master-api -- -D warnings` passed.
- `cd frontend; npm audit --audit-level=moderate`, `cd frontend/master-ui; npm audit --audit-level=moderate`, and `cd frontend/worker-ui; npm audit --audit-level=moderate` passed.
- `npm run build` passed for `frontend/`, `frontend/master-ui/`, and `frontend/worker-ui`; `cd frontend/worker-ui; npm test` passed.
- `cd executor-rs/crates/monty-js; npm audit --audit-level=moderate`, `npm run build:debug`, `npm run lint`, `npm test`, and `npm run smoke-test` all passed.
- `cd executor-rs; cargo audit` exited successfully with only allowed unmaintained `unic-*` warnings; `cargo test --workspace --all-targets --all-features`, `cargo clippy --workspace --tests --all-features -- -D warnings`, and `cargo fmt --check` passed.

Residual risk after this batch:

- Worker output/result/usage reports are stored only in worker process memory and are not yet durable, not synchronized to the master database, and not scoped to task ownership or assignment. This is a minimal contract implementation rather than full end-to-end reporting persistence.
- Executor Rust audit still warns about unmaintained `unic-*` crates through pinned Ruff dependencies. Removing that warning likely requires moving the pinned Ruff revisions and should be handled as a separate compatibility effort.

### Fifth Repair Batch

- Finding 6 fixed for the active stop path: `hivemind-worker-executor` now keeps an active task registry keyed by task id, exposes `WorkerExecutor::stop_task_execution`, and wires worker gRPC `StopTaskExecution` to that registry. A stop request cancels the running executor wait path, kills the direct child process, and returns a failed task result with `Task execution stopped` rather than leaving the child to run after scheduler cancellation.
- Finding 6 fixed end to end through node-manager for assigned tasks: node-manager `StopTask` still records scheduler cancellation first, then uses the cancelled task row's `worker_ip` and the scheduler `worker_endpoint` normalizer to call worker `StopTaskExecution`. If the task was pending/queued with no worker endpoint, it reports scheduler cancellation only. If the worker is offline or returns an unsuccessful stop response, cancellation remains persisted and the response explicitly says worker stop was not confirmed.
- Additional task-stop race fix: push dispatch now reloads the current task immediately before `ExecuteTask` and skips the worker RPC unless the task is still assigned/running for the same worker. This closes the local window where a task could be cancelled after assignment but still be sent to a worker.
- Additional ZIP hardening: task ZIP extraction now performs a validation pass before writing files and rejects file/directory path conflicts such as `main.py` plus `main.py/child.py`. Validation failures do not leave partially extracted task files behind.

Residual risk after this batch:

- Worker output upload/result upload/task usage RPCs remain intentionally unimplemented and still return `Status::unimplemented`; this batch fixed live stop control, not the full worker RPC surface.

Verification run after the fifth batch:

- `cd hivemind-rs; cargo test -p hivemind-worker-executor stop_task_execution -- --nocapture` passed after worker registry/process-stop implementation.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor grpc_server::tests::stop_task_execution -- --nocapture` passed after worker gRPC stop wiring.
- `cd hivemind-rs; cargo test -p hivemind-node-manager test_stop_task_sends_stop_execution_to_assigned_worker -- --nocapture` failed before node-manager called worker `StopTaskExecution`, then passed.
- `cd hivemind-rs; cargo test -p hivemind-node-manager stop_task -- --nocapture` passed with 4 stop tests covering non-owner rejection, scheduler-only cancellation, worker stop dispatch, and worker-stop failure without DB rollback.
- `cd hivemind-rs; cargo test -p hivemind-task-scheduler test_execute_on_worker_skips_task_cancelled_after_assignment -- --nocapture` failed before the dispatcher preflight check, then passed.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor zip_extraction_rejects_file_directory_conflict_before_writing -- --nocapture` failed before ZIP conflict preflight, then passed.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler --lib` passed with 99 tests across the three crates.
- `cd hivemind-rs; cargo clippy -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler -p hivemind-master-api -- -D warnings` passed.
- `cd hivemind-rs; cargo fmt --check` passed.

### Sixth Repair Batch

- Finding 6 process-tree residual risk fixed for wrapper executors: worker-executor now configures Unix executor processes into their own process group and terminates that group on stop; on Windows it calls `taskkill /PID <pid> /T /F` before falling back to direct child kill. This prevents wrapper executors from leaving child/grandchild processes running after `StopTaskExecution`.

Verification run after the sixth batch:

- `cd hivemind-rs; cargo test -p hivemind-worker-executor stop_task_execution_kills_wrapper_spawned_child_process -- --nocapture` failed before process-tree termination, then passed after the fix.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor stop_task_execution -- --nocapture` passed with 5 stop tests covering unknown tasks, direct child stop, worker gRPC stop, and wrapper-spawned child cleanup.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor --lib` passed with 49 tests.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler --lib` passed with 100 tests across the three crates.
- `cd hivemind-rs; cargo clippy -p hivemind-worker-executor -p hivemind-node-manager -p hivemind-task-scheduler -p hivemind-master-api -- -D warnings` passed.
- `cd hivemind-rs; cargo fmt --check` passed.

### Fourth Repair Batch

- Finding 22 fixed for trusted local artifacts: `hivemind-worker-executor` no longer calls Monty with Hivemind-only task flags. It now prepares local `.py` files and `.zip` task packages from the configured task artifact directory, extracts top-level `main.py`, and invokes Monty with its native contract: `--max-duration`, `--max-memory`, and the script path. Remote torrent/artifact download remains unimplemented and is reported explicitly when no trusted local artifact is available.
- Finding 22 security hardening added: worker task sources are canonicalized and restricted to `config.torrent.api_dir`; user-controlled direct paths outside that artifact root are rejected. Magnet display-name resolution now verifies the selected local package's torrent info-hash against `xt=urn:btih:<hash>`, so `dn=<known.zip>` cannot be paired with an arbitrary BTIH to execute the wrong local package. ZIP extraction rejects unsafe paths and a directory named `main.py`.
- Finding 23 fixed: Worker UI first-login auto-registration now passes the freshly normalized `/api/worker-info` profile directly into registration instead of reading just-updated React state.
- Finding 24 fixed: Worker UI registration now includes the local `worker_id` from `/api/worker-info` in `POST /api/register-worker`, while keeping the logged-in username as the provider owner.
- Finding 6 partially reduced for user-facing/API honesty: the task `/stop` path still records scheduler cancellation rather than terminating a worker process, but node-manager, master API, and master UI no longer claim the task was stopped. Successful responses now report `Task cancellation recorded`, and the UI action/status says `Cancel` / cancellation request. Real worker process termination remains open under the worker `StopTaskExecution` implementation gap.
- Additional worker-executor resource-limit fix: Monty `--max-memory` now uses the lower of the global executor maximum and the task's requested memory, instead of always using `EXECUTOR_MAX_MEMORY_MB`. This prevents a smaller task request from being expanded to the global maximum at execution time.
- Additional worker-executor ZIP hardening: task package extraction now rejects excessive entry counts, duplicate normalized paths such as `main.py` plus `./main.py`, and total uncompressed size over the task storage budget before copying file contents. This reduces ZIP-bomb and ambiguous-entry risk in the local artifact execution path.

Verification run after the fourth batch:

- `cd hivemind-rs; cargo test -p hivemind-worker-executor local_task_source_rejects_paths_outside_artifact_dir -- --nocapture` failed before the artifact-root fix, then passed.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor magnet_display_name_rejects_mismatched_btih -- --nocapture` failed before BTIH verification, then passed.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor --lib` passed with 38 tests.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor -p hivemind-task-scheduler --lib` passed with 69 tests across both crates.
- `cd hivemind-rs; cargo clippy -p hivemind-worker-executor -- -D warnings` passed.
- `cd hivemind-rs; cargo fmt --check` passed.
- `cd frontend/worker-ui; npm test` passed with 2 Node tests covering profile normalization and register-worker payload construction.
- `cd frontend/worker-ui; npm run build` passed.
- `cd hivemind-rs; cargo test -p hivemind-node-manager test_stop_task_reports_cancellation_recorded -- --nocapture` failed on the old `Task stopped` message, then passed after the wording fix.
- `cd hivemind-rs; cargo test -p hivemind-task-scheduler overwrite -- --nocapture` passed, covering cancellation/completion race protections.
- `cd hivemind-rs; cargo clippy -p hivemind-node-manager -p hivemind-master-api -- -D warnings` passed.
- `cd frontend/master-ui; npm run build` passed.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor task_requested_memory_caps_monty_memory_argument -- --nocapture` failed before the memory mapping fix, then passed.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor memory -- --nocapture` passed after confirming zero/unspecified task memory uses the global executor memory limit.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor zip_task_rejects_duplicate_paths -- --nocapture` and `zip_task_rejects_too_many_entries -- --nocapture` failed before ZIP validation, then passed.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor zip_extraction_rejects_uncompressed_size_over_limit -- --nocapture` passed after the ZIP byte-limit helper was added.
- `cd hivemind-rs; cargo test -p hivemind-worker-executor --lib` passed with 43 tests after the memory/ZIP hardening fixes.

### Third Repair Batch

- Finding 13 fixed: `executor-rs/crates/monty-js/scripts/smoke-test.sh` is now LF-normalized in source control through `.gitattributes`, uses `/usr/bin/env bash`, runs with `set -euo pipefail`, installs the tarballs it just packed with `--no-save`, and cleans generated `npm/`, tarballs, and `optionalDependencies` on exit. The stale `pydantic-monty-1.0.0.tgz` smoke fixture dependencies were removed, and the smoke fixture type was updated for the current `MontyNameLookup` union return type. Verified by `bash -n`, `npm run type-check` in the smoke fixture, and full `npm run smoke-test` passing with 42 assertions.
- Finding 20 fixed for the generated Windows worker launcher: `scripts/package-worker-windows.ps1` now emits an `Import-DotEnv` parser that rejects malformed and duplicate keys, trims intentional whitespace/quotes, validates required worker settings, and rejects empty/default `JWT_SECRET` before launching the worker binary. Verified by rebuilding the debug package and exercising blank-template, valid-env, invalid-key, duplicate-key, and default-secret launcher cases.

Verification run after the third batch:

- `bash -n executor-rs/crates/monty-js/scripts/smoke-test.sh` passed.
- `cd executor-rs/crates/monty-js/smoke-test; npm run type-check` passed.
- `cd executor-rs/crates/monty-js; npm run smoke-test` passed after building, packing, installing, type-checking, and running 42 smoke assertions.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package-worker-windows.ps1 -Configuration debug -OutputDir dist\windows-worker-review-smoke3` passed.
- Generated `start-worker.ps1` rejected blank `WORKER_ADVERTISE_ADDR`, invalid dotenv key names, duplicate keys, and default `JWT_SECRET`; a valid env file reached the binary execution step.
- `git diff --check` on the third-batch changed files passed with line-ending warnings only.

### Second Repair Batch

- Finding 1 fixed: nodepool startup no longer seeds the public `testuser` account by default. Seeding now requires explicit `HIVEMIND_SEED_DEFAULT_USER=true`, the database helper log no longer prints the password, and the root frontend no longer advertises `testuser / testpass123`. Added and verified `default_user_seed_requires_explicit_truthy_env_value`.
- Finding 2 fixed for the current single-worker task model: VPN RPCs now validate JWT auth tokens before join/leave/status/peer operations, bind worker operations to the authenticated worker id or registered worker owner, and `GetTaskPeers` no longer returns all online peers. Added and verified `join_vpn_rejects_invalid_auth_token_before_creating_peer` and `get_task_peers_returns_only_peer_assigned_to_authorized_task`.
- Finding 3 fixed: master API and worker control API CORS now use configured exact origin allowlists instead of `Any`; methods and headers are narrowed. Added and verified CORS tests for master routes and worker control API.
- Finding 8 fixed for master/nodepool startup and packaging defaults: auth service startup rejects empty/default JWT secrets, `docker-compose.yml` now requires `JWT_SECRET`, and the Windows worker template no longer writes `change-me-in-production`. Added and verified JWT secret validation tests and `auth_service_startup_rejects_default_jwt_secret`.
- Finding 10 fixed for authenticated provider registration: `RegisterWorkerNodeRequest` now carries a distinct `worker_id`; master API registration can bind a provider owner to a separate worker id; node-manager preserves the distinction while keeping legacy fallback behavior. Added and verified `test_provider_can_manage_worker_registered_with_distinct_worker_id` and updated provider flow coverage.
- Finding 18 fixed: the root frontend keeps bearer tokens in React memory state only and no longer reads/writes `localStorage` for `hivemind_token`. Verified with root frontend build and static `rg`.
- Additional business-logic guard fixed: worker late completion can no longer overwrite `CANCELLED` tasks, and owner cancellation can no longer overwrite already completed tasks. Added and verified `test_complete_for_worker_does_not_overwrite_cancelled_task` and `test_cancel_does_not_overwrite_completed_task`.

Verification run after the second batch:

- `cd hivemind-rs; cargo test --workspace --all-targets --all-features` passed.
- `cd hivemind-rs; cargo clippy -- -D warnings` passed.
- `cd hivemind-rs; cargo fmt --check` passed.
- `cd executor-rs; cargo test --workspace --all-targets --all-features` passed.
- `cd executor-rs; cargo clippy --workspace --tests --all-features -- -D warnings` passed.
- `cd executor-rs; cargo fmt --check` passed with existing stable-rustfmt warnings about ignored nightly-only options.
- `npm run build` passed for `frontend/`, `frontend/master-ui/`, and `frontend/worker-ui`.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package-worker-windows.ps1 -Configuration debug -OutputDir dist\windows-worker-review-smoke2` passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File test_tasks\package_tasks.ps1` passed and produced three sample task ZIP archives.
- `git diff --check` passed with line-ending warnings only.

### First Repair Batch

- Finding 4 fixed: `GetAllUserTasks` now maps persisted task owner, output/result references, billing fields, runtime metrics, and deterministic state through a `Task` -> proto `TaskInfo` helper. Added and verified `test_get_all_user_tasks_preserves_persisted_task_fields`.
- Finding 5 fixed: admin billing overview now sums `payer_debit` ledger rows and counts `FAILED` rather than non-modeled `FAILING` pending-billing tasks. Added and verified `test_admin_billing_overview_uses_payer_debit_and_failed_pending_tasks`.
- Finding 7 fixed: removed the worker-executor `clippy::needless_return`; `cd hivemind-rs; cargo clippy -- -D warnings` now passes.
- Finding 11 fixed: inlined the Monty `time.sleep` format argument and fixed two additional executor test-target clippy failures exposed by the broader lint gate; `cd executor-rs; cargo clippy --workspace --tests --all-features -- -D warnings` now passes.
- Finding 14 fixed: restored `test_tasks/package_tasks.ps1` as valid PowerShell, made paths `$PSScriptRoot`-based, added three minimal sample task source directories, and verified the script packages all three ZIP archives.
- Finding 17 fixed: gated Monty benchmark `pprof` usage to non-Windows local runs, excluded the cargo-fuzz crate from normal executor workspace target selection, made the fuzz manifest standalone, and verified `cd executor-rs; cargo test --workspace --all-targets --all-features` passes.
- Finding 21 fixed: missing worker reputation rows are now treated as untrusted in both scheduler filtering and worker claim paths. Added and verified tests for trusted-worker filtering and claim blocking when the reputation row is absent.

Verification run after these fixes:

- `cd hivemind-rs; cargo test --workspace --all-targets --all-features` passed.
- `cd hivemind-rs; cargo clippy -- -D warnings` passed.
- `cd hivemind-rs; cargo fmt --check` passed.
- `cd executor-rs; cargo test --workspace --all-targets --all-features` passed.
- `cd executor-rs; cargo clippy --workspace --tests --all-features -- -D warnings` passed.
- `cd executor-rs; cargo fmt --check` passed with the existing stable-rustfmt warnings about ignored nightly-only options.
- `powershell -NoProfile -ExecutionPolicy Bypass -File test_tasks\package_tasks.ps1` passed and produced three sample task ZIP archives.

## Confirmed Findings

### 1. Production startup seeds a public default account

Status: Fixed. Default user seeding now requires explicit `HIVEMIND_SEED_DEFAULT_USER=true`, the password is no longer logged, and the root frontend no longer advertises the public test account.

- Severity: High
- Files: `hivemind-rs/crates/hivemind-bin/src/main.rs:73`, `hivemind-rs/crates/database/src/postgres.rs:321`, `hivemind-rs/crates/database/src/postgres.rs:328`, `hivemind-rs/crates/database/src/postgres.rs:329`, `frontend/src/App.jsx:579`
- Evidence: `seed_default_user` is called on nodepool startup and creates `testuser` / `testpass123` with balance 1000 if absent. Runtime API verification against local `hivemind-bin all` confirmed `POST /api/login` with `testuser` / `testpass123` returns `success=true` and issues a token.
- Impact: Any deployed instance that runs this startup path exposes a known credential with initial credit. The root frontend also tells users they can try `testuser / testpass123`, making the credential discoverable.
- Recommendation: Gate bootstrap account creation behind an explicit dev/test environment variable, remove it from normal startup, and remove public UI text that advertises the credential.

### 2. VPN RPC auth tokens and task scoping are ignored

Status: Fixed for the current single-worker task model. VPN RPCs validate JWT auth tokens, bind worker operations to the authenticated worker id or owner, and `GetTaskPeers` returns only the authorized assigned peer.

- Severity: High
- Files: `proto/vpn.proto:14`, `proto/vpn.proto:29`, `proto/vpn.proto:49`, `proto/vpn.proto:62`, `hivemind-rs/crates/vpn-service/src/grpc_server.rs:30`, `hivemind-rs/crates/vpn-service/src/grpc_server.rs:55`, `hivemind-rs/crates/vpn-service/src/lib.rs:40`, `hivemind-rs/crates/vpn-service/src/lib.rs:75`, `hivemind-rs/crates/vpn-service/src/lib.rs:85`, `hivemind-rs/crates/vpn-service/src/lib.rs:99`
- Evidence: The proto declares `auth_token` for Join/GetPeers/Leave/Update, but `join_vpn` takes `_auth_token` and never validates it. `leave_vpn` and `update_vpn_status` take only worker identity data. `get_task_peers` takes `_task_id` and returns every online VPN peer via `load_online_peers()`.
- Impact: A caller that can reach the VPN gRPC service can request preauth keys, remove/update peers by worker id, and enumerate all online peers rather than only peers for an authorized task.
- Recommendation: Validate JWT/claims on every VPN RPC, bind worker operations to the authenticated subject/registered worker, and return peers only for tasks the caller owns or participates in.

### 3. API CORS is open to arbitrary origins, including worker hardware profile

Status: Fixed. Master API and worker control API CORS now use configured exact origin allowlists instead of wildcard origins.

- Severity: Medium
- Files: `hivemind-rs/crates/master-api/src/routes.rs:12`, `hivemind-rs/crates/master-api/src/routes.rs:13`, `hivemind-rs/crates/master-api/src/routes.rs:14`, `hivemind-rs/crates/worker-executor/src/control_api.rs:53`, `hivemind-rs/crates/worker-executor/src/control_api.rs:54`, `hivemind-rs/crates/worker-executor/src/control_api.rs:55`
- Evidence: Both routers use `allow_origin(Any).allow_methods(Any).allow_headers(Any)`. Runtime verification with `Origin: http://evil.example` returned `Access-Control-Allow-Origin: *` for `GET /health` and for `GET http://127.0.0.1:18080/api/worker-info`; the worker endpoint returned worker id, address, CPU, memory, GPU name, GPU score, and storage.
- Impact: Any website can read unauthenticated worker profile data from a browser that can reach the worker control API. The master API also accepts broad cross-origin calls, which raises risk if future endpoints use cookies or browser-held credentials.
- Recommendation: Restrict CORS to configured UI origins and restrict worker control API binding/exposure. Keep unauthenticated worker info local-only or require a token if exposed beyond localhost.

### 4. User task list drops real owner, billing, runtime, and deterministic fields

Status: Fixed. `GetAllUserTasks` now maps persisted task owner, output/result references, billing fields, runtime metrics, and deterministic state through the task-to-proto helper.

- Severity: Medium
- Files: `hivemind-rs/crates/node-manager/src/grpc.rs:480`, `hivemind-rs/crates/node-manager/src/grpc.rs:483`, `hivemind-rs/crates/node-manager/src/grpc.rs:484`, `hivemind-rs/crates/node-manager/src/grpc.rs:486`, `hivemind-rs/crates/node-manager/src/grpc.rs:487`, `hivemind-rs/crates/node-manager/src/grpc.rs:492`
- Evidence: `GetAllUserTasks` maps persisted tasks to proto `TaskInfo` with `owner: String::new()`, billing fields set to `0/false`, runtime metrics set to `0`, and `deterministic: false`. Runtime API verification after task creation returned a task with `owner: "` and default billing/runtime fields even though the task belongs to the authenticated user.
- Impact: UIs and clients cannot rely on task list data for ownership, billing status, elapsed time, memory, retry, or deterministic-state decisions. This can hide billing state and produce incorrect user/admin behavior.
- Recommendation: Populate `TaskInfo` from the stored `Task` fields consistently with single-task result/log endpoints and add an integration test that asserts non-empty owner and real billing/runtime values.

### 5. Admin billing overview undercounts payer debits and pending billing

Status: Fixed. Admin billing overview now sums `payer_debit` ledger rows and counts `FAILED` pending-billing tasks.

- Severity: Medium
- Files: `hivemind-rs/crates/task-scheduler/src/task_repository.rs:297`, `hivemind-rs/crates/node-manager/src/grpc.rs:708`, `hivemind-rs/crates/node-manager/src/grpc.rs:711`
- Evidence: Billing settlement writes ledger entries with kind `payer_debit`, but admin overview sums payer debit with `kind='task_debit'`. The pending billing query also checks status `FAILING`, which is not a modeled `TaskStatus` variant; the model uses `FAILED`.
- Impact: Admin billing totals can report zero payer debits even when charges exist, and pending billing counts can miss failed/unsettled tasks. This corrupts financial reporting.
- Recommendation: Use the same ledger kind constants for writes and reporting, change `FAILING` to the intended status, and add tests covering admin billing totals after settlement and pending-billing scenarios.

### 6. Worker execution control RPCs are declared but not implemented

Status: Fixed for the worker-service boundary and active stop path. Worker gRPC now implements bounded output/result/usage reporting, validates JWTs, and wires `StopTaskExecution` to the active task registry; process-tree stop handling is implemented for Unix process groups and Windows `taskkill`.

- Severity: Medium
- Files: `proto/hivemind.proto:744`, `proto/hivemind.proto:745`, `proto/hivemind.proto:746`, `proto/hivemind.proto:747`, `proto/hivemind.proto:748`, `proto/hivemind.proto:749`, `hivemind-rs/crates/worker-executor/src/grpc_server.rs:109`, `hivemind-rs/crates/worker-executor/src/grpc_server.rs:124`, `hivemind-rs/crates/worker-executor/src/grpc_server.rs:134`, `hivemind-rs/crates/worker-executor/src/grpc_server.rs:145`, `hivemind-rs/crates/worker-executor/src/grpc_server.rs:162`
- Evidence: Worker proto exposes output upload, result upload, task output retrieval, stop execution, and usage reporting RPCs, but worker executor returns `Status::unimplemented` for each. Runtime API verification showed `POST /api/tasks/:task_id/stop` only cancelled the master/nodepool task record; the worker stop RPC path itself is unimplemented.
- Impact: Active executions cannot be stopped through the worker service, output/result upload paths are unavailable, and usage reporting cannot work through the declared contract. Task state can diverge from actual execution state.
- Recommendation: Either implement these RPCs end to end or remove/disable API affordances that imply live worker control until the contract is implemented.

### 7. Rust lint gate fails under repository lint command

Status: Fixed. The Hivemind Rust lint gate was repaired and `cd hivemind-rs; cargo clippy -- -D warnings` passes in the recorded repair evidence.

- Severity: Low
- Files: `hivemind-rs/crates/worker-executor/src/resource_monitor.rs:164`
- Evidence: `cargo clippy -- -D warnings` fails with `clippy::needless_return` at `return detect_gpus_windows();`. `cargo fmt --check` passes.
- Impact: The documented lint command and `make lint` cannot pass, so CI or release gates that enforce it will fail.
- Recommendation: Remove the needless `return` or allow the lint intentionally with a narrowly scoped reason.

### 8. Docker compose and worker packaging defaults keep a weak JWT secret

Status: Fixed. Master/nodepool startup rejects empty/default JWT secrets, `docker-compose.yml` requires `JWT_SECRET`, and the Windows worker package no longer writes `change-me-in-production`.

- Severity: Low to Medium
- Files: `docker-compose.yml:55`, `scripts/package-worker-windows.ps1:48`, `hivemind-rs/crates/config/src/lib.rs:177`, `hivemind-rs/crates/worker-executor/src/executor.rs:33`
- Evidence: Compose defaults `JWT_SECRET` to `change-me-in-production`, and the Windows worker package template writes the same value into `.env.worker.example`. Worker executor rejects this only when executing a task in production mode, but master/nodepool startup still accepts and uses the default secret.
- Impact: If a deployment forgets to set `JWT_SECRET`, issued JWTs are forgeable with a known secret until a worker task hits the executor-side guard.
- Recommendation: Fail startup for master/nodepool/all modes when the configured JWT secret is empty or a known default, and require compose deployments to provide a secret.

### 9. Rust dependency audit reports vulnerable crates

Status: Fixed. The Hivemind Rust workspace dependency stack was updated, and the current `cargo audit` run exits successfully with `quinn-proto` also patched to `0.11.15`.

- Severity: Medium
- Files: `hivemind-rs/Cargo.lock`
- Evidence: `cargo audit` reported 6 vulnerabilities: `rsa 0.9.10` RUSTSEC-2023-0071, `rustls-webpki 0.102.8` RUSTSEC-2026-0049 / 0098 / 0099 / 0104, and `sqlx 0.7.4` RUSTSEC-2024-0363. It also reported unmaintained warnings for `paste 1.0.15` and `rustls-pemfile 2.2.0`.
- Impact: The workspace currently depends on crates with known security advisories, including TLS certificate validation issues and SQLx binary protocol parsing issues.
- Recommendation: Upgrade `sqlx` to at least `0.8.1`, update TLS dependencies so `rustls-webpki` is at least `0.103.13`, and review whether `rsa` is reachable in production paths or can be replaced/isolated.

### 10. Provider worker settings cannot be managed by the provider account for self-registered workers

Status: Fixed for authenticated provider registration. Registration now carries a distinct `worker_id` while preserving the authenticated provider owner, and provider settings coverage verifies the owner can manage the separately identified worker.

- Severity: Medium
- Files: `hivemind-rs/crates/node-manager/src/grpc.rs:274`, `hivemind-rs/crates/node-manager/src/grpc.rs:1280`, `hivemind-rs/crates/node-manager/src/grpc.rs:1296`
- Evidence: Worker registration maps `worker_id` and `username` from the request username. The worker auto-registration loop uses `WORKER_ID`, so the worker row owner becomes the worker id, not the provider account. Runtime verification with a provider account against its local auto-registered worker returned `Not authorized` for both `GET` and `PUT /api/provider/workers/:worker_id/settings`.
- Impact: Provider users cannot manage settings for workers that auto-register under machine/worker ids, unless they are admins or log in as the worker id. This breaks provider workflow expectations.
- Recommendation: Separate worker id from owning username in registration/auth, or require authenticated provider registration that binds workers to the provider account.

### 11. Executor Rust lint gate fails under workspace test command

Status: Fixed. Monty runtime/test-target clippy issues were repaired, and the recorded executor gate `cd executor-rs; cargo clippy --workspace --tests --all-features -- -D warnings` passes.

- Severity: Low
- Files: `executor-rs/crates/monty/src/modules/time.rs:121`
- Evidence: `cargo clippy --workspace --tests --all-features -- -D warnings` from `executor-rs/` fails with `clippy::uninlined_format_args` at the `format!` call that builds the float conversion type error. `cargo test --workspace` passed, and `cargo fmt --check` returned exit 0 with warnings that stable rustfmt ignores nightly-only options.
- Impact: The executor subproject cannot pass a strict lint gate even though its tests pass.
- Recommendation: Use inline format arguments or allow the lint intentionally with a narrow justification, then rerun the executor clippy command.

### 12. Executor JavaScript dependency audit reports vulnerable packages

Status: Fixed. `executor-rs/crates/monty-js/package-lock.json` was updated by a controlled audit fix and the package build, lint, unit, and smoke-test gates passed in the recorded repair evidence.

- Severity: Medium
- Files: `executor-rs/crates/monty-js/package-lock.json`
- Evidence: `npm audit --audit-level=moderate` from `executor-rs/crates/monty-js` failed with 5 vulnerabilities: `brace-expansion` moderate, plus high-severity advisories for `lodash`, `minimatch`, `picomatch`, and `tar`. `npm test` passed after `npm run build:debug`, and `npm run lint` passed, so this is a dependency hygiene/security issue rather than a unit-test failure.
- Impact: Packaging/build tooling currently pulls vulnerable transitive dependencies, including ReDoS, prototype pollution/code injection, and tar hardlink path traversal advisories.
- Recommendation: Run a controlled `npm audit fix`, review lockfile changes, and verify `npm run build:debug`, `npm test`, `npm run lint`, and package smoke testing afterward.

### 13. Monty JS package smoke test is not runnable from the current Windows/bash environment

Status: Fixed. The smoke-test script is LF-normalized via `.gitattributes`, uses `/usr/bin/env bash`, runs with strict shell options, and the recorded `npm run smoke-test` gate passes.

- Severity: Low to Medium
- Files: `executor-rs/crates/monty-js/package.json:62`, `executor-rs/crates/monty-js/scripts/smoke-test.sh:2`, `executor-rs/crates/monty-js/scripts/smoke-test.sh:10`
- Evidence: `npm run smoke-test` failed immediately under bash with CRLF parsing symptoms: `set: -\r: invalid option`, `cd: ...scripts\r/..: No such file or directory`, and `npm error Missing script: build\r`. The script itself uses bash and starts with `set -e`, then calls `npm run build`.
- Impact: The package-level smoke test cannot validate the publish/install flow in this environment, so release packaging confidence is lower than the unit test result suggests.
- Recommendation: Normalize this shell script to LF in source control and add a cross-platform invocation path or CI check that proves the smoke test runs on Windows-hosted development machines.

### 14. Test task packaging script is syntactically broken

Status: Fixed. `test_tasks/package_tasks.ps1` was restored as valid PowerShell with `$PSScriptRoot`-based paths and verified to generate the three sample task ZIP archives.

- Severity: Low to Medium
- Files: `test_tasks/package_tasks.ps1:34`
- Evidence: `powershell -ExecutionPolicy Bypass -File test_tasks/package_tasks.ps1` fails during parsing with a missing string terminator at line 34. The file also displays garbled text around its user-facing messages, consistent with content or encoding corruption.
- Impact: Developers cannot package the sample test tasks through the documented helper script, which weakens runnable test coverage for sample workloads.
- Recommendation: Restore the script text with valid PowerShell syntax and a consistent encoding, then run it to confirm the expected ZIP files are generated.

### 15. Frontend development dependencies have known vulnerabilities

Status: Fixed. Root, master UI, and worker UI frontend lockfiles were updated so full `npm audit --audit-level=moderate` passes for all three packages, not only production-only audits.

- Severity: Low to Medium
- Files: `frontend/package-lock.json:1274`, `frontend/package-lock.json:1442`, `frontend/package-lock.json:1612`, `frontend/master-ui/package-lock.json:1274`, `frontend/master-ui/package-lock.json:1612`, `frontend/worker-ui/package-lock.json:1274`, `frontend/worker-ui/package-lock.json:1612`
- Evidence: Full `npm audit --audit-level=moderate` failed for all three frontend packages. Root `frontend/` reports vulnerable `esbuild <=0.24.2`, `postcss <8.5.10`, and Vite depending on vulnerable esbuild. `frontend/master-ui/` and `frontend/worker-ui/` report vulnerable esbuild/Vite. Production-only `npm audit --omit=dev` still passes, so this is dev-tooling exposure rather than a production dependency issue.
- Impact: Local or CI dev servers can be affected by Vite/esbuild development-server advisories, and CSS stringify output may carry PostCSS XSS risk in build tooling contexts.
- Recommendation: Upgrade Vite/plugin lockfiles in all frontend packages, run `npm audit` without `--omit=dev`, and rerun all three `npm run build` commands.

### 16. Executor Rust dependency audit reports vulnerable and unmaintained crates

Status: Vulnerabilities fixed; residual unmaintained dependency warnings remain documented. The executor Rust audit no longer reports vulnerable `thin-vec`, `time`, or unsound `rand` paths, but still allows `unic-*` warnings through the pinned Ruff/Ty dependency stack.

- Severity: Medium
- Files: `executor-rs/Cargo.lock:165`, `executor-rs/Cargo.lock:2184`, `executor-rs/Cargo.lock:2195`, `executor-rs/Cargo.lock:2958`, `executor-rs/Cargo.lock:2984`, `executor-rs/Cargo.lock:3170`, `executor-rs/Cargo.lock:3179`, `executor-rs/Cargo.lock:3185`, `executor-rs/Cargo.lock:3191`, `executor-rs/Cargo.lock:3203`
- Evidence: `cargo audit` from `executor-rs/` failed with 2 vulnerabilities: `thin-vec 0.2.14` RUSTSEC-2026-0103 and `time 0.3.44` RUSTSEC-2026-0009. It also reported unmaintained warnings for `atomic-polyfill` and several `unic-*` crates, plus unsound warnings for `rand 0.8.5` and `rand 0.9.2`.
- Impact: The executor subproject depends on crates with known memory-safety and denial-of-service advisories, plus several unmaintained/unsound dependency warnings.
- Recommendation: Upgrade affected transitive dependencies where possible, review whether vulnerable crates are reachable in shipped executor paths, and rerun `cargo audit`.

### 17. Executor all-target/all-feature test gate fails on bench and fuzz targets

Status: Fixed. Monty benchmark profiler usage is gated away from Windows local all-target runs, the cargo-fuzz crate is excluded from normal executor workspace target selection, and the recorded executor all-target/all-feature gate passes.

- Severity: Low to Medium
- Files: `executor-rs/crates/monty/benches/main.rs:6`, `executor-rs/crates/monty/benches/main.rs:7`, `executor-rs/crates/monty/Cargo.toml:53`, `executor-rs/crates/monty/Cargo.toml:54`, `executor-rs/crates/fuzz/Cargo.toml:15`, `executor-rs/crates/fuzz/Cargo.toml:22`
- Evidence: `cargo test --workspace --all-targets --all-features` from `executor-rs/` failed. The benchmark imports `pprof` under `cfg(not(codspeed))`, but `pprof` is only declared for `cfg(not(windows))`, so Windows all-target builds cannot resolve it. The same command also tries to link fuzz binaries and fails with `LINK : fatal error LNK1561: entry point must be defined` for `string_input_panic` and `tokens_input_panic`.
- Impact: The broadest executor test gate cannot run on this Windows environment, so CI or release validation that uses all targets/features will fail before running tests.
- Recommendation: Gate the benchmark profiler import on both `not(codspeed)` and `not(windows)` or provide a Windows-compatible dependency path; exclude cargo-fuzz targets from normal all-target tests or configure them for cargo-fuzz only.

### 18. Root frontend stores bearer tokens in localStorage

Status: Fixed. The root frontend now keeps bearer tokens in React memory state only; static search of frontend source found no remaining `localStorage` or `hivemind_token` token persistence.

- Severity: Low to Medium
- Files: `frontend/src/App.jsx:144`, `frontend/src/App.jsx:161`, `frontend/src/App.jsx:264`, `frontend/src/App.jsx:276`
- Evidence: The root frontend initializes auth state from `localStorage.getItem(hivemind_token)`, sends it as an Authorization bearer token, stores new login tokens with `localStorage.setItem`, and removes them on logout.
- Impact: Any XSS in the root frontend can steal long-lived bearer tokens from localStorage. This is especially relevant because the app uses broad CORS elsewhere and the root frontend advertises a default account.
- Recommendation: Prefer HttpOnly/SameSite cookies or short-lived in-memory tokens with refresh-token hardening, and add CSP/escaping coverage before keeping bearer tokens in localStorage.

### 19. Provider installer updates copy unsigned local artifacts

Status: Fixed across the eleventh and sixteenth repair batches. Install/update scripts first gained checksum verification, then were hardened to require signed `SHA256SUMS` manifests verified with a trusted public key outside the untrusted `release/` directory.

- Severity: Medium
- Files: `subagents/provider-installer/install-worker.ps1:34`, `subagents/provider-installer/install-worker.ps1:38`, `subagents/provider-installer/update-worker.ps1:12`, `subagents/provider-installer/update-worker.ps1:17`, `subagents/provider-installer/install-worker.sh:47`, `subagents/provider-installer/update-worker.sh:18`, `subagents/provider-installer/README.md:36`, `subagents/provider-installer/README.md:37`
- Evidence: RED checks first proved matching but unsigned `SHA256SUMS` let PowerShell and Bash update scripts install artifacts. After the fix, unsigned manifests, tampered signed manifests, and release-directory public-key substitution are rejected; signed install/update checks pass when `HIVEMIND_RELEASE_PUBLIC_KEY` or a trusted `release-public-key.pem` is provided outside `release/`.
- Impact: A replaced local release artifact can no longer be installed by replacing only the artifact and checksum manifest; the attacker would also need the trusted signing private key.
- Recommendation: Keep private signing keys offline and distribute trusted public keys through operator-controlled configuration or installer packaging, not the artifact `release/` directory.

### 20. Windows worker launcher uses a fragile dotenv parser

Status: Fixed for the generated Windows worker launcher. The package script now emits `Import-DotEnv`, rejects malformed and duplicate keys, validates required worker settings, and rejects empty/default `JWT_SECRET`; `scripts/package-worker-windows.Tests.ps1` passes.

- Severity: Low
- Files: `scripts/package-worker-windows.ps1:71`, `scripts/package-worker-windows.ps1:74`, `scripts/package-worker-windows.ps1:76`
- Evidence: A Codex packaging subagent and local review both confirmed the generated launcher loads `.env.worker` by trimming each line and splitting on the first equals sign. It does not handle quoted values, inline comments, escapes, or surrounding whitespace the way common dotenv parsers do.
- Impact: Providers editing `.env.worker` like a normal dotenv file can accidentally pass quotes/comments as literal environment values, causing hard-to-diagnose worker configuration errors.
- Recommendation: Use a small dotenv parser or document/enforce a strict `KEY=value` format and reject unsupported syntax instead of silently accepting it.

### 21. Missing worker reputation rows bypass trust filtering

Status: Fixed. Missing reputation rows are now default-denied in scheduler claim and trusted-worker filtering paths, with regression tests `test_trusted_workers_excludes_missing_reputation_rows` and `test_claim_pending_for_worker_blocks_missing_reputation_row`.

- Severity: Low to Medium
- Files: `hivemind-rs/crates/task-scheduler/src/task_repository.rs:132`, `hivemind-rs/crates/task-scheduler/src/task_repository.rs:138`, `hivemind-rs/crates/node-manager/src/grpc.rs:266`, `hivemind-rs/crates/database/src/postgres.rs:155`
- Evidence: Normal worker registration inserts a default `worker_reputation` row, but `claim_pending_for_worker` only blocks low-score/banned workers when a reputation row exists. If the row is missing because of data drift, manual insertion, or cleanup, the worker can claim tasks without passing the `MIN_WORKER_REPUTATION_SCORE` gate.
- Impact: Trust enforcement depends on reputation rows remaining present and consistent with worker rows. Data inconsistency can silently degrade the scheduler to trusting unknown workers.
- Recommendation: Treat missing reputation rows as untrusted or auto-create them transactionally during worker registration/upsert, and add tests covering missing-reputation workers in both dispatch and claim paths.

### 22. Worker executor calls Monty CLI with unsupported task flags

Status: Fixed for trusted local task artifacts. The worker executor now materializes local `.py` and `.zip` packages from the configured task artifact directory and invokes Monty with its supported file contract: `--max-duration`, `--max-memory`, and the script path. Remote torrent/artifact download remains explicitly unimplemented when no trusted local artifact is available.

- Severity: High
- Files: `hivemind-rs/crates/worker-executor/src/executor.rs:123`, `executor-rs/crates/monty-cli/src/main.rs:39`, `scripts/package-worker-windows.ps1:49`, `hivemind-rs/crates/config/src/lib.rs:145`
- Evidence: The worker executor invokes the configured Monty executable with flags such as `--task-id`, `--torrent-source`, `--btih`, `--max-cpu-percent`, `--max-memory-mb`, `--max-storage-mb`, `--max-wall-time-secs`, `--gpu-required`, and `--vram-required-mb`. The Monty CLI definition accepts interactive/type-check/code/file options and resource flags such as `--max-allocations`, `--max-duration`, and `--max-memory`, but not those worker task flags. The Windows worker package and default config point providers at `monty.exe`.
- Impact: Real assigned worker tasks can fail at CLI argument parsing before running the workload, even though scheduler, packaging, and smoke tests pass independently.
- Recommendation: Add a worker-executor integration test using the real Monty CLI argument contract, then either adapt worker execution to the supported CLI/API or provide a Hivemind-specific executor wrapper that accepts the worker task flags.

### 23. Worker UI first login can register stale or zero local capacity

Status: Fixed. First-login registration now passes the freshly fetched `/api/worker-info` profile and endpoint directly into registration, and Worker UI tests cover normalized local worker info before React state updates.

- Severity: Medium
- Files: `frontend/worker-ui/src/App.jsx:88`, `frontend/worker-ui/src/App.jsx:113`, `frontend/worker-ui/src/App.jsx:173`
- Evidence: `refreshLocalProfile()` fetches `/api/worker-info`, updates React state, and returns the profile, but `handleLogin()` then calls `registerWorker(authToken)` which reads `workerIp` and `profile` from React state. Because React state updates are async, first-login registration can use `emptyProfile` and the previous endpoint instead of the freshly fetched local worker profile.
- Impact: Provider registration can create or update worker rows with zero/stale CPU, memory, GPU, and IP values, which degrades scheduling and provider workflow correctness.
- Recommendation: Pass the fetched profile and worker address directly into `registerWorker`, or make `registerWorker` accept explicit values and avoid reading just-updated state.

### 24. Worker UI does not send the local worker id during provider registration

Status: Fixed. Worker UI registration now includes the local `worker_id` from `/api/worker-info` while keeping the authenticated username as the provider owner; Worker UI tests verify the registration payload.

- Severity: Medium
- Files: `frontend/worker-ui/src/App.jsx:122`, `frontend/worker-ui/src/App.jsx:141`, `hivemind-rs/crates/master-api/src/handlers.rs:1099`, `hivemind-rs/crates/hivemind-bin/src/main.rs:185`
- Evidence: The worker UI registration body sends provider username and resource fields, but no `worker_id`; master API defaults missing `worker_id` to the authenticated owner. The actual worker process derives its local identity from `WORKER_ID`, `COMPUTERNAME`, `HOSTNAME`, or UUID.
- Impact: Provider UI registration can target a row keyed by the login name while the real worker registration loop uses a different machine/worker id, leaving settings and resource data split across rows.
- Recommendation: Include the local worker id from `/api/worker-info` in the worker UI registration request and keep the authenticated username as owner only.


### 25. New quinn-proto remote memory exhaustion vulnerability (RUSTSEC-2026-0185)

Status: Fixed. `hivemind-rs/Cargo.lock` now resolves `quinn-proto` to `0.11.15`, and `cargo audit` passes for the Hivemind Rust workspace.

- Severity: High
- Date: 2026-06-27 (advisory published 2026-06-22)
- ID: RUSTSEC-2026-0185 (CVSS 7.5)
- Crate: quinn-proto v0.11.15
- Evidence: cargo audit previously found this advisory against `quinn-proto v0.11.14` after earlier rounds passed clean. Remote memory exhaustion in quinn-proto came from unbounded out-of-stream order reassembly and affected Hivemind's QUIC transport layer when using the default transport stack. The lockfile has been updated to the patched `0.11.15` release.
- Fix: Upgraded `quinn-proto` to `0.11.15`. The patched version limits the amount of memory consumed by buffering out-of-order stream data.
- Recommendation: Keep `cargo audit` in the Hivemind Rust verification gate so newly published transport advisories are caught quickly.

### 26. New memmap2 unsound advisory in executor-rs (RUSTSEC-2026-0186)

Status: Fixed. `executor-rs/Cargo.lock` now resolves `memmap2` to `0.9.11`; `cargo audit` for the executor workspace no longer reports RUSTSEC-2026-0186 and exits successfully with only the existing allowed `unic-*` unmaintained warnings.

- Severity: Warning (allowed)
- Date: 2026-06-27 (advisory published 2026-06-20)
- ID: RUSTSEC-2026-0186
- Crate: memmap2 v0.9.11
- Evidence: executor-rs cargo audit previously reported an allowed warning for unchecked pointer offset in `memmap2 v0.9.9`. After `cargo update -p memmap2`, the lockfile uses `0.9.11` and the warning is no longer present.
- Recommendation: Keep executor `cargo audit` in the verification gate and continue tracking the separate allowed `unic-*` unmaintained warnings.

### 27. Monty runtime does not predefine `__name__` for script-style task entry points

Status: Fixed. Added a regression test for `if __name__ == "__main__"` script guards, seeded script-level `__name__` to `"__main__"` when referenced, rebuilt `monty.exe`, and confirmed all three repository sample task scripts now exit 0 and print the expected output.

- Severity: Medium
- Files: `test_tasks/01_hello_world/main.py`, `test_tasks/02_math_compute/main.py`, `test_tasks/03_text_processing/main.py`, `executor-rs/crates/monty/src/run.rs`, `executor-rs/crates/monty/tests/main.rs`
- Evidence: When I submitted the repository sample ZIP tasks to a live all-mode instance, all three failed with `NameError: name '__name__' is not defined` at the usual `if __name__ == "__main__"` guard. Re-running with equivalent top-level scripts succeeded. The runtime already has internal `__name__` handling for functions and type introspection, but script-level task execution does not seed a module `__name__` value.
- Impact: Common Python entry-point patterns in sample tasks and real user uploads fail even though the same code would run under CPython and in most task runners.
- Recommendation: Completed for the Monty execution path; keep the regression test in place so script-style task entry points remain supported.

## Tooling / Coverage Notes

- Docker integration test command could not complete because Docker Desktop returned a 500 error while reading `redis:7-alpine` image metadata. This is an environment/tooling failure, not a code test failure.
- Docker integration remained blocked after retrying both `desktop-linux` and `default` Docker contexts; both returned Docker API 500 on `/version`.
- `psql` is not in PATH, so direct ad hoc SQL verification was unavailable; runtime API verification was used instead.

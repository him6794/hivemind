# Centralized Compute Marketplace State

## Goal
Turn Hivemind into a centralized compute marketplace: requestors buy compute with platform-managed credits, providers contribute worker capacity, and the platform handles trust, pricing, scheduling, billing, and payout accounting.

## Success Criteria
- Requestor can submit a job with resource requirements and a budget.
- Provider can register a worker with resource caps and price preferences.
- Platform records task charges and provider earnings in durable ledgers.
- Worker scheduling can consider capability, availability, and eventually price.
- Public release is gated by sandbox, abuse, and observability checks.

## Status
running

## Current Step
Worker pull path now enforces trust bans; next slice is full low-score claim blocking test coverage.

## Completed Work
- Loaded planning and long-task coordination instructions.
- Confirmed repo is dirty with many existing modified files; changes must be scoped and non-destructive.
- Spawned three explorer subagents for backend, frontend/DevEx, and billing/security analysis.
- Created durable planning/state files.
- Consolidated audits and selected the first implementation slice: append-only ledger plus idempotent settlement.
- Dispatched ledger implementation worker with TDD requirement and limited write scope.
- Integrated ledger worker result: append-only `ledger_entries`, model enums, and idempotent task settlement.
- Dispatched and integrated browser ZIP upload worker result: authenticated `POST /api/tasks/upload`.
- Ran focused and crate-level verification for the touched Rust crates.
- Resumed work and selected provider onboarding as the next highest-leverage gap.
- Integrated worker-control HTTP API for local provider onboarding.
- Wired Docker/worker-ui build configuration to the worker-control endpoint.
- Integrated worker ownership hardening for HTTP worker registration.
- Integrated requestor pricing quote API and budget guardrails.
- Integrated provider earnings query API over the append-only ledger.
- Integrated provider resource caps/settings API for provider-owned workers.
- Integrated scheduler filtering for provider enablement, effective caps, availability, and minimum price.
- Integrated Worker UI provider control panel for registration, settings, earnings, and worker list.
- Integrated requestor CLI `hivemind submit <job.zip>` for low-friction ZIP task submission.
- Integrated requestor CLI `hivemind status <task-id>` and `hivemind result <task-id>`.
- Integrated guarded task assignment so stale dispatch cannot overwrite an assigned worker.
- Integrated row-locked batch claiming for worker pull so concurrent claimers do not overlap.
- Integrated worker-scoped completion/failure/reset fencing so stale worker results cannot mutate a redispatched task.
- Fixed and verified `subagents/sandbox-egress` compile/test health.
- Integrated executor network egress configuration, policy validation, and production release gate.
- Added provider installer scaffold scripts for Windows/Linux install/update flows in `subagents/provider-installer`.
- Integrated marketplace settlement split: payer debit + provider credit + platform fee (10% default).
- Added authenticated admin billing overview endpoint for settled totals and pending billing count.
- Added trust persistence tables: `worker_reputation` and `task_attestations`.
- Added artifact lifecycle table `artifacts` with dedup/resume/expiry metadata.
- Integrated scheduler-side trust writes on task completion/failure (reputation and attestation events).
- Added provider worker trust profile API and admin artifact overview API.
- Integrated trust enforcement in dispatcher: banned/low-score workers are excluded before scheduling.
- Integrated artifact retention cleanup workflow with dry-run and execution modes.
- Integrated reusable cleanup core and periodic cleanup loop in master runtime (`ARTIFACT_CLEANUP_ENABLED`, `ARTIFACT_CLEANUP_INTERVAL_SECS`).
- Integrated cache affinity ranking before worker selection (historical completed tasks with matching torrent source increase worker priority).
- Tuned cache affinity with recency weighting (last 7 days weighted higher) and `cache_hits` additive signal.
- Added admin scheduling cache metrics endpoint with global totals, hit rate, and top-worker breakdown.
- Added admin scheduling cache alert endpoint with configurable thresholds (`low`, `high`) and server-side severity classification.
- Added admin worker trust-control endpoint for `banned` and `score` updates on `worker_reputation`.
- Added admin authorization check (`HIVEMIND_ADMIN_USERS`, default `testuser`) for all `/api/admin/*` handlers.
- Added explicit integration test proving non-admin users are forbidden from admin read/write endpoints.
- Added `HIVEMIND_ADMIN_USERS` to `.env.example` and README configuration table.
- Added per-user task submission rate limit in `create_task`/`upload_task` path (`HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE`).
- Added trust gate in `claim_pending_for_worker` so banned workers cannot claim tasks through worker pull path.

## Active Owners
- Coordinator: parent Codex agent.
- Backend audit: subagent `019e7d3d-edb8-7df0-abf0-af83aca09815`.
- Frontend/DevEx audit: subagent `019e7d3e-2bdf-7150-8b02-cb7b8148af98`.
- Billing/security audit: subagent `019e7d3e-585f-7931-b254-c13cf8da07f6`.
- Ledger implementation: subagent `019e7d40-f91f-75b1-9cca-4a9335ea7044`.
- Browser ZIP upload implementation: subagent `019e7d48-bc07-7750-a10f-14240fa7a79c`.
- Worker control API implementation: subagent `019e7d50-74c7-7381-bd37-d4ab40746c2e`.
- Worker ownership hardening: subagent `019e7d59-a832-75b3-bbf7-ad6300c871d5`.
- Pricing quote and budget guardrails: subagent `019e7de1-4e84-7a80-a4ac-8f604ebdacc9`.
- Provider earnings API: parent-integrated after attempted worker handoff became unavailable.
- Provider resource caps/settings explorer: subagent `019e7e03-45e0-7353-86a5-cb3b49f9594a`.
- Provider resource caps/settings implementation: parent Codex agent.
- Scheduler provider settings explorer: subagent `019e7e0f-e3ac-7182-9c87-e91c867eff17`.
- Scheduler effective caps/price implementation: parent Codex agent.
- Worker UI provider controls explorer: subagent `019e7e14-d5d9-7951-8b1a-80aae238c9b1`.
- Worker UI provider controls implementation: parent Codex agent.
- Requestor CLI submit explorer: subagent `019e803d-9359-7332-a060-bd3b39c187f2`.
- Requestor CLI submit implementation: parent Codex agent.
- Requestor CLI status/result explorer: subagent `019e8067-d12a-73c2-ba4d-7abd6a1ca069`.
- Requestor CLI status/result implementation: parent Codex agent.
- Atomic assignment explorer: subagent `019e806d-6eb1-7482-9100-2563358672e9`.
- Guarded task assignment implementation: parent Codex agent.
- Batch claim implementation: parent Codex agent.
- Stale worker result fencing implementation: parent Codex agent.

## Next Action
Add low-score claim-blocking coverage and align pull-path trust thresholds with dispatcher policy docs.

## Next Checkpoint
When beginning the next implementation round.

## Blockers
- None yet.

## Verification
- `cargo fmt --check`: pass.
- `cargo test -p hivemind-database postgres::tests::test_migration_idempotent -- --nocapture`: pass.
- `cargo test -p hivemind-database`: pass.
- `cargo test -p hivemind-task-scheduler task_repository::tests::test_complete -- --nocapture`: pass.
- `cargo test -p hivemind-task-scheduler -- --test-threads=1`: pass.
- `cargo test -p hivemind-master-api multipart_upload -- --nocapture`: pass.
- `cargo test -p hivemind-master-api -- --test-threads=1`: pass.
- `cargo test -p hivemind-worker-executor control_api --no-default-features`: pass.
- `cargo test -p hivemind-worker-executor --no-default-features`: pass.
- `cargo test -p hivemind-bin --no-default-features`: pass.
- `npm run build` in `frontend/worker-ui`: pass.
- `npm run build` in `frontend/master-ui`: pass.
- `docker compose config`: pass.
- `cargo test -p hivemind-master-api test_register_worker_ -- --nocapture`: pass.
- `cargo test -p hivemind-master-api -- --test-threads=1`: pass.
- `cargo test -p hivemind-master-api quote -- --nocapture`: pass.
- `cargo test -p hivemind-master-api -- --test-threads=1`: pass, including multipart low-budget rejection.
- `cargo test -p hivemind-master-api provider_earnings -- --nocapture`: pass.
- `cargo test -p hivemind-master-api -- --test-threads=1`: pass, including provider earnings.
- `cargo fmt --check`: pass after rustfmt.
- `cargo test -p hivemind-master-api provider_can_update -- --nocapture`: failed first with `404`, then passed.
- `cargo test -p hivemind-database`: pass.
- `cargo test -p hivemind-node-manager --no-default-features`: pass.
- `cargo test -p hivemind-task-scheduler --no-default-features -- --test-threads=1`: pass.
- `cargo test -p hivemind-master-api -- --test-threads=1`: pass, including provider settings.
- `cargo fmt --check`: pass.
- `cargo test -p hivemind-task-scheduler scheduler::tests::test_provider_settings_filter -- --nocapture`: failed first by selecting an ineligible worker, then passed.
- `cargo test -p hivemind-task-scheduler scheduler::tests::test_provider_settings_use_available -- --nocapture`: failed first by sorting on raw storage, then passed.
- `cargo test -p hivemind-task-scheduler --no-default-features -- --test-threads=1`: pass, including effective caps/price filtering.
- `cargo test -p hivemind-node-manager --no-default-features`: pass, including available-memory persistence.
- `cargo fmt --check`: pass.
- `npm run build` in `frontend/worker-ui`: pass.
- `npm run build -- --outDir .tmp-build --emptyOutDir` in `frontend/worker-ui`: pass after a `D:\tmp` permissions failure unrelated to app compilation.
- `cargo test -p hivemind-bin cli::tests:: --no-default-features -- --nocapture`: pass.
- `cargo test -p hivemind-bin --no-default-features`: pass.
- `cargo fmt --check`: pass.
- `cargo test -p hivemind-bin cli::tests:: --no-default-features -- --nocapture`: failed first for missing status/result implementation, then passed.
- `cargo test -p hivemind-bin cli::tests:: --no-default-features -- --nocapture`: pass with six CLI tests after adding parser/error edge cases.
- `cargo test -p hivemind-bin --no-default-features`: pass, including status/result CLI tests.
- `cargo fmt --check`: pass.
- `cargo test -p hivemind-task-scheduler task_repository::tests::test_assign_to_worker_does_not_overwrite -- --nocapture`: failed first because assignment was unconditional, then passed.
- `cargo test -p hivemind-task-scheduler dispatcher::tests::test_dispatch_one_does_not_overwrite_stale_assignment -- --nocapture`: pass.
- `cargo test -p hivemind-task-scheduler --no-default-features -- --test-threads=1`: pass with 17 tests.
- `cargo fmt --check`: pass.
- Note: parallel `cargo test -p hivemind-task-scheduler` had a transient shared-DB dispatcher failure; rerunning the failing test alone passed.
- `cargo test -p hivemind-task-scheduler task_repository::tests::test_claim_pending_for_worker -- --nocapture`: pass.
- `cargo test -p hivemind-task-scheduler --no-default-features -- --test-threads=1`: pass with 18 tests.
- `cargo test -p hivemind-node-manager --no-default-features`: pass.
- `cargo fmt --check`: pass.
- `cargo test -p hivemind-task-scheduler task_repository::tests::test_complete_for_worker_rejects_stale_worker_after_redispatch --no-default-features -- --nocapture`: failed first because worker-scoped methods were missing, then passed.
- `cargo test -p hivemind-task-scheduler --no-default-features -- --test-threads=1`: pass with 21 tests.
- `cargo test -p hivemind-node-manager --no-default-features`: pass.
- `cargo fmt --check`: pass.
- `cargo check` in `subagents/sandbox-egress`: pass.
- `cargo test` in `subagents/sandbox-egress`: pass with 8 tests.
- `cargo check -p hivemind-worker-executor --no-default-features`: pass.
- `cargo test -p hivemind-worker-executor --no-default-features`: pass with 27 tests.
- `cargo test -p hivemind-task-scheduler task_repository::tests::test_complete_settles_billing_when_balance_is_sufficient -- --nocapture`: pass.
- `cargo test -p hivemind-task-scheduler --no-default-features -- --test-threads=1`: pass with 21 tests.
- `cargo test -p hivemind-master-api test_admin_billing_overview_returns_settled_totals_and_pending_count -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_provider_worker_trust_profile_returns_reputation -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_admin_artifact_overview_returns_lifecycle_metrics -- --nocapture`: pass.
- `cargo test -p hivemind-task-scheduler dispatcher::tests::test_dispatch_pending_excludes_banned_worker_by_trust -- --nocapture`: pass.
- `cargo test -p hivemind-task-scheduler --no-default-features -- --test-threads=1`: pass with 22 tests.
- `cargo test -p hivemind-master-api test_admin_artifact_cleanup_dry_run_reports_candidates_without_deleting -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_admin_artifact_cleanup_execute_deletes_expired_rows_and_file -- --nocapture`: pass.
- `cargo check -p hivemind-bin --no-default-features`: pass.
- `cargo test -p hivemind-task-scheduler dispatcher::tests::test_rank_workers_prefers_worker_with_cache_affinity -- --nocapture`: pass.
- `cargo test -p hivemind-task-scheduler --no-default-features -- --test-threads=1`: pass with 23 tests.
- `cargo test -p hivemind-master-api test_admin_scheduling_cache_metrics_returns_totals_and_top_workers -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_admin_scheduling_cache_alert_returns_normal_within_threshold -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_admin_scheduling_cache_alert_rejects_invalid_thresholds -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_admin_worker_trust_control_updates_ban_and_score -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_admin_worker_trust_list_returns_joined_worker_and_reputation -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_admin_endpoints_reject_non_admin_user -- --nocapture`: pass.
- `cargo test -p hivemind-master-api test_create_task_rate_limit_rejects_second_submit_within_one_minute -- --nocapture`: pass.
- `cargo test -p hivemind-task-scheduler task_repository::tests::test_claim_pending_for_worker_blocks_banned_worker -- --nocapture`: pass.

## Candidate First Slice
Marketplace ledger foundation:
- Add durable tables for task charge ledger and provider earnings ledger.
- Add Rust models and repository methods.
- Add focused tests.
- Avoid frontend churn until the data/API contract is stable.

## Next Implementation Backlog
- Requestor browser ZIP upload API: done for API; Master UI upload flow remains.
- Requestor CLI submit/status/result: done for ZIP package login/upload, status lookup, and result torrent lookup; direct artifact download remains.
- Atomic assignment: guarded `assign_to_worker`, worker-pull batch claim with `FOR UPDATE SKIP LOCKED`, and worker-result fencing are done; explicit lease IDs/expiry remain.
- Provider onboarding control plane: done for local `GET /api/worker-info`; backend resource caps/settings API and Worker UI controls are done.
- Scheduler marketplace filtering: done for provider enablement, effective caps, available memory/storage, and minimum price.
- Worker UI provider panel: done for login, local profile, registration, settings, earnings, and worker list.
- Worker ownership hardening: HTTP worker registration now binds to authenticated subject; verification status remains.
- Pricing quote API: deterministic requestor quote and max-CPT budget guardrails are done.
- Provider earnings API: done for backend; Worker UI display remains.
- Atomic lease: use DB transaction or `FOR UPDATE SKIP LOCKED` for pull/assign to prevent duplicate assignment.
- Sandbox release gate: production must not fall back to shell simulation, must enforce cwd, timeout, and network policy.

## Audit Summary
- Backend: Rust has centralized master/nodepool/worker skeleton, but missing marketplace ledger, worker trust/auth, real artifact execution, and lifecycle close-loop.
- Frontend/DevEx: Master UI can submit by magnet/master-host zip path, but lacks browser upload; Worker UI assumes a missing local worker-control HTTP service.
- Billing/security: Current billing deducts `max_cpt` after completion, lacks provider payout ledger, lacks atomic/idempotent settlement, and has sandbox/auth gates before public release.

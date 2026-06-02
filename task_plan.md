# Centralized Compute Marketplace Plan

## Goal
Build Hivemind into a centralized, no-crypto, low-friction compute marketplace similar in purpose to Golem Network: requestors submit jobs, providers run a worker agent, the platform handles trust, pricing, billing, scheduling, and payouts.

## Product Scope
- First public MVP: centralized batch compute pool.
- Requestor path: login, submit a zip or container-like package, set resource needs and budget, stream status/logs, download result artifacts.
- Provider path: install or launch worker, login/register, set resource limits, see status and earnings ledger.
- Platform path: account balance/credits, task cost ledger, provider payout ledger, basic marketplace pricing, admin observability.

## Phase 0: Repo Grounding
- [complete] Use subagents to audit backend, frontend/onboarding, and billing/security gaps.
- [complete] Consolidate findings into implementation slices with disjoint write scopes.
- [complete] Choose the first implementation slice that has high product leverage and low merge risk.

## Phase 1: Marketplace Data Foundation
- [complete] Add durable marketplace tables for usage/provider payout ledger.
- [complete] Add domain model for ledger entries.
- [complete] Add repository methods and tests for idempotent ledger creation and task settlement.
- [complete] Expose requestor cost estimate and provider earnings endpoints.
- [complete] Make scheduler respect provider enablement, effective caps, availability, and minimum price.
- [complete] Add guarded atomic task assignment so stale dispatch cannot overwrite an assigned task.
- [complete] Add `FOR UPDATE SKIP LOCKED` batch claim for worker pull so concurrent claimers do not overlap.
- [complete] Add worker-result fencing so stale worker completion/failure/reset cannot mutate a redispatched task.

## Phase 2: Requestor DevEx
- [complete] Add browser-friendly ZIP upload API for requestor job submission.
- [complete] Add a simple requestor CLI command or script: submit job package.
- [complete] Extend requestor CLI to poll status and fetch result references.
- [complete] Add artifact lifecycle and direct artifact download beyond result torrent references.
- [complete] Add task templates for Python script and generic zip package.
- [complete] Add API support for quote-before-submit and budget guardrails.
- [pending] Add task log/result polish in Master UI.

## Phase 3: Provider Onboarding
- [complete] Add worker profile/settings model: CPU/GPU/RAM/storage caps, allowed schedule, minimum price.
- [complete] Add worker self-registration/control flow that exposes detected resources to Worker UI.
- [complete] Add Worker UI controls for resource caps and status.
- [pending] Add packaging path for Windows-first worker launch.

## Phase 4: Trust and Safety Gate
- [pending] Harden sandbox policy: network egress, filesystem isolation, resource hard limits, secret handling.
- [pending] Add task verification strategy for deterministic jobs: retry/replica compare or checksum proof.
- [in_progress] Add abuse controls: task size limits, rate limits, account state, worker ban/reputation fields.
- [complete] Bind HTTP worker registration ownership to the authenticated provider identity.
- [pending] Add admin dashboard and release gates.

## Active Subagents
- Backend audit: complete.
- Frontend/DevEx audit: complete.
- Billing/security audit: complete.
- Ledger implementation worker: complete.
- Browser ZIP upload worker: complete.
- Worker control API worker: complete.
- Worker ownership hardening worker: complete.
- Pricing quote/budget guardrail worker: complete.
- Provider earnings API worker: complete.
- Provider resource caps/settings explorer: complete.
- Provider resource caps/settings implementation: complete.
- Scheduler provider settings explorer: complete.
- Scheduler effective caps/price implementation: complete.
- Worker UI provider controls explorer: complete.
- Worker UI provider controls implementation: complete.
- Requestor CLI submit explorer: complete.
- Requestor CLI submit implementation: complete.
- Requestor CLI status/result explorer: complete.
- Requestor CLI status/result implementation: complete.
- Atomic assignment explorer: complete.
- Guarded task assignment implementation: complete.
- Batch claim implementation: complete.
- Stale worker result fencing implementation: complete.

## Acceptance Criteria For This Planning Round
- [complete] Durable plan and state files exist.
- [complete] Subagent audit results are captured.
- [complete] First implementation milestone is selected.
- [complete] At least one bounded implementation task is dispatched to a worker agent or explicitly deferred with reason.


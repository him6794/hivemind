# Managed runtime v2 plan (proposal)

Status: proposal. Nothing in this document is implemented. The decisions that led
here are recorded in `MANAGED_RUNTIME_EVOLUTION_PLAN.md` (2026-10-26). This file
lists each open item, the gate it sits behind, the options, a recommendation, the
decisions it needs, and the work it implies.

## Gates

- **Gate 1: cost-model identity.** `HCODE_RUNTIME.md` requires a new runtime,
  cost-model, and manifest identity, with compatibility tests, for any change to
  cost vectors or settlement inputs. It forbids silent mutation. Items A and D change
  settlement or the default fee, so they need Gate 1. Items B and C change neither,
  so they may proceed as opt-in or hardening.
- **Gate 2: buyer.** Cost-model v2 was deferred until a real buyer exists. Items A
  and D inherit this gate.

## A. Billing for FAILED tasks without a certificate

Gate: 1 and 2. Decision needed.

- Current: usage-only settlement charges the requester for accepted evidence, and
  the task ends FAILED. Observed in T1.4: one timeout task charged 121 CPT.
- Options:
  - A0. Keep the current rule.
  - A1. Requester pays 0. Providers with valid receipts are paid from a
    platform-funded pool.
  - A2. Requester pays 0. Providers are paid 0. Only certified work is paid, as
    BOINC does by default according to forum reports.
- Recommendation: A2. It needs no platform subsidy, and providers still have a
  reason to answer in time. Move to A1 only if provider attrition appears.
- Work, if approved: a new identity set. In v2 attempts, the no-certificate path in
  `settle_managed_v1_hold_without_certificate` refunds the full hold. v1 attempts
  keep the current branch. Tests: a RED test for the v2 refund. The v1 billing
  tests stay unchanged; `managed_consensus_no_quorum_fences_late_observations`
  still expects 66 CPT for v1. Migration: drain in-flight v1 attempts, or record
  the identity on each attempt.
- Effort: large.

## B. Timeout escalation latency: opt-in hedge

Gate: none, because it is opt-in and the default is unchanged.

Status: implemented in `9a1f3d32` with the proposed deadline rule (the hedge keeps
the deadline). The threshold has no default; the operator sets
`MANAGED_CONSENSUS_HEDGE_AFTER_SECS`. The release benchmark is recorded in
`MANAGED_RUNTIME_EVOLUTION_PLAN.md`.

- Current: the lazy spare is added only at the deadline. Measured timeout median is
  30.1 s with lazy dispatch and 9.6 s with eager. Held replicas keep their Worker
  slots until then, which feeds the slot waits found in T1.4.
- Design:
  - Config `MANAGED_CONSENSUS_HEDGE_AFTER_SECS`. Unset means off. When set, it must
    be at least 1 and below `MANAGED_CONSENSUS_TIMEOUT_SECS`.
  - When an attempt has an outstanding replica, no quorum, a free spare, and
    `hedge_after` seconds have passed since dispatch, add one spare with reason
    `hedge`.
  - Put the decision in a pure function, `hedge_due`, and write its unit tests first.
  - Deadline rule. Proposal: the hedge spare does not extend the attempt deadline.
    The existing deadline escalation keeps its extension. This needs a flag on
    `add_managed_consensus_replica`.
  - Cancellation after quorum is already in place.
- Cost: the default is unchanged. A hedge adds a replica only when a reply is slow.
  In the bench, normal replies arrive in about 10 ms (`reported_s` ≈ 0.01), so the
  normal path does not hedge. Each hedged task costs about one replica, roughly
  121 CPT in the bench's pricing.
- Risk: hedges add concurrency on a Worker under bursts. The trigger is slowness,
  and the one-spare limit bounds the extra load.
- Verification: unit tests for `hedge_due`. Then a release benchmark with a hedge
  variant (threshold 3 s, proposed). Expected: timeout median falls to the 3–5 s
  range, and the normal path is unchanged.
- Decisions: the default threshold (proposal: 3 s) and the deadline rule (proposal:
  no extension).
- Effort: medium.

## C. Persist the initial replica count and make settlement checks strict

Gate: none, because valid attempts behave the same.

Status: implemented in `568213a8`, with the additive migration approved.

- Current: settlement accepts relaxed replica counts. A lazy attempt that loses or
  gains a replica row is not detected.
- Design:
  - Additive migration: `managed_consensus_attempts.initial_replicas INTEGER NULL`
    and `escalations INTEGER NOT NULL DEFAULT 0`.
  - Attempt creation sets `initial_replicas`. Each escalation increments
    `escalations` in the same transaction as the replica insert, under the attempt
    row lock that escalation already takes.
  - Settlement checks, when `initial_replicas` is set: rows must equal
    `initial_replicas + escalations`, and rows must not exceed `replica_count`.
    NULL, which covers legacy rows, keeps the relaxed check.
  - Eager attempts have `initial_replicas = replica_count` and `escalations = 0`,
    so their behavior is unchanged.
- Tests: a RED test that removes a replica row from a lazy attempt and expects
  settlement to refuse. Positive tests for normal lazy and escalated attempts.
- Effort: medium. The migration runs when a nodepool starts.

## D. Lazy dispatch as the default

Gate: 1 and 2. Do not start before B and C.

- Current: eager is the default. Lazy saves 31% on the normal path (351 → 242 CPT
  per task). Lazy timeouts are about three times slower until B lands, and lazy
  settlement checks stay relaxed until C lands.
- Recommendation: do not flip. The default fee changes from three replicas to two,
  which is a cost-vector change and needs Gate 1.

## E. Cost-model v2 identities

Gate: 1 and 2. Not started. A and D depend on it.

- Work: new runtime, cost-model, and manifest identities; compatibility tests; a
  drain or per-attempt identity; a decision on the client-visible runtime name.

## Recommended order

1. C. Hardening with no change for valid attempts.
2. B. Opt-in hedge, with a release benchmark.
3. A and D together, only when Gate 2 opens. Design A2 first.

## Decisions needed

1. A: A0, A1, or A2. Whether to start E now.
2. B: the default threshold, and whether the hedge extends the deadline.
3. C: approve the additive migration.
4. Whether to implement C and B now. Both preserve the default behavior.

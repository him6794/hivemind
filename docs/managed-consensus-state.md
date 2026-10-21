# Managed consensus state

## Scope

Managed DSL execution settles through Nodepool-coordinated replicated
execution by default. Consensus is an agreement signal between authenticated
Workers; it does not independently establish that a result is semantically
correct or that reported usage actually occurred.

## Configuration

The Nodepool and each consensus-capable Worker must use the same protocol
settings:

```text
MANAGED_CONSENSUS_ROLLOUT_MODE=enforce
MANAGED_CONSENSUS_REPLICA_COUNT=3
MANAGED_CONSENSUS_QUORUM=2
MANAGED_CONSENSUS_TIMEOUT_SECS=120
MANAGED_CONSENSUS_MAX_RESULT_BYTES=262144
MANAGED_CONSENSUS_REQUIRE_DISTINCT_OWNERS=true
```

The default `enforce` policy dispatches two replicas to distinct registered
Workers and requires two matching results. A third replica is added only when
needed (see [Replica dispatch and escalation](#replica-dispatch-and-escalation)). Successful completion requires a quorum
certificate; Nodepool does not lower the quorum or use a single-Worker fallback
when fewer Workers are available. Valid assignment-bound v1 usage may still be
billable if the task fails or no quorum forms.

Replica selection is a uniform random draw from the eligible Workers. With
`MANAGED_CONSENSUS_REQUIRE_DISTINCT_OWNERS=true`, each owner (the provider
account that registered the Worker, compared after trimming whitespace)
contributes at most one replica. If fewer distinct owners are eligible, the
task stays pending and the replica count is not lowered. Worker-reported status
and CPU usage do not influence selection. Setting the variable to `false`
removes the owner rule and logs a warning. Use `false` only for local smoke and
E2E runs where one account operates several Workers. A shared deployment with
`false` lets one operator fill every replica.

`observe` dispatches all replicas and records quorum/no-quorum metadata. The task
ends as `OBSERVED`, without user output, billing, or settlement.

Only deterministic, side-effect-free managed DSL tasks are eligible. The
`host_count` field is a pricing/input field, separate from the replica count.

## Replica dispatch and escalation

By default, enforce-mode `hcode` attempts dispatch all
`MANAGED_CONSENSUS_REPLICA_COUNT` replicas at once. Lazy dispatch is opt-in: set
`MANAGED_CONSENSUS_INITIAL_REPLICAS` to a value from `MANAGED_CONSENSUS_QUORUM` up
to the replica count, and the attempt starts with that many replicas. Nodepool then
adds the next replica only when the replicas in flight cannot reach quorum, or when
the attempt deadline passes with quorum still missing:

- A disagreement, a refused or failed replica, or a timeout can trigger
  escalation. The added Worker comes from the same eligible pool. It must satisfy
  the owner rule and have a free execution slot, and Nodepool checks both again
  when it inserts the replica.
- Each escalation extends the attempt deadline by one
  `MANAGED_CONSENSUS_TIMEOUT_SECS`, capped at twice that timeout from creation.
  Replicas dispatched before an extension keep the RPC budget they were given.
- Escalation stops at `MANAGED_CONSENSUS_REPLICA_COUNT`. When no eligible Worker
  can be added, the attempt ends with no quorum. Nodepool does not lower the
  quorum to finish.
- After a Nodepool restart, recovery rebuilds the spare pool from the trusted
  registered Workers, so escalation resumes from the persisted replica set.

The acceptance rule is unchanged. A certificate still requires
`MANAGED_CONSENSUS_QUORUM` matching results from distinct replicas, and each
replica reports through its own assignment-bound execution token. Lazy dispatch
changes how many replicas run, not which results are accepted.

The usage hold still reserves the worst case for `MANAGED_CONSENSUS_REPLICA_COUNT`
replicas. With lazy dispatch, a replica that was never dispatched has no evidence
and no payout, so its allowance returns to the owner as a refund at settlement.
Observe mode and other managed runtimes assign every replica at dispatch, and so
does enforce mode unless `MANAGED_CONSENSUS_INITIAL_REPLICAS` is set.

## Acceptance and settlement

Nodepool creates a durable round and a separate replica assignment for each
Worker. Every execution token binds the task, logical execution, round, replica,
attempt, idempotency key, request digest, and protocol version. Nodepool accepts
only matching results from distinct assigned Worker identities and computes the
result digest from the canonical result bytes.

A quorum certificate is created and the task is completed in one Nodepool
transaction. The round, replica assignments, endpoint/provider snapshots,
observations, worker-slot reservations, certificate, and settlement record are
persisted independently. No quorum, stale response, conflicting result, or
single Worker success may complete a consensus task. The certificate and bounded
`ManagedConsensusResult` bytes in each replica's internal `result_json` are
persisted as Nodepool evidence; the raw
transport envelope and per-replica evidence are not exposed through public APIs.
Outstanding replicas are explicitly fenced and receive an attempt-bound stop
request after a quorum, timeout, or cancellation. If a stop cannot be
confirmed, Nodepool retains the Worker-slot reservation in a durable
`stop_pending`/`cancel_pending` state and retries the stop rather than
redispatching the Worker immediately.

For `hcode` in enforce mode, settlement uses validated,
assignment-bound per-replica usage evidence—including valid divergent results—
within the task-wide `max_cpt` cap. Missing or invalid receipts are not billable;
`observe` remains non-settling and creates no usage hold. A consensus certificate
is agreement evidence, not an independent correctness proof or usage attestation.
Historical `managed-function-v0` billing remains governed by its frozen
Nodepool-owned fixed-reservation contract.

## Failure model

The default policy assumes an honest majority among the selected Workers. Worker
IDs do not prove operator independence: a single operator can run multiple
Workers, and a colluding quorum can return the same incorrect result. Transport,
capacity, and timeout failures do not automatically establish Worker
misbehaviour. Invalid identity/result envelopes are rejected and audited.

The owner rule caps each provider account at one replica of a task. It does not
bind a Worker to an operator: one party that registers several provider accounts
can still fill a quorum. Stopping that requires registration controls outside
the scheduler.

Rollout classification uses persisted task policy identity. A managed task
without a persisted consensus policy is held closed (awaiting policy) regardless
of the current rollout setting; a policy-backed task is held closed when rollout
is disabled and never silently downgraded. Direct gRPC numeric overrides are
checked before narrowing, and runtime identities are normalized before
persistence/classification.

Quorum identity uses canonical result bytes with diagnostic `usage_units` and
`executed_ops` zeroed. Those Worker-reported values remain available for audit,
but cannot split matching deterministic outputs into separate quorum groups.
Typed consensus failures retain execution/round/replica identity so Nodepool can
record a failed observation instead of accepting an incomplete response.

Consensus is the only managed settlement path. New consensus tasks must not
fall back to legacy completion, single-Worker settlement, or `observe`/`disabled`
as success evidence.

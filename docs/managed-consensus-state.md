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
```

The default is `enforce`: managed tasks settle only from a quorum
certificate and never fall back to a single-Worker result. The initial
bounded policy is three distinct registered Workers and a strict majority of
two. The Nodepool never lowers the quorum when fewer Workers are available.
`observe` is non-settling: the dispatcher fans out all replicas and ends the
task in non-settling `OBSERVED` state. It records shadow quorum/no-quorum
metadata while keeping output, billing, and settlement empty; no single-Worker
fallback is allowed.

Only deterministic, side-effect-free managed DSL tasks are eligible. The
existing `host_count` field remains a pricing/input field and is not silently
repurposed as the consensus factor.

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
single Worker success may complete a consensus task. The certificate is
persisted as Nodepool evidence; raw replica payloads are not stored or exposed.
Outstanding replicas are explicitly fenced and receive an attempt-bound stop
request after a quorum, timeout, or cancellation. If a stop cannot be
confirmed, Nodepool retains the Worker-slot reservation in a durable
`stop_pending`/`cancel_pending` state and retries the stop rather than
redispatching the Worker immediately.

Consensus does not authorize variable usage billing. Until a separate usage
attestation is designed, settlement remains the Nodepool-owned reservation,
with the provider share split deterministically across the matching Workers.
Worker usage is diagnostic only. A consensus certificate must never be
described as independent correctness validation.

## Failure model

The default policy assumes an honest majority among the selected Workers. Worker
IDs do not prove operator independence: a single operator can run multiple
Workers, and a colluding quorum can return the same incorrect result. Transport,
capacity, and timeout failures do not automatically establish Worker
misbehaviour. Invalid identity/result envelopes are rejected and audited.

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

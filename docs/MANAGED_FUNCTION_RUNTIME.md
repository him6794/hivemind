# Managed Function Runtime

## Goal

The Managed Function Runtime is a restricted, metered execution path for small
serverless-style Hivemind tasks. It parses a fixed syntax, evaluates it with a
closed Rust-owned interpreter, and returns deterministic output and execution
receipts. It does not execute user-supplied executables or expose file, network,
import, subprocess, reflection, or arbitrary host-function capabilities.

General-compute tasks are a separate runtime. Only that arbitrary-compute route
may use the native Windows HCS backend; managed functions never require HCS,
Docker, WSL, a VM, SSH, or a direct host-process fallback.

## Active contract

`managed-function-v1` is the only managed-function runtime accepted for new
submissions. It uses the closed interpreter described below with a versioned
per-replica usage allowance, checked arithmetic, structural safety limits, and
Nodepool-coordinated consensus settlement. `managed-function-v0` is retained
only so historical tasks, receipts, consensus evidence, and legacy capability
reports remain readable; it is rejected at every new-work admission boundary.

## Frozen v0 contract

The machine-readable `managed-function-v0` semantics, metering, billing, and
result contract are frozen in
[`executor-rs/crates/managed-function-runtime/managed-function-v0-semantics.json`](../executor-rs/crates/managed-function-runtime/managed-function-v0-semantics.json).
Its canonical JSON SHA-256 is
`d61a8134f665100855402d7455cfcf3b3e701a79ad43e0039f4ad6c5f05bafef`.
The manifest includes executable cost vectors, admission limits, default runtime
limits, the canonical managed-consensus result contract, and Nodepool-owned
fixed billing metadata. Worker usage and operation counts are diagnostic only;
they do not authorize settlement. An incompatible change requires new runtime,
cost-model, and semantics manifest identifiers; this file is not a mutable
latest configuration.

The v0 limitations are part of that frozen contract:

- Source string literals are decoded byte by byte and therefore do not preserve
  non-ASCII UTF-8. The lexer also does not accept `\uXXXX` or surrogate escape
  syntax. It only recognizes quote, backslash, line-feed, carriage-return, and
  tab escapes. JSON input and canonical output remain UTF-8.
- Managed integers are signed `i64`. Arithmetic overflow currently uses the evaluator's
  unchecked Rust integer operators, so overflow is not a portable cross-worker language result;
  tasks must keep arithmetic in range.
- `RuntimeError` does not expose the evaluator's partial receipt. Worker
  evaluation failures synthesize zeroed counters; final output-render failures
  retain only `executed_ops`. Failed receipts are diagnostic evidence, not settlement evidence.
- `ExecutionLimits::unlimited()` is a legacy/testing convenience, not the
  production v0 default.

## Managed-function-v1 contract

`managed-function-v1` is the versioned usage-billing contract for the same
closed interpreter. Its runtime, backend, cost-model, and semantics identities
are pinned by
[`executor-rs/crates/managed-function-runtime/managed-function-v1-semantics.json`](../executor-rs/crates/managed-function-runtime/managed-function-v1-semantics.json)
and digest
`c2dc962dcf6762df51fa94af2ee1f00a4d1aabdf84321ec67a3ab7f892692853`.

- Each managed task is executed by the configured replica set (three replicas
  by default, with a two-replica quorum; at most seven replicas). A single
  Worker cannot complete, settle, or bill the task.
- `max_cpt` is an independent execution-credit allowance for each replica. One
  executed operation is one CPT. The evaluator does not impose a separate
  fixed `max_ops` or `max_loop_iterations` work ceiling; execution stops before
  charging an operation that would exceed that replica's allowance.
- Results use checked signed-`i64` arithmetic. Overflow fails with
  `integer_arithmetic_overflow`; division by zero remains a separate runtime
  failure.
- A budget-exhausted result is failed for consensus, but valid work recorded
  before exhaustion remains billable. Nodepool pays every replica with valid
  accepted evidence, including a valid divergent result outside the winning
  certificate participants.
- Nodepool holds the worst-case per-replica allowance before enforce-mode
  dispatch, settles actual accepted usage plus its calculated fee, and refunds
  the unused hold. Observe mode records shadow evidence only and never creates
  a hold or financial ledger mutation.
- Structural safety limits and the task deadline remain bounded. Missing or
  invalid runtime assets fail closed; no unsafe execution fallback is allowed.

## Runtime Contract

Input:

- source text using the supported syntax below
- a positive per-replica `max_cpt` usage allowance plus structural safety limits
  such as call depth, value size, output size, and the task deadline
- optional function arguments in a later milestone

Output:

- final value
- printed output
- execution receipt
- structured failure when parsing, validation, metering, or runtime evaluation
  fails

Hivemind task integration:

- set `runtime` to `managed-function-v1` for every new managed submission
- set `task_source` to the managed function source text
- set `torrent` / `torrent_source` to the JSON input payload when input is
  needed
- v1 submissions are admitted only when their runtime, backend, semantics
  digest, source, and input identities match the Nodepool policy
- historical v0 task rows and evidence remain readable but cannot be resumed
  or submitted as new work
- ZIP/torrent-based executable task execution has been removed; the JSON
  payload is data for the closed interpreter, not an executable package

Receipt fields:

- `status`
- `usage_units`
- `executed_ops`
- `function_calls`
- `loop_iterations`
- `max_call_depth`
- `output_bytes`
- `failure_code`
- `failure_message`

Worker `ExecuteTaskResponse` forwards the receipt summary back to the scheduler:

- `managed_executed_ops`
- `managed_output_bytes`
- `managed_receipt_json`

The scheduler stores these fields as typed replica evidence. For v0 they remain
compatibility and diagnostic data. For v1 Nodepool independently validates the
current-attempt evidence and uses the accepted per-replica usage for settlement;
a single Worker receipt or usage claim is never authoritative.

## Supported Syntax

The active v1 language and the frozen v0 language share this closed syntax. The
syntax is intentionally separate from each version's metering and settlement
rules, so a historical v0 fixture can be parsed without making v0 available for
new submission.

Statements:

```text
let name = expression;
fn name(arg1, arg2) { return expression; }
for item in expression { statements... }
return expression;
print(expression);
expression;
```

Expressions:

```text
integer
true
false
"string"
[1, 2, 3]
{"key": value}
name
name(arg1, arg2)
if condition { expression } else { expression }
(expression)
expression + expression
expression - expression
expression * expression
expression / expression
expression == expression
expression != expression
expression < expression
expression <= expression
expression > expression
expression >= expression
```

Rules:

- Identifiers are ASCII letters, digits, and `_`, and must not start with a
  digit.
- Integers are signed 64-bit values.
- Strings are UTF-8 string literals with `\"`, `\\`, `\n`, `\r`, and `\t`
  escapes.
- User functions are pure runtime functions over values in the managed
  environment.
- `print` appends to the receipt output and is bounded by `max_output_bytes`.
- The last expression statement becomes the final value unless an earlier
  `return` exits the program.
- `input` is available when the caller provides JSON input.
- `for` only iterates lists. In v1, work is bounded by the per-replica
  `max_cpt` allowance while structural limits and the task deadline remain
  active; frozen v0 fixtures additionally retain their historical
  `max_loop_iterations` limit.
- Built-in functions currently include `len(value)`, `get(target, key)`, and
  `contains(target, value)`.

Forbidden in managed-function-v1 (and in the frozen v0 language):

- imports
- file I/O
- network I/O
- environment variables
- subprocesses
- dynamic eval
- reflection
- arbitrary host functions
- unbounded recursion
- unbounded loops

## GPU-v1 extension

GPU-enabled managed functions use a separate runtime identity,
`managed-function-gpu-v1`, and the canonical
`executor-rs/crates/managed-function-runtime/managed-function-gpu-v1-semantics.json`
manifest. This keeps floating-point and GPU behavior out of the frozen v0
result contract.

GPU-v1 adds only fixed, Rust-owned operations:

- `gpu_add_f32(lhs, rhs)`
- `gpu_scale_f32(value, scalar)`
- `gpu_matmul_f32(lhs, rhs)`

The DSL receives bounded host-side numeric values. It cannot provide CUDA C,
PTX, pointers, device handles, kernel source, dynamic libraries, or an
executable. The operator-selected backend owns CUDA/cuBLAS resources, and a
GPU-required request fails closed when a trusted compatible GPU is unavailable;
it never silently uses the CPU reference backend.

GPU-v1 also permits the separately declared floating-point and math surface
only inside its explicit GPU execution context. GPU-v1 uses the authoritative
typed result contract and consensus settlement path. A GPU result without the
required consensus evidence is not settled, and the route never falls back to
legacy result-torrent completion.

## Metering v0

Every executed statement and expression consumes at least one operation.

Initial cost table:

| Operation | Cost |
| --- | ---: |
| literal or variable read | 1 |
| assignment | 1 |
| unary/binary expression | 1 + child costs |
| comparison | 1 + child costs |
| `if` condition | child costs |
| selected `if` branch | child costs |
| function call overhead | 5 + argument costs |
| `print` overhead | 5 + argument costs |
| `for` iteration | bounded by `max_loop_iterations` |
| return | 1 + expression cost |

Execution stops with `op_limit_exceeded` before an operation would exceed the
configured limit.

## Metering v1

The v1 cost vectors use the same evaluator operation accounting, including
function-call, print, and loop-iteration costs. `max_usage_units` is the
per-replica work allowance and is the only dynamic work stop; `max_ops` and
`max_loop_iterations` use the unbounded sentinel. Receipt `usage_units` and
`executed_ops` are equal and are checked before settlement.

## Billing and consensus

The frozen v0 contract retains its historical fixed-reservation and diagnostic
receipt semantics. V0 is no longer accepted for new work; completed v0 records
are preserved as historical evidence, and unfinished v0 work is cancelled at
Nodepool startup rather than being converted to v1.

For v1, consensus and payment are deliberately separate:

```text
canonical result = managed-consensus-result-v1
output digest    = sha256
quorum           = Nodepool certificate over the configured replica quorum
billing          = accepted per-replica executed operations + Nodepool fee
```

A managed task is dispatched to distinct eligible Workers. Nodepool validates
the task, attempt, replica, result, identity, and usage evidence. The winning
certificate selects the output, while every valid accepted replica in the
attempt is paid for its own measured work. A valid divergent replica need not be
a certificate participant to receive payment.

No certificate is fabricated merely to justify payment. Accepted terminal
evidence may settle an enforce-mode hold without a winning certificate, but
usage-only settlement never marks the task completed. If policy is missing,
disabled, malformed, or cannot safely execute, the route fails closed. There is
no single-Worker, receipt-only, fixed-split, or unsafe fallback path.

## Execution boundary

The runtime is a closed interpreter over the `Value` enum. It has no API for
arbitrary process creation, shell commands, filesystem access, network access,
imports, reflection, dynamic evaluation, or host callbacks. The Worker only
returns the typed result and receipt; Nodepool is the sole verification,
consensus, payment, settlement, and billing authority.

The v0 and v1 contracts are intentionally versioned. Changes to parser
semantics, limits, receipt meaning, cost vectors, or settlement inputs require a
new runtime/cost-model/manifest identity and compatibility tests rather than a
silent mutation of v0.

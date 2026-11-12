# hcode Runtime

## Overview

The hcode Runtime is a restricted, metered execution path for small
serverless-style Hivemind tasks. It parses a fixed syntax, evaluates it with a
closed Rust-owned interpreter, and returns deterministic output and execution
receipts. It does not execute user-supplied executables or expose file, network,
import, subprocess, reflection, or arbitrary host-function capabilities.

General-compute tasks are a separate runtime. Managed functions never require a
container runtime, Docker, WSL, a VM, SSH, or a direct host-process fallback.

## Active contract

`hcode` is the CPU managed-function runtime accepted for new
submissions. It uses the closed interpreter described below with versioned
per-replica metering, a task-wide fee-inclusive charge cap, checked arithmetic,
structural safety limits, and Nodepool-coordinated consensus settlement.
`managed-function-v0` is retained
only so historical tasks, receipts, consensus evidence, and legacy capability
reports remain readable; it is rejected at every new-work admission boundary.

## Coordinated upgrade

This is a hard wire rename: `managed-function-v1` is retired, not an alias for
`hcode`; `managed-function-gpu-v1` is retired in favor of `hcode-gpu`. Runtime,
backend, cost-model, billing and GPU protocol IDs have changed, as have the
CPU/GPU semantics manifest digests. Operation costs and settlement rules have
not changed.

Stop new submissions and use the old Nodepool to drain and settle all old CPU
rounds, including stop-pending rounds on terminal tasks and outstanding Worker
reservations. Drain or cancel nonterminal old GPU tasks with persisted typed
requests. Then stop the old Nodepool and coordinate the Nodepool, Master and
Worker upgrade; do not run mixed versions against one queue.

The transactional database migration refuses to proceed while these old
executions remain in flight. After the checks pass it relabels old CPU task rows
and only the old billing/cost metadata of held usage reservations. Settled and
released billing history, request/result/attempt/certificate evidence, and
frozen v0 identities are not rewritten. Old GPU request/result bytes and digests
are never converted into new requests; the existing quarantine for active old
GPU rows without a manifest remains in place. A blocked migration rolls back
instead of partially renaming data. Do not bypass the guard with manual SQL.

For a first task, see the [繁體中文 hcode 教學](hcode-tutorial.zh-TW.md).

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

## hcode contract

`hcode` is the versioned usage-billing contract for the same
closed interpreter. Its runtime, backend, cost-model, and semantics identities
are pinned by
[`executor-rs/crates/managed-function-runtime/hcode-semantics.json`](../executor-rs/crates/managed-function-runtime/hcode-semantics.json)
and digest
`0c13a1da5fbbb94de2fbb5395ee807ab43e082b5c6d064deca7ece90700de822`.

- Each managed task is assigned to three distinct Workers by default, with a
  two-replica quorum; at most seven replicas may be configured. A single Worker
  cannot establish a quorum or authorize a successful result. Valid work from
  a lone replica can still be paid and charged when quorum is not reached.
- For a new hcode task, `max_cpt` is the maximum total charge for the entire task,
  including the platform fee and all configured replicas (three by default). It
  is not a per-replica allowance. Nodepool deterministically derives one identical integer execution
  budget for each replica from the fee-exclusive portion of this task cap. Any
  indivisible remainder stays with the owner rather than being assigned to a
  replica. For example, a 100 CPT cap with three replicas gives each replica a
  30 CPT execution budget; the worst-case hold is 99 CPT (90 usage + 9 fee),
  and the remaining 1 CPT stays with the owner.
- One executed operation is one CPT of replica usage. The evaluator does not
  impose a separate fixed `max_ops` or `max_loop_iterations` work ceiling;
  execution stops before charging an operation that would exceed that
  replica's assigned budget. The pinned semantics manifest's per-replica
  `max_usage_units` remains an internal runtime limit; Nodepool supplies the
  derived per-replica value from the task-wide `max_cpt`.
- Results use checked signed-`i64` arithmetic. Overflow fails with
  `integer_arithmetic_overflow`; division by zero remains a separate runtime
  failure.
- A budget-exhausted result is failed for consensus, but valid work recorded
  before exhaustion is billable when the task settles with a winning
  certificate. Nodepool aggregates valid accepted usage across replicas, adds
  the platform fee, and never charges more than the task's `max_cpt`, including
  for a valid divergent result outside the winning certificate participants. A
  task without a certificate is not charged under the current billing identity
  (see the certificate rules below).
- Nodepool holds the worst-case charges for the deterministic per-replica
  budgets before enforce-mode dispatch, settles actual accepted usage plus its
  calculated fee, and refunds unused held CPT. Observe mode records shadow
  evidence only and never creates a hold or financial ledger mutation.
- Automatic paid retries are disabled; no retry work is charged to the task
  owner. Automatic retries remain off until a funded platform treasury is
  available.
- Structural safety limits and the task deadline remain bounded. Missing or
  invalid runtime assets fail closed; no unsafe execution fallback is allowed.
- The task-wide cap applies to new hcode submissions. Historical v1 tasks
  settled under the earlier per-replica `max_cpt` contract remain recorded as
  billed; the new cap does not retroactively recalculate or refund them.

## Runtime Contract

Input:

- source text using the supported syntax below
- a positive task-wide `max_cpt` maximum charge, inclusive of platform fees and
  all configured replicas (three by default); Nodepool deterministically splits
  the fee-exclusive execution budget into identical integer per-replica runtime
  allowances, with any remainder staying with the owner
- structural safety limits such as call depth, value size, output size, and the
  task deadline

Output:

- final value
- printed output
- execution receipt
- structured failure when parsing, validation, metering, or runtime evaluation
  fails

Hivemind task integration:

- set `runtime` to `hcode` for every new managed submission
- set `task_source` to the managed function source text
- set `torrent` / `torrent_source` to the JSON input payload when input is
  needed
- treat `max_cpt` as the task-wide fee-inclusive charge cap, not a per-replica
  execution allowance; automatic paid retries are disabled until a funded
  platform treasury is available and do not charge retry work to the task owner
- hcode submissions are admitted only when their runtime, backend, semantics
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
compatibility and diagnostic data. For hcode Nodepool independently validates the
current-attempt evidence and sums accepted per-replica usage for task-level
settlement under the fee-inclusive `max_cpt`; a single Worker receipt or usage
claim is never authoritative.

## Supported Syntax

The active hcode language and the frozen v0 language share this closed syntax. The
syntax is intentionally separate from each version's metering and settlement
rules, so a historical v0 fixture can be parsed without making v0 available for
new submission.

Statements:

```text
let name = expression;
fn name(arg1, arg2) { return expression; }
for item in expression { statements... }
while expression { statements... }
if expression { statements... } else { statements... }
break;
continue;
return expression;
print(expression);
expression;
```

CPU hcode also accepts `//` line comments. A comment runs from `//` to the end of its line, and `//` inside a string literal is ordinary text. The frozen v0 surface has no comments.

CPU hcode also has `while`, `break`, `continue`, and `if` statements. `while` checks its bool condition before each pass through its body, and each pass counts as one loop iteration against the same limit as `for`. `break;` ends the innermost loop, and `continue;` ends the current pass. A leading `if` is a statement when its branches are statements, and an expression otherwise, as before. A function body starts a new loop context, so `break` and `continue` outside a loop are parse errors. The frozen v0 surface keeps these words as ordinary names.

CPU hcode also has list builtins. `append(list, value)` returns a new list with the value at the end and leaves its argument unchanged; it costs the length of the new list. `sort(list)` returns a new ascending list whose items are all ints, all floats, or all strings, and it costs the list length times the number of bits in that length, charged before any comparison. Strings sort by their UTF-8 bytes. `keys(dict)` returns the dict keys as a list of strings in ascending order, and it costs one unit per key. `split(text, separator)` returns the pieces between occurrences of a non-empty separator, at one unit per byte of text and one per piece. `join(list, separator)` joins a list of strings with a separator that may be empty, at one unit per byte of the result.

`split` and `join` check usage, value size, collection size, depth, and remaining
cumulative materialization limits before allocating result storage. Safety sizing
uses exact canonical JSON bytes, including string escapes and list delimiters;
size arithmetic overflow fails with `value_limit_exceeded`. Each successful result
consumes its canonical size from the materialization budget once.

Expressions:

```text
integer
float (CPU hcode and hcode-gpu, for example 1.5 or 1e3)
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

CPU hcode only. The frozen v0 language rejects these tokens:

```text
expression % expression
expression & expression
expression | expression
expression ^ expression
expression << expression
expression >> expression
~expression
```

From lowest to highest precedence: `or`; `and`; `not`; `==` and `!=`; the
ordering comparisons; `|`; `^`; `&`; `<<` and `>>`; `+` and `-`; `*`, `/`, and
`%`; then unary `-`, `+`, and `~`. Bitwise operators therefore bind tighter
than comparisons, as in Python: `6 & 3 == 2` means `(6 & 3) == 2`.

Rules:

- Identifiers are ASCII letters, digits, and `_`, and must not start with a
  digit.
- Integers are signed 64-bit values. Overflow fails the execution; use
  `wrapping_add`, `wrapping_sub`, or `wrapping_mul` where wrap-around is the
  intent.
- In CPU hcode, `%` truncates like `/`: the remainder takes the sign of the
  dividend. A zero divisor fails. `&`, `|`, `^`, and `~` act on two's-complement
  int64 values. `<<` and `>>` require a count from 0 through 63; `<<` discards
  bits shifted out of the word, and `>>` is arithmetic.
- Source strings use double quotes and `\"`, `\\`, `\n`, `\r`, and `\t`
  escapes, without `\uXXXX`. Byte-wise source-literal decoding in both CPU
  contracts does not preserve non-ASCII UTF-8 text. Pass Unicode data through
  JSON input.
- User functions are pure runtime functions over values in the managed
  environment.
- `print` appends to the receipt output and is bounded by `max_output_bytes`.
- The last expression statement becomes the final value unless an earlier
  `return` exits the program.
- `input` is available when the caller provides JSON input.
- `for` only iterates lists. In hcode, work is bounded by each replica's
  deterministically assigned runtime usage budget derived from the task-wide
  `max_cpt`; structural limits and the task deadline remain active. Frozen v0
  fixtures additionally retain their historical `max_loop_iterations` limit.
- Built-ins available in both CPU contracts are `len(value)`, `get(target, key)`,
  and `contains(target, value)`. `len(string)` counts UTF-8 bytes; string
  indexing counts characters. Frozen v0 input and syntax remain integer-only.
- CPU hcode also has `abs`, `min`, and `max`; `range(stop)`,
  `range(start, stop)`, and `range(start, stop, step)`; `wrapping_add`,
  `wrapping_sub`, and `wrapping_mul`; `float(x)` and `int(x)`; and the float
  math functions `sqrt`, `floor`, `ceil`, `round`, `exp`, `ln`, `pow`, `sin`,
  `cos`, and `tanh`. `range` builds a list of ints, charges one operation per
  element before building it, and fails above `max_collection_items`. `range`
  requires a non-zero step. `int` truncates toward zero and fails outside the
  int64 range.
- CPU hcode tensors are immutable, row-major arrays of rank 1 through 8 with
  dtype `i64` or `f64`. Build them with `tensor(nested_list)` (the dtype is `f64`
  if any element is a float), or with `zeros(shape, "i64")` and
  `zeros(shape, "f64")`. Other builtins: `shape`, `reshape`, `transpose`,
  `tolist`, `cast`, `concat` (joins a list of tensors along the first axis; the
  tensors must share one dtype and every dimension after the first), `sum`,
  `dot`, `matmul` (matrix-matrix, matrix-vector, or
  vector-matrix), `relu`, `gelu` (tanh approximation), and `softmax` (along the last
  axis). `t[i]` returns a scalar for rank 1 and otherwise the sub-tensor, and
  `len(t)` is the first dimension. Tensors support `+ - * /` with an equal-shape
  tensor or a scalar operand (no other broadcasting), `==` and `!=`, and unary `-`.
  They have no ordering operators, no index assignment, and no `for` iteration.
- A tensor's canonical output is the object
  `{"data":[...],"dtype":"f64","shape":[...]}`. `tolist` converts to nested lists,
  which lose the shape when a dimension is zero. Every shape must satisfy
  `walk_count <= max_collection_items`, where `walk_count` is the product of its
  dimensions with each zero dimension counted as one. The check runs before any
  storage is reserved, so a zero-size tensor cannot hide a large dimension.
  Each operation is charged before it runs, per position it visits (`walk_count`
  of its operand): one unit for elementwise, reshape, transpose, cast, sum, and
  conversion work; `m + m*n + m*k*n` for `matmul` (one per output row, one per
  output position, and one per multiply-add); one per multiply-add for `dot`; 24
  per position for `gelu`; and 20 per position for `softmax`. `concat` charges
  one unit per position of the result it joins, measured from the joined shape
  before any element is copied.
- `freivalds_check(a, b, c, seed, rounds)` verifies a claimed integer product
  `a * b == c` without recomputing it. `a`, `b`, and `c` are `i64` matrices with
  entries in the i32 range, and `rounds` is 1 through 64. Each round is charged
  `cols + inner*(cols+1) + rows*(1+inner+cols)` positions, where `a` is `rows` by
  `inner`, `b` is `inner` by `cols`, and `c` is `rows` by `cols`. A correct product
  always returns `true`. A wrong product passes one round with probability at most
  1/2 when the round vector is uniformly random, so it passes all rounds with
  probability at most `2^-rounds` with independent uniform vectors, independent
  of the fixed claim. In a verifier protocol, fix the claim first, then choose
  an independent unpredictable challenge. This builtin instead uses a
  deterministic splitmix64 stream from a 64-bit seed: it has no proven
  `2^-rounds` adversarial soundness guarantee, even with a secret seed. It is a
  reproducible diagnostic check, not Nodepool verification or a replacement for
  replica consensus.
- CPU hcode reserves every builtin name above: a `fn` with one of those names is
  rejected before execution, because builtins resolve first and the user function
  would never run. The frozen v0 language keeps its previous behavior.
- CPU hcode floats are IEEE-754 binary64 values written as `1.5`, `1e3`, or
  `2.0`, and JSON numbers with a fraction or exponent are accepted as floats.
  Every float stays finite: an operation whose result is NaN or infinite fails
  the execution where it happens, including division by zero and literals such
  as `1e400`. `-0.0` is folded to `0.0`. Mixed int and float arithmetic
  promotes the int to float first.
- `exp`, `ln`, `sin`, `cos`, `tanh`, and `pow` are computed by fixed software
  series, never by the host math library, so every replica gets the same bits.
  Each call is charged 16 operations on top of its arguments. `sin` and `cos`
  accept `|x|` up to 1000000. `ln` requires a positive argument, and `pow` fails
  for a negative base with a non-integral exponent. Accuracy is a few units in
  the last place for `exp`, `ln`, `sin`, `cos`, and `tanh`; `pow` can lose more
  digits when `|exponent * ln(base)|` is large (see `known_limitations`).
- `if` requires an `else` and one expression per branch. Bare reassignment,
  `while`, `break`, `continue`, member-dot access, and source comments are
  unsupported. Use `let` to rebind a variable.

Forbidden in hcode (and in the frozen v0 language):

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

## Tensor artifact inputs (CPU hcode)

The embedding API accepts immutable safetensors bytes through `TensorArtifacts`.
The host calls `insert(name, bytes, expected_sha256)` and then
`ManagedExecutor::execute_json_input_with_artifacts`. Insertion verifies the full
file's SHA-256 and layout before exposing any tensor. The digest is 64 lowercase
hex digits, optionally prefixed with `sha256:`. Names are exact lookup keys, not
paths or URLs; hcode still has no filesystem or network access.

| Builtin | Result |
| --- | --- |
| `load_tensor(artifact, name)` | Decode the entire named tensor with its stored shape. |
| `load_tensor_slice(artifact, name, offset, shape)` | Decode a contiguous row-major slice, starting at a zero-based **element** offset, into the requested shape. This is not strided or axis slicing. |
| `tensor_info(artifact, name)` | Return `{"dtype":"F32","shape":[2,2]}` (the original storage dtype and shape), without decoding elements. |

For a supplied artifact named `model` with an F32 tensor `weights = [[1,2],[3,4]]`:

```hcode
let w = load_tensor("model", "weights");
return tolist(matmul(w, tensor([1.0, 2.0])));
```

The output is `[5.0,11.0]`.

Storage types `I8`, `U8`, `I16`, `I32`, and `I64` decode to exact i64 values;
`F16`, `BF16`, `F32`, and `F64` decode to finite f64. Negative zero normalizes to
positive zero. A non-finite value fails when its slice is decoded. All tensors
have rank 1–8; rank-zero scalars and other storage dtypes are rejected.

Limits apply independently of the 1 MiB JSON input cap:

- At most 16 artifacts, 64 MiB per complete file, and 256 MiB total per execution.
- At most 1 MiB of header JSON and 4,096 tensors per file. Artifact and tensor
  names must be nonempty and at most 255 UTF-8 bytes.
- Headers must have unique tensor names and valid shapes; data ranges must match
  dtype sizes and cover the payload without overlaps, gaps, or trailing bytes.
- Decoded slices retain the existing 100,000-position, 1 MiB value, 16 MiB
  cumulative materialization, and 1 MiB output limits. A large stored tensor
  must be read in bounded slices; loading it does not bypass value limits.

Loading charges one usage unit per selected position **before** decoding, in
addition to normal expression/call costs. As with other tensor operations,
zero dimensions count as one for the work bound. `tensor_info` charges one unit
per dimension. Header parsing and digest verification are bounded host admission
work, not interpreter `executed_ops`.

This API does not expose artifact outputs, arbitrary model code, ONNX/GGUF
execution, GPU loading, or an automatic large-language-model inference engine.

### Submitting model inputs

The CLI prepares the manifests, creates the task, then uploads the exact bytes it
hashed, in chunks of at most 1 MiB. Repeat `--artifact` for multiple files:

```powershell
hivemind-bin submit inference.hcode --api http://127.0.0.1:8082 --username alice --password "$env:HIVEMIND_PASSWORD" --input "[1.0,2.0]" --artifact model=weights.safetensors --max-cpt 10000
```

To price the task before sending it, run the same function and files locally.
`check` needs no account and prints the usage of one replica and the smallest
`--max-cpt` that funds it; `--max-cpt auto` on `submit` uses that number:

```powershell
hivemind-bin check inference.hcode --input "[1.0,2.0]" --artifact model=weights.safetensors
```

Use the normal CLI authentication options. The name before `=` is the lookup key
used by `load_tensor`; the path is read by the CLI, never by the hcode program.
Task artifact IDs use the platform's safe ASCII identifier rules. Tensor names
inside the safetensors header retain the UTF-8 rules above. A runnable
[linear inference example](../examples/hcode/tensor-inference/README.md) includes
a standard-library Python generator for a multi-chunk safetensors fixture.

The task input sent to Master/Nodepool is an explicit JSON envelope:

```json
{
  "$hcode_tensor_inputs": 1,
  "input": [1.0, 2.0],
  "artifacts": ["ArtifactManifest objects, not file paths or URLs"]
}
```

The example illustrates the envelope shape; each `artifacts` entry must actually
be a validated input `ArtifactManifest` with MIME type
`application/x-safetensors`, a whole-file SHA-256, size, and a complete contiguous
chunk manifest. Inline bytes are not accepted. The CLI constructs these objects;
clients do not need to construct the envelope manually. The complete envelope
must still fit the 1 MiB JSON input limit. The root key `$hcode_tensor_inputs` is
reserved: its presence opts into strict envelope validation, not ordinary input.

The consensus request/input digest binds the **complete envelope**, including
all model hashes and chunk coordinates. Workers pass only its `input` field to
the interpreter. Existing JSON inputs without the reserved key are unchanged.

Nodepool persists manifests with the task before accepting chunks through the
existing Master endpoint
`POST /api/tasks/{task_id}/general-compute/artifacts/chunk`. The historical route
name does not change the task runtime: this remains CPU `hcode`, not general
compute. Uploads require the task owner's session (or an administrator), and
cannot replace pinned bytes. Incomplete uploads leave the task pending, without
an execution hold or replica assignment, and do not occupy dispatch candidate
slots. Expired inputs fail a pending task without billing once it has no held
usage or active Worker reservations. Identical chunk uploads are idempotent;
this CLI version does not offer a resume command.

Artifact tasks have an at-most-24-hour input-availability deadline, reserve at
least 1 GiB of Worker memory, and share a 1 GiB per-owner quota measured in
declared artifact bytes (not physical PostgreSQL storage). Reservations count
before upload. Expired hcode payloads are reclaimed when that owner submits new
artifact work; this is not a periodic garbage collector. Storage and transfer
have hard bounds but no separate CPT price in this implementation. Execution
continues to use the normal hcode metering and consensus settlement.

Workers download only declared chunks with a Nodepool-issued, attempt-bound
execution token. Each request checks the current task, owner, Worker, replica,
round, request digest, deadline, and active assignment. A session token or stale
execution token cannot read model bytes. The Worker checks chunk hashes and the
whole-file digest and validates safetensors layout before execution. Cancellation
also stops an outstanding download. This transport adds no network or filesystem
capability to hcode itself.

`GeneralComputeArtifactService.AuthorizeHcodeChunk` performs the same live
assignment checks without reading or returning chunk bytes. It returns the
pinned whole-file and chunk SHA-256 digests and sizes. The current consensus
transfer generation must be `1`; missing or different generations are rejected
by both authorization and download. This response is a point-in-time access
check, not an offline capability or proof that any peer still has the content.
The existing complete/available metadata and expiry checks remain in effect.
This control-plane API prepares for P2P and authorized cache reads; the current
CLI upload and Worker download path still transfers bytes through Nodepool.

`hivemind_auth::artifact_read` provides the separate P2P read-token codec. Tokens
use Ed25519, `typ=hcode-artifact-read+jwt` and `purpose=hcode-artifact-read-v1`,
with a source UUID audience, lease UUID, task/artifact IDs, whole-file and chunk
SHA-256 digests, sizes, and chunk offset. Their lifetime is at most 60 seconds,
with no expiry grace, and their encoded size is at most 4 KiB. Signing must use
the earliest live assignment, artifact and lease deadline. Account and execution
claims are deliberately absent; account, enrollment and execution credentials
are not interchangeable with these tokens. Each capability covers at most
1 MiB, including a potentially shorter final chunk.

This codec authenticates claims only. It does not issue leases or enable peer
transfers yet. The source registry must persist the lease's current Worker,
attempt, round, generation and immutable scope. Every peer read must recheck that
lease with Nodepool, including on a local cache hit; signature validity alone is
not authorization to serve content. The Worker execution token stays on the
Worker-to-Nodepool control path and must never be forwarded to a source peer.

Assigned Workers necessarily receive the input and model bytes. Authentication
limits access to assigned replicas; it does not make public-network Workers
confidential or prevent them from retaining bytes they received. Do not submit
private data or model weights that cannot be disclosed to those providers.

## hcode-gpu extension

GPU-enabled managed functions use a separate runtime identity,
`hcode-gpu`, and the canonical
`executor-rs/crates/managed-function-runtime/hcode-gpu-semantics.json`
manifest. This keeps floating-point and GPU behavior out of the frozen v0
result contract.

hcode-gpu adds only fixed, Rust-owned operations:

- `gpu_add_f32(lhs, rhs)`
- `gpu_scale_f32(value, scalar)`
- `gpu_matmul_f32(lhs, rhs)`

The DSL receives bounded host-side numeric values. It cannot provide CUDA C,
PTX, pointers, device handles, kernel source, dynamic libraries, or an
executable. The operator-selected backend owns CUDA/cuBLAS resources, and a
GPU-required request fails closed when a trusted compatible GPU is unavailable;
it never silently uses the CPU reference backend.

hcode-gpu permits floating-point and math operations only inside its explicit GPU
execution context. Its completion path is separate from CPU hcode consensus:
Nodepool validates the typed result against the persisted request manifest,
assignment, and trusted capability snapshot, then stores the result and performs
GPU-specific fixed-reservation settlement in one transaction. It does not require
a CPU quorum certificate or use CPU hcode operation-based billing, and does not fall
back to legacy result-torrent completion.

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

## hcode metering

The hcode cost vectors use the same evaluator operation accounting, including
function-call, print, and loop-iteration costs. The pinned runtime's
`max_usage_units` is an internal per-replica work allowance and is the only
dynamic work stop; Nodepool derives that value by evenly splitting the
fee-exclusive execution budget from task-wide `max_cpt` across the configured
replicas. `max_ops` and `max_loop_iterations` use the unbounded sentinel.
Receipt `usage_units` and `executed_ops` are equal per replica and are checked
before task-level settlement.

## Billing and consensus

The frozen v0 contract retains its historical fixed-reservation and diagnostic
receipt semantics. V0 is no longer accepted for new work; completed v0 records
are preserved as historical evidence, and unfinished v0 work is cancelled at
Nodepool startup rather than being converted to hcode.

For hcode, consensus selects the output; validated usage determines payment:

```text
canonical result = managed-consensus-result-v1
output digest    = sha256
quorum           = Nodepool certificate over the configured replica quorum
usage            = sum of valid accepted per-replica executed operations
charge           = aggregate usage + Nodepool fee, never above task max_cpt
```

A managed task is dispatched to distinct eligible Workers. Nodepool validates
the task, attempt, replica, result, identity, and usage evidence. The winning
certificate selects the output, while valid accepted replica work is aggregated
for settlement. The task-wide `max_cpt` is the hard maximum total charge,
including the 10% platform fee; accepted usage plus fee cannot exceed it. For
the default three-replica set, Nodepool assigns the same deterministic integer
usage budget to each replica from the fee-exclusive cap, leaving any indivisible
remainder with the owner. A valid divergent replica need not be a certificate
participant to have its verified work included in the capped task settlement.

No certificate is fabricated merely to justify payment. A task that ends
without a winning certificate never becomes `COMPLETED`. Under the current
billing identity, `hcode-billing-v3-failed-no-charge`, such a task is not
charged: the full hold is refunded and no Worker is paid. Holds created under
the earlier identities, `hcode-billing-v1` and `hcode-billing-v2-total-budget`,
keep the usage-only settlement they were created with, in which accepted
terminal evidence settles without a certificate. If policy is missing,
disabled, malformed, or cannot safely execute, the route fails closed. There is
no single-Worker, receipt-only, evidence-free settlement, or unsafe fallback
path.

## Execution boundary

The runtime is a closed interpreter over the `Value` enum. It has no API for
arbitrary process creation, shell commands, filesystem access, network access,
imports, reflection, dynamic evaluation, or host callbacks. The Worker only
returns the typed result and receipt; Nodepool is the sole verification,
consensus, payment, settlement, and billing authority.

The v0 contract is frozen and must not be mutated. Active hcode revisions
replace the existing hcode contract in place, without a new runtime name. Every
semantic or cost-model change updates the canonical manifest and its pinned
hash, with compatibility and metering tests. Nodepool and Workers must deploy
matching builds; runtime names alone do not establish semantic compatibility.

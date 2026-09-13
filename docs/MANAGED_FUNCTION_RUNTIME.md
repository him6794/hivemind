# Managed Function Runtime Plan

## Goal

The Managed Function Runtime is a restricted, metered execution path for small
serverless-style Hivemind tasks. It is the only supported task runtime:
ZIP/package execution has been removed, and managed functions provide
predictable billing and a small, tightly bounded execution surface.

The first milestone is a Rust executor that parses a fixed syntax, evaluates it
without file, network, import, subprocess, or reflection support, and returns an
execution receipt for diagnostics and audit records.

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

## Runtime Contract

Input:

- source text using the supported syntax below
- execution limits: max operations, max call depth, max output bytes
- optional function arguments in a later milestone

Output:

- final value
- printed output
- execution receipt
- structured failure when parsing, validation, metering, or runtime evaluation
  fails

Hivemind task integration:

- set `runtime = "managed-function-v0"`
- set `task_source` to the managed function source text
- set `torrent` / `torrent_source` to the JSON input payload when input is
  needed
- `managed-function-v0` is the only supported task runtime; ZIP/torrent-based
  task execution has been removed

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

The scheduler stores these fields as diagnostic evidence. Managed settlement is
authorized only by the Nodepool-owned consensus certificate and the fixed task
reservation, never by a single Worker receipt or usage claim.

## Supported Syntax v0

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
- `for` only iterates lists and is bounded by `max_loop_iterations`.
- Built-in functions currently include `len(value)`, `get(target, key)`, and
  `contains(target, value)`.

Forbidden in v0:

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

## Billing Direction

Managed function settlement uses the Nodepool-owned fixed reservation. The
execution receipt remains useful for diagnostics, audit, and capacity analysis,
but Worker-reported usage does not determine the amount charged.

The result contract is:

```text
canonical result = managed-consensus-result-v1
output digest    = sha256
settlement       = Nodepool certificate over a strict-majority quorum
billing          = fixed task reservation
```

A managed task is dispatched to distinct eligible Workers. Settlement requires
a valid quorum certificate, matching persisted replica evidence, and the exact
task, attempt, and round identity. Worker usage and operation counts remain
diagnostic evidence and do not authorize settlement.

If the consensus policy is missing, disabled, or cannot reach quorum, the task
remains un-settled or fails closed. There is no single-Worker, receipt-only,
or legacy fallback path.

## Implementation Plan

1. Add a new Rust crate under `executor-rs/crates/managed-function-runtime`.
2. Implement a small lexer and recursive-descent parser for v0 syntax.
3. Implement evaluation over a closed `Value` enum.
4. Implement metering in the evaluator, not only in the parser.
5. Return a structured `ExecutionReceipt`.
6. Add CLI/service integration after the core crate is stable.
7. Keep fixed-reservation billing and consensus certificate settlement in the
   Nodepool control plane; receipts remain diagnostic evidence.

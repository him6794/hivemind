# Managed Function Runtime Plan

## Goal

The Managed Function Runtime is a restricted, metered execution path for small
serverless-style Hivemind tasks. It is separate from ZIP/package execution:
package execution stays available for trusted or private worker pools, while
managed functions provide predictable billing and a smaller sandbox surface.

The first milestone is a Rust executor that parses a fixed syntax, evaluates it
without file, network, import, subprocess, or reflection support, and returns an
execution receipt that can be used for deterministic billing.

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

Receipt fields:

- `status`
- `executed_ops`
- `function_calls`
- `loop_iterations`
- `max_call_depth`
- `output_bytes`
- `failure_code`
- `failure_message`

## Supported Syntax v0

Statements:

```text
let name = expression;
fn name(arg1, arg2) { return expression; }
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
- loops

Loops are intentionally excluded from v0. They can be added later with explicit
iteration budgets once the operation metering and receipt path are proven.

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
| return | 1 + expression cost |

Execution stops with `op_limit_exceeded` before an operation would exceed the
configured limit.

## Billing Direction

Managed function billing should be derived from the receipt, not from wall-clock
time alone.

Suggested first formula:

```text
total_cpt =
  base_invocation_cpt
  + ceil(executed_ops / 1000) * op_block_cpt
  + ceil(output_bytes / 1024) * output_kib_cpt
```

The runtime should persist the receipt before settlement so billing can be
recomputed from source, limits, pricing config, and receipt data.

## Implementation Plan

1. Add a new Rust crate under `executor-rs/crates/managed-function-runtime`.
2. Implement a small lexer and recursive-descent parser for v0 syntax.
3. Implement evaluation over a closed `Value` enum.
4. Implement metering in the evaluator, not only in the parser.
5. Return a structured `ExecutionReceipt`.
6. Add CLI/service integration after the core crate is stable.
7. Add billing ledger integration after receipts are persisted.

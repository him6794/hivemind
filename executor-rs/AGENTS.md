# EXECUTOR RUNTIME KNOWLEDGE

## OVERVIEW
This independent Rust workspace supplies the managed-function runtime, transpiler, CLI, and typeshed consumed by Hivemind workers.

## WHERE TO LOOK
- `crates/managed-function-runtime` — task runtime behavior
- `crates/managed-function-transpiler` — source transformation
- `crates/monty-cli` — runtime CLI
- crate `tests` and `test_cases` — runtime contracts

## CONVENTIONS
- Keep runtime behavior deterministic and sandbox-compatible.
- Test task source, limits, and error behavior before changing runtime semantics.
- A worker release must verify the runtime executable exists and is pinned.

## ANTI-PATTERNS
- Do not broaden filesystem/network capabilities silently.
- Do not confuse upstream Monty release notes with Hivemind release policy.
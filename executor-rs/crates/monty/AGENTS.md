# MONTY CRATE KNOWLEDGE

## OVERVIEW
Monty is the high-complexity interpreter crate with extensive fixtures and behavioral tests.

## WHERE TO LOOK
- `src/` — interpreter implementation
- `tests/`, `test_cases/` — executable language and compatibility contracts
- workspace manifests — feature/MSRV/lint constraints

## CONVENTIONS
- Change behavior only with a focused regression test and relevant fixture coverage.
- Keep parser/runtime errors deterministic and user-observable.
- Run the narrow test target before the full executor workspace suite.

## ANTI-PATTERNS
- Do not delete or weaken fixtures to silence failures.
- Do not add network or host filesystem access to interpreter semantics.
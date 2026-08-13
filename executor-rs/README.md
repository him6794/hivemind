# Hivemind executor runtimes

This Rust workspace contains the execution libraries used by Hivemind:

- `managed-function-runtime` implements the deterministic, metered
  `managed-function-v0` DSL used by the Worker and managed-proof guest.
- `general-compute-runtime` owns the versioned contracts and bounded
  supervisor primitives for the planned `general-compute-v1alpha1` backends.

The general-compute crate is a backend/supervisor library, not a replacement
for the managed-function runtime and is not currently routed by the Worker.

Run from this directory:

```text
cargo check --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
```

The Hivemind Worker imports `managed-function-runtime` by path, so changes to
that crate must preserve deterministic semantics, metering, cancellation, and
the proof-facing output contract.

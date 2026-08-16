# Hivemind executor runtimes

This Rust workspace contains the execution libraries used by Hivemind:

- `managed-function-runtime` implements the deterministic, metered
  `managed-function-v0` DSL used by the Worker and managed-proof guest.
  Production closed-DSL execution is exposed as `production_sandboxed_dsl`;
  it is cross-platform and requires no Windows Containers or HCS.
- `general-compute-runtime` owns the versioned contracts and bounded
  supervisor primitives for the planned `general-compute-v1alpha1` backends.

The general-compute crate is a separate backend/supervisor library, not a
replacement for the managed-function runtime. The Worker routes
`general-compute-v1alpha1` through an operator-owned capability registry:
`reference_direct` is reference/test-only, while
`production_sandboxed_oci` requires a pinned OCI runner, task-bound artifact
materialization, and a validated bundle. Windows general-compute uses the
separate `production_sandboxed_windows` HCS backend and only requires Windows
Containers when that backend is selected. The runner must emit the versioned
`general-compute-result-v1` envelope; its `input_sha256` is the canonical,
length-framed digest of the materialized source followed by inputs. Missing or
mismatched production configuration or result claims fail closed as
`backend_unavailable`.

Run from this directory:

```text
cargo check --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
```

The Hivemind Worker imports `managed-function-runtime` by path, so changes to
that crate must preserve deterministic semantics, metering, cancellation, and
the proof-facing output contract.

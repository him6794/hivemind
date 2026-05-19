# Architecture Notes

- Worker task execution currently has two executor tracks in the same Go package.
- The runtime-facing path is already wired through `services/worker/cmd/server/main.go` into `pkg/executor`.
- The duplicate implementation is a packaging/build issue, not a protocol mismatch.
- No proto schema change is needed for this failure mode.
- The v2 executor path includes resource limit and monitor support and is the only executor path now included in default worker builds.
- The legacy executor remains available only when explicitly building with `-tags legacy_executor`.
- Rust sandbox execution is expected to use a repo-local `executor-rs` binary when available, with `MONTY_EXECUTABLE` as an explicit override.
- The current sandbox contract is binary/CLI based; no proto schema or service topology change is involved.

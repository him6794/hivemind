# Copilot guidance

Read the repository [AGENTS.md](../AGENTS.md) for current commands and layout, and [AGENT.md](../AGENT.md) for the product boundaries and load-bearing trust model. Rust-specific workspace guidance is in [hivemind-rs/AGENTS.md](../hivemind-rs/AGENTS.md).

## Architecture essentials

- The backend is a Rust workspace. `hivemind-bin` composes the `master`, `nodepool`, and `worker` roles; Nodepool owns trusted state and validates claims from user-deployed Master and Worker processes.
- New managed work uses the closed `managed-function-v1` runtime. General compute is a separate, explicitly sandboxed execution path. Do not conflate the Windows DSL interpreter with Windows HCS general-compute execution.
- The three frontend surfaces are `frontend/` (Next.js official site), `frontend/master-ui/`, and `frontend/worker-ui/` (Vite/React). See `AGENT.md` for which product responsibilities belong on each surface.

## Focused checks

- Backend: run `cargo test` from `hivemind-rs/`; use `cargo test -p <crate>` for a focused crate. The managed/general-compute runtime also has its own `executor-rs/` workspace.
- Frontends: run `npm test` in the affected frontend directory; use that directory's `npm run build` to verify its production build.
- For release and documentation contracts, follow the PowerShell test commands in `AGENTS.md` and the corresponding scripts under `scripts/`.

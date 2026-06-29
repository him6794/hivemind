# Public Network Limitations

Hivemind should currently be operated as a controlled or semi-controlled worker
pool. It is not yet a drop-in replacement for Golem Network or Akash Network,
and it should not be exposed as an open public marketplace without additional
work.

## Current safe operating model

- Run workers that are owned by the operator or explicitly allowlisted.
- Keep requestor accounts and worker registration behind normal authentication.
- Use `EXECUTOR_SANDBOX_MODE=production` for non-development workers.
- Configure a non-default `JWT_SECRET`.
- Configure network egress policy before allowing production task execution.
- For workers without shared storage, serve `TORRENT_API_DIR` over controlled
  HTTP and set `TORRENT_TASK_ARTIFACT_BASE_URL`.
- Treat CPT as an internal quota/budget unit only.

## Not yet complete

The following public-network capabilities are not complete:

- open provider onboarding;
- resource attestation and anti-cheat benchmarks;
- hostile workload hardening beyond the current sandbox guardrails;
- public marketplace bidding and settlement;
- CPT-to-fiat or token conversion;
- dispute resolution;
- public SLA guarantees;
- large-scale scheduler benchmarks;
- full torrent/swarm artifact fetch for arbitrary public workers.

## Recommended rollout

1. Start with allowlisted providers and artifact-based CPU tasks.
2. Run the smoke benchmarks in `SMOKE_BENCHMARKS.md`.
3. Add worker capability calibration before accepting self-reported resources.
4. Add public marketplace and settlement only after execution, metering, and
   trust controls are proven.

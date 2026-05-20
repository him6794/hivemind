# Reliability Report

- Generated: 2026-05-20T01:09:00.976780+00:00
- Artifacts: `D:\hivemind\test_logs\reliability\20260520-090709`
- Runs requested: 1
- Runs completed: 1
- Long-running seconds configured: 30
- Network latency/jitter: 100ms + 0..250ms
- Overall pass: True
- DoD satisfied: False

## Run Results
- Run 1 `rel-20260520090714-r01`: passed=True failures=0

## DoD Gate Status
- [ ] 10 consecutive successful full pipeline runs
- [ ] 3 consecutive node failure simulations
- [x] true network delay simulation
- [ ] long-running workload >= 15 min
- [x] failure-injected workload
- [x] no task duplication
- [x] no leaked workers
- [x] no stuck task ownership
- [x] no zombie reconnect state

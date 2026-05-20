# Reliability Report

- Generated: 2026-05-20T01:02:35.399080+00:00
- Artifacts: `D:\hivemind\test_logs\reliability\20260520-085706`
- Runs requested: 1
- Runs completed: 1
- Long-running seconds configured: 30
- Network latency/jitter: 100ms + 0..250ms
- Overall pass: False
- DoD satisfied: False

## Run Results
- Run 1 `rel-20260520085710-r01`: passed=False failures=5

## DoD Gate Status
- [ ] 10 consecutive successful full pipeline runs
- [ ] 3 consecutive node failure simulations
- [x] true network delay simulation
- [ ] long-running workload >= 15 min
- [x] failure-injected workload
- [ ] no task duplication
- [ ] no leaked workers
- [ ] no stuck task ownership
- [ ] no zombie reconnect state

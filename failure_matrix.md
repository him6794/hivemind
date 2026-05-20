# Failure Matrix

| Run | Scenario | Result | Evidence |
| --- | --- | --- | --- |
| 1 | network latency/jitter | configured | `test_logs/reliability/.../rel-20260520084623-r01` |
| 1 | worker kill/reconnect | events=2 | `test_logs/reliability/.../rel-20260520084623-r01` |
| 1 | duplicate submission | rejected | `test_logs/reliability/.../rel-20260520084623-r01` |
| 1 | failure-injected task | checked | `test_logs/reliability/.../rel-20260520084623-r01` |
| 1 | long-running task | checked | `test_logs/reliability/.../rel-20260520084623-r01` |
| 1 | parallel workload | checked | `test_logs/reliability/.../rel-20260520084623-r01` |

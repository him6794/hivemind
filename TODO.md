# TODO

## Remaining issues
- [x] Make `services/worker/pkg/executor` compile cleanly by excluding the duplicate implementation from default builds.
- [x] Re-run `go test ./...` for `services/worker`, `services/master`, and `services/nodepool`.
- [x] Fix worker executor tests failing because `MontyExecutable` points to `C:\Users\user\Desktop\monty\dist\monty.exe` while this repo contains `executor-rs\dist\monty.exe`.
- [x] Fix Rust sandbox CLI argument compatibility: current args use `--memory-limit`, `--max-stack-depth`, and `--max-allocations`; `monty.exe --help` shows `--max-memory`, `--max-recursion-depth`, and `--max-allocations`.
- [x] Fix monitored Monty execution stdout/stderr capture; `TestExecuteTask_SimpleScript` now executes successfully but reports empty stdout.
- [x] Fix `services/worker/examples/vpn_demo.go` build failure from obsolete `TaskResult.Output` usage.
- [x] Fix nodepool VPN handler build failure caused by missing generated VPN pb service/types.
- [x] Fix nodepool VPN handler `GetDERPMap` call signature mismatch.
- [x] Fix duplicate `task_id` submission causing redispatch and state overwrite.
- [x] Run one clean live pipeline with nodepool, master, 3 workers, 5 labeled task submissions, duplicate submission rejection, worker crash cleanup, and reconnect.
- [x] Update `architecture_notes.md` with iteration 8 observations only, no architecture changes.
- [x] Append the latest verified test output to `test_logs/latest.log`.
- [ ] Complete a true network-delay simulation rather than only a worker outage/reconnect simulation.
- [x] Add a reliability harness calibration path with latency/jitter proxies and generated `reliability_report.md`, `failure_matrix.md`, and `flaky_behavior.md`.
- [x] Fix nodepool pre-dispatch worker probe timeout being too short for latency/jitter.
- [ ] Fix or configure the next evidenced latency failure: master `/api/upload-task` currently times out after 5s under the calibration latency path.
- [ ] Exercise a real failure-injected task through the live worker runtime; current live Monty path completed the labeled failure task.
- [ ] Exercise a real long-running workload through the live worker runtime; current live Monty path completed the labeled long-running task quickly.
- [ ] Investigate separate live billing/status issue where a newly registered user with zero balance can upload a result but the task list status becomes `FAILED: insufficient balance`.
- [ ] Investigate separate DB-backed nodepool test isolation issue when multiple parallel tests call `initDB` against the same fresh Postgres database.
- [ ] Run the full required E2E gate: 10 consecutive pipeline runs and 3 consecutive node failure simulations.

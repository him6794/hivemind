# TODO

## Remaining issues
- [x] Make `services/worker/pkg/executor` compile cleanly by excluding the duplicate implementation from default builds.
- [x] Re-run `go test ./...` for `services/worker`, `services/master`, and `services/nodepool`.
- [x] Fix worker executor tests failing because `MontyExecutable` points to `C:\Users\user\Desktop\monty\dist\monty.exe` while this repo contains `executor-rs\dist\monty.exe`.
- [x] Fix Rust sandbox CLI argument compatibility: current args use `--memory-limit`, `--max-stack-depth`, and `--max-allocations`; `monty.exe --help` shows `--max-memory`, `--max-recursion-depth`, and `--max-allocations`.
- [x] Fix monitored Monty execution stdout/stderr capture; `TestExecuteTask_SimpleScript` now executes successfully but reports empty stdout.
- [x] Fix `services/worker/examples/vpn_demo.go` build failure from obsolete `TaskResult.Output` usage.
- [ ] Fix nodepool VPN handler build failure caused by missing generated VPN pb service/types.
- [ ] Run the required end-to-end pipeline with 3 workers, 5 task types, crash/delay/reconnect/duplicate submission simulation.
- [ ] Update `architecture_notes.md` with observations only, no architecture changes.
- [ ] Append the latest verified test output to `test_logs/latest.log`.

# Findings

## Root cause
- `services/worker/pkg/executor` contains two parallel implementations in the same package: `executor.go`/`monty_runner.go` and `executor_v2.go`/`monty_runner_v2.go`.
- The v2 files are compiled by default, so Go sees duplicate declarations for `MaxDownloadSize`, `TaskResult`, `ExecuteTask`, `httpClient`, `validateURL`, `downloadFileToTemp`, `MontyRunner`, `NewMontyRunner`, and `FindExecutableScript`.
- Evidence: `go test ./...` in `services/worker` fails with redeclaration errors before any runtime test can run.

## Scope
- This is a build-time root cause, not a proto/API issue.
- The smallest safe fix is to keep one implementation in the default build and exclude the other from normal builds without changing architecture or proto schema.

## Fix applied
- Added the `legacy_executor` build tag to `services/worker/pkg/executor/executor.go` and `services/worker/pkg/executor/monty_runner.go`.
- Default builds now compile the v2 executor path only.

## Verification
- `go test ./pkg/executor -run TestResourceLimits` in `services/worker`: pass.
- `go test ./...` in `services/worker`: duplicate declaration failure is gone; remaining failures are separate root causes (`monty.exe` hard-coded path and `examples/vpn_demo.go` using an obsolete `TaskResult.Output` field).
- `go test ./...` in `services/master`: pass.
- `go test ./...` in `services/nodepool`: fails in VPN handler packages because generated `pb` does not contain VPN service types.

## Iteration 2 root cause
- Worker executor tests still failed because `NewMontyRunner` only checked `C:\Users\user\Desktop\monty\dist\monty.exe`.
- Evidence: `go test ./pkg/executor -run TestExecuteTask_SimpleScript -count=1` failed with `monty.exe not found at C:\Users\user\Desktop\monty\dist\monty.exe`.
- The repo contains the Rust sandbox binary at `executor-rs\dist\monty.exe` and `executor-rs\target\release\monty.exe`.

## Iteration 2 fix applied
- `NewMontyRunner` now resolves `MONTY_EXECUTABLE`, then repo-local `executor-rs` candidates, then the legacy path.
- Added a focused test that proves configured Monty paths are honored.
- Updated test startup logging to use the same resolver so it reports the actual sandbox binary.

## Iteration 2 verification
- `go test ./pkg/executor -run TestNewMontyRunnerUsesConfiguredExecutable -count=1`: pass.
- `go test ./pkg/executor -run TestMontyRunnerBuildArgs -count=1`: pass.
- `go test ./pkg/executor -run TestExecuteTask_SimpleScript -count=1`: no longer fails because Monty is missing; it finds `D:\hivemind\executor-rs\dist\monty.exe` and now fails with `exit status 2`, a separate CLI argument compatibility root cause.

## Iteration 3 root cause
- Go passed stale resource-limit flags to the Rust sandbox.
- Evidence: `monty.exe --help` shows `--max-memory` and `--max-recursion-depth`, while `buildMontyArgs` emitted `--memory-limit` and `--max-stack-depth`.
- The mismatch made Monty exit before executing the task (`exit status 2`).

## Iteration 3 fix applied
- Updated `buildMontyArgs` to emit `--max-memory <N>MB`, `--max-recursion-depth <N>`, and the existing `--max-allocations <N>`.
- Updated `TestMontyRunnerBuildArgs` expectations to match the current Rust sandbox CLI.

## Iteration 3 verification
- `go test ./pkg/executor -run TestMontyRunnerBuildArgs -count=1`: pass.
- `go test ./pkg/executor -run TestExecuteTask_PrimeCalculation -count=1`: pass.
- `go test ./pkg/executor -run TestExecuteTask_SimpleScript -count=1`: Monty execution succeeds, but the test still fails because stdout is empty. That is a separate stdout capture root cause.

## Iteration 4 root cause
- The monitored executor path started Monty with `cmd.Start()` and `cmd.Wait()` but did not attach stdout/stderr buffers before starting the process.
- After `Wait`, it attempted to read `cmd.Stdout` and `cmd.Stderr` as `*os.File`, so successful executions returned empty output.
- Evidence: `TestExecuteTask_SimpleScript` showed `Execution completed successfully` but failed with `Expected stdout output, got empty`.

## Iteration 4 fix applied
- Attached `bytes.Buffer` to `cmd.Stdout` and `cmd.Stderr` inside `executeWithMonitoring` before process start.
- Returned the captured buffer contents after `cmd.Wait()`.

## Iteration 4 verification
- `go test ./pkg/executor -run TestExecuteTask_SimpleScript -count=1`: pass.
- `go test ./pkg/executor -run TestExecuteTask_PrimeCalculation -count=1`: pass.
- `go test ./pkg/executor -count=1`: pass.

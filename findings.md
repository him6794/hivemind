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

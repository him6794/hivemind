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

## Iteration 5 root cause
- `services/worker/examples/vpn_demo.go` still used the removed `TaskResult.Output` field.
- Evidence: `go test ./...` in `services/worker` failed in `services/worker/examples` with `result.Output undefined` and `unknown field Output in struct literal`.

## Iteration 5 fix applied
- Updated the example to read and set `TaskResult.Stdout`, matching the current executor result API.

## Iteration 5 verification
- `go test ./examples -count=1` in `services/worker`: pass.
- `go test ./...` in `services/worker`: pass.

## Iteration 6 root cause
- `services/nodepool/internal/handler` referenced VPN gRPC types and registration functions, but `services/nodepool/pb` did not include generated files for `proto/vpn.proto`.
- Evidence: `go test ./...` in `services/nodepool` failed with undefined symbols such as `pb.UnimplementedVPNServiceServer`, `pb.JoinVPNRequest`, and `pb.GetTaskPeersRequest`.
- `proto/vpn.proto` already defined the service and messages; the schema was present but the generated Go files were missing.

## Iteration 6 fix applied
- Generated `services/nodepool/pb/vpn.pb.go` and `services/nodepool/pb/vpn_grpc.pb.go` from the existing `proto/vpn.proto`.
- No proto schema changes were made.

## Iteration 6 verification
- `go test ./...` in `services/nodepool`: VPN pb undefined-symbol failure is gone.
- Remaining failure: `internal/handler/vpn_handler.go:68` calls `GetDERPMap(context.Context)`, while the manager method accepts no arguments. That is a separate root cause.

## Iteration 7 root cause
- `services/nodepool/internal/handler/vpn_handler.go` called `h.vpnManager.GetDERPMap(ctx)` even though `HeadscaleManager.GetDERPMap()` takes no arguments.
- Evidence: `go test ./...` in `services/nodepool` failed with `too many arguments in call to h.vpnManager.GetDERPMap`.

## Iteration 7 fix applied
- Changed the handler to call `GetDERPMap()` with no arguments.

## Iteration 7 verification
- `go test ./internal/handler -count=1` in `services/nodepool`: pass.
- `go test ./...` in `services/nodepool`: pass.

## Iteration 8 root cause
- Live duplicate submission could execute the same logical task twice.
- Evidence from the pre-fix live run `test_logs/live/submission` with run id `20260520073902`:
  - `upload-responses.json` shows the second submission for `e2e-20260520073902-cpu` returned `success=true` and `status_message=dispatched`.
  - `nodepool-events.log` contains two `task_dispatch_success` lines for the same `task_id=e2e-20260520073902-cpu`.
  - `cpu-log.json` contains two separate `task accepted` and `task completed` sequences, and the task result BTIH was overwritten by the duplicate submission BTIH.
- Code root cause: `masterNodeServer.UploadTask` always updated `m.tasks[task_id]` and proceeded to `dispatchTaskToWorkerWithExcludes` even when the task already existed.

## Iteration 8 fix applied
- Added a duplicate `task_id` guard inside the existing `UploadTask` critical section.
- If an existing task already has an owner, the request now returns `success=false` with `status_message=duplicate task_id`, logs `task_duplicate_rejected`, and does not mutate task state or dispatch to a worker.
- Added `TestMasterNode_UploadTaskRejectsDuplicateTaskIDWithoutRedispatch` to prove the second upload is rejected, `ExecuteTask` is called once, and the original BTIH remains unchanged.

## Iteration 8 verification
- RED: `go test ./cmd/server -run TestMasterNode_UploadTaskRejectsDuplicateTaskIDWithoutRedispatch -count=1` failed before the fix with a second `task_dispatch_success`.
- GREEN: `go test ./cmd/server -run TestMasterNode_UploadTaskRejectsDuplicateTaskIDWithoutRedispatch -count=1` passed after the fix.
- `go test ./...` in `services/nodepool`: pass in the default local test environment.
- Rebuilt nodepool and ran a clean live pipeline with Postgres on `25432`, Redis on `26379`, nodepool on `50051/18081`, master on `18082`, and workers on `50053/50054/50055`.
- Live run id `20260520074442` submitted 5 task labels (`cpu`, `io`, `failure-injected`, `retry`, `long-running`); all 5 reached `COMPLETED` and their result endpoints returned `success=true`.
- Duplicate submission for `e2e-20260520074442-cpu` returned `success=false`, and `nodepool-dispatch-lines.log` contains one dispatch plus one `task_duplicate_rejected`, with no second dispatch.
- Worker2 crash simulation: worker2 process was killed, `/api/workers?include_offline=true` showed worker2 `OFFLINE` after heartbeat timeout, and after restart it returned to `ACTIVE`.
- Remaining gap: this iteration did not complete 10 consecutive full runs, 3 consecutive node failure simulations, or a true network-delay injection. Those remain separate DoD items.

## Iteration 9 root cause
- Under real latency/jitter, nodepool's pre-dispatch worker probe used a hard-coded `1*time.Second` timeout.
- Evidence from reliability calibration `test_logs/reliability/20260520-083100`, run `rel-20260520083101-r01`:
  - workers registered successfully and `/api/workers?include_offline=true` returned worker1, worker2, and worker3 as `ACTIVE`.
  - every task upload failed dispatch with `[PROBE_FAIL] worker probe failed at 127.0.0.1:50055`.
  - `latency-proxy.log` showed worker gRPC proxies were configured with `100ms + 0..250ms` jitter, while no task reached execution.
- Focused RED evidence: `TestMasterNode_DispatchTaskToWorker_WithLatencyProxy` failed with `reason="worker probe failed ..."` when a 300ms local TCP latency proxy wrapped the worker gRPC server.

## Iteration 9 fix applied
- Added `envDurationSeconds` in `services/nodepool/cmd/server/main.go`.
- Changed pre-dispatch probe timeout to `NODEPOOL_WORKER_PROBE_TIMEOUT_SEC`, defaulting to `5s`.
- Added `TestMasterNode_DispatchTaskToWorker_WithLatencyProxy` to prove dispatch succeeds through a latency proxy with pre-dispatch probe enabled.
- No proto schema or architecture changes were made.

## Iteration 9 verification
- RED: `go test ./cmd/server -run TestMasterNode_DispatchTaskToWorker_WithLatencyProxy -count=1` failed before the fix with `worker probe failed`.
- GREEN: `go test ./cmd/server -run TestMasterNode_DispatchTaskToWorker_WithLatencyProxy -count=1` passed after the fix.
- `go test ./...` in `services/nodepool`: pass.
- Reliability calibration rerun with worker registration direct and network latency still enabled on master->nodepool and nodepool->worker paths: `test_logs/reliability/20260520-084035`, run `rel-20260520084038-r01`.
- Current blocking issue after this fix: first upload now fails at the master HTTP layer with `POST /api/upload-task returned HTTP 502` and `rpc error: code = DeadlineExceeded desc = context deadline exceeded`; worker/task scenarios were not reached in that run.

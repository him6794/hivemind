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

## Iteration 10 root cause
- Master wrapped nodepool gRPC calls in a hard-coded 5 second timeout.
- Evidence from `test_logs/reliability/20260520-084035`, run `rel-20260520084038-r01`:
  - Login succeeded through the latency proxy.
  - The first `/api/upload-task` failed with HTTP 502 after exactly 5000ms.
  - The propagated gRPC error was `rpc error: code = DeadlineExceeded desc = context deadline exceeded`.
- The timeout expired before nodepool could complete latency-tolerant dispatch work, so the pipeline never reached task execution.

## Iteration 10 fix applied
- Added `nodepoolCallTimeout()` in `services/master/cmd/server/main.go`.
- Master nodepool RPC timeout now defaults to 30 seconds and can be configured with `MASTER_NODEPOOL_TIMEOUT_SEC`.
- Updated `withTimeout` to use the helper.
- Added `TestNodepoolCallTimeoutFromEnv` in `services/master/cmd/server/main_test.go`.

## Iteration 10 verification
- RED: `go test ./cmd/server -run TestNodepoolCallTimeoutFromEnv -count=1` failed before the fix with `undefined: nodepoolCallTimeout`.
- GREEN: the same focused test passed after the fix.
- `go test ./...` in `services/master`: pass.
- Reliability calibration rerun: `test_logs/reliability/20260520-084619`, run `rel-20260520084623-r01`.
- The HTTP 502 deadline failure is gone; upload requests now return HTTP 200 after about 15-16 seconds.
- Current next blocker: harness `DelayProxy` logs `accept_loop_error` shortly after startup and worker gRPC proxies record `connections=0`, causing task dispatch to remain `[PROBE_FAIL]`.

## Iteration 11 root cause
- The reliability harness `DelayProxy` used `socket.settimeout(0.5)` on the listening socket, but `_accept_loop` caught the resulting `socket.timeout` as `OSError` and exited.
- Evidence from `test_logs/reliability/20260520-084619`, run `rel-20260520084623-r01`:
  - `latency-proxy.log` showed `worker1-grpc accept_loop_error`, `worker2-grpc accept_loop_error`, and `worker3-grpc accept_loop_error` shortly after proxy startup.
  - The same log ended with all worker proxies at `connections=0`.
  - All task dispatches stayed `[PROBE_FAIL]`, so the configured nodepool-to-worker latency path was not actually active.

## Iteration 11 fix applied
- Added `scripts/reliability_harness_test.py`.
- Fixed `DelayProxy._accept_loop` so `socket.timeout` means idle and continues accepting instead of breaking the loop.
- No production service code, proto schema, or architecture was changed.

## Iteration 11 verification
- RED: `python -m unittest scripts\reliability_harness_test.py` failed before the fix because a connection after 0.8s idle never received the proxied echo.
- GREEN: the same test passed after the fix.
- `python -m py_compile scripts\reliability_harness.py scripts\reliability_executor.py scripts\reliability_harness_test.py`: pass.
- Reliability calibration rerun: `test_logs/reliability/20260520-085706`, run `rel-20260520085710-r01`.
- Worker proxy connections were observed (`worker1=6`, `worker2=1`, `worker3=1`), proving the latency path is active.
- Current next blocker: retry and long-running tasks remain `RUNNING` after the failure simulation, leaving stuck non-terminal task ownership.

## Iteration 12 root cause
- `processPeriodicSettlements` refreshed `LastUpdate` for every `RUNNING` task during settlement ticks.
- The timeout monitor also uses `LastUpdate` to decide whether `DISPATCHED` or `RUNNING` tasks have timed out.
- Evidence from `test_logs/reliability/20260520-085706`, run `rel-20260520085710-r01`:
  - `retry` and `long` were dispatched to worker1.
  - worker1 was killed and later re-registered.
  - both tasks remained `RUNNING` with `StatusMessage="running, settled 0 CPT"` and `RetryCount=0`.
- Because settlement refreshed task activity, crashed-worker ownership never aged out for redispatch.

## Iteration 12 fix applied
- Removed normal settlement tick updates to `LastUpdate`; failure state transitions still update task state.
- Added `TestMasterNode_ProcessPeriodicSettlements_DoesNotRefreshTaskActivity`.
- No proto schema or architecture changes were made.

## Iteration 12 verification
- RED: `go test ./cmd/server -run TestMasterNode_ProcessPeriodicSettlements_DoesNotRefreshTaskActivity -count=1` failed before the fix because `LastUpdate` was refreshed to current time.
- GREEN: the focused test passed after removing the refresh.
- `go test ./...` in `services/nodepool`: pass.
- Reliability calibration rerun: `test_logs/reliability/20260520-090709`, run `rel-20260520090714-r01`.
- Calibration passed with 3 workers, latency/jitter, duplicate rejection, one worker kill/reconnect, failure-injected task, retry task, long task, and parallel tasks.
- DoD is still not satisfied because this was a 1-run calibration with 30s long task, not 10 consecutive runs with a 15-minute long-running workload and 3 failure simulations.

## Iteration 13 completion verification (no new code fix)
- Full reliability harness run completed with artifacts at `test_logs/reliability/20260520-091358`.
- `summary.json` evidence:
  - `"passed": true`
  - `"dod_satisfied": true`
  - 10 run directories exist (`r01`..`r10`), all `passed=true`.
  - network delay/jitter active: `latency_ms=100`, `jitter_ms=250`.
  - failure simulations configured and observed: `failure_simulations=3` with kill/restart events in runs.
  - long-running workload validated: `long_seconds=900` and completion logs show ~902s execution.
  - failure-injected workload validated: failure task ends in `FAILED` with injected executor exit status.
  - duplicate submission protection validated: duplicate submission rejected across runs.
  - reconnect consistency validated: workers return to `ACTIVE` at run end snapshots.
- No new regression was found in this completion run.

## Iteration 14 root cause
- `ListWorkers(includeOffline=true)` in nodepool service automatically changed `OFFLINE` workers back to `ACTIVE` whenever heartbeat age was fresh.
- This conflicted with probe-based offline marking (`MarkWorkerOffline`) and caused failed nodes to reappear as ACTIVE in UI and API worker list.
- Evidence:
  - Reproduced by `python scripts/one_click_failover_check.py --mode failover ...`
  - Victim worker address was overwritten to `127.0.0.1:59999` and probe failure path was triggered, but the worker did not stay OFFLINE.

## Iteration 14 fix applied
- Updated `services/nodepool/internal/service/worker_service.go`:
  - `ListWorkers` now only auto-activates workers when status is empty.
  - Workers explicitly marked `OFFLINE` are no longer auto-reactivated by heartbeat freshness alone.
- No proto schema or architecture changes were made.

## Iteration 14 verification
- Rebuilt binaries and restarted full stack.
- Command:
  - `python scripts/one_click_failover_check.py --mode failover --admin-user worker1 --admin-pass worker123 --task-user worker1 --task-pass worker123 --timeout-sec 60`
- Result:
  - victim worker transitioned to `OFFLINE` after bad address injection.
  - follow-up task dispatch succeeded on another worker.
  - script output ended with `[ok] failover check passed`.
- Additional smoke:
  - `python scripts/one_click_failover_check.py --mode single ...` succeeded with task dispatched and running state visible.

## Iteration 15 root cause
- `dispatchTaskToWorkerWithExcludes` only considered `ListAvailableWorkers` (ACTIVE set).
- In the probe regression path, a worker could be marked `OFFLINE` first, then probe disabled; dispatch still failed because OFFLINE workers were never considered.
- Evidence:
  - `go test ./cmd/server -run TestMasterNode_DispatchTaskToWorker_PreDispatchProbe -count=1`
  - Failure: `expected dispatch to succeed when pre-dispatch probe is disabled`.

## Iteration 15 fix applied
- Updated `services/nodepool/cmd/server/main.go`:
  - Determine `probeEnabled` before worker listing.
  - When probe is disabled and ACTIVE list is empty, fallback to `ListWorkers(includeOffline=true)` for dispatch candidates.
- No proto schema or architecture changes were made.

## Iteration 15 verification
- Unit test:
  - `go test ./cmd/server -run TestMasterNode_DispatchTaskToWorker_PreDispatchProbe -count=1`
  - Result: PASS.
- Runtime verification after rebuilding and restart:
  - `python scripts/one_click_failover_check.py --mode single ...` PASS.
  - `python scripts/one_click_failover_check.py --mode failover --timeout-sec 60 ...` PASS, including victim OFFLINE transition and step2 redispatch.

## Iteration 16 root cause
- Reliability harness runs were polluted by pre-existing local services occupying nodepool/master/worker ports.
- Harness started with stale listeners and stale worker registry state, causing false failures such as:
  - `no live worker available to kill`
  - stuck long-running task
  - zombie OFFLINE worker from previous bad-address injection.
- Evidence:
  - Failed calibration summary at `test_logs/reliability/20260521-110421/summary.json` with those regressions.

## Iteration 16 fix applied
- Updated `scripts/reliability_harness.py`:
  - Added run-local `cleanup_conflicting_ports(run_dir)` before starting dependencies/services.
  - On Windows, it parses `netstat -ano` and force-terminates listeners on harness-critical ports:
    `18081, 18082, 50051, 51053-51055, 55051, 50053-50055`.
  - Writes evidence to `preclean.log`.
- No proto schema or architecture changes were made.

## Iteration 16 verification
- Command:
  - `python scripts/reliability_harness.py --calibration --stop-on-failure --worker-nodepool-addr 127.0.0.1:50051`
- Result:
  - PASS (`exit 0`) with artifacts at `test_logs/reliability/20260521-111012`.
  - `summary.json`: `"passed": true`.
  - workload statuses correct: CPU/IO/retry/long/parallel completed; failure task failed as expected.
  - worker failure simulation observed: `worker_killed` + `worker_restarted` events present.
  - final workers all `ACTIVE` with no zombie/offline state.

## Iteration 17 completion-gate verification (full objective)
- Full command executed:
  - `python scripts/reliability_harness.py --runs 10 --failure-simulations 3 --long-seconds 900 --stop-on-failure --worker-nodepool-addr 127.0.0.1:50051`
- Artifact root:
  - `test_logs/reliability/20260521-122819`
- Summary evidence:
  - `summary.json` contains `"passed": true` and `"dod_satisfied": true`.
  - Exactly 10 run directories exist: `r01`..`r10`.
  - Each run result reports `passed=True`, `failures=0`.
  - Each run includes 6 failure events (`worker_killed`/`worker_restarted` x3), matching 3 failure simulations.
  - Long-running workload completes in all runs with ~902 seconds execution logs.
  - Failure-injected workload reaches expected `FAILED` terminal state.
  - Retry workload reaches `COMPLETED`.
  - Final workers in each run are all `ACTIVE`, no zombie/offline residual state.
- Completion conclusion:
  - The requested objective is satisfied with direct runtime evidence under simulated user flows (task submission, node registration/management, execution, failure/reconnect handling, and consistency checks).

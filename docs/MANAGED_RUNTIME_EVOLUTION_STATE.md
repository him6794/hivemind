# Task State: Managed Runtime 演進

## Goal

完整實作 [`MANAGED_RUNTIME_EVOLUTION_PLAN.md`](MANAGED_RUNTIME_EVOLUTION_PLAN.md) 的 M0–M5：保留 `managed-function-v0` 的 deterministic、bounded、proof-friendly 契約，同時交付隔離的 `general-compute-v1` runtime、科學運算 ABI／backend、Worker 與 Nodepool 接線、GPU beta，以及可用性發布 gates。

## Success criteria

- 每個可獨立驗收的小單元都先有能正確失敗的測試，再做最小實作、相容性驗證與本地 Conventional Commit；不 push。
- M0 凍結 v1 request/result、artifact manifest、capability matrix 與 threat model，schema/property tests 全綠且不破壞 v0 proof vectors。
- M1 交付 reference interpreter、bounded supervisor、Minsky／recursion／heap／cancel fixtures，以及 timeout/cancel 的 kill/reap 與 hostile escape gates。
- M2 交付 tensor ABI、dtype/complex、broadcast/reduce、BLAS/LAPACK、FFT、ODE、RNG、Monte Carlo、sparse，並以 NumPy/SciPy/reference golden 驗證數值與 failure semantics。
- M3 完成 Worker runtime routing、CAS/chunk transfer、quota/telemetry、retry/idempotency；Nodepool 只結算可信驗證後的 claims，多節點 E2E 通過。
- M4 完成 CUDA/ROCm capability negotiation、driver/image matrix、device artifacts 與明確 CPU fallback，錯配不誤派。
- M5 完成文件、SDK 範例、benchmark dashboard、support matrix、rollback，且 reproducibility/security/performance/release image digest 全部簽核。
- 每個 milestone 都保存測試命令與結果、fixture/hash、benchmark 原始資料、已知限制、rollback 與 owner；最終逐要求完成 completion audit。

## Status

running

### 2026-08-14 optimized backend registration checkpoint

- RED test at `executor-rs/crates/general-compute-runtime/tests/backend_registration.rs` was
  observed failing because the registration API was absent; the minimal GREEN
  implementation is now committed as `497f293`.
- Focused registration 3/3, locked runtime, production, sandbox, and MSVC/GNU
  Worker/Task Scheduler/Bin checks are green. Next action is the remaining
  multi-process/container OCI E2E and operator deployment validation.

## Current step

M0a/M0b contracts、M1 reference fixtures/supervisor/CPython adapter、M3
Worker/Nodepool routing、Worker durable CAS state、Nodepool immutable artifact
identity/source repository、bounded scientific references、typed GPU claim binding，
以及 Nodepool terminal-result persistence 均已有已測試的 local commits。
Production OCI packaging、reviewed multi-process fixture protocol、operator-owned
runner state root 與 canonical seccomp profile也已接線並 fail closed。

Nodepool transfer-lease lifecycle 已隔離為
`5b22af8 feat(scheduler): persist transfer lease lifecycle`：assignment／claim、
generation rotation、attempt/Worker binding、expiry 與 terminal revoke 都由
Nodepool transactionally 持久化；`b22fed5` 也已把完整 identity 與 generation
綁入 Ed25519 Worker execution token；`bae0207` 再凍結 bounded lease-authority
protobuf envelope 與 wire validator；`ecbbee4` 已完成 Nodepool RPC、Ed25519
full-identity comparison 與 repository-backed active-lease authority。四個過時的
general-compute fixtures 另以 `f017606`、`b7d8e34`、`a9d2e35`、`acc173a`
獨立修復，未混入 authority commit。`df48f19` 已完成 Worker production
fail-closed authority enforcement；`94576b6` 再完成 dispatcher authenticated
Prepare／Resume／Upload、immutable Nodepool source transfer、generation-bound token、
lease失效 redispatch 與 no-penalty typed failure。下一個 repository-local 單元是
隔離 production OCI result 的 Nodepool-owned canonical input-digest validation。
真正的 multi-process OCI run 仍必須由 operator 提供 pinned registry/runner/
rootfs/profile 與 canonical case plan；fake runner、image probe、preflight 或
單進程 DB tests 都不得被當成 production E2E 證據。

## Completed

- `be39bb7 refactor(runtime): remove unused Monty executable contract`
  - 移除 Hivemind build、Docker、config 與 Windows worker package 的舊 Monty executable contract。
  - executor workspace 29 tests passed。
  - `hivemind-config` 與 `hivemind-worker-executor` focused `cargo check --locked` passed。
  - Docker Compose release contract 與 Windows worker package contract passed。
- 使用者授權移除剩餘未接線的 Monty source、bindings、typeshed、fuzz crate 與專用建置／CI
  metadata；`executor-rs` 現在只保留 Hivemind 的兩個 runtime crate。刪除後 executor
  workspace tests、Worker check 與 release contract tests 均通過。
- 完成演進計畫文件，明確區分 v0 proof-friendly DSL 與 v1 general compute，並定義 M0–M5 gates；文件尚待本輪狀態修正後獨立提交。
- `f34b8eb feat(runtime): add general compute v1 contracts`
  - 新增獨立 `general-compute-runtime` crate，不依賴 Hivemind DB／scheduler。
  - 凍結 `GeneralComputeRequest`／`GeneralComputeResult`、execution/determinism policy、usage claim、artifact/chunk manifest 與 inline SHA-256 validation。
-  - schema tests 3 passed；executor workspace（v0 + v1）與 Hivemind config/worker consumer checks passed。
- `37ae840 feat(runtime): enforce v1 capabilities and threat boundaries`
  - 新增 typed validation errors、有限 execution quota、read-only filesystem gate、backend/image/worker capability matrix，以及 network/GPU/thread mismatch fail-closed checks。
  - artifact chunk manifest 必須連續且完整覆蓋 bytes；gap、overlap、overflow、checksum mismatch 都拒絕。
  - capability/schema tests 7 passed；executor workspace（含 v0 regression）7+1+3+25 passed；format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- `f4ac2b9 feat(runtime): add bounded framed protocol`
  - 新增 4-byte big-endian length-prefixed JSON frame encoder/decoder，先驗證 payload cap，再反序列化；decoder 回傳 consumed bytes 以支援連續 frame。
  - truncated header/payload、oversized payload、invalid JSON、encode cap 與 exact-one-frame consumption tests 4 passed；M0 schema tests 7 passed、executor workspace v0 regression、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- `5b342f1 feat(runtime): add bounded supervisor lifecycle`
  - 新增分離的 program/args 啟動、monotonic timeout、cooperative cancellation，以及 timeout/cancel 後 hard kill + wait/reap；空白 program fail-closed。
  - lifecycle tests 4 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- `0dea304 feat(runtime): bound supervisor output capture`
  - supervisor 以獨立 reader 持續 drain stdout/stderr，僅保留 `output_limit` bytes，並回傳各 stream 的 truncation 標記，避免 pipe back-pressure 與無界記憶體。
  - lifecycle tests 5 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- `a073180 feat(runtime): kill supervisor process trees`
  - timeout/cancel 時 Unix 建立獨立 process group 並以 group kill 清理 descendants；Windows 使用 `taskkill /T /F` 的 tree-kill fallback，完成後 wait/reap。
  - hostile descendant marker fixture 與 lifecycle tests 6 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 reference Minsky interpreter 小單元（`790b10b`）
  - 新增獨立 bounded reference module：`Inc`、`DecJump`、`Halt` instruction tape、checked jump targets、BigUint registers、step quota 與 cooperative cancellation。
  - Minsky halt/zero-test、non-terminating `resource_exhausted`、invalid target 與 cancellation fixtures 4 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 mutable heap 小單元（`5ba2ac5`）
  - 新增 checked `Set`／`Allocate`／`Store`／`Load` heap instructions，BigUint cell values、pointer/index validation、cell quota 與 typed `resource_exhausted`。
  - reference tests 6 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 recursion/call-depth 小單元（`74d532d`）
  - 新增 checked `Call`／`Return` recursion tape、return stack、deterministic depth tracking、stack-underflow error 與 call-depth quota。
  - reference tests 8 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 exception/exit semantics 小單元（`3dc57ae`）
  - 新增 `Raise`／`Exit`／`Jump` signal tape，明確回傳 `Exception`、`Exited`、`ResourceExhausted`、`Cancelled` 與 `Halted` 狀態及 optional exit code。
  - reference tests 10 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 differential harness 小單元（`9c0e49a`）
  - 新增可序列化 `DifferentialCase`／`ReferenceObservation`，固定 source、JSON input、seed 與 canonical status/steps/output；只允許 registry-pinned fixture 執行。
  - replay、backend mismatch、source/input/seed mismatch fail-closed tests 3 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 pinned CPython adapter interface 小單元（`752425a`）
  - 新增 registry-approved `PythonBackendRegistration`／`PythonBackendRegistry`／`PinnedPythonAdapter`，要求 executable、sha256 guest image、protocol version 與 output cap；observation 使用 `deny_unknown_fields` 並拒絕未知 status/超限 output。
  - adapter tests 3 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 bounded CPython subprocess 小單元（`870aa66`）
  - supervisor 新增 bounded stdin writer；CPython adapter 以 fixed `python -c` runner、framed stdin/stdout 傳遞 source/input/seed，payload 不進 command line；timeout/cancel 映射為 typed supervisor failure。
  - lifecycle tests 7、CPython adapter tests 6 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 CPython response hardening 小單元（`942cf0d`）
  - source exception 映射為 bounded `exception` observation；framed response 若有 trailing bytes 即 fail closed。
  - focused CPython tests 8 passed；executor workspace 69 tests、Worker check 與 Docker/Windows release-contract tests passed。
- M0 alpha runtime id 小單元（`d632e3d`）
  - 先加入要求 alpha id 且拒絕 stable `general-compute-v1` 的 RED test，再將 contract 常數與 fixtures 固定為 `general-compute-v1alpha1`。
  - focused contracts 8 passed；executor workspace 69 tests、Worker check 與 Docker/Windows release-contract tests passed。
- M0 request identity/digest binding 小單元（`6155b08`）
  - 先以 RED tests 證明 request/result 缺少 retry identity；加入 immutable `execution_id`、`attempt_id`、idempotency key、canonical request SHA-256 digest 與 `deny_unknown_fields`。
  - `GeneralComputeResult::validate_against` 驗證 request/result identity、runtime/backend/image/determinism binding；跨 attempt 的 result fail closed。
  - focused contracts 10 passed；executor workspace 69 tests、Worker check 與 Docker/Windows release-contract tests passed。
- M0 evidence/usage/artifact result validation 小單元（`89cb73e`）
  - 先以 RED tests 鎖定 evidence envelope、output manifest root、status/exit-code、usage quota 與 output role；加入 `EvidenceEnvelope`、worker-only `unverified` gate、canonical artifact root 與 result validator。
  - focused contracts 14 passed；executor workspace 73 tests、Worker check 與 Docker/Windows release-contract tests passed。
- M0 artifact/CAS manifest 小單元（`a1215ad`）
  - 先以 RED tests 鎖定 inline/CAS canonical root、chunk checksum、chunk-aligned range 與 resume；加入 metadata-only canonical artifact root、inline chunk verification、range validation 與 missing-chunk selection。
  - focused contracts 18 passed；executor workspace 77 tests、Worker check 與 Docker/Windows release-contract tests passed。
- M0 contiguous tensor ABI 小單元（`d02e9c4`）
  - 先以 RED tests 鎖定 `tensor-v1alpha1`、dtype/shape/byte-order/layout、checked shape/byte arithmetic、empty/zero-dimensional tensor、binary-only payload、unknown-field 與 logical hash。
  - 新增有限 contiguous tensor manifest；BigInt 僅允許 scalar，尚未宣稱 stride/view、sparse 或科學運算支援。
  - focused tensor tests 6 passed；executor workspace 83 tests、Worker check 與 Docker/Windows release-contract tests passed。
- M1 combined supervisor output 小單元（`dca1235`）
  - 先以 RED tests 證明 per-stream retained cap 會讓 discarded hostile output 逃過總量限制；加入 shared output budget、`OutputLimitExceeded` 狀態，超限立即 kill/reap。
  - focused lifecycle tests 9 passed；executor workspace 86 tests、Worker check 與 Docker/Windows release-contract tests passed。
- M1 trusted backend executable gate 小單元（`4f69bc4`）
  - 先以 RED tests 證明 shell interpreter、參數注入與 shell metacharacter 可進 Python registry；加入 registry-safe executable validation，避免 `CommandSpec` 由不可信字串構造 shell。
  - focused CPython tests 10 passed；executor workspace 87 tests、Worker check 與 Docker/Windows release-contract tests passed。
- M0a v0 semantics/cost/proof freeze 小單元（`docs(runtime): freeze managed v0 semantics manifest`）
  - 新增 canonical `managed-function-v0-semantics.json`，凍結 runtime/cost-model、default limits、billing、proof/admission binding、真實 receipt fixture 與可執行 cost vectors；canonical SHA-256 為 `8ed716dc07c7bc9abcfc5338b1888e71dd041c3fb397c45d0efb1ff76af1deee`，fixture SHA-256 為 `8221629b1ba7f2a22430cb4b18a8f2ecb02b306bedb1069d6290cbab95f890bb`。
  - 公開文件明列 v0 source Unicode byte-decoding、無 `\uXXXX`、unchecked `i64` overflow、synthetic/partial failure receipt 與 `ExecutionLimits::unlimited()` 非 production default 等限制；不改動既有 v0 語義。
  - RED 證據：runtime/proof tests 在 manifest/export 尚不存在時編譯失敗；proof integration target 漏帶 feature 時原會 0-test 假綠，改以 Cargo `required-features` 後會明確拒絕執行。
  - GREEN／相容性：managed runtime 1+4+25+4 passed；executor workspace 全綠；managed-proof 37+2 passed；proto 3 passed；scheduler 80 passed/1 environment-gated ignored；Worker GNU 83 passed；Worker MSVC check、Docker Compose與Windows package release contracts passed；獨立 review APPROVE。
- `12e4b06 feat(runtime): enforce production sandbox policy`
  - M1 production Linux sandbox launch policy 小單元已提交。
  - RED：public `ReferenceCommandSpec`／`ReferenceProcessSupervisor` 可被 crate 外直接構造任意 host command；compile-fail doctest 先以成功編譯重現 bypass。
  - GREEN：加入具名 sandbox policy enum 與 `ProductionSandboxLauncher`；production policy 要求 rootless OCI、user/pid/mount/network namespaces、cgroup v2、default-deny seccomp profile、no_new_privs、read-only root、network deny 與 explicit safe mounts，違規或未支援平台 fail closed；direct CPython 明確標為 reference-only。
  - API hardening：direct supervisor 收回 `pub(crate)`，lifecycle tests 移入 crate-internal module；compile-fail doctest、sandbox 6、CPython 11、lifecycle 9、executor workspace 98 與 doc tests 皆通過。
  - 跨元件：`cargo check -p hivemind-worker-executor --locked`、Docker Compose release contract、Windows worker package contract 與 runtime check 通過；scoped rustfmt/diff check 通過。strict clippy 仍受 crate 既有 35 個 warning-as-error debt 阻擋，未宣稱全綠。
- `dbf5765 feat(runtime): execute pinned OCI bundles safely`
  - production launcher 新增受驗證 OCI bundle invocation：要求 absolute regular runner、operator SHA-256 pin、合法 container ID 與 bundle root/rootfs；拒絕 symlink、relative path、未知/重複 namespace、未知 nested OCI fields、非 1.0.2、root user、mount/source traversal、未知 annotations 與 identity/policy mismatch。
  - runner 參數以 direct `Command` path 傳遞，重用 process-tree supervisor；timeout、cancel、output cap 與 descendant kill/reap 共享既有 lifecycle contract。
  - RED→GREEN sandbox suite 21 tests passed；`cargo test -p general-compute-runtime --locked`、`cargo test --workspace --locked`、`cargo check -p hivemind-worker-executor --locked`、Docker Compose release contract、Windows worker packaging contract 與 scoped rustfmt checks passed。

- `8b34285 feat(runtime): transport alpha manifests across task dispatch`
  - `general-compute-v1alpha1` manifest 從 HTTP Master 直通 Nodepool、Postgres、scheduler 與 Worker admission；Nodepool 拒絕 legacy `torrent`，Worker 用 operator backend/image allowlist admission，尚未安裝 backend 時 fail-closed `UNIMPLEMENTED`。
  - Compatibility: Master API 29、Nodepool 69、scheduler 81 passed/1 intentional ignored、proto 3、worker admission 7；scheduler/Master API/Nodepool/worker executor/binary offline cargo check passed。
- `feat(runtime): persist trusted worker capability admission`（本輪待提交）
  - 新增 operator-owned `TrustedWorkerCapabilityRegistration` 與 config registry；worker capability JSON 使用 `deny_unknown_fields`，不接受 worker RPC 自報資料。
  - Nodepool registration 以 authenticated owner／admin 驗證 registry entry，將 approved snapshot 寫入 Postgres；untrusted heartbeat 只更新 liveness/resource fields，保留 snapshot；owner-authorized registration 可明確撤銷 snapshot。
  - scheduler 對 `general-compute-v1alpha1` 僅選 persisted snapshot 與 request matching 的 worker；缺 snapshot、錯 image/backend/capability 都 fail closed。
  - 驗證：executor workspace、config 21、node-manager 74、task-scheduler 84 passed／1 intentional ignored，以及 Master API／Worker executor／binary offline `cargo check --locked`。

## Active owners

- Origin：使用者，擁有完整 M0–M5 目標與「每小單元測試、相容性驗證、local commit」驗收規則。
- Coordinator／implementation：Codex。
- Nodepool trust review：M3 接線時需依 `AGENT.md` 的 trusted-authority model 驗收。

## Blockers

- `general-compute-runtime` 的 strict clippy `-D warnings` 目前仍有既有 crate-wide pedantic debt（主要在 reference/lib/tensor 與既有 API must-use/docs）；本 M1 policy scoped tests、format 與跨元件 checks 已通過，但未把無關 lint debt 混入本單元。
- 實際 Linux rootless OCI namespace/cgroup/seccomp/no_new_privs primitives 仍由外部 operator runner 負責；本程式只驗證 bundle envelope 並透過 pinned runner 啟動，尚未宣稱 host platform isolation 或 Worker/Nodepool runtime routing 已完成。
- 真實 OCI fixture 尚缺 operator-provided pinned registry、runner、rootfs、
  canonical seccomp profile 與 case plan；缺少時 harness 必須維持 fail closed。

## Next action

下一個 repository-local 單元以 TDD 隔離 production OCI result 的 trusted input
digest boundary：Nodepool 必須只使用已從 immutable artifact repository 載入且
重新驗證 size/SHA-256 的 source/input bytes 計算 canonical input digest，並在
completion／settlement 前拒絕缺少 bytes、manifest drift 或 Worker digest mismatch。
此 gate 只套用 operator-approved `ProductionSandboxedOci` completed results；
`ReferenceDirect` 的既有相容語義不得被誤改。先從 dirty Scheduler slice 抽出
單一 regression 與最小 production path，完成 focused/full/cross-component gates
後獨立 local commit。若 operator assets 可用，才執行 reviewed multi-process OCI
fixture；缺少 assets 時不啟動容器也不宣稱 E2E。

## Next checkpoint

The remaining release gates are real rootless OCI user-namespace/cgroup v2/seccomp/
no-new-privileges/network isolation, the Postgres-backed multi-process completion
fixture, and Nodepool trusted usage/billing settlement. Overall status remains
`running`; no production-readiness claim is permitted.

### M3 trusted capability registry checkpoint (2026-08-13)

The Nodepool trusted registry gate is now implemented. Operator configuration is the only source for a worker's general-compute capability snapshot; registration binds the configured worker id to the authenticated owner (or admin) and rejects mismatches with `PermissionDenied`. Snapshots are persisted in `worker_nodes.general_compute_capabilities_json`, survive untrusted heartbeat refreshes, and can be explicitly revoked by an owner-authorized registration. Scheduler admission for `general-compute-v1alpha1` parses only the persisted Nodepool snapshot and validates backend, image, capability, thread, network, filesystem, and GPU requirements against the request. Missing or malformed snapshots fail closed. Attempt-bound request/result compatibility is now complete; the next checkpoint is backend execution and CAS materialization.

### M3 attempt-bound request/result compatibility checkpoint (2026-08-13)

The scheduler now forwards the validated alpha manifest identity (`execution_id`, `attempt_id`, `idempotency_key`, and canonical `request_digest`) in the Worker RPC request. Worker alpha responses echo those fields for both success and failure; legacy `managed-function-v0` responses keep the fields empty. Nodepool validates the response identity against the persisted, validated request before any completion or settlement. A mismatch is fail-closed and redispatched without settlement. Retry reset preserves the execution and idempotency identities, rotates the attempt id, and recomputes the canonical request digest. Repository completion additionally compares the exact persisted manifest, preventing a stale manifest from completing a current attempt. Evidence: scheduler lib tests 89 passed, 1 intentional verifier ignore; DB-backed retry, stale-response, and completion-manifest guard tests passed; scheduler, Worker, and proto locked checks passed. The Worker test binary remains blocked by the pre-existing Windows MSVC/MinGW mixed-linker symbol (`__mingw_fprintf_cgo_beginthread`), while the Worker library locked check passes.

### M3 inline artifact materialization checkpoint (2026-08-13)

The general-compute runtime now exposes an operator-rooted `ArtifactMaterializer` for the first execution slice. It validates the existing `ArtifactManifest`, accepts only verified `inline_bytes`, canonicalizes an absolute materialization root, rejects traversal/path-like artifact ids and symlink targets, writes with `create_new` plus `sync_all`, and replays an identical artifact idempotently. A pre-existing file with different bytes fails closed. CAS-only manifests remain unavailable rather than being guessed from an id or path; network fetch, chunk transfer, resume, and CAS persistence are intentionally deferred to the next checkpoint. RED→GREEN evidence: the new artifact integration test initially failed because the materializer module did not exist, then 3 artifact tests passed; the full `general-compute-runtime` locked suite passed (10 unit, 3 artifact, 18 contract, 11 CPython, 3 differential, 4 protocol, 10 reference, 21 sandbox, 6 tensor, and 1 compile-fail doc test).

### M3 reference backend execution checkpoint (2026-08-13)

`ReferenceBackendExecutor` now validates the full alpha request and trusted capability matrix, materializes source/input artifacts through `ArtifactMaterializer`, invokes only a registry-approved `ReferenceDirect` CPython adapter with the fixed `main` entrypoint, and emits a typed `GeneralComputeResult` whose identity, usage, output manifest, image digest, and unverified evidence are revalidated before return. Source exceptions become typed failed results; unsupported entrypoints, invalid UTF-8, multiple inputs, capability mismatches, and backend/image mismatches fail closed. This adapter is reference/test-only and is not Worker routing or production execution; production registrations remain on the OCI bundle path. RED→GREEN evidence: 3 execution tests passed, full `general-compute-runtime` locked suite passed, and `cargo check -p hivemind-worker-executor --locked` passed. A supervisor descendant-fixture test was flaky once in the combined run and passed on the subsequent full suite; no production code was changed for that unrelated existing fixture.

### M3 typed Worker result transport checkpoint (2026-08-13)

The Worker RPC now carries `ExecuteTaskResponse.general_compute_result_json` with a bounded 2 MiB payload contract. Worker `TaskResult` preserves the typed envelope separately from legacy `status_message`; legacy managed-function responses remain unchanged. Alpha execution is no longer an unconditional `UNIMPLEMENTED`: the Worker runs the reference CPython backend only when the operator supplies both trusted capability admission and an explicit `HIVEMIND_GENERAL_COMPUTE_REFERENCE_BACKENDS` registry. Missing or invalid reference configuration emits a typed `backend_unavailable` result and never falls back to an arbitrary host command. Source and input artifacts are materialized through verified inline-only `ArtifactMaterializer`, and cancellation is forwarded to the reference supervisor.

Nodepool scheduler validation now requires a non-empty, bounded, well-formed `GeneralComputeResult`, revalidates the persisted request against the Nodepool-owned worker capability snapshot, checks identity/runtime/backend/image/determinism/artifact/usage/evidence through `validate_against`, and checks the response success bit against typed status. Missing, malformed, oversized, stale, or mismatched results are redispatched without settlement. A successful alpha completion persists the full typed envelope (including usage/evidence claims) in the Nodepool-owned `general_compute_results` table and uses typed stdout for the legacy task output field; it never settles alpha from `status_message`. Failed typed results remain fail-closed and are not persisted as successful completions.

The scheduler boundary suite now explicitly covers malformed JSON, stale attempt identity, persisted capability/image mismatch, a valid typed envelope, and the 2 MiB response limit. The repository manifest-guard test also proves that only the current attempt persists the typed envelope. These are unit/DB decoder gates; production E2E still needs CAS resume, OCI execution, and trusted usage/billing settlement.

Verification: `cargo test -p hivemind-proto --locked` (5 passed), `cargo test -p hivemind-database --locked` (9 passed), `cargo test -p hivemind-task-scheduler --lib --locked` (95 passed, 1 intentional ignored), `cargo check -p hivemind-worker-executor --locked`, `cargo check -p hivemind-task-scheduler --locked`, and `cargo test -p general-compute-runtime --locked` (all runtime suites passed: 10 supervisor unit tests, 3 artifact, 18 contract, 11 CPython, 3 differential, 3 execution, 4 protocol, 10 reference, 21 sandbox, 6 tensor, plus the compile-fail doctest). The runtime descendant fixture can be timing-sensitive under parallel cargo invocations; an isolated rerun and the subsequent full suite both passed. Worker test binary execution remains blocked by the pre-existing Windows MSVC/MinGW `__mingw_fprintf_cgo_beginthread` linker symbol; the Worker library check is green. CAS/chunk transfer/resume, production OCI execution wiring, and Nodepool usage/billing settlement from typed claims remain incomplete; the task stays `running` and is not production-ready.

### M3 local CAS chunk assembly checkpoint (2026-08-13)

The runtime now has an operator-rooted `CasChunkStore` for verified local chunk objects. Each chunk path is derived only from a validated SHA-256 digest; writes use create-new plus `sync_all`, identical replays are idempotent, and both reads and writes rehash bytes before use. `ArtifactMaterializer::materialize_with_cas` assembles only complete manifest chunks, checks per-chunk sizes and the full artifact digest, and then applies the existing safe artifact-id/symlink/idempotency rules. Resume state is represented by `missing_chunks`; absent chunks can be supplied incrementally without accepting an unknown digest. No network fetch, URL interpretation, remote authentication, CAS eviction policy, or Nodepool artifact persistence was added.

RED→GREEN evidence: the new artifact tests first failed because `CasChunkStore`, CAS materialization, and checksum errors did not exist; after the minimal implementation, the artifact suite passed 5 tests and the full `cargo test -p general-compute-runtime --locked` suite passed (10 supervisor, 5 artifact, 18 contract, 11 CPython, 3 differential, 3 execution, 4 protocol, 10 reference, 21 sandbox, 6 tensor, plus the compile-fail doctest). One CPython cancellation fixture was timing-sensitive in an initial combined run, passed in isolation, and passed again in the subsequent full run. The workspace-wide formatter still reports unrelated pre-existing dirty files; only the touched artifact files were rustfmt-checked. Production CAS/chunk transport, Worker/Nodepool wiring, resume across retries, OCI execution, and trusted usage/billing settlement remain incomplete.

### M3 CAS-only reference execution checkpoint (2026-08-13)

`ReferenceBackendExecutor::execute_with_cas` now accepts an operator-supplied local `CasChunkStore` and materializes source/input artifacts through the same verified path used by inline execution. The default `execute` and cancellation methods remain inline-only and fail closed for CAS-only manifests unless the caller explicitly supplies the store. Artifact metadata now has a 1 GiB upper bound before any CAS assembly allocation. This is still a reference/test backend seam: Worker runtime routing does not yet provision or populate a remote CAS store, and no network transfer or retry-resume protocol is implied.

RED→GREEN evidence: the CAS execution test initially failed because the API did not exist, then passed after the minimal materializer selection was added; the oversized artifact metadata test likewise failed before the shared validation bound and passed after it. Full `cargo test -p general-compute-runtime --locked` passed: 10 supervisor, 5 artifact, 19 contract, 11 CPython, 3 differential, 4 execution, 4 protocol, 10 reference, 21 sandbox, 6 tensor, plus the compile-fail doctest. A first combined run hit the existing CPython timeout timing fixture; isolated rerun and subsequent full run passed. Production CAS/chunk transport, Worker/Nodepool population and resume, OCI execution, and trusted usage/billing settlement remain incomplete.

### M3 Worker local CAS routing checkpoint (2026-08-13)

Worker general-compute execution can now receive an explicitly operator-configured local `CasChunkStore` through `HIVEMIND_GENERAL_COMPUTE_CAS_ROOT`. The root must be absolute and non-symlinked; invalid or absent configuration disables CAS materialization rather than inferring a path or using arbitrary host storage. The Worker passes the same supervisor cancellation token through CAS and inline reference execution, so timeout/stop semantics remain identical. Existing callers keep the inline-only wrapper, and the CAS store is shared as a pre-populated local object store under the operator root; this checkpoint does not add a network upload/download RPC, authentication, chunk lease, eviction, or Nodepool-owned persistence.

RED→GREEN evidence: the Worker CAS execution test first failed because no CAS-aware Worker route existed; the route then compiled with `cargo check -p hivemind-worker-executor --locked` and `cargo check -p hivemind-worker-executor --tests --locked`. Runtime cancellation was separately locked by a failing CAS cancellation test before adding `execute_with_cas_with_cancellation`; the test now passes, proving the reference supervisor receives the Worker cancellation. `cargo test -p general-compute-runtime --locked` passed (10 supervisor, 5 artifact, 19 contract, 11 CPython, 3 differential, 5 execution, 4 protocol, 10 reference, 21 sandbox, 6 tensor, plus the compile-fail doctest). Running the Worker test binary remains blocked by the pre-existing Windows MSVC/MinGW `__mingw_fprintf_cgo_beginthread` linker symbol; no production code change was made for that toolchain issue. Remote CAS/chunk transport, retry resume across workers, production OCI routing, and trusted usage/billing settlement remain incomplete.

### M3 typed chunk upload/resume transport contract checkpoint (2026-08-13)

The runtime now defines identity-bound `ChunkUploadEnvelope` and
`ChunkResumeEnvelope` contracts at the local CAS boundary. Every upload carries
the execution id, attempt id, idempotency key, canonical request digest,
artifact id, manifest offset/size/digest, and raw bytes. The ingest path
validates the current request first, requires an exact manifest chunk, applies
a 16 MiB single-upload limit, rehashes the payload, and only then submits it to
the operator-owned `CasChunkStore`. Identical retries remain idempotent;
stale attempts, wrong request digests, unknown/mismatched manifest chunks,
oversized payloads, tampered bytes, and conflicting existing objects fail
closed. Resume selection is likewise identity-bound and delegates completed
digest validation to the artifact manifest contract.

This is deliberately a transport contract, not a network implementation. No
raw unbound byte RPC, authentication scheme, lease, eviction policy, remote CAS
population, retry-across-worker orchestration, OCI routing, or typed usage/
billing settlement was added. The next unit is to carry these envelopes through
an authenticated proto/gRPC boundary and connect remote population to Worker
local CAS ingest without weakening the Nodepool trust model.

RED→GREEN evidence: the transport integration test first failed because the
module and envelopes did not exist; after the minimal implementation,
`cargo test -p general-compute-runtime --test transport --locked` passed 7
tests, and the full locked runtime suite passed. Scoped rustfmt/checks remain
green; crate-wide strict clippy remains blocked by the pre-existing runtime
pedantic debt documented above.

### M3 typed proto chunk envelope checkpoint (2026-08-13)

The Hivemind proto contract now carries `GeneralComputeChunkUpload` and
`GeneralComputeChunkResumeRequest` messages. Both include the Nodepool-issued
execution token, execution/attempt/idempotency identity, canonical request
digest, and artifact id. Uploads additionally bind offset, positive size,
SHA-256, and raw bytes; resume requests carry completed chunk digests. The
proto crate validates required identity fields, digest syntax, and declared
size/bytes equality, rejects negative offsets, zero/mismatched sizes, invalid
digests, and payloads over the 16 MiB per-chunk cap. Runtime CAS ingest remains
the authority that recomputes SHA-256 over the actual bytes.

This checkpoint intentionally adds messages and pure wire validation only. It
does not add a new RPC method or enlarge the existing 4 MiB Worker unary RPC;
the next transport unit must define an authenticated streaming/service boundary
whose configured message limits can carry the chunk cap, then invoke the
runtime's identity-bound local-CAS ingest. Nodepool request/attempt binding and
token authorization remain mandatory at that service boundary.

RED→GREEN evidence: proto tests first failed because the typed messages and
validators were absent; `cargo test -p hivemind-proto --locked` now passes 8
tests and `cargo check -p hivemind-proto --locked` is green.

## Current coordination round (2026-08-14)

- Status: `running`.
- Completed this round: process-tree cancellation/descendant timing fixtures were stabilized in `a0ac3ff`; sparse CSR/CSC/COO metadata validation landed in `b8c264e`; materialized sparse bytes now enforce checksums, CSR/CSC indptr bounds and monotonicity, index bounds, sorted/duplicate policy, COO pair ordering, byte order, and signed-index rules in `85f2bcd`; CPython timeout startup contention is stabilized in `a22c70c`; deterministic f64 broadcast/add/multiply and axis reduction kernels landed in `35d4264`; bounded 2-D f64 matmul with zero-inner-dimension semantics landed in `14c6c4d`; typed dense f32/f64/complex64/complex128 kernels landed in `aceff3a`; bounded typed vector dot landed in `e1daccb`; bounded typed batched matmul landed in `88ae864`; bounded f64 linear solve with partial pivoting landed in `272b6fe`; bounded complex128 FFT reference landed in `819868f`; deterministic splitmix64 RNG with seed/stream/subsequence binding landed in `a788f70`; bounded fixed-step scalar RK4 ODE reference landed in `d032c52`; bounded deterministic unit-circle Monte Carlo reference landed in `ac0580b`; bounded CSR/CSC/COO f64 sparse matvec reference landed in `2235caf`; sparse segment scans were tightened to linear bounded iteration in `048d2aa`; f64 solve residual/error validation landed in `048509f`; FFT round-trip accuracy and golden-vector gates landed in `7c44660`; bounded f64 LU factorization landed in `2123da0`; sparse f64 residual/tolerance validation landed in `75c930c`; bounded deterministic tall/square f64 Householder QR landed in `dc8e66b`; bounded deterministic thin f64 one-sided Jacobi SVD landed in `31c3a21`; bounded full-spectrum real f64 FFT/IFFT with conjugate-symmetry validation landed in `3e5d53e`; bounded deterministic mean, population/sample variance, and linear-interpolated quantile reference functions landed in `e450580`; seeded standard-normal and parameterized normal sampling with pinned Box–Muller output landed in `00c0ad7`; adaptive scalar RK4 step-doubling with minimum-step and attempt/accept accounting landed in `3a78b27`; sparse row/column reductions, deterministic CSR conversion, and bounded partial-pivot solve landed in `032ef80`; optimized backend identity pin contract landed in `c997898`, with operator image-digest binding in `838be9c`; typed GPU vendor/runtime/driver/VRAM/stream/image negotiation with explicit CPU fallback landed in `31f82ea`.
- Verification: numeric integration tests (18), RNG integration tests (4), ODE integration tests (4), Monte Carlo integration tests (3), complex FFT accuracy/golden integration tests (3), real FFT integration tests (3), sparse ABI integration tests (8), sparse numeric integration tests (10), sparse algebra and residual integration tests (8), residual-gate integration tests (2), LU integration tests (3), QR integration tests (4), SVD integration tests (4), statistics integration tests (3), backend pin integration tests (3), the locked `general-compute-runtime` suite (serial and four-thread parallel), runtime check, GNU Worker/Task Scheduler/Bin checks, scoped rustfmt, and scoped `git diff --check` passed. One concurrent serial/parallel invocation exposed an existing production-fixture temp-root race; the affected production test passed standalone and in a later four-thread suite, with no production code change. Strict clippy remains blocked by pre-existing crate-wide debt; no LU, sparse-residual, QR, SVD, real-FFT, statistics, RNG, adaptive-ODE, sparse-solve, or backend-pin-specific warning remains.
- Recovery note: one concurrent full-suite invocation transiently hit the known supervisor output-drain fixture; the focused test and a standalone four-thread runtime suite passed on rerun, with no supervisor production path change. The sparse linear-scan correction was rechecked with the focused sparse suite and runtime check.
- Next action: close multi-process/container OCI E2E and operator runner deployment gates. Trusted usage/billing settlement remains open; the optimized backend registration is a bounded reference-vector/claim-level gate and does not claim native optimized execution.
- Checkpoint: no production-readiness claim; multi-process/container OCI E2E and operator-owned runner deployment validation remain open.
- LU, sparse residual, QR, SVD, real FFT, statistics, seeded normal-distribution, adaptive ODE, sparse solve/reduce, backend identity/image-pin, typed GPU negotiation, trusted registration, scheduler comparison, and Worker selection checkpoints are complete at `2123da0`, `75c930c`, `dc8e66b`, `31c3a21`, `3e5d53e`, `e450580`, `00c0ad7`, `3a78b27`, `032ef80`, `c997898`, `838be9c`, `31f82ea`, `b22aaab`, `5be0e48`, and `0052444`; the next integration slice is the pinned optimized image/backend reference-vector gate. The overall task remains `running`.

## Typed dense numeric checkpoint (2026-08-14)

- Commit `aceff3a` generalizes the bounded dense reference kernels from `f64`
  to `DenseTensor<T>` while preserving the existing shape, broadcast, reduction,
  and 2-D matmul bounds. Public aliases now cover `F32Tensor`, `F64Tensor`,
  `Complex64Tensor`, and `Complex128Tensor`, with deterministic component-wise
  complex arithmetic.
- RED→GREEN evidence: the numeric integration suite passes 8 tests, including
  f32 broadcasting/multiplication, complex64 multiplication, and complex128
  addition. The locked runtime suite passes serially and with four test
  threads; runtime `cargo check` and GNU Worker/Task Scheduler/Bin checks pass.
- This remains a bounded CPU reference slice, not a BLAS/LAPACK, FFT, ODE, RNG,
  Monte Carlo, sparse-algebra, GPU, or production backend. The next S2 unit
  must be selected and tested independently. Multi-process/container OCI E2E,
  operator runner deployment validation, and trusted usage/billing settlement
  remain open; no production-readiness claim is made.

## Bounded typed dot checkpoint (2026-08-14)

- Commit `e1daccb` adds a checked `DenseTensor<T>::dot` kernel for matching
  one-dimensional vectors. It returns the typed sum of products, so the same
  bounded path covers f32, f64, and complex values; rank and length mismatches
  fail closed with explicit numeric errors.
- RED→GREEN evidence: the first focused test failed because `dot` was absent;
  after the minimal implementation, the dot-focused suite passed 3 tests. The
  full numeric suite now passes 11 tests, and the complete runtime plus GNU
  cross-crate checks remain green.
- This is still a CPU reference BLAS-style primitive, not a pinned BLAS/LAPACK
  backend or production readiness claim. Keep LU/solve/QR/SVD and the remaining
  FFT/ODE/RNG/Monte Carlo/sparse units as separate RED→GREEN gates.

## Bounded typed batched matmul checkpoint (2026-08-14)

- Commit `88ae864` adds checked three-dimensional batched matmul for
  `DenseTensor<T>`. Batch dimension `1` broadcasts to the other operand, zero
  inner dimensions produce typed zero outputs, and rank, batch, and inner-shape
  mismatches fail closed with explicit errors.
- RED→GREEN evidence: the initial batch test failed because the method was
  absent; the batched matmul focused suite now passes 3 tests covering batch
  broadcast, zero-inner/complex arithmetic, and mismatch rejection. The full
  numeric suite passes 14 tests and all runtime/cross-crate checks remain green.
- This remains a bounded CPU reference kernel. It is not a pinned BLAS/LAPACK
  implementation and does not close the OCI E2E, deployment, GPU, or trusted
  settlement gates.

## Bounded f64 solve checkpoint (2026-08-14)

- Commit `272b6fe` adds `F64Tensor::solve` for square systems with vector
  right-hand sides. It uses deterministic partial pivoting, rejects singular
  pivots and non-finite inputs/intermediates, and keeps shape mismatch errors
  explicit; complex and multi-RHS solve remain separate units.
- RED→GREEN evidence: the initial pivoted-system test failed because `solve`
  was absent; the solve-focused suite now passes 2 tests for pivoting and
  invalid/singular/non-finite inputs. The full numeric suite passes 16 tests,
  and runtime plus GNU cross-crate checks remain green.
- This is a bounded reference solve, not a validated production LAPACK backend.
  Residual/backward-error gates, LU/QR/SVD expansion, and all deployment and
  settlement blockers remain open.

## Bounded complex FFT reference checkpoint (2026-08-14)

- Commit `819868f` adds a complex128 one-dimensional forward/inverse DFT
  reference exposed as `fft(inverse)`. Forward transforms are unnormalized;
  inverse transforms use fixed `1/n` normalization. A 4096-element cap and
  finite-value validation keep the O(n²) reference bounded and fail closed.
- RED→GREEN evidence: the initial round-trip test failed because `fft` was
  absent; the focused FFT suite now passes 2 tests for round-trip/DC behavior,
  empty input, rank/length limits, and non-finite rejection. Numeric now passes
  18 tests and all runtime/cross-crate checks remain green.
- This is not an optimized FFT backend or production support claim. FFT error
  tolerances, real transforms, backend pinning, and remaining ODE/RNG/Monte
  Carlo/sparse/GPU/release gates remain separate.

## Deterministic RNG checkpoint (2026-08-14)

- Commit `a788f70` adds the public `splitmix64-v1` reference RNG with explicit
  `seed`, `stream`, and `subsequence` inputs. The pinned u64 vector freezes the
  algorithm, `next_f64` maps to `[0, 1)`, and `sample_f64` enforces a one-million
  sample cap.
- RED→GREEN evidence: the initial RNG integration test failed because the
  module was absent; the RNG suite passes 2 tests for the pinned replay vector,
  stream/subsequence separation, unit-interval mapping, and cap rejection.
  Full runtime and GNU cross-crate compatibility checks remain green.
- This is a deterministic reference primitive, not a cryptographic RNG or
  production nondeterministic backend. Parallel splitting policy, statistical
  quality/coverage gates, Monte Carlo confidence fixtures, and Nodepool
  determinism-policy wiring remain open.

## Bounded RK4 ODE reference checkpoint (2026-08-14)

- Commit `d032c52` adds a fixed-step scalar RK4 reference integrator with
  explicit initial/target times, step size, state and step-count caps, and
  direction-aware stepping. Configuration, non-finite derivative/state,
  oversize-step, and step-budget failures are rejected before or during the
  bounded integration loop.
- RED→GREEN evidence: the focused ODE suite passes 2 tests covering an
  exponential known solution plus invalid configuration, target direction,
  step-cap, and non-finite derivative/state cases. The complete locked runtime
  suite passes serially and with four test threads; runtime check and GNU
  Worker/Task Scheduler/Bin checks remain green.
- This is a scalar CPU reference solver only, not a production ODE backend.
  Adaptive methods, vector/tensor state, stiffness handling, error/residual
  gates, GPU execution, cross-worker/container E2E, and trusted settlement
  remain separate open gates.

## Bounded deterministic Monte Carlo checkpoint (2026-08-14)

- Commit `ac0580b` adds a fixed unit-circle π estimator built directly on the
  pinned `splitmix64-v1` RNG identity. Each trial consumes two unit-interval
  samples, reports hits/estimate/variance/standard error, and exposes pinned
  90%, 95%, or 99% normal-approximation confidence intervals. A 500,000-trial
  cap preserves the RNG's one-million-sample budget.
- RED→GREEN evidence: the focused Monte Carlo suite passes 3 tests covering a
  10,000-trial replay fixture (`7,813` hits), empty/over-budget rejection, and
  confidence-level interval widening. The complete locked runtime suite passes
  serially and with four test threads; runtime check and GNU Worker/
  Task Scheduler/Bin checks remain green.
- This is a deterministic statistical reference fixture, not a cryptographic
  RNG, general-purpose sampler, production confidence guarantee, GPU backend,
  cross-worker/container E2E, or trusted settlement implementation.

## Bounded sparse f64 matvec checkpoint (2026-08-14)

- Commit `2235caf` adds a materialized-byte-bound sparse `f64` matrix-vector
  reference kernel over the validated CSR, CSC, and COO ABI. It decodes signed
  or unsigned one-/zero-based indices in either byte order, preserves manifest
  ordering for deterministic duplicate accumulation, and rejects unsupported
  dtypes, non-finite values, vector shape mismatches, oversized dimensions, and
  nonzero counts above the one-million-entry reference cap.
- RED→GREEN evidence: the focused sparse-algebra suite passes 6 tests for CSR
  multiplication, CSC/COO equivalence, allowed duplicate summation, one-based
  big-endian decoding, non-finite/vector failures, and unsupported/capped input.
  The full locked runtime suite passes serially and with four test threads;
  runtime check and GNU Worker/Task Scheduler/Bin checks remain green.
- This is a bounded CPU reference kernel only, not a production sparse backend
  or BLAS library. Sparse residual/error golden vectors, optimized backends,
  GPU execution, cross-worker/container E2E, and trusted settlement remain
  separate open gates.

## Bounded f64 solve residual checkpoint (2026-08-14)

- Commit `048509f` adds a sequential infinity-norm residual evaluator and a
  `solve_with_residual` gate around the existing partial-pivot f64 reference
  solve. Negative or non-finite tolerances fail closed, and a solution whose
  residual exceeds the requested bound is rejected rather than being treated as
  numerically valid.
- RED→GREEN evidence: the focused residual suite passes 2 tests covering a
  pinned 3×3 solve residual below `1e-15`, tolerance acceptance/rejection, and
  invalid tolerance inputs. The complete locked runtime suite passes serially
  and with four test threads; runtime check and GNU cross-crate checks remain
  green.
- This is a reference error gate, not a backward-error proof or production
  LAPACK guarantee. Condition estimates, multi-RHS/complex solves, FFT/sparse
  golden vectors, GPU execution, OCI E2E, and trusted settlement remain open.

## Bounded FFT accuracy checkpoint (2026-08-14)

- Commit `7c44660` adds a component-wise infinity-norm round-trip error
  evaluator and a finite/non-negative tolerance gate around the bounded
  complex128 reference DFT. The gate is paired with an impulse golden vector so
  normalization and phase conventions are checked independently of tolerance.
- RED→GREEN evidence: the focused FFT accuracy suite passes 3 tests for the
  round-trip error bound, invalid/too-tight tolerance rejection, and the pinned
  four-point impulse spectrum. The complete locked runtime suite passes
  serially and with four test threads; runtime check and GNU cross-crate checks
  remain green.
- This remains an O(n²) CPU reference quality gate, not an optimized or real
  transform backend. Backend pinning, broader golden vectors, GPU execution,
  OCI E2E, operator deployment, and trusted settlement remain open.

## Bounded f64 LU factorization checkpoint (2026-08-14)

- Commit `2123da0` adds a bounded deterministic partial-pivot f64 LU
  reference factorization. It returns `P*A = L*U`, exposes the lower/upper
  factors and pivot permutation, solves vector right-hand sides through the
  factors, and reconstructs the permuted product. Non-square, singular,
  non-finite, dimension-over-cap, and RHS-shape inputs fail closed; the
  reference dimension cap is 1024.
- RED→GREEN evidence: the focused LU suite passes 3 tests covering pivoting,
  reconstruction/solve, invalid and singular/non-finite inputs, RHS mismatch,
  and the dimension cap. The locked runtime suite (serial and four threads),
  runtime check, GNU Worker/Task Scheduler/Bin checks, scoped rustfmt, and
  `git diff --check` remain green.
- This is a bounded CPU reference factorization, not a production
  BLAS/LAPACK backend or a backward-error guarantee. QR/SVD, sparse tolerance
  vectors, optimized backend pinning, GPU/OCI E2E, operator deployment, and
  trusted settlement remain open gates.

## Bounded sparse f64 residual checkpoint (2026-08-14)

- Commit `75c930c` adds a sequential infinity-norm residual evaluator and
  tolerance gate to `SparseF64Matrix::matvec`. It validates RHS length and
  finiteness, rejects negative or non-finite tolerances, and returns a typed
  failure when `A*x - rhs` exceeds the requested bound without changing the
  sparse ABI, materialization, transport, or Worker admission boundary.
- RED→GREEN evidence: the sparse numeric suite now passes 8 tests, including
  residual reporting, accepted/rejected tolerance values, invalid tolerance,
  and RHS-shape failure semantics. The locked runtime suite, runtime check,
  GNU Worker/Task Scheduler/Bin checks, scoped rustfmt, and `git diff --check`
  remain green.
- This is a reference residual gate, not a sparse solve, backward-error proof,
  optimized backend, GPU/OCI E2E, operator deployment, or settlement claim.

## Bounded f64 QR factorization checkpoint (2026-08-14)

- Runtime commit `dc8e66b` adds a bounded deterministic thin Householder QR
  reference factorization for tall and square `f64` matrices. It exposes
  orthogonal and upper factors, reconstruction, orthogonality and reconstruction
  infinity norms, and a finite/non-negative tolerance gate. Wide, rank-deficient,
  non-finite, over-cap (1024), shape-mismatched, and invalid-tolerance inputs
  fail closed.
- RED→GREEN evidence: the focused QR suite passes 4 tests covering
  reconstruction/orthogonality, invalid shapes/rank/non-finite values, the
  dimension cap, and reconstruction shape mismatch. The locked runtime suite
  passes serially and with four test threads; runtime check, GNU Worker/
  Task Scheduler/Bin checks, and scoped `git diff --check` remain green.
- This remains a bounded CPU reference implementation, not production
  BLAS/LAPACK, SVD, an optimized backend, GPU execution, OCI/container E2E,
  operator deployment, or trusted usage/billing settlement. SVD or a pinned
  optimized backend is the next numerical gate; no production-readiness claim
  is made.

## Bounded f64 SVD factorization checkpoint (2026-08-14)

- Runtime commit `31c3a21` adds a bounded deterministic thin one-sided Jacobi
  SVD for tall, square, and wide `f64` matrices. It returns `U`, descending
  singular values, and `Vᵀ`, supports reconstruction plus dual-factor
  orthogonality/error norms, and uses deterministic sign normalization. Empty
  and rank-deficient inputs are represented; non-finite, over-cap (1024), shape
  mismatch, invalid-tolerance, and non-convergence cases fail closed.
- RED→GREEN evidence: the focused SVD suite passes 4 tests covering ordered
  singular values/reconstruction, wide rank-deficient matrices, invalid and
  over-cap inputs, tolerance rejection, and reconstruction shape mismatch. The
  locked runtime suite passes serially and with four test threads; runtime
  check, GNU Worker/Task Scheduler/Bin checks, scoped rustfmt, and `git diff
  --check` remain green.
- This is a bounded CPU reference implementation, not production
  BLAS/LAPACK, a pinned optimized backend, GPU execution, OCI/container E2E,
  operator deployment, or trusted usage/billing settlement. Real FFT/broader
  FFT golden vectors are the next numerical gate; no production-readiness claim
  is made.

## Bounded real f64 FFT checkpoint (2026-08-14)

- Runtime commit `3e5d53e` adds a bounded full-spectrum real forward DFT and
  `1/n` inverse transform with `rfft`/`irfft` aliases. The inverse validates
  finite values, DC/Nyquist reality, and conjugate symmetry before producing a
  real signal; round-trip error and finite/non-negative tolerance gates are
  exposed without changing the existing complex FFT ABI.
- RED→GREEN evidence: the focused real FFT suite passes 3 tests covering the
  pinned four-point spectrum, inverse round-trip, invalid shape/non-finite and
  over-cap inputs, non-real spectra, and tolerance failures. The locked runtime
  suite passes serially and with four test threads; runtime check, GNU Worker/
  Task Scheduler/Bin checks, scoped rustfmt, and `git diff --check` remain green.
- This is an O(n²) bounded CPU reference transform, not an optimized FFT
  backend or production scientific image. Broader golden vectors, statistics/
  RNG coverage, backend pinning, GPU/OCI E2E, operator deployment, and trusted
  usage/billing settlement remain open; no production-readiness claim is made.

## Deterministic normal-distribution checkpoint (2026-08-14)

- Runtime commit `00c0ad7` extends the pinned `splitmix64-v1` reference RNG with
  bounded standard-normal and parameterized normal sampling. Box–Muller uses an
  open-interval uniform for the logarithm, emits a pinned vector, and preserves
  replay across identical seed/stream/subsequence coordinates.
- RED→GREEN evidence: the RNG integration suite passes 4 tests covering the
  existing `u64` vector, unit-interval sampling, pinned standard-normal replay,
  finite/non-negative parameter validation, and the sample-count cap. Serial and
  four-thread locked runtime suites plus runtime and GNU Worker/Task
  Scheduler/Bin checks pass.
- Invalid means, standard deviations, output-budget overflow, and arithmetic
  overflow fail closed. This remains a bounded CPU reference distribution, not
  a cryptographic source, parallel stream backend, GPU implementation,
  production OCI E2E, or trusted settlement path.

## Adaptive scalar ODE checkpoint (2026-08-14)

- Runtime commit `3a78b27` adds `AdaptiveRk4Config` and `AdaptiveRk4Result` for
  deterministic scalar RK4 step-doubling. The controller records accepted and
  attempted steps, tolerance, final time/value, and last accepted step size;
  it clamps growth/shrink factors and enforces a positive minimum step and
  global attempt cap.
- RED→GREEN evidence: the ODE integration suite passes 4 tests covering the
  existing fixed-step contract, adaptive exponential integration/metadata,
  invalid limits, step-limit semantics, and an unresolvable stiff step that
  fails with `AdaptiveStepTooSmall`. Locked runtime serial and four-thread
  suites, runtime check, and GNU Worker/Task Scheduler/Bin checks pass.
- This is a bounded scalar CPU reference solver, not a vector/Jacobian backend,
  stiff solver, optimized scientific library, GPU implementation, OCI/container
  E2E, operator deployment, or trusted settlement path.

## Sparse solve/reduce/format checkpoint (2026-08-14)

- Runtime commit `032ef80` extends the validated CSR/CSC/COO f64 reference with
  deterministic row/column reductions, canonical row-major `CsrF64Matrix`
  conversion, and a bounded square solve. The solve accumulates duplicate
  entries, applies deterministic partial pivoting, and caps dense reference
  solve dimensions at `MAX_REFERENCE_SPARSE_SOLVE_DIM = 2048`.
- RED→GREEN evidence: sparse numeric integration now passes 10 tests covering
  existing ABI/matvec/residual behavior plus reductions, CSR conversion,
  square solve, non-square/RHS mismatch, and singular-matrix fail-closed
  behavior. The locked runtime suite (serial and four-thread), runtime check,
  and GNU Worker/Task Scheduler/Bin checks pass.
- This is a bounded CPU reference solve/conversion layer, not an optimized
  sparse backend, distributed solver, GPU implementation, OCI/container E2E,
  operator deployment, or trusted settlement path.

## Optimized backend identity/image-pin checkpoint (2026-08-14)

- Runtime commits `c997898` and `838be9c` add a versioned
  `OptimizedBackendPin` and `BackendRuntimeIdentity` contract. A pin binds
  backend id/version, a strictly sorted unique CPU-feature set, exact thread
  count, a SHA-256 digest of the reference vector suite, and an optional
  operator-approved guest image SHA-256. Verification requires exact identity
  equality and rejects malformed or drifted fields before any optimized result
  is trusted.
- RED→GREEN evidence: backend integration tests 3/3 pass for exact identity
  acceptance, thread/vector-digest drift rejection, canonical feature ordering,
  thread-count validation, image-digest binding, and image-digest drift
  rejection. The locked `general-compute-runtime` suite passes both serially and
  with four test threads; `cargo check -p general-compute-runtime --locked` and
  GNU Worker/Task Scheduler/Bin checks pass. Scoped rustfmt for the backend
  source/tests and `git diff --check` pass. Crate-wide `cargo fmt --all --check`
  and `cargo clippy -D warnings` remain blocked by pre-existing formatting/lint
  debt outside this slice; no such cleanup was made.
- Image binding is complete as an admission/identity contract only. No native
  BLAS/LAPACK library, optimized scientific image execution, or reference-vector
  execution is installed or claimed; actual operator-approved backend/image
  execution remains open.

## Optimized backend reference-vector registration checkpoint (2026-08-14)

- Commit `497f293` adds `OptimizedBackendRegistration`,
  `OptimizedBackendRegistrationError`, and `ReferenceVectorReport`.
- The registration binds the pin to one backend id, one operator-approved guest
  image SHA-256, and a bounded canonical SHA-256 reference-vector suite. Runtime
  identity verification is exact; execution replays only the registered
  reference interpreter; observation verification rejects digest drift, count
  mismatch, and differential observation mismatch.
- RED→GREEN evidence: the new registration test first failed because the API was
  absent, then passed 3/3. The locked runtime suite, production 5/5, sandbox
  21/21, Worker/Task Scheduler/Bin MSVC and GNU checks, scoped rustfmt, and
  scoped diff-check all pass. Crate-wide strict clippy remains blocked by
  pre-existing lint debt outside this slice.
- This remains a reference-vector/claim-level gate only. It does not claim a
  native BLAS/GPU backend, hardware attestation, multi-process/container OCI
  E2E, operator deployment validation, or trusted usage/billing settlement.

## Operator-owned Compose deployment boundary checkpoint (2026-08-14)

- The release Compose contract now gives the Worker a fixed in-container
  `HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS` path at
  `/etc/hivemind/general-compute/backends.json` and a dedicated mutable
  `HIVEMIND_GENERAL_COMPUTE_CAS_ROOT` at
  `/var/lib/hivemind/general-compute/cas`.
- The registry/config and pinned runner are exposed through the named
  `worker-general-compute-config` volume with `read_only: true`; task bundles,
  materialized artifacts, and the durable CAS journal use the separate
  `worker-general-compute-state` volume. No host path is inferred. Backend
  registrations must provide absolute operator-owned bundle/rootfs/artifact
  paths, and a missing registry file still leaves production routing disabled.
- RED→GREEN evidence: the release contract first failed on the absent volume
  and fixed-path assertions, then passed after the Compose/.env wiring. The
  script also parses `docker compose config --format json` and verifies the
  read-only config mount, mutable state mount, fixed paths, existing managed
  proof defaults, required secrets, and the absence of Monty references.
- Verification: `powershell -NoProfile -ExecutionPolicy Bypass -File
  scripts/docker-compose-release.Tests.ps1` passes; `docker compose config`
  passes when the four required release secrets are supplied. This is an
  operator deployment contract, not proof of real rootless OCI isolation or a
  multi-process Postgres completion run; those gates remain open.

## Notes

- 2026-08-13 Monty removal was revalidated after the cleanup commit: the root repository has no tracked Monty paths, `executor-rs/Cargo.toml` exposes only `managed-function-runtime` and `general-compute-runtime`, and the executor workspace plus Docker/Windows release-contract gates pass. The untracked nested `executor-rs/.git` upstream metadata and stale `executor-rs/target` build artifacts were physically removed after explicit user authorization; neither was part of any Hivemind build or runtime path.
- 2026-08-13 M1 leader-exit process-tree hardening is implemented in the reference lifecycle supervisor: Unix process groups remain scoped to the invocation; Windows starts suspended, assigns a Job Object with kill-on-close, resumes the initial thread, and terminates the job before joining inherited output pipes. Spawn setup failures explicitly kill/reap the child. RED→GREEN coverage now includes normal leader exit with an inherited descendant pipe, timeout descendant cleanup, and Windows fixtures that prove descendant launch without relaxing the 600 ms timeout. `cargo test --workspace --locked`, `cargo check -p hivemind-worker-executor --locked`, Docker Compose release contracts, and Windows worker packaging contracts all pass.
- Future-drop cleanup remains covered by the existing Worker managed-prover/execute-future guards; `dbf5765` now wires a validated OCI bundle invocation through the same cleanup boundary, while actual Linux isolation remains an operator-runner responsibility.
- 2026-08-14 production routing hardening: `BackendRegistration` now carries explicit `reference_direct` vs `production_sandboxed_oci` mode. The reference executor rejects production registrations; Worker startup snapshots the operator capability and production registries once, production tasks materialize verified source/input artifacts below task-contained operator roots, reject task-root/rootfs/config symlinks, copy only non-symlinked pinned bundle rootfs templates, and emit a strict `general-compute-result-v1` envelope. The runner's `input_sha256` is checked against the canonical length-framed digest of the materialized source plus inputs. Raw runner stdout, unbound mounts, missing input mounts, relative/task-traversal paths, invalid output roots, and usage overclaims fail closed. Scoped runtime tests (21 contract, 5 production, 21 sandbox, 10 supervisor plus all existing suites) and GNU Worker/Nodepool/Scheduler/Bin checks pass. Multi-process/container E2E and trusted usage/billing settlement remain open; Monty remains removed.
- 此檔先前的 `complete` 只代表「舊 Monty 清理與計畫文件」完成，並不代表使用者要求的完整演進計畫完成；2026-08-12 已依實際 scope 修正為 `running`。
- 不要對工作樹中的其他 dirty frontend/API 變更使用 `reset`、`checkout` 或整批刪除；它們不屬於目前小單元。
- `managed-function-v0` 的有限配額與 proof settlement 是 load-bearing 契約，不得為了 v1 任意運算而放寬。
- `general-compute-v1` 必須使用獨立 runtime/version/cost/verifiability contract，不能冒充現有 RISC Zero proof path。
- 2026-08-14 S1 tensor binary boundary: `TensorManifest::validate_bytes` now revalidates materialized size, SHA-256, and inline-byte identity before decode; `canonical_little_endian_bytes` performs lossless endian conversion, reverses complex components independently to preserve IEEE-754 bit patterns, and validates canonical signed-magnitude BigInt scalars. RED→GREEN evidence: tensor integration tests (9) pass, the locked `general-compute-runtime` suite passes serially, and cross-crate `cargo check` for Worker/Task Scheduler/Bin passes. Parallel runtime execution still has pre-existing timing-sensitive cancellation/descendant fixtures; that is tracked separately and is not included in this tensor commit (`012c935`).

## GPU request-level binding checkpoint (2026-08-14)

- The current RED→GREEN slice binds the typed `GpuRequirement` to
  `ExecutionPolicy`. A request with `gpu_required=true` must carry a valid
  typed requirement; a typed requirement without the flag is rejected. The
  optional field is omitted from CPU-policy JSON so existing CPU request
  serialization remains canonical and compatible.
- Focused coverage is staged in `tests/gpu_request.rs`; the existing capability
  matrix regression now supplies a typed requirement before request validation.
  The staged diff has been checked with `git diff --cached --check` and keeps
  unrelated production-routing changes out of this slice.
- Compatibility gates are green: focused request 2/2, contracts 21/21, locked
  runtime serial/four-thread suites, runtime check, and GNU Worker/Task
  Scheduler/Bin checks. The local commit is `6400099 feat(runtime): bind GPU
  requirements to execution policy`.
- Status: `running` overall. This checkpoint is complete at request validation
  only; it is not trusted Worker/Nodepool registration, scheduler admission,
  selected-device/result identity, or actual CUDA/ROCm execution.
- Next action: wire the typed requirement/capability and selected device identity
  into trusted Nodepool registration, scheduler/Worker admission, and result
  binding; keep OCI E2E, operator deployment, and trusted settlement open.

## Trusted GPU registration slice (2026-08-14)

- Next bounded unit: extend the operator-owned
  `TrustedWorkerCapabilityRegistration` with a typed GPU capability list and a
  deterministic request-selection helper. Legacy snapshots without the new
  field must continue to deserialize as an empty list.
- RED→GREEN: focused registration tests initially failed because the typed list
  and helper were absent; they now pass 3/3 for round-trip/legacy JSON default,
  deterministic device selection, and malformed capability rejection. The
  locked runtime serial/four-thread suites and GNU Worker/Task Scheduler/Bin
  checks also pass.
- Commit: `b22aaab feat(runtime): persist trusted GPU capability identities`.
  The operator-owned registration now carries a typed GPU list and selects a
  stable device identity; legacy snapshots without the field decode as empty.
- Status: `running`; this is trusted registration data plus runtime selection
  only. Scheduler/Worker admission and result identity binding, actual
  CUDA/ROCm execution, OCI E2E, operator deployment, and trusted settlement
  remain open.
- Next action: have scheduler/Worker admission consume the typed registration
  and persist the selected GPU identity into the attempt/result envelope.

## GPU result identity slice (2026-08-14, GREEN)

- Next bounded unit: add an optional typed GPU selection to
  `GeneralComputeResult`, bind it to the request's GPU requirement/fallback
  policy, and preserve omission for CPU results. The trusted registration
  selector remains the source of the selected device; this slice only defines
  result identity validation.
- RED→GREEN: `GeneralComputeResult` now carries an optional typed
  `GpuSelection`. CPU results omit the field for JSON compatibility; GPU
  results must carry a matching capability identity or an explicitly allowed
  CPU fallback. `validate_against` invokes the fail-closed GPU identity gate,
  while existing Worker/Scheduler constructors remain CPU-compatible.
- Evidence: focused `gpu_result` 3/3, locked runtime serial and four-thread
  suites, runtime `cargo check`, and GNU Worker/Task Scheduler/Bin checks all
  pass. The result identity slice is committed as `67bc1c9`; scheduler/Worker
  transport and Nodepool settlement remain out of scope.
- Status: `running`; the next bounded unit is to have scheduler/Worker
  admission consume trusted registration and persist the selected GPU identity
  into the result, with Nodepool comparison against the operator-owned
  registration.

## Scheduler trusted GPU result identity slice (2026-08-14, GREEN)

- RED→GREEN: the scheduler now parses the operator-owned
  `TrustedWorkerCapabilityRegistration`, deterministically selects the trusted
  GPU for the persisted request, and compares that exact `GpuSelection` with
  the Worker result before accepting it. A forged device identity is rejected
  even when all vendor/runtime/VRAM/image fields otherwise match.
- Evidence: forged-identity regression test passes; the full GNU scheduler
  library gate passes (118 passed, 1 ignored); `git diff --cached --check`
  passes. Commit `5be0e48 feat(scheduler): verify trusted GPU result identity`
  is local and not pushed.
- Status: `running`; this closes the claim-to-registration comparison only.
  Worker-side device discovery/selection, CUDA/ROCm execution, OCI/container
  E2E, operator deployment, and trusted usage/billing settlement remain open.

## Worker trusted GPU selection slice (2026-08-14, GREEN)

- RED→GREEN：Worker admission now consumes the operator-owned
  `TrustedWorkerCapabilityRegistration` and deterministically selects a typed
  GPU identity. A typed GPU request without a compatible approved identity is
  rejected fail closed; the worker's boolean `gpu_available` claim is never
  upgraded into a typed capability.
- The Worker reference executor receives the same trusted snapshot and the
  execution wrapper binds the selected `GpuSelection` into both successful and
  failure `GeneralComputeResult` envelopes. Existing CPU result JSON remains
  unchanged.
- Evidence：runtime GPU execution 1/1、Worker admission 2/2、GNU Worker
  `cargo check --tests`、and `git diff --cached --check` all pass. Local commit:
  `0052444 feat(worker): bind trusted GPU selection to results`.
- Boundary：this is trusted claim plumbing only. CUDA/ROCm execution,
  hardware attestation, real OCI/container E2E, operator deployment, and
  trusted usage/billing settlement remain open; overall status is `running`.

## Rootless OCI runner image packaging checkpoint (2026-08-14)

- RED→GREEN：release contract 先因 runtime image 缺少 `runc`、`uidmap`、
  `general-compute-runtime` staging 與 subordinate UID/GID 設定而失敗；
  最小修正已由 `48069ea feat(deploy): package rootless OCI runner` 提交。
- Builder image 只明確複製 `managed-function-runtime` 與
  `general-compute-runtime`，不把整個 `executor-rs` workspace 帶進 image。
  Runtime image 安裝 `runc`/`uidmap`，建立 `/app/general-compute`，並為
  非 root `hivemind` user 寫入 `hivemind:100000:65536` 的 `/etc/subuid` 與
  `/etc/subgid`。
- 驗證：`scripts/docker-compose-release.Tests.ps1` 通過；
  `docker build -f hivemind-rs/Dockerfile -t hivemind-worker:runc-check .`
  成功；image probe 確認 UID/GID `10001`、`runc 1.1.15`、subuid/subgid
  range 與 `/app/general-compute` 均存在；staged `git diff --check` 通過。
- Boundary：這只證明 image packaging／operator prerequisites，不證明
  rootless user namespace、cgroup、seccomp、network deny 的真實 host
  isolation，也不證明 Worker→Nodepool→Postgres 的 multi-process completion。
  Overall status remains `running`。
- Next action：新增下一個 RED contract，要求隔離 OCI E2E harness 以
  operator-provisioned bundle/rootfs 啟動 Worker backend，並驗證成功完成、
  timeout/cancel kill-reap、network/filesystem deny 與 Postgres-backed
  multi-process completion；若環境 primitive 不足，保留明確 fail-closed
  evidence，不把 fake runner fixture 當 production E2E。

## Operator OCI E2E preflight harness checkpoint (2026-08-14)

- RED→GREEN：新增 `scripts/general-compute-oci-e2e.Tests.ps1`；RED 階段在
  harness 尚不存在時失敗，GREEN 階段鎖定 operator registry、absolute
  bundle/rootfs/artifact/runner paths、runner SHA-256、rootless namespaces、
  cgroup v2、no-new-privileges、read-only root、deny-all network、default
  deny `SCMP_ACT_ERRNO` seccomp digest、隔離 Compose project 與 cleanup。
- `scripts/general-compute-oci-e2e.ps1` 的 `-CheckOnly` 只做 fail-closed
  preflight；缺少 registry、rootfs、pinned runner、digest 或 required
  release secrets 都會拒絕。`-Run` 需要明確
  `HIVEMIND_ENABLE_REAL_OCI_E2E=1` 與 Postgres-backed task fixture，尚未
  啟動容器或把 fake runner 當成 E2E。
- 驗證：`powershell -NoProfile -ExecutionPolicy Bypass -File
  scripts/general-compute-oci-e2e.Tests.ps1` 通過；直接 `-CheckOnly` 在
  未提供 operator registry 時按預期 fail closed；staged diff-check 通過。
  本地 commit：`1e6d513 test(deploy): add OCI E2E preflight harness`。
- Boundary：這是部署前置條件與恢復安全的 harness，不是 rootless OCI
  namespace/cgroup/seccomp isolation 或 Worker→Nodepool→Postgres completion
  的證據；overall status remains `running`。

## OCI runner state-root binding checkpoint (2026-08-14)

- RED→GREEN：registry 先缺少 dedicated runner state root，materialized
  production launch 也會在 bundle validation 前缺少 state binding；新增
  `ProductionBackendConfig.runner_state_root`、sandbox launcher 的
  `with_runner_state_root` 與 materialized-launch requirement，並將 state
  root 以 `--root <operator path>` 傳給 pinned OCI runner。
- Relative、parent-traversal、missing、non-directory 或 symlink state roots
  一律回 `RunnerStateRootUnavailable`；Worker production dispatch 會把
  registry snapshot 的 root 傳入 launcher。Legacy process-level `run_bundle`
  fixtures 可繼續驗證 command boundary，但 production materialized path
  必須有 state root。
- RED→GREEN evidence：production registry 6/6、sandbox 22/22、locked
  general-compute-runtime suite、Worker GNU test check、Task Scheduler/Bin
  checks 與 staged diff-check 通過。Local commit：
  `4dfe4b0 feat(runtime): bind OCI runner state root`。
- Boundary：這只修正 runc state isolation 的 operator binding；實際
  rootless user namespace/cgroup/seccomp/network primitives、Postgres-backed
  multi-process completion 與 variable trusted settlement 仍未驗證，overall
  status remains `running`。

## OCI seccomp profile binding checkpoint (2026-08-14)

- RED→GREEN：production materializer test 先因缺少 operator seccomp profile
  path/error contract 而編譯失敗；新增 `ProductionBackendConfig.seccomp_profile_path`
  與 `SeccompProfileUnavailable`，並將 profile bytes 的 SHA-256 綁定到
  `policy.seccomp.profile_sha256`。
- Profile 必須是 absolute regular non-symlink file、canonical JSON，default
  action 必須是 `SCMP_ACT_ERRNO`，syscall allowlist 不得為空；未知欄位、空或
  重複 syscall 名稱、非 `SCMP_ACT_ALLOW` group、disabled policy、digest drift
  一律 fail closed。materialized bundle 將完整 profile 寫入 `linux.seccomp`，
  sandbox validator 在 runner 啟動前重新檢查 allowlist shape。
- OCI preflight 同步檢查 `seccomp_profile_path`、regular/non-symlink、profile
  SHA-256、default action 與 syscall allowlist；contract test 完成 RED→GREEN。
- Evidence：production 7/7、sandbox 22/22、locked `general-compute-runtime`
  全 suite、Worker GNU test check、Task Scheduler/Bin checks、harness contract
  與 scoped `git diff --check` 通過。
- Local commit：`43dd537 feat(runtime): bind operator seccomp profiles`。
- Boundary：這只證明 operator profile binding 與 fail-closed bundle contract；
  真實 rootless user namespace/cgroup/seccomp/network primitives、
  Worker→Nodepool→Postgres multi-process completion 與 trusted usage/billing
  settlement 仍未完成，overall status remains `running`。

## Isolated OCI Compose project checkpoint (2026-08-14)

- RED→GREEN：preflight 原先雖產生隨機 project name，release Compose 卻固定了
  `container_name`、network name、nodepool IPv4 與 subnet；contract test 先失敗，
  再移除這些固定 binding，讓 Compose project 管理 container/network/IPAM。
- Internal torrent advertisement 改用 `nodepool:6881` service DNS。Preflight 會
  在檢查與後續 run 暫時設定全部 named volume 為 project-prefixed names，驗證
  resolved Compose JSON，並在任何成功、fail-closed 或 exception path 還原原有
  process environment。
- Evidence：OCI harness contract、Compose release contract、resolved Compose
  config 與 scoped diff-check 通過。Local commit：`c24d036 fix(deploy): isolate
  OCI compose projects`。
- Boundary：這是 Compose resource isolation，不是 rootless OCI user namespace/
  cgroup/seccomp/network enforcement 或 Postgres-backed multi-process task
  completion；overall status remains `running`。

## Reviewed multi-process OCI fixture protocol checkpoint (2026-08-14)

- `scripts/general-compute-oci-e2e.ps1 -Run` now has an explicit reviewed
  fixture protocol. It invokes the operator fixture twice (`provision`, then
  `execute`), starts the isolated Compose project only after provisioning, and
  cleans every service/volume in `finally`.
- The harness validates `general-compute-oci-e2e-v1` evidence bound to the
  generated project/task. Required checks are Worker registration, successful
  task completion, Nodepool/Postgres settlement, timeout/cancel kill-reap,
  network deny, and filesystem deny; the result must identify a validated
  `general-compute-result-v1` `ProductionResultEnvelope`.
- Compose carries optional worker username/password credentials and an explicit
  opt-in default test account for this isolated run. Host ports are randomized
  and restored after cleanup; evidence is retained in `test_logs/` or a caller
  supplied absolute path.
- The repository now ships the reviewed fixture implementation at
  `scripts/general-compute-oci-task-fixture.ps1`. It provisions operator
  registry/rootfs/runner/profile material into the named volumes, performs real
  Master authentication and Worker-registration waits, submits/polls tasks,
  queries trusted result/settlement rows, and runs the three hostile cases.
  Operators still supply those deployment assets and the explicit case plan
  containing canonical request digests. Missing plans or unsupported host
  primitives remain fail-closed; this is not yet a production-readiness claim.

## Current coordination round (2026-08-14, continued)

- `6166bff fix(deploy): retry OCI fixture login during startup` adds a bounded
  Master-login retry so service-listening does not race Nodepool gRPC readiness.
- `34a91a5 fix(deploy): validate OCI case plan before startup` adds a RED→GREEN
  harness contract requiring an absolute regular
  `HIVEMIND_GENERAL_COMPUTE_OCI_E2E_CASES` file before Compose `up`.
- Verification in this round: OCI harness contract, Compose release contract,
  `cargo test -p general-compute-runtime` (including the compile-fail boundary
  doctest), and `git diff --check` all pass. The M1 direct-process supervisor
  remains crate-internal; no Monty path or production direct-spawn fallback was
  reintroduced.
- Status: `running`. The remaining release gate is operator-dependent: provide
  `HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS` with real rootfs/runner/state/
  seccomp material, a canonical `HIVEMIND_GENERAL_COMPUTE_OCI_E2E_CASES` plan,
  and `HIVEMIND_ENABLE_REAL_OCI_E2E=1`; then run the multi-process fixture and
  retain evidence for OCI isolation, typed result, timeout/cancel, hostile
  workloads, and Nodepool settlement.

## Typed general-compute cancellation persistence checkpoint (2026-08-14)

- Commit `f7495a3 fix(scheduler): persist typed cancellation results` makes
  `TaskRepository::cancel` transactional: an accepted general-compute cancel
  now updates the task and persists one Nodepool-generated
  `general-compute-result-v1` envelope atomically. The result is
  `cancelled`/`task_cancelled`, preserves execution, attempt, idempotency,
  request, runtime, backend, image, and determinism identity, and never creates
  a settlement.
- Inline requests bind `input_sha256` to the canonical materialized input
  digest. A cancellation that occurs before CAS materialization instead binds
  the envelope to a domain-separated digest of the immutable manifest
  coordinates; it does not claim that unobserved bytes were executed.
- DB-backed evidence on the current locked compatibility baseline: scheduler
  cancel/terminal-state tests 4/4 pass, including the new typed-result
  regression; Nodepool `test_stop_task_reports_cancellation_recorded` passes.
  A detached-HEAD probe remains blocked by pre-existing baseline drift in
  dispatcher fixtures (`execution_mode` and typed GPU fields) plus an
  out-of-sync lockfile; those failures predate this cancellation slice.
- Status remains `running`. This closes typed cancellation persistence needed
  by the reviewed OCI fixture, but does not prove real rootless OCI execution,
  hostile-workload isolation, operator deployment, or trusted settlement.

## Typed general-compute stale-timeout persistence checkpoint (2026-08-14)

- Commit `0ec476c fix(scheduler): persist typed timeout results` closes the
  remaining Nodepool terminal-result gap for stale `RUNNING` tasks. The stale
  sweep now updates matching rows with `UPDATE ... RETURNING`, writes a
  Nodepool-generated `general-compute-result-v1` envelope for each
  general-compute task, and commits the status and result atomically.
- The envelope is `timed_out`/`worker_heartbeat_lost`, preserves immutable
  request, execution, attempt, runtime, backend, image, and determinism
  identity, and never creates a settlement. Inline inputs retain their
  canonical digest; a timeout before CAS materialization uses a timeout-specific
  domain-separated digest of immutable manifest coordinates. Legacy runtimes
  still receive the same `TIMED_OUT` transition and result count without a
  general-compute result row.
- TDD evidence: the DB regression first failed with `RowNotFound`, then passed
  after the transactional persistence change (1/1). Scheduler cancellation and
  terminal-state regressions remain green (4/4), locked Scheduler/Nodepool
  checks pass, and Nodepool stop-task compatibility remains green (1/1).
- Status remains `running`. Typed scheduler timeout persistence does not prove
  real OCI kill/reap, rootless namespace/cgroup/seccomp/network enforcement,
  operator-provided deployment assets, hostile-workload isolation, or trusted
  settlement evidence.
- Recovery audit also confirmed that Nodepool preparation failures lack a typed
  `backend_unavailable` row, but the relevant method and dispatcher callers are
  part of a separate pre-existing uncommitted CAS/preparation slice and are not
  present in HEAD. The exploratory test/change was removed and the index stayed
  empty rather than mixing that parent feature into an unrelated commit.

## Typed Nodepool failure persistence checkpoint (2026-08-14)

- Commit `f186b4b fix(scheduler): persist typed nodepool failures` closes the
  HEAD-present `fail_for_worker` gap used when an assigned task exhausts its
  redispatch budget. For general-compute, the task transition and a
  Nodepool-generated `failed`/`nodepool_task_failed` result now commit
  atomically; the typed envelope preserves immutable request/backend/image
  identity and creates no settlement.
- Existing failure attribution remains compatible: the Worker still receives
  one failed-task reputation update and one rejected attestation after the
  task/result transaction. Legacy managed-proof failure behavior and stale
  Worker assignment guards remain unchanged.
- TDD evidence: the new DB regression first failed with `RowNotFound`, then
  passed (1/1). The `fail_for_worker` focused gate passes 2/2, the managed-proof
  rejection compatibility test passes 1/1, scoped rustfmt/diff checks pass,
  and locked Scheduler/Nodepool/Master checks pass.
- Status remains `running`. This is terminal-result completeness at the trusted
  scheduler boundary, not real OCI execution, isolation, operator deployment,
  hostile-workload evidence, or trusted variable settlement.

## Guarded generic failure persistence checkpoint (2026-08-14)

- Commit `c10b803 fix(scheduler): persist guarded typed failures` closes the
  public `TaskScheduler::fail_task`/`TaskRepository::fail` gap. Generic failure
  now accepts only `PENDING`, `QUEUED`, `ASSIGNED`, or `RUNNING`; it cannot
  rewrite a completed/cancelled/timed-out/failed task. For general-compute, the
  active-state transition and Nodepool `failed`/`nodepool_task_failed` envelope
  commit atomically and create no settlement.
- TDD evidence: typed persistence first failed with `RowNotFound`; the terminal
  guard separately failed because a completed task was overwritten. Both tests
  are now green (1/1 each), scoped rustfmt/diff checks pass, and locked
  Scheduler/Nodepool/Master checks pass.
- Status remains `running`. All HEAD-present scheduler terminal methods now
  have typed general-compute persistence where applicable, but the deferred
  dirty preparation slice, four existing CAS/immutable-artifact scheduler test
  failures, real OCI isolation/E2E, and operator settlement evidence remain.

## Durable CAS transfer-state checkpoint (2026-08-14)

- Commit `ec44b65 feat(runtime): persist resumable CAS transfer state` makes a
  Worker CAS transfer durable across store recreation and attempt rotation.
  The persisted identity is keyed by stable execution/artifact identity rather
  than `attempt_id`; a later attempt may resume identical coordinates but
  cannot redefine the manifest.
- Transfer manifests and completion markers use create-new writes below the
  operator CAS root. Verified CAS objects remain the source of truth after a
  crash, missing markers are reconciled from rehashed objects, and malformed
  manifests or markers fail closed.
- TDD evidence: the test-only clean-HEAD probe failed because
  `prepare_transfer`, `put_transfer_chunk`, and `missing_transfer_chunks` did
  not exist. Focused transfer tests pass 4/4; the exact staged artifact suite
  passes 9/9; the current integrated runtime suite passes. The exact staged
  snapshot also passes offline Worker/Scheduler/Bin checks, and the current
  integrated workspace passes the same three checks with `--locked`.
- Baseline caveat: a clean HEAD full-runtime run still encounters the existing
  `BackendRegistration.execution_mode` fixture drift, and clean Hivemind
  `--locked` checks encounter the pre-existing managed-runtime lockfile drift;
  this commit changes no dependency manifests. Neither baseline issue was
  bundled into the CAS commit.
- The four scheduler CAS/immutable-artifact regressions are now GREEN in the
  current dirty parent slice, and the full scheduler gate is 124 passed,
  1 ignored, 0 failed. Those fixes are not yet independently committable
  because their DB/repository/dispatcher APIs are absent from HEAD. Next action
  is to isolate and commit the Nodepool artifact identity/source repository
  layer before the dispatcher preparation layer.
- Status remains `running`; real rootless OCI isolation/E2E, operator assets,
  hostile-workload evidence, and trusted settlement remain open.

## General-compute settlement schema recovery checkpoint (2026-08-14)

- Commit `9f1d332 fix(database): restore general compute settlement schema`
  restores the `general_compute_settlements` migration required by the already
  committed cancellation, timeout, and failure-result paths. The missing table
  was reproduced as four DB-backed `relation does not exist` failures before
  the migration was added.
- The table binds task/worker/execution/attempt/idempotency/request identity,
  billing and cost-model versions, usage JSON, evidence level, settlement
  basis, non-negative amount, and creation time. The focused migration test and
  all four affected terminal-result regressions pass.
- This is schema recovery for existing fixed-reservation settlement behavior;
  it is not evidence for variable usage billing or production OCI execution.

## Nodepool immutable artifact repository checkpoint (2026-08-14)

- Commit `a13804b feat(scheduler): persist immutable artifact sources` adds
  Nodepool-owned artifact identity, manifest-chunk, verified source, and chunk
  tables. General-compute task creation now validates the request and persists
  the task plus every immutable artifact coordinate in one transaction, so a
  missing or invalid manifest leaves no partial task row.
- Inline bytes are rehashed before use; an existing source row with drifted
  content, hash, size, or expiry fails closed and is never mistaken for a
  missing row. A genuinely missing inline source may be reconstructed only
  from the Nodepool-persisted, revalidated task manifest. CAS-only uploads must
  match the immutable chunk manifest, enforce the upload cap and checksum, and
  accept only byte-identical retries.
- Chunk insertion and the `complete`/`available` transition now share one
  row-locked transaction. A constraint-injection regression first left one
  orphan chunk under the old autocommit flow, then passed after the atomic
  change. Expired identities remain unavailable and reject later uploads.
- Evidence: focused repository tests 22/22, Database 11/11, full Scheduler
  107/107 with 1 intentional ignore, and Scheduler/Worker/Bin `--tests` checks
  pass in the isolated validation worktree. The exact commit, without fixture
  shims, passes production Scheduler/Worker/Bin checks. Clean test compilation
  still needs validation-only fixes for pre-existing `execution_mode`/typed GPU
  fixture drift; none of those unrelated files entered the commit.
- Status remains `running`. This closes the repository identity/source layer,
  not transfer leases, dispatcher chunk preparation/RPC, real rootless OCI
  evidence, hostile-workload isolation, or trusted variable settlement.

## Nodepool transfer-lease lifecycle checkpoint (2026-08-14)

- Commit `5b22af8 feat(scheduler): persist transfer lease lifecycle` adds the
  Nodepool-owned `active`/`revoked`/`expired` lease table, a single-active-task
  index, immutable identity index, and a database constraint on lease state.
  Assignment and claim create a generation in the same transaction as task
  ownership; legacy tasks receive no general-compute lease.
- Each generation binds task, execution, attempt, Worker, and task deadline.
  Redispatch revokes the old generation before rotating the attempt; completion,
  typed Worker failure, Nodepool failure, cancellation, and stale timeout revoke
  authority in their existing transactions. Lookup materializes expiry and also
  revokes a lease whose task is no longer active or whose assigned Worker drifted.
- TDD evidence: clean-HEAD probes first failed on the absent table/API. In the
  isolated validation worktree, focused lease tests pass 5/5, typed-failure
  compatibility tests pass 1/1 each, Scheduler passes 113 with 1 intentional
  ignore, and Database passes 12/12. Scheduler, Worker, Node Manager, Master API,
  and Bin production plus test-target checks pass with fixture-only overlays
  excluded from the commit.
- The exact feature commit passes the same five production source checks after
  Cargo regenerates only the pre-existing stale lockfile. That unrelated drift
  was not bundled into the feature; it is repaired separately by `b3abec8`, so
  the same five consumers now pass clean `--locked --offline` checks.
- After forward-merging the checkpoint into the broader dirty transport slice,
  the extra operator-admission no-penalty failure path also revokes its lease;
  integrated lease tests pass 6/6, full Scheduler passes 130 with 1 intentional
  ignore, and the five production consumers pass locked checks.
- Status remains `running`. The persistence lifecycle does not yet by itself
  authenticate a remote Worker or authorize a chunk: token/proto/gRPC authority,
  Worker-side validation, dispatcher transfer, real rootless OCI evidence, and
  trusted variable settlement remain open.

## Managed-runtime lockfile compatibility checkpoint (2026-08-14)

- Commit `b3abec8 fix(build): refresh managed runtime lockfile` updates the
  committed path-package version from `managed-function-runtime` 0.0.7 to the
  already committed workspace manifest version 0.1.0. The remaining four
  touched lines are Cargo's deterministic whitespace/dependency ordering.
- RED evidence: a clean `cargo check -p hivemind-task-scheduler --locked
  --offline` stopped before compilation because Cargo needed to update the
  lockfile. After offline regeneration, Scheduler, Worker, Node Manager, Master
  API, and Bin all pass `cargo check --locked --offline`; diff check passes.
- The broader dirty worktree's additional dependency cleanup remains preserved
  as an unstaged lockfile/Cargo manifest superset and was not folded into this
  commit. Status remains `running`; next action is still authenticated
  token/protobuf/Nodepool gRPC transfer-lease validation.

## Worker execution-token transfer identity checkpoint (2026-08-14)

- Commit `b22fed5 feat(auth): bind transfer identity to worker tokens` adds a
  typed identity containing execution id, attempt id, idempotency key, canonical
  request digest, and Nodepool transfer generation to Ed25519 Worker execution
  claims. The existing base `encode_claims`/`decode` API remains compatible and
  can read the base task/Worker claims from an extended token.
- The signer rejects blank identity fields and zero/negative generations before
  signing. Extended decode keeps fields optional only so legacy tokens remain
  parseable; general-compute consumers are required to compare every field and
  must not treat omission as authority.
- TDD evidence: the first focused test failed to compile because the typed
  identity and extended sign/decode API were absent. The second failed because
  the signer accepted a whitespace execution id. Auth now passes 7/7, scoped
  rustfmt and strict auth clippy pass, and Scheduler, Worker, Node Manager,
  Master API, and Bin pass locked offline production checks. The integrated
  dirty slice repeats auth 7/7 and Worker/Node Manager/Bin locked checks.
- Status remains `running`. A signed identity is necessary but not sufficient:
  the protobuf validation surface, Nodepool active-lease lookup, Worker
  fail-closed enforcement, dispatcher transfer, real OCI evidence, and trusted
  variable settlement remain open.

## Bounded transfer-lease authority envelope checkpoint (2026-08-14)

- Commit `bae0207 feat(proto): add bounded transfer lease authority envelope`
  freezes a Worker-to-Nodepool request carrying the Nodepool-issued execution
  token plus task, Worker, execution, attempt, idempotency, request-digest, and
  positive transfer-generation identity. The response explicitly carries an
  active/rejected boolean and bounded status text.
- The shared wire validator rejects blank identity, malformed SHA-256 digests,
  nonpositive generations, tokens over 8 KiB, identifiers over 255 bytes, and
  encoded requests over 16 KiB before any authority lookup.
- TDD evidence: the roundtrip test first failed to compile because the messages
  and validator did not exist; the boundary test then failed because a stub
  accepted whitespace token. The isolated Proto suite passes 12/12, scoped
  rustfmt and strict clippy pass with only the pre-existing constant-assertion
  lint allowed, and Scheduler, Worker, Node Manager, Master API, and Bin all
  pass locked offline production checks. The broader dirty transport slice
  retains its source-upload/RPC additions and passes Proto 13/13 plus the same
  five integrated consumer checks.
- Status remains `running`. This commit defines and bounds the envelope only;
  Nodepool RPC registration, token/lease authority, Worker fail-closed calls,
  dispatcher transfer, real OCI isolation/E2E, and trusted settlement remain
  open.

## Nodepool transfer-lease authority checkpoint (2026-08-15)

- Fixture compatibility was kept separate from production behavior:
  `f017606`, `b7d8e34`, and `a9d2e35` refresh Node Manager, Worker, and
  Scheduler general-compute fixtures; `acc173a test(scheduler): refresh
  admission fixtures` adds the required `reference_direct` mode to the three
  remaining hand-written capability snapshots. The two stale admission tests
  were observed RED individually, then passed; the isolated full Scheduler
  gate is 113 passed with 1 intentional environment-gated ignore.
- Commit `ecbbee4 feat(nodepool): validate transfer lease authority` registers
  the bounded RPC and verifies the Nodepool-issued Ed25519 token against every
  task, Worker, execution, attempt, idempotency, request-digest, and generation
  field before consulting the trusted repository lease. The repository lookup
  materializes expiry and rejects terminal-state or assignment drift; invalid
  tokens and inactive leases return explicit denials rather than widening
  authority.
- TDD evidence: the missing generated RPC first produced a compile RED; a stub
  then produced the expected authority RED; the wire-boundary test exposed
  validator bypass; and the identity-drift table exposed a missing claim check
  before the final GREEN implementation. The real tonic/Postgres test covers
  active authorization, invalid token, seven identity drifts, revocation,
  reassignment, attempt/generation rotation, replacement authorization, and
  expiry materialization.
- Exact-commit gates: Nodepool authority 2/2, Proto 12/12, Auth 7/7, Scheduler
  113 passed with 1 ignored, Worker test-target compile, and locked/offline
  Scheduler, Worker, Node Manager, Master API, and Bin checks pass. Strict
  Node Manager clippy passes after allowing only five pre-existing Scheduler
  lint hits; no authority diff is implicated by those baseline warnings.
- The commits were safely fast-forwarded into the dirty main worktree with an
  empty index. Integrated gates pass: authority 2/2, Proto 13/13, Auth 7/7,
  Scheduler 130 passed with 1 ignored, and fresh-target locked checks for
  Scheduler, Worker tests, Node Manager all-targets, Master API all-targets,
  and Bin. A shared-target production check briefly selected protobuf output
  generated from another worktree; source/generated-output inspection proved
  the cache collision, and the clean dedicated target passed without source
  changes.
- Status remains `running`. Nodepool authority is complete, but Worker
  production fail-closed enforcement and dispatcher transfer must still be
  isolated and committed; real rootless OCI isolation/E2E, hostile workloads,
  operator deployment assets, and trusted variable settlement remain open.

## Generation-bound chunk wire-contract checkpoint (2026-08-15)

- Commit `cacd0eb feat(proto): bind transfer generation to chunk contracts`
  adds a required positive Nodepool generation to chunk upload/resume and
  freezes bounded Prepare request/response messages carrying the complete
  task, Worker, execution, attempt, idempotency, request-digest, runtime,
  manifest, token, and generation identity. The service method and Worker
  authority behavior intentionally remain outside this contract-only commit.
- TDD evidence: contract tests first failed to compile on the absent Prepare
  messages and generation fields. After the minimal proto/validator change,
  Proto passed 13/13; Worker compatibility then failed on exactly four stale
  upload/resume fixtures, which were refreshed before Worker test-target and
  focused GNU chunk gates passed. Scheduler, Node Manager, Master API, and Bin
  also pass locked/offline consumer checks; scoped format, clippy, and diff
  gates pass.
- The commit is integrated into dirty main with an empty index. Integrated
  Proto passes 14/14, Worker test-target compilation passes, and the dedicated
  GNU Worker target completed the focused chunk commands successfully (the
  final unit-test result is 7/7). Shared worktree build outputs remain avoided
  for proof-quality checks because generated protobuf caches can collide.
- Status remains `running`. Next implement, test, and commit Worker production
  fail-closed Nodepool authority for Prepare/Upload/Resume; dispatcher transfer,
  real rootless OCI isolation/E2E, hostile workloads, operator assets, and
  trusted variable settlement remain open.

## Worker production transfer-authority checkpoint (2026-08-15)

- Commit `df48f19 feat(worker): enforce nodepool transfer authority` registers
  `PrepareGeneralCompute`, adds the shared bounded Prepare validator, and makes
  production Worker construction require an explicit transfer-lease authority.
  The real client forwards the complete typed identity to Nodepool under a
  five-second connect/RPC deadline; rejection maps to `PermissionDenied`, while
  endpoint/connect/RPC/timeout failures map to `Unavailable` with no local-allow
  fallback.
- Prepare verifies the Nodepool-issued Ed25519 claims, runtime admission,
  request/manifest identity, and live Nodepool lease before recording prepared
  state. Upload and resume require the same prepared generation and revalidate
  Nodepool authority before CAS reads or writes. An authorized higher generation
  clears stale report state; stale or redefined generations fail closed.
- TDD evidence includes the initial missing-authority compile failures, a
  compile-fail regression proving the old no-authority production constructor
  was still callable, and a separate RED for the missing shared Prepare
  validator. The isolated checkpoint passed Worker library 105/105, chunk
  transport 8/8, GPU 2/2, runtime admission 7/7, and doctest 1/1, plus Proto,
  Nodepool, Scheduler, and five downstream locked checks.
- The commit was safely advanced into dirty `main` with an empty index and no
  checkout of working files. Fresh integrated gates pass: Proto 15/15; Worker
  GNU library 107/107, chunk transport 9/9, GPU 2/2, runtime admission 7/7;
  compile-fail doctest 1/1; real Postgres-backed Nodepool authority 2/2; and
  Scheduler 130 passed with 1 intentional environment-gated ignore. Worker,
  Scheduler, Node Manager, Master API, and Bin locked checks also pass.
- Status remains `running`. The next unit is dispatcher authenticated
  preparation/source transfer. Real rootless OCI isolation/E2E, hostile
  workloads, operator deployment assets, and trusted variable settlement are
  still unproven release gates.

## Dispatcher authenticated source-transfer checkpoint (2026-08-15)

- Commit `94576b6 feat(scheduler): prepare authenticated chunk transfers`
  makes Nodepool obtain the active attempt-bound transfer lease, issue one
  generation-bound Ed25519 token, load every source byte from the immutable
  Nodepool repository, call Prepare before execution, resume by verified missing
  descriptors, and upload only manifest-bound chunks. Mutable inline manifest
  bytes are not a production source-authority fallback.
- Deterministic Nodepool-side failures cannot strand an assignment or blame the
  Worker: missing/expired/mismatched leases and transport failures redispatch
  without penalty; signing or missing trusted source fails terminally with a
  typed `nodepool_task_failed` result, revoked lease, no attestation/reputation
  change, and no settlement. Unknown Worker chunk descriptors fail closed.
- TDD evidence covered compile REDs for the token/planner/transport/repository
  APIs and behavioral REDs for source drift, descriptor widening, missing lease,
  signing/source failure, preparation failure, and stuck assignments. The
  isolated commit passes Scheduler 125 with 1 intentional ignore, Proto 14/14,
  downstream all-target checks, and scoped clippy after only the five known
  baseline lint hits.
- Dirty-main integration used a three-way preview plus `apply_patch`, then
  `update-ref`/`read-tree`; no checkout/reset was used and the index remains
  empty. The exact integrated source passes Scheduler 142 with 1 intentional
  ignore, Proto 15/15, Worker/Node Manager/Master API/Bin all-target checks,
  and scoped clippy after only six identified pre-existing dirty-main lint hits.
  No push was performed.
- Status remains `running`. Next isolate Nodepool-owned production input-digest
  validation. Real rootless OCI isolation/E2E, hostile workloads, operator
  deployment assets, and trusted variable settlement remain unproven.

## Nodepool-owned production input-digest checkpoint (2026-08-15)

- Commit `6967c78 feat(scheduler): validate nodepool-owned production input
  digests` makes a completed `ProductionSandboxedOci` result eligible only when
  its `input_sha256` equals `canonical_input_digest` recomputed over the bytes
  Nodepool loaded from its own immutable artifact source rows. Every artifact is
  re-verified against manifest size and SHA-256 first, so a drifted repository
  row is never a trusted input. `ReferenceDirect` keeps its legacy semantics.
- The loaded source map is now hoisted past preparation and handed to result
  validation, so the gate runs before completion or settlement. A production
  result with no loaded Nodepool bytes fails closed rather than settling; a
  mismatch resets the task for redispatch with no output, no settlement, and no
  Worker penalty.
- TDD evidence: the seven unit tests first produced `E0425` compile REDs for the
  absent validator; the DB-backed dispatch test then produced a behavioral RED of
  `Completed` where `Pending` was required, proving Nodepool would previously
  complete and settle a production task whose input digest the untrusted Worker
  had chosen. Coverage pins drift rejection, canonical acceptance, missing bytes,
  manifest drift, input-artifact participation, absent source map, and the
  reference-direct regression lock.
- Isolated gates at the exact commit: Scheduler 133 passed with 1 intentional
  environment-gated ignore against a 125-passed clean-HEAD baseline; scoped
  `rustfmt --check` and `git diff --check` clean; strict clippy reports only the
  five pre-existing Scheduler baseline hits, with no dead-code regression because
  the validator is wired to a live call site. Locked checks pass for Worker
  executor, Master API, Bin, Node Manager all-targets, and Scheduler all-targets.
- Integration into dirty `main` used `update-ref` plus `read-tree` with an empty
  index and no checkout; all 76 dirty working files are preserved and no push was
  performed. The dirty `dispatcher.rs` still carries an older non-`Option`
  variant of the validator without `trusted_artifact_bytes`; that residual slice
  must be reconciled to the committed fail-closed signature before it lands, or
  it will revert this gate.
- Status remains `running`. Real rootless OCI isolation/E2E, hostile-workload
  evidence, operator deployment assets, trusted variable usage/billing
  settlement, M4 GPU capability, and M5 release readiness are all still open.

## M4 GPU capability audit and device-bound tensor checkpoint (2026-08-15)

- Audited the three remaining S3 GPU bullets in the plan against the existing
  `gpu` module before writing anything new. `gpu::negotiate_gpu` and
  `tests/gpu.rs::gpu_negotiation_rejects_runtime_driver_image_and_capacity_mismatches`
  already enforce the CUDA/ROCm-specific fixed image/driver compatibility
  matrix (exact vendor/runtime/driver_abi/image_digest/vram/stream match), and
  `gpu_negotiation_requires_explicit_cpu_fallback` already proves a missing
  compatible device fails closed unless `allow_cpu_fallback` is explicitly
  set. Neither needed new work; no test was added to re-prove already-covered
  behavior.
- The one genuine gap was the last bullet: GPU buffer I/O had no device
  metadata or VRAM-bound validation. Commit
  `67b89f3 feat(runtime): bind GPU tensor buffers to a device and VRAM
  budget` adds `gpu_tensor::GpuTensorManifest`, wrapping the existing
  `TensorManifest` (checksum/size/dtype/shape) with a `device_id` binding.
  `validate_for_device` rejects a device_id that does not match the
  negotiated `GpuCapability` and rejects declared bytes larger than the
  capability's `vram_bytes` with a typed `ExceedsDeviceVram` error before any
  bytes are touched; `validate_bytes_for_device` additionally rehashes
  materialized bytes through the existing tensor checksum gate. This is a
  reference-level contract only — it does not perform real device allocation,
  chunked on-device transfer, or CUDA/ROCm execution.
- While isolating this unit, found and fixed (as a separate, isolated
  `3502760 test(runtime): pin backend registration execution mode in
  contracts fixtures` commit) a pre-existing compile break at clean HEAD: four
  `BackendRegistration` literals in `tests/contracts.rs` predated the
  `execution_mode` field from the M1 production sandbox policy commit and no
  longer compiled. A fifth, matching break in `tests/execution.rs` was folded
  into the `67b89f3` commit since it was directly adjacent to the touched
  fixture. Neither fix changed production code or test intent; both were
  pre-existing drift unrelated to any feature in flight.
- TDD evidence: `tests/gpu_tensor.rs` first failed to compile with
  `E0432: unresolved import` for the absent module, then passed 6/6 after the
  minimal implementation. Full `general-compute-runtime` locked suite passed
  (every test binary green) after the fixture fixes. Scoped rustfmt on both
  new files is clean; `git diff --check` is clean; strict clippy on the crate
  lib shows zero findings in `gpu_tensor.rs` (106 reported errors are
  pre-existing crate-wide pedantic debt already documented in prior
  checkpoints — casts, missing `#[must_use]`, missing `# Panics` docs, mostly
  in `reference`/`production`/`sandbox` — not touched by this change). Worker
  executor, Task Scheduler, and Bin locked checks pass.
- Both commits were fast-forwarded into dirty `main` via `update-ref` plus
  `read-tree` with an empty index; no checkout/reset touched any of the other
  ~76 dirty files. Because these commits introduced brand-new files
  (`gpu_tensor.rs` source and test) that only existed in the isolated
  worktree, `read-tree` alone left them showing as deleted in the shared
  working tree's status (index/HEAD had them, disk did not) — a new wrinkle
  versus the previous input-digest checkpoint, which only modified an
  already-materialized tracked file. Fixed by copying the two new files'
  content into the shared working tree so it matches history exactly; this
  touched no other file. No push was performed.
- Remaining M4 scope per the plan is now believed closed at the reference-
  contract level: vendor/runtime/driver/image/VRAM/stream negotiation,
  explicit CPU fallback, device-bound buffer validation. Real CUDA/ROCm
  device execution, GPU hardware attestation, and GPU-specific OCI E2E are
  not claimed and remain blocked on the same real-OCI/operator-asset gates as
  the rest of M3/M4.

## Fixed-reservation settlement provenance checkpoint (2026-08-15)

- The `general_compute_settlements` table has existed in the schema since an
  earlier checkpoint (identity columns, `billing_version`, `cost_model_version`,
  `usage_claim_json`, `evidence_level`, `settlement_basis`, `amount_cpt`), but
  an audit before writing anything new found zero `INSERT` statements into it
  anywhere in committed history — every prior test only asserted the table
  stayed at `COUNT(*) = 0` on failure/cancel/timeout paths. A successful
  general-compute completion was charging `task.max_cpt` through the same
  generic ledger path used by v0, with no settlement provenance row at all.
- Commit `f2b6089 feat(scheduler): persist fixed-reservation settlement
  provenance` closes that gap. `complete_guarded` now parses the
  already-validated typed result and its matched request (the exact bytes the
  completion `UPDATE`'s `WHERE` clause just proved match the persisted
  manifest) to recover `billing_version`/`cost_model_version`, and inserts one
  settlement row alongside the existing ledger entries whenever a charge
  succeeds. The amount is always the same Nodepool-owned `billable_cpt`
  already used for the ledger — never derived from the Worker's usage claim —
  and `settlement_basis` is recorded as `"fixed_reservation"` so the basis is
  auditable instead of implicit. `usage_claim_json`/`evidence_level` are
  stored for audit only; `evidence_level` is read directly from the typed
  result because `validate_against` (called before this point, in dispatcher)
  already rejects any non-`unverified` claim via `EvidenceEnvelope::
  validate_worker_claim`, so the stored value is trustworthy without
  re-validation.
- This explicitly does **not** change the billing amount formula or add
  variable/usage-driven settlement. That remains blocked on Nodepool-verified
  replicated/TEE/zk evidence per the trust model (`v0 繼續走目前 proof
  settlement... 先採 Nodepool-owned fixed reservation/tariff，或在
  replicated/TEE/zk evidence驗證後才升級可信度`) — upgrading the basis is a
  separate, larger design decision deliberately left open here.
- TDD evidence: the new settlement-provenance test first failed with
  `RowNotFound` against a real Postgres instance (no row existed), then
  passed after the minimal implementation. An existing manifest-guard test's
  minimal result-JSON stub (`{"status":"completed","usage":{"wall_time_ms":1}}`)
  was replaced with a complete, valid `GeneralComputeResult` because the new
  code path now parses it fully; the stub was an artificial shortcut that
  never reflected what the production caller (dispatcher, after full
  `validate_against`) actually supplies, so tightening it made the test more
  representative without changing its assertions. Full scheduler lib suite:
  134 passed, 1 intentional ignore (baseline 133). Scoped rustfmt and `git
  diff --check` clean. Strict clippy introduced two new findings (a redundant
  closure and an 8-argument function) that were fixed in-unit — `.map(...)`
  now passes `serde_json::from_slice` directly as a function reference, and
  the result/billing-version/cost-model-version triple is bundled into one
  parameter — bringing clippy back to the same pre-existing baseline. Worker
  executor, Master API, Bin, and Node Manager locked checks pass.
- Integration risk: the dirty (uncommitted) `task_repository.rs` in the
  shared working tree carries a large prior-session WIP diff (+2677/-1969)
  that independently touches the exact same regions this commit changed
  (`complete_guarded`'s general-compute block and `insert_ledger_entry`'s
  neighborhood). This was not merged or inspected in depth — same reasoning
  as the flagged dirty `dispatcher.rs` risk above: it is unfamiliar, large,
  in-flight work from a prior session, and surgical edits into it risk
  corrupting real progress. Whoever isolates that dirty slice into a clean
  commit must reconcile it against `insert_general_compute_settlement` and
  `general_compute_settlement_source`, or this settlement-provenance gate may
  be silently dropped or duplicated.
- Fast-forwarded into dirty `main` via `update-ref` plus `read-tree` with an
  empty index; no checkout/reset touched any other dirty file. No push was
  performed.
- Status remains `running`. Real rootless OCI isolation/E2E, hostile-workload
  evidence, operator deployment assets, variable/verified-evidence settlement,
  and the two flagged dirty-tree reconciliations remain open.

## M5 release-readiness audit (2026-08-15) — no unit shipped, findings only

- Audited the remaining M5 items before attempting any of them, to avoid
  either fabricating low-value work or overclaiming production support.
- "Image digest pinning verification" (the one item that looked
  code-shaped) turned out not to be a real accept/reject gap on inspection:
  `GeneralComputeRequest::validate()` already requires
  `guest_image_digest` to be a well-formed `sha256:` digest, and
  `CapabilityMatrix::validate_request` only ever accepts a backend/worker
  image registration via exact-string equality against that
  already-validated request digest. A malformed registered digest (e.g. a
  mutable tag) can therefore never cause a wrong admission — it can only
  make a backend silently unreachable, which is a weak operator-diagnostics
  issue, not a trust or correctness gap. This is a materially smaller finding
  than the equivalent, already-fixed gap for GPU capabilities (which do call
  `capability.validate()` before matching), but adding validation here would
  not change any test-observable behavior, so no unit was created for it to
  avoid manufacturing work against the "don't add validation for scenarios
  that can't happen" guidance.
- The remaining M5 items — canary/migration, a benchmark dashboard, signed
  image/SBOM, and a support matrix — were not attempted. Canary/migration,
  the dashboard, and image signing need real release infrastructure (a CI
  runner, a dashboard host, a signing/SBOM toolchain) that does not exist in
  this repository-local environment. A support matrix is inherently a public
  claim about what is production-supported; given M3/M4/M5 gates are still
  explicitly open (no real OCI E2E, no hostile-workload evidence, no
  variable-usage settlement), writing one now risks overclaiming readiness
  the project has been careful never to claim at any prior checkpoint.
  Publishing a general-compute-v1alpha1 SDK example has the same problem one
  level down: it is a product decision about what surface to publish, not a
  TDD-provable correctness gap, and is not this session's call to make
  unilaterally.
- No code, test, or documentation change was made for this item. It is
  reported back to the user rather than closed with fabricated work.

## Native Windows HCS execution checkpoint (2026-08-15)

- Commits `9241dcf`, `a59ae7f`, `2778f2b`, `e0b812d`, `d3de4d0`, `4e06110`,
  `27ec36e`, `594c78e`, `b327ef7`, `7e3a11b`, and `8212cd8` add the distinct
  `production_sandboxed_windows` mode, Windows policy and operator registry
  contracts, fail-closed registry loading, validated HCS specification
  generation, a ComputeCore/HCS lifecycle boundary, Worker routing, and the
  Windows package registry setting.
- Windows policy validation requires process isolation, deny-all networking,
  read-only root, explicit artifact/scratch mounts, bounded resources, safe
  artifact IDs, and operator-owned roots. Linux OCI policy is never
  reinterpreted as Windows isolation.
- The HCS launcher is platform-specific: Windows builds call ComputeCore.dll
  lifecycle APIs with bounded cancellation/timeout polling and termination;
  non-Windows builds return `UnsupportedPlatform` without a direct-process,
  Docker, WSL, PowerShell, or Job-Object-only fallback.
- TDD evidence: runtime HCS boundary tests pass 11/11, Windows registry and
  HCS-spec tests pass 6/6, and `cargo check -p general-compute-runtime
  --target x86_64-pc-windows-msvc` passes. The Hivemind Worker integration
  check remains blocked by pre-existing generated protobuf/API drift in the
  dirty workspace (`GeneralComputePrepareRequest` and transfer-generation
  fields).
- This checkpoint does not claim real Windows HCS/container E2E evidence.
  Operator-provided Windows container image layers, result-envelope transport,
  hostile-workload evidence, and a real Windows multi-process/Postgres E2E
  remain release gates.

## Windows HCS result transport checkpoint (2026-08-15)

- Commits `89debcd`, `d19c63c`, and `51f170a` add bounded result-envelope
  transport for the native Windows route and switch the HCS calls to the
  pinned `windows-sys` HostComputeSystem bindings. The generated HCS spec now exposes an operator-owned
  writable scratch mount, a fixed `result.json` host path, a fixed Windows
  container path, and the configured maximum result size; policy destinations
  are translated to Windows container paths rather than passed through as Linux
  OCI paths.
- After a successful HCS exit, the launcher rejects missing, non-regular,
  symlinked, oversized, or improperly mounted result files. It reads at most
  `max_output_bytes + 1` bytes, then returns the bytes as the existing
  `ProductionResultEnvelope` stdout channel, preserving the existing typed
  protocol/input-digest/output-root validation in the Worker.
- Worker startup removes only a pre-existing regular result file beneath the
  validated operator scratch root; a pre-existing symlink or directory fails
  closed. HCS configuration passes the fixed result path through the
  operator-generated process environment, with no Worker-provided executable or
  filesystem path.
- TDD evidence: HCS unit tests pass 3/3, the Windows HCS-spec contract passes,
  runtime contracts/sandbox/production suites pass 22/22, 13/13, and 24/24,
  Windows-target runtime compilation passes, and the Windows packaging Pester
  contract passes. Worker integration remains blocked by the pre-existing
  protobuf/API drift listed above. No real Windows provider or settlement E2E
  is claimed.

## Windows HCS lifecycle mock checkpoint (2026-08-15)

- Commit `07c43bd test(runtime): cover Windows HCS lifecycle cleanup` factors
  the start/wait/terminate/shutdown decision loop behind a small lifecycle
  provider boundary. The native Windows adapter still performs the real HCS
  calls; the cross-platform mock verifies the cleanup contract without
  pretending to be provider evidence.
- Mock coverage verifies normal exit performs shutdown, timeout performs
  termination, cancellation performs termination, and start failure does not
  issue cleanup against an unstarted system. The native adapter continues to
  use bounded one-second exit polling and closes HCS operation/system handles.
- TDD evidence now passes HCS unit tests 6/6 and Windows-target compilation.
  This remains lifecycle-policy evidence only; it is not real Windows HCS,
  container, network, filesystem, hostile-workload, or Postgres E2E evidence.

## Worker protobuf integration checkpoint (2026-08-15)

- Commit `6b9e515 fix(proto): rebuild generated bindings when schemas change`
  adds explicit `cargo:rerun-if-changed` directives for the shared protobuf
  schemas. The Worker integration compile blocker was generated-code staleness,
  not a missing Windows execution API: after regeneration,
  `cargo check -p hivemind-worker-executor` passes.
- The Worker library test binary reaches linking but remains environment
  blocked by an existing Windows native-link failure from the client-runtime
  dependency (`__mingw_fprintf_cgo_beginthread`, LNK2019/LNK1120). This is
  reported as a failed test gate, not as passing Worker tests. No production
  code was weakened to bypass the linker.

## Windows provider prerequisite checkpoint (2026-08-15)

- The current host reports `vmcompute:Running`, but the Windows `Containers`
  optional feature is `Disabled`. Docker Server 29.6.2 is available, but its
  provider mode is not Windows HCS evidence and is not substituted for the
  required native Windows container provider.
- Consequently, real Windows HCS/container completion, network/filesystem
  denial, hostile workloads, and multi-process/Postgres settlement remain
  blocked on operator infrastructure. No Docker/Linux VM/WSL/direct-process
  result is recorded as Windows production evidence.

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

## Current step

M0a v0 semantics/cost/proof freeze 與 M0b 的 alpha runtime、request/result/evidence、artifact/tensor、capability 契約均已完成；M1 的 reference fixtures、bounded supervisor、CPython adapter、combined output cap、trusted executable gate、production sandbox policy 與受驗證 OCI bundle runner 也已落地。runner 只接受 operator-pinned absolute executable 與 SHA-256、精確 OCI 1.0.2 config、rootless/non-root、namespace/cgroup/seccomp/no_new_privileges/read-only root/network-deny/mount annotations，並透過既有 process-tree supervisor 套用 timeout、cancel、output cap、kill/reap；驗證失敗一律 fail closed。實際 Linux rootless OCI namespace/cgroup/seccomp primitives 仍由外部 runner 負責，尚未宣稱平台隔離完成。M0 capability matrix 仍是 supervisor 啟動前的 fail-closed gate。

M3 alpha manifest transport/admission 已由 `8b34285` 完成並提交：HTTP Master、Master→Nodepool gRPC、Nodepool task persistence、scheduler→Worker request 與 Worker capability admission 都帶有明確的 validated manifest bytes。Nodepool 拒絕 alpha runtime 的 legacy `torrent`，Worker 只接受 operator allowlist 的 backend/image；未安裝實際 backend 時刻意 fail-closed 回傳 `UNIMPLEMENTED`。下一個小單元是 Nodepool trusted registry 與 persisted worker-capability compatibility gate。

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

## Active owners

- Origin：使用者，擁有完整 M0–M5 目標與「每小單元測試、相容性驗證、local commit」驗收規則。
- Coordinator／implementation：Codex。
- Nodepool trust review：M3 接線時需依 `AGENT.md` 的 trusted-authority model 驗收。

## Blockers

- `general-compute-runtime` 的 strict clippy `-D warnings` 目前仍有既有 crate-wide pedantic debt（主要在 reference/lib/tensor 與既有 API must-use/docs）；本 M1 policy scoped tests、format 與跨元件 checks 已通過，但未把無關 lint debt 混入本單元。
- 實際 Linux rootless OCI namespace/cgroup/seccomp/no_new_privs primitives 仍由外部 operator runner 負責；本程式只驗證 bundle envelope 並透過 pinned runner 啟動，尚未宣稱 host platform isolation 或 Worker/Nodepool runtime routing 已完成。

## Next action

建立 Nodepool trusted registry／persisted worker-capability compatibility gate，然後以 RED→GREEN 做 attempt-bound request/result compatibility；後續再接實際 general-compute backend execution、artifact materialization 與 CAS/chunk resume。保持 pinned CPython direct harness 只作 reference/test backend；M2 dtype/complex/數值運算仍是獨立小單元。

## Next checkpoint

M3 alpha manifest transport/admission 的跨 crate RED→GREEN 已由 `8b34285` 提交，且未安裝 backend 時 Worker 仍 fail closed。下一 checkpoint 是 Nodepool trusted registry/capability persistence，再來是 request/result attempt compatibility。

## Notes

- 2026-08-13 Monty removal was revalidated after the cleanup commit: the root repository has no tracked Monty paths, `executor-rs/Cargo.toml` exposes only `managed-function-runtime` and `general-compute-runtime`, and the executor workspace plus Docker/Windows release-contract gates pass. The untracked nested `executor-rs/.git` upstream metadata and stale `executor-rs/target` build artifacts were physically removed after explicit user authorization; neither was part of any Hivemind build or runtime path.
- 2026-08-13 M1 leader-exit process-tree hardening is implemented in the reference lifecycle supervisor: Unix process groups remain scoped to the invocation; Windows starts suspended, assigns a Job Object with kill-on-close, resumes the initial thread, and terminates the job before joining inherited output pipes. Spawn setup failures explicitly kill/reap the child. RED→GREEN coverage now includes normal leader exit with an inherited descendant pipe, timeout descendant cleanup, and Windows fixtures that prove descendant launch without relaxing the 600 ms timeout. `cargo test --workspace --locked`, `cargo check -p hivemind-worker-executor --locked`, Docker Compose release contracts, and Windows worker packaging contracts all pass.
- Future-drop cleanup remains covered by the existing Worker managed-prover/execute-future guards; `dbf5765` now wires a validated OCI bundle invocation through the same cleanup boundary, while actual Linux isolation remains an operator-runner responsibility.
- 此檔先前的 `complete` 只代表「舊 Monty 清理與計畫文件」完成，並不代表使用者要求的完整演進計畫完成；2026-08-12 已依實際 scope 修正為 `running`。
- 不要對工作樹中的其他 dirty frontend/API 變更使用 `reset`、`checkout` 或整批刪除；它們不屬於目前小單元。
- `managed-function-v0` 的有限配額與 proof settlement 是 load-bearing 契約，不得為了 v1 任意運算而放寬。
- `general-compute-v1` 必須使用獨立 runtime/version/cost/verifiability contract，不能冒充現有 RISC Zero proof path。

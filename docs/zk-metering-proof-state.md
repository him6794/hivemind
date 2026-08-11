# Task State: ZK Metering Proof

## Goal

讓 Nodepool 只根據經零知識證明驗證成功的 managed-function 執行與 usage units 結算。

## Success criteria

- Proof 綁定 task、程式、輸入、輸出、版本與計費數字。
- 篡改、重播、錯誤 image id 或錯誤版本均無法通過。
- Nodepool fail closed，且 rollout 前後均有完整測試與可觀測性。

## Status

running

## Current step

階段 5 進行中。階段 4（off／observe／enforce rollout policy、verification metrics、audit events、admin 查詢介面）已完成並驗證；guest image ID、build attestation 與真實 receipt fixture 已於 2026-08-11 重建並通過 verifier suite；prover sidecar 已打包進 worker image 並以實際映像驗證。剩餘為多節點 Docker E2E 與瀏覽器回歸。

## Completed

- 盤點現有 receipt/checksum/JWT/settlement 路徑。
- 確認目前沒有 ZKP 或 receipt signature verification。
- 選擇成熟 zkVM 作為完整執行證明方向。
- 持久化五階段實作計畫。
- 以 RED→GREEN 完成 backend-neutral public execution claim。
- Journal 綁定 protocol/runtime/cost-model、task id、source/input/output SHA-256、budget 與 execution metrics。
- Proof crate 3 tests、clippy、fmt、audit 與 GNU workspace 246 tests 通過。
- 固定 RISC Zero 3.0.6 stable；production verifier 必須使用 `disable-dev-mode`。
- 將 canonical output rendering 下沉到 managed runtime；runtime 16 tests、Worker 52 tests 通過。
- RISC Zero guest 已執行完整 deterministic managed runtime，journal 與 native claim golden vector 完全一致。
- 真實 receipt 已以固定 guest image id 驗證；錯誤 image id 與篡改 journal 均被拒絕。
- Builder 使用 `r0.1.88.0@sha256:3e12f71bacd27527a61dea96fa0e53e468c99aa261d3a1019b593f6dbd943eb3`。
- Guest image id 曾為 `[3606400121, 4250889949, 2277454476, 3430793801, 2111044864, 2713379816, 851522248, 2751351423]`；bounded renderer 等 guest source 變更後已於 2026-08-11 重建為 `[466412732, 2327327967, 2963073729, 178423767, 1914766815, 1823038484, 4206432854, 2659673256]`，詳見 `docs/zk-managed-proof-build-attestation.md`。
- zkVM tests 2 passed；真實 proving 約 570–580 秒；fmt 與 clippy `-D warnings` passed。
- Worker proof envelope 已固定 `risc0-zkvm-3.0.6` scheme、guest image id、journal 與完整 receipt，並支援有遞迴深度限制的 JSON round-trip。
- Verifier 先拒絕錯誤 scheme、image id 或 envelope/receipt journal 不一致，再驗證 receipt，最後才解析 execution claim。
- 新 envelope 的真實 proof round-trip 通過；proving 570.02 秒，claim 與 native runtime 相同。
- Protobuf `ManagedProofEnvelope` 已定義 scheme、固定長度 image-id words、journal 與 receipt JSON，並以 optional `ExecuteTaskResponse.managed_proof` 傳遞；舊欄位編號維持不變。
- Nodepool-owned verifier 已依序 fail closed：錯誤 scheme、image-id 長度、非可信 image id、無效 receipt JSON、journal mismatch、fake/無效 proof 與無效 verified claim。
- `risc0-verifier` feature 僅啟用 protobuf transport、no-default-feature RISC Zero verifier 與 `disable-dev-mode`；未啟用 `std`、`prove`、methods 或 Docker builder。
- 真 fixture 大小 664,026 B；receipt 661,720 B、journal 656 B、單一 poseidon2 Composite segment、無 assumptions，seal 63,914 words；完整 deterministic claim逐欄硬編碼驗證。
- Verifier 在 decode/crypto 前限制 journal 4 KiB、receipt 2 MiB、單 segment、seal 131,072 words，拒絕非 Composite、錯誤 index/hashfn 與所有 assumption 路徑；production context以 thread-local重用最小 segment parameters。
- Verifier feature tests 25 passed；包含一筆 within-cap seal bit flip 確實進入 crypto gate 並回 `InvalidProof`；release warm end-to-end p99約 23.6 ms，新 process cold outlier 426.95 ms，因此 phase-1 process isolation暫定 1 s timeout／concurrency 1／128 MiB RSS。
- Tracked host regression test會把 current methods build產生的 guest ID與 Nodepool trust pin直接比對；本輪 current-source rebuild通過，ELF/input hashes與清理證據記錄於 `docs/zk-managed-proof-build-attestation.md`。
- Verifier focused fmt、clippy `-D warnings`、diff check 與 feature graph通過；移除不必要的 RISC Zero `std` feature 後，主 workspace `cargo audit` 為 0 vulnerabilities，並將 `event-listener` 由 5.4.1 升至 5.4.2 修正新公布的 unsound advisory。
- `ExecutionClaim::validate_bindings` 已逐項 fail closed 驗證 protocol/runtime/cost-model、task id、source/input/output SHA-256、max budget、usage bound 與 output length；default 15 tests、all-feature 37 tests、clippy 與 package fmt 均通過，並以本機 commit `eb9894a` 保存。
- Scheduler verified-claim 轉換的 12 個 focused tests 已完成 RED→GREEN；重播、source/input/output/budget 與 protocol/runtime/cost-model mismatch 均被拒絕，成功路徑只採用 claim 的 usage/output 並忽略 Worker legacy scalars/receipt。
- Hidden `--verify-managed-proof` mode 已在 `hivemind-bin` 與 `hivemind-nodepool` 於 tracing/config 啟動前攔截；protobuf stdin 有 2,166,784-byte cap，stdout 只回 verified claim JSON；真 fixture、crypto tamper、malformed/trailing/oversize、exact-mode 與 backtrace-on/off 固定錯誤輸出 tests 共 11 passed，clippy/fmt/diff check 皆通過。
- Nodepool parent verifier adapter 已完成 5 個 focused tests：全域 concurrency 1、8-slot bounded wait queue、1 秒 end-to-end deadline、timeout kill/reap、4 KiB stdout cap、nonzero/malformed/oversize fail closed，並在 Windows 使用 128 MiB Job Object process-memory hard limit、Unix 使用 hard `RLIMIT_AS`。
- Dispatcher 已呼叫隔離 verifier；proof/subprocess/binding 任一失敗都 `fail_for_worker`，成功時只把 verified claim 的 `usage_units`、`output_bytes` 與 claim JSON交給 transaction completion；failed/valid verifier focused tests 3 passed。
- Worker response application caps 已完成 RED→GREEN：status message 1 MiB、legacy managed receipt 64 KiB，超限在 proof/settlement 前拒絕；3 tests passed。
- Scheduler full lib 68 tests 與 all-target clippy `-D warnings` 已通過；Windows timeout cleanup 測試原 250 ms 啟動假設在並行負載下產生 PID-marker flake，確認實際已回 `DeadlineExceeded` 且無殘留 child 後，只將 test deadline 調為 750 ms，10/10 serial 與 full 5-test suite 均通過，production 仍為 1 秒。
- Read-only code review 為 APPROVE/WATCH、0 blockers；唯一 MEDIUM 指出 nodepool-local verifier queue 壓力不應歸責 Worker。已以 RED→GREEN 區分 `QueueDeadlineExceeded` 與真正 child deadline；`QueueFull`／queue wait timeout 會 redispatch且不寫 worker failure/attestation，其餘 verifier/binding failure仍永久 fail closed。

## Active owners

- Origin: 使用者
- Coordinator/implementer: Codex
- Claim binding review: complete（CLEAR，0 blockers）
- Verifier subprocess owner: complete（68 tests／clippy GREEN，review APPROVE，0 blockers）
- Existing-binary verifier mode owner: `verifier_cli_tdd`（complete；11 tests、clippy/fmt/diff check GREEN）
- Admission caps owner: complete（`367c71d`；final review CLEAR／APPROVE）
- Prover sidecar owner: `prover_sidecar_finish`（complete；locked-metadata RED→GREEN，review CLEAR／APPROVE，0 blockers，待本機commit）
- Worker lifecycle／bounded prover: complete（`1a9fa8f`、`d99c8f7`；tests/review GREEN）
- Rollout policy owner: Codex（running；RED config contract → scheduler policy → focused/full gates）

## Blockers

- 獨立 zkVM prover/toolchain lockfile 的 `rsa`（RUSTSEC-2023-0071，經 rzup，無修正版）與 `tracing-subscriber 0.2.25`（RUSTSEC-2025-0055，卡在 `ark-relations` 的 `^0.2`）已改為可稽核的接受政策：`zkvm/managed-proof/.cargo/audit.toml` 記錄兩者，可達性分析與重新檢視觸發條件寫在 `docs/zk-managed-proof-dependency-audit.md`。主 workspace `cargo audit` 為 0 vulnerabilities。
- 單次 proving 約 9.5 分鐘，現階段不可直接啟用 enforce；需後續 timeout/queue/benchmark 設計。
- ~~guest image ID／attestation／真 proof fixture 過期~~：已於 2026-08-11 重建。新 pin 為 `[466412732, ...]`，pin-test 在更新後重跑仍 GREEN，真 fixture 重新 proving（732 秒）且 verifier suite 37 passed。
- ~~sidecar 尚未進 release image／Compose~~：已打包。`packaging/managed-prover/` → `hivemind-rs/Dockerfile` → `/app/prover/`，Compose 預設指向該路徑；實際建置的 worker 映像已驗證 binary 可執行且 fail-closed。
- Admission caps 已完成四個 direct task-ID RPC gate 與 no-DB runtime-bypass test 修正；最終 review 為 `CLEAR / APPROVE`、零 blockers，GNU full/focused tests、clippy、fmt/diff evidence 均已保留。
- C: Docker VHD 空間不足，prover 改走 WSL/native Linux 並將 artifacts/TMP 放 D:；既有 Docker stack 已恢復且保持 healthy。
- RISC Zero 3.0.6 只承諾 Linux/macOS first-class prover host，沒有官方 native Windows prover workaround。發布策略已明確化而非繞過：prover 在受支援的 Linux/macOS（或 WSL）以 `scripts/build-managed-prover.sh` 建置一次，產物 stage 到 `packaging/managed-prover/`，由 worker 映像烘入。此限制已寫入 README 的「Managed-function proving」與 `docs/GETTING_STARTED.md`。缺 sidecar 時每個 managed task 明確 fail closed，不會靜默降級。

## Next action

執行多節點 Docker E2E 與瀏覽器回歸（`scripts/release-stack-piecemeal` 之外的完整 `release-stack-smoke.ps1 -KeepRunning` 加 Playwright），並在該環境下實測一次真實 managed-function 端到端結算。

## Next checkpoint

Compose 起的完整 stack 中，一個 `managed-function-v0` 任務能由 worker 產生 proof、由 nodepool 獨立驗證並只依 verified claim 結算；`/api/admin/managed-proof/metrics` 的 `verified` 計數增加，且 audit log 出現對應的 `managed_proof_verification` 項目。

## Notes

- Docker 測試 stack 仍供使用者測試，不停止、不清理。
- 不 push；每個完成切片各自本機 commit。
- Worker prover 採獨立 sidecar，不把 `zkvm/managed-proof/methods/build.rs` 或 RISC Zero `prove` graph 拉入主 workspace；版本化 transport 放在 guest 不依賴的輕量 protocol crate，以免 transport 變更漂移 image ID；Worker 負責有界 I/O 與 cancel/timeout/kill/reap。
- Verifier kill/reap regression 已改由父端在 `spawn` 後觀察 PID，避免將子程序獲 CPU 的時機錯當成 child lifecycle；production verifier 仍採 1 秒總 deadline，沒有為測試放寬。
- Verifier／settlement 已封存為本機 commit `03a080e feat(proof): isolate verified settlement`；提交前 scheduler full（70 passed、1 intentional ignored）、focused kill/reap、nodepool verifier（7 passed）與 GNU clippy `-D warnings` 均通過。未 push，Docker stack 未受影響。
- Admission caps 已封存為本機 commit `367c71d feat(api): enforce managed task admission caps`；範圍限於 proto、Master API、Node Manager、Worker admission gates，final review 為 `CLEAR / APPROVE`。

## Continuation checkpoint — 2026-08-09

- Current status remains `running`; the project is not release-ready.
- Next code slice is Worker-side bounded proving: validated sidecar request/response, `TaskResult.managed_proof`, supervisor-owned active-task cleanup, cancellation/timeout kill-and-reap, and explicit Worker/server/scheduler transport bounds.
- Before release, Worker and guest require consistent finite runtime limits; guest image ID, attestation, and real-proof fixture must then be regenerated and verified.
- Managed proof release validation requires a supported Linux/macOS prover host or an explicit verified Linux/WSL strategy; native Windows RISC Zero proving is not an accepted release path.
- The untracked `tdd-red/target` test artifact must not be staged. Shell stalls in the prior round are not test outcomes and must not be cited as such.
- Prover protocol/sidecar is committed as `6e7af38 feat(proof): add managed prover sidecar`; Worker integration, package delivery, and supported-host proving remain required release gates.

## 2026-08-09 Worker 與 RPC 狀態

已提交 `1a9fa8f feat(rpc): bound worker proof transport` 與 `d99c8f7 feat(worker): generate managed proofs safely`。

- Worker 對 `managed-function-v0` 只會在 native function 執行和 sidecar proof 都成功後回覆 success；缺 proof 不可結算。
- sidecar 有單一 proving slot、bounded stdin/stdout、timeout/cancel/abort kill-and-reap，以及 generic fail-closed error。future drop 不會讓 child 或 permit 無人管理。
- Scheduler/Worker RPC 設定了 4 MiB 全訊息 cap、5 秒連線上限與 20 分鐘 execution deadline。connect/transport failure 會安全重派、不影響 Worker reputation；不可信 proof、binding failure 與實際 worker failure 仍 fail closed。
- Worker 公開 `TaskResult` serde 契約已保留，proof 不會被 `skip` 或靜默遺失；舊 JSON 仍可讀取。
- 本機 GNU Worker suite 81/81、sidecar 15/15、clippy/format/diff、worker binary compile 已通過，且兩次獨立 code review 無 blocker。

此狀態仍不是 release-ready：runtime 仍需從 unlimited 改為有限安全 guard，之後必須重建 guest image/attestation/真實 fixture；還需要支援的 Linux/macOS prover host 實證、sidecar Docker/Compose 打包與多節點 E2E。

## 2026-08-09 Runtime limits and current proof state

本機提交 `097c98a fix(runtime): enforce finite managed execution limits` 已完成。Worker 與 zkVM guest 現在均採用有限的 default evaluator limits，同時維持 managed task/envelope usage budget 為唯一的 billing limit。depth-65 回歸測試證明舊 unlimited Worker policy 會放行、而新的 matched policy 會拒絕。

這已保護 operation、recursion、print-output 與 loop 的無上限路徑，但也變更了 guest source。因此目前受信任的 guest image ID、build attestation 與真實 proof fixture 均已過期，不能用來宣稱可發布。

在重新生成前還有一個 runtime safety blocker：canonical returned value 仍在 evaluation 後以沒有 size-bound API 的方式 render，且 string/list/dict materialization 沒有累積 allocation budget；`max_output_bytes` 只檢查 `print`。下一個切片會以測試先行補上 shared、allocation-safe 的限制，然後在支援的 Linux/macOS host 重建並 prove 最終 guest source、刷新 trust pin 與 fixture。

Current owner/checkpoint: `runtime_value_limits_tdd` owns the shared-runtime RED→GREEN implementation. The return condition is bounded-render and materialization regression coverage plus a clean runtime verification report; the coordinator will then wire that API into Worker, guest and native claim parity before independent review.

## 2026-08-10 Bounded renderer 與剩餘發布差距

本機提交 `0158129 fix(runtime): bound canonical output and value materialization` 已關閉
finding #29。`max_output_bytes` 先前只作用於 `Stmt::Print`，因此 managed function 仍可
建立超大 canonical 回傳值或中間 string/list/dict；在 render 完成後才檢查長度並不安全，
因為超大配置已經發生。

現在 Worker、zkVM guest 與 host golden-vector claim 三者共用 `render_output_bounded`：
逐次 append 前檢查上限，被拒絕的值不會先實體化超大序列化中間結果。手寫 JSON escaping 以
`serde_json` 輸出為基準測試釘住，避免 backend 之間分歧。另加入 per-value（canonical
bytes／collection items／depth）與 cumulative materialization 上限，全部使用固定寬度 u64
邏輯位元組計數，使 native worker 與 zkVM guest 做出完全相同的 accept／reject 決定。這些是
安全上限，`usage_units` 的唯一計費上限仍是 task／envelope budget。

驗證（`x86_64-pc-windows-gnu`）：runtime 25、worker-executor lib 83、managed-proof 15、
task-scheduler lib 75（1 intentional ignored）、clippy `-D warnings`、`cargo fmt --all` 全綠。

此變更改動了共用 guest source，因此目前受信任的 guest image ID、build attestation 與真實
receipt fixture 全部過期，必須在支援的 Linux/macOS prover host 重建並跑一次真 proof 之後，
才能用來支持任何發布宣稱。

尚存發布差距：prover sidecar 尚未被正式 worker image 打包（`MANAGED_PROVER_EXECUTABLE`
預設空字串，Compose worker 在 enforce 下會安全 fail closed）；階段 4 的 off/observe/enforce
rollout mode、metrics 與 audit events 已完成；階段 5 的惡意 Worker 測試、多節點 Docker E2E、
資源釋放與依賴稽核未開始；單次 proving 約 570-580 秒的經濟模型尚未定案。

Windows 原生無法編譯 `risc0-circuit-rv32im-sys`（C++ 需 `/std:c++20`），失敗發生在
`cargo check` 進入本專案 crate 之前，屬既有環境限制。

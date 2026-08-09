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

階段 3：Nodepool-owned RISC Zero verifier 的真 receipt、負向鏈、資源 gate 與 focused quality gates 已 GREEN；正在以 TDD 接入 scheduler／DB 的驗證後結算路徑。

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
- Guest image id 為 `[3606400121, 4250889949, 2277454476, 3430793801, 2111044864, 2713379816, 851522248, 2751351423]`；verifier 依賴精簡後重建仍保持相同 ID。
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

## Active owners

- Origin: 使用者
- Coordinator/implementer: Codex
- Background review: complete（CLEAR／APPROVE，0 blockers）

## Blockers

- 獨立 zkVM prover/toolchain lockfile 仍有 `rsa` timing side-channel advisory（經 rzup，無修正版）；主 workspace verifier 已不再拉入有漏洞的舊 `tracing-subscriber`。
- 單次 proving 約 9.5 分鐘，現階段不可直接啟用 enforce；需後續 timeout/queue/benchmark 設計。
- Scheduler 尚未驗證 proof 與 task/source/input/output/budget binding，transport cap、bounded verifier process、deadline與 admission limits亦尚未接入，因此仍不可發布或 enforce。
- C: Docker VHD 空間不足，prover 改走 WSL/native Linux 並將 artifacts/TMP 放 D:；既有 Docker stack 已恢復且保持 healthy。

## Next action

先寫 scheduler proof-gate RED 測試，驗證成功前不得完成任務或結算。

## Next checkpoint

Scheduler 對 proof 缺漏、無效、重播與 task/source/input/output/budget mismatch 全部 fail closed，且只使用 verified claim 的 usage/output 寫入結算。

## Notes

- Docker 測試 stack 仍供使用者測試，不停止、不清理。
- 不 push；每個完成切片各自本機 commit。

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

階段 3：定義 protobuf proof transport 與 Nodepool 獨立 verifier。

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
- Guest image id 為 `[506971590, 3534501277, 2979422208, 3812948145, 3156049081, 3116419688, 526806072, 1153593187]`。
- zkVM tests 2 passed；真實 proving 約 570–580 秒；fmt 與 clippy `-D warnings` passed。
- Worker proof envelope 已固定 `risc0-zkvm-3.0.6` scheme、guest image id、journal 與完整 receipt，並支援有遞迴深度限制的 JSON round-trip。
- Verifier 先拒絕錯誤 scheme、image id 或 envelope/receipt journal 不一致，再驗證 receipt，最後才解析 execution claim。
- 新 envelope 的真實 proof round-trip 通過；proving 570.02 秒，claim 與 native runtime 相同。

## Active owners

- Origin: 使用者
- Coordinator/implementer: Codex

## Blockers

- RISC Zero 3.0.6 lockfile 有 2 個 transitive advisories：`rsa` timing side-channel（經 rzup，無修正版）與舊 `tracing-subscriber` ANSI log injection（經 ark-relations）。發布前需隔離或建立明確 audit policy。
- 單次 proving 約 9.5 分鐘，現階段不可直接啟用 enforce；需後續 timeout/queue/benchmark 設計。
- C: Docker VHD 空間不足，prover 改走 WSL/native Linux 並將 artifacts/TMP 放 D:；既有 Docker stack 已恢復且保持 healthy。

## Next action

以 RED 測試定義 protobuf transport 與 Nodepool 獨立 verifier；此 checkpoint 仍不接入結算。

## Next checkpoint

Nodepool 能從 protobuf envelope 還原 proof、使用可信 image id 驗證，並比對 task/source/input/output/budget binding；尚不寫入結算。

## Notes

- Docker 測試 stack 仍供使用者測試，不停止、不清理。
- 不 push；每個完成切片各自本機 commit。

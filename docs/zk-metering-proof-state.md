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

階段 2：建立 RISC Zero 3.0.6 Docker guest 與 host/guest golden vector。

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

## Active owners

- Origin: 使用者
- Coordinator/implementer: Codex

## Blockers

- 無立即 blocker；Linux Docker builder 可用，但 builder image digest 尚未固定。

## Next action

建立獨立的 RISC Zero methods/guest workspace，先寫 guest journal 與 native runtime claim 相同的 RED test。

## Next checkpoint

zkVM guest 能執行最小 managed function 並輸出與 host runtime 相同的 public claim。

## Notes

- Docker 測試 stack 仍供使用者測試，不停止、不清理。
- 不 push；每個完成切片各自本機 commit。

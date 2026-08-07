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

階段 2：評估並固定成熟 zkVM backend、toolchain 與可重現 guest image build。

## Completed

- 盤點現有 receipt/checksum/JWT/settlement 路徑。
- 確認目前沒有 ZKP 或 receipt signature verification。
- 選擇成熟 zkVM 作為完整執行證明方向。
- 持久化五階段實作計畫。
- 以 RED→GREEN 完成 backend-neutral public execution claim。
- Journal 綁定 protocol/runtime/cost-model、task id、source/input/output SHA-256、budget 與 execution metrics。
- Proof crate 3 tests、clippy、fmt、audit 與 GNU workspace 246 tests 通過。

## Active owners

- Origin: 使用者
- Coordinator/implementer: Codex

## Blockers

- 本機 Windows 未安裝 zkVM toolchain；第二階段需解決 Linux/reproducible guest builder。

## Next action

確認 RISC Zero／SP1 等候選的 host/guest 相容性，固定一個 backend 與版本，先寫 guest golden-vector RED test。

## Next checkpoint

zkVM guest 能執行最小 managed function 並輸出與 host runtime 相同的 public claim。

## Notes

- Docker 測試 stack 仍供使用者測試，不停止、不清理。
- 不 push；每個完成切片各自本機 commit。

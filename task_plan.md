# .hmf 取代 Monty 執行路徑計劃

## 目標

讓 `managed-function-v0`（`.hmf`）成為 Hivemind 的正式 managed task runtime，完成使用者預算驅動的逐 operation 計價，遷移現有可支援任務，最後移除 worker/runtime 對 Monty 的正式依賴。

## 成功條件

- `.hmf` 的計價單位由 evaluator 逐個 primitive operation/function call 累加。
- 任務預算由 API/task contract 傳到 scheduler 與 worker；不再使用硬編碼的 runtime 預設計價上限。
- 使用量用盡時以結構化 `budget_exhausted` 結果結束；成功與失敗都能產生可結算 receipt。
- 現有 managed-function templates 全部可執行，並有 golden/negative/limit tests。
- legacy task 若仍保留相容期，必須明確遷移；正式切換後 worker 不再依賴 Monty executable。
- `cargo test`、`cargo check`、release build 與 worker smoke test 通過。

## 階段

1. **基線與契約（進行中）**
   - 固化目前 runtime、worker、scheduler、API、DB、proto 與 Monty 使用點。
   - 定義 operation taxonomy、單價、receipt schema、預算與 settlement 語意。
   - 保存目前測試基線。

2. **.hmf 語言與計價核心**
   - 以測試先行建立每種 primitive operation 的 deterministic unit count。
   - 修正 UTF-8、負數、overflow、source location 與 parser error。
   - 補齊 `.hmf` 實際 migration 所需的 expressions/statements。
   - 加入 fuzz/property/golden tests。

3. **預算與執行契約整合**
   - API、models、proto、scheduler、worker 傳遞使用者選定的預算/單價版本。
   - worker 按使用量消耗預算，不使用 `ExecutionLimits::default()` 作為商業限制。
   - 完成 budget exhausted、runtime error、partial receipt 與 retry 語意。

4. **計價與可信度**
   - nodepool/scheduler 以 receipt 結算，不信任 caller 任意聲稱的費用。
   - 加入 reservation、settlement、refund/failed-task 規則。
   - receipt schema versioning、hash/source identity 與跨版本相容測試。

5. **任務遷移與雙跑驗證**
   - 將現有適用 Monty task 改為 `.hmf` 或明確標示不可遷移。
   - 建立同輸入的 Monty/HMF 結果比較工具與 fixtures。
   - 進行 performance、結果一致性、資源消耗與 billing 差異驗證。

6. **切換與移除 Monty**
   - 將 managed task 設為預設/唯一正式 execution path。
   - 移除 worker 的 `MONTY_EXECUTABLE`、process spawn、Monty artifact 與 Docker build 依賴。
   - 更新 docs、templates、package scripts、CI 與部署檔。
   - 執行完整 workspace gate，確認 git diff 與 release artifact 不再包含正式 Monty 路徑。

## 當前 checkpoint

- 狀態：`running`
- 當前步驟：基線與計價契約設計
- 下一步：先為 usage budget / operation accounting 寫 failing tests，再修改 runtime API。
- Blocker：尚未決定對既有 Python/Monty 任務的完整語言遷移範圍；先以目前 Hivemind managed templates 作為第一個可替換集合。

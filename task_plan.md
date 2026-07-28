# .hmf 取代 Monty 執行路徑計劃

## 目標

讓 `managed-function-v0`（`.hmf`）成為 Hivemind 的正式 managed task runtime，完成使用者預算驅動的逐 operation 計價，遷移現有可支援任務，最後移除 worker/runtime 對 Monty 的正式依賴。

## 成功條件

- `.hmf` 的計價單位由 evaluator 逐個 primitive operation/function call 累加。
- `.hmf` v1 的核心語意具備圖靈完備能力；實際執行以使用者 budget 作為可中止的 step budget。
- worker 以固定 IR command 執行，每 100 個 commands 形成一個 settlement chunk 並回報 nodepool。
- nodepool 在放行下一個 chunk 前，以 cost table 中單一 command 的最高可能成本乘以 100，檢查餘額是否足夠；不足則停止該 task。
- 每個 chunk 必須附帶可由 nodepool 驗證的 zero-knowledge execution proof。
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
   - 將 parser AST 編譯成版本化、deterministic、可計價的 `.hmf` IR；command 定義以 IR instruction 為準，不以 source text statement 為準。
   - 補齊圖靈完備核心：可變 binding、`while`、recursion、可擴張 heap/list/dict；固定 worker 資源只是實際執行限制，不是語言語意限制。

3. **預算與執行契約整合**
   - API、models、proto、scheduler、worker 傳遞使用者選定的預算/單價版本。
   - worker 按使用量消耗預算，不使用 `ExecutionLimits::default()` 作為商業限制。
   - 完成 budget exhausted、runtime error、partial receipt 與 retry 語意。

4. **計價與可信度**
   - nodepool/scheduler 以 receipt 結算，不信任 caller 任意聲稱的費用。
   - 加入 reservation、settlement、refund/failed-task 規則。
   - receipt schema versioning、hash/source identity 與跨版本相容測試。
   - 建立 100-command chunk protocol：chunk index、pre-state root、post-state root、command count、usage units、cost table version、balance reservation 與 proof。
   - nodepool 只在前一 chunk proof 驗證成功且下一 chunk 的 worst-case reserve 足額時放行下一 chunk；worst-case reserve = `max_command_cost(cost_table) * 100`，最後不足 100 commands 的 chunk 按實際 command count 計算。
   - chunk 完成後按實際 usage settlement，釋放 worst-case reserve 與實際費用的差額；所有 reserve/settlement 必須 idempotent。
   - ZK proof 的 public inputs 至少包含 task/source/IR hash、runtime/cost-table version、chunk index、pre/post state commitment、command count、usage units；witness 是 execution trace，不把完整 trace 交給 nodepool。
   - 先 benchmark proof generation、verification、proof size、memory 與 100-command throughput；未達 worker/nodepool SLA 前不得切 production。

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

## 語法支援矩陣

### P0：取代 Hivemind 現有 managed/Python task 所需

- literals：`null`、bool、signed integer、bounded float、string、list、tuple、dict。
- expression：parentheses、unary `+/-/not`、arithmetic、comparison、equality、short-circuit `and/or`、conditional expression、membership `in/not in`。
- access：list/string/dict indexing、safe/strict missing-key semantics、slice（若資料處理 task 需要）。
- statements：expression、assignment、augmented assignment、`if/elif/else`、`for`、`while`、`break`、`continue`、`return`、`pass`。
- functions：多 statement body、nested function、recursion、closure、positional/keyword arguments、default arguments、明確 arity error。
- collections：literal、unpacking、comprehension（list/dict；每次迭代必須計價）。
- output/errors：`print`、structured error、line/column、call trace、deterministic serialization。
- builtins：至少 `len`、`get`/index、`contains`、numeric conversion/aggregation、JSON-compatible helpers；每個 builtin 都要有版本化 cost。

### P1：需要接近 Monty 行為時支援

- `try/except/finally`、`raise`、`assert`。
- attribute/method call，以及受控的 string/list/dict methods。
- `*args`、`**kwargs`、keyword-only parameters、tuple unpacking。
- `float`/numeric promotion、bytes（若 task contract 暴露 binary data）。
- `async def`、`await`、host callback suspension/resume。
- 受控標準庫：`json`、`math`、`re`、`datetime`、`typing`、必要的 `sys`；禁止任意 import。
- type annotations/type checking（若要取代 Monty CLI 的 `--type-check` 能力）。

### 明確不納入第一個 replacement release

- classes/metaclasses、reflection、dynamic `eval/exec`。
- arbitrary third-party packages。
- filesystem、network、environment、subprocess；若日後需要，必須走 capability API。
- interactive REPL；Hivemind worker 只需要 deterministic task execution。

### 語法與語意必須先凍結

- `.hmf` grammar version（例如 `hmf/v1`）、operator precedence、evaluation order、scope/closure rules。
- numeric overflow、division by zero、NaN/Infinity、Unicode、missing key/index、duplicate dict key 的規則。
- source file size、AST depth、literal/container size 與 output serialization contract。
- 每個 statement、expression、builtin、function call、loop iteration、allocation 的 billing cost 與 cost-table version。

## 補充的設計檢查點

- **Budget vs safety**：`max_cpt` 是使用者選的計價預算；另需定義 worker infrastructure watchdog（CPU time、memory、output、parse size），不可把它混成商業額度。
- **Settlement**：執行前 reservation、執行中 budget exhaustion、執行後 receipt settlement、失敗/重試/redispatch 的扣款與退款規則必須明確且 idempotent。
- **可信度**：receipt 必須包含 source hash、runtime/cost-table version、usage units、failure state；nodepool 要驗證 task identity、assigned worker 與 receipt schema，不能只信 worker 自報金額。
- **Partial execution**：budget 用盡時是否保存 partial output、是否可 retry、retry 是否重新計費，需固定語意。
- **Input/output**：JSON type mapping、float/bytes 支援、output canonicalization、UTF-8、最大 payload 與 artifact 參照方式要在 proto/API 文件中定義。
- **Host capabilities**：明確選擇 pure deterministic runtime 或 capability-call runtime；若開放 callback，必須定義 auth、suspend/resume、cost、timeout 與 snapshot。
- **Compatibility/migration**：建立 Monty feature inventory、`.py`→`.hmf` migration validator/transpiler、unsupported diagnostic、golden corpus、dual-run 差異分類與 rollback/canary plan。
- **Operations**：metrics/log fields、receipt retention、billing audit、runtime version rollout、old receipt replay、跨 worker version compatibility。
- **Testing**：parser negative tests、property/fuzz tests、resource exhaustion tests、differential tests、cross-platform deterministic tests、benchmark 與 release artifact scan。
- **ZK correctness**：valid trace 可證明、錯誤 opcode/成本/狀態轉移/跨 chunk state/重播 chunk 必須被拒絕；proof key、circuit/IR version 與 verifier rollout 必須可治理。
- **Chunk failure semantics**：chunk 中途 budget 不足、worker 掉線、proof invalid、nodepool timeout、redispatch、最後不足 100 commands 的計費與退款規則必須固定。

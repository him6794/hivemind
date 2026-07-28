# .hmf 取代 Monty 工作進度

## 狀態

- overall: `running`
- current_stage: `design-audit-after-budget-wireup`
- owner: Codex
- last_checkpoint: 建立計劃前已確認 runtime 與 worker 基線測試通過

## 已完成

- 確認 `.hmf` runtime 已有 lexer/parser/evaluator、JSON input、receipt 與 worker branch。
- 確認目前 runtime crate 測試 8 passed、transpiler 測試 4 passed。
- 確認 `hivemind-worker-executor` `cargo check` 通過。
- 確認 worker 目前固定呼叫 `ExecutionLimits::default()`，這是計價模型整合的第一個必改點。
- 新增 runtime `usage_units` receipt 欄位、可選 `max_usage_units` 與 `ExecutionLimits::unlimited()`。
- 新增 runtime budget exhaustion 測試並通過；`budget_exhausted` 已成為結構化錯誤碼。
- 新增 `ExecuteTaskRequest.managed_budget_units`，scheduler 會由 managed task 的 `max_cpt` 傳給 worker。
- worker 已將 managed budget 傳入 evaluator，並將 evaluator failure 轉為 managed failure receipt。
- scheduler managed settlement 已改用 `base_invocation_cpt + usage_units`，不再按 output bytes 或 1,000-op block 計價。
- master API 已要求 managed runtime 同時提供 `task_source` 與正數 `max_cpt`，並拒絕未知 runtime 名稱。
- 完成 replacement scope 審查：補上 P0/P1/非目標語法矩陣，以及 budget、settlement、receipt trust、I/O、capability、migration、rollback、operations、testing 設計點。
- 新增圖靈完備核心目標：`while`、可變 binding、recursion、可擴張 heap；實際執行由 budget 中止。
- 新增 100-command chunk settlement 與 worst-case reserve 規則：下一 chunk 需先通過 `max_command_cost * 100` 餘額檢查。
- 新增每個 chunk 的 zero-knowledge execution proof、state commitment、cost-table version 與 proof benchmark 門檻。

## 下一個動作

1. 將 P0 grammar 轉成 parser/evaluator failing tests，先補 `null`、unary、logical、indexing、multi-statement function。
2. 定義並測試版本化 `.hmf` IR 與 command cost table，確認圖靈完備核心的 step semantics。
3. 實作 100-command chunk protocol、worst-case reserve、actual settlement 與 failure/retry semantics。
4. 建立 ZK circuit/prover/verifier spike 與 proof benchmark，再接入 nodepool authorization。
5. 建立 `.hmf` 語言 migration fixtures，對照現有 Monty-compatible task 能力。
6. 逐步把 worker legacy execution 改為 `.hmf`，再移除 Monty executable。

## Verification

- `executor-rs`: `cargo test -p managed-function-runtime` — 10 passed。
- `hivemind-rs`: `cargo check -p hivemind-task-scheduler` — passed。
- `hivemind-rs`: `cargo check -p hivemind-worker-executor` — passed。
- `hivemind-rs`: `cargo check -p hivemind-master-api` — passed。
- Master API and worker unit-test linking is blocked by the existing Windows linker error `__mingw_fprintf_cgo_beginthread`; this is environment/toolchain linkage, not a Rust compile error.

## Blockers / 風險

- 現有 `.hmf` 是自訂語法，不能直接執行現有 Python/Monty source；需以 managed templates 定義第一批 migration scope。
- 移除 Monty 前必須確認 legacy ZIP/Python 任務是否仍需相容；不能無聲破壞既有 task contract。

# Backend Service Progress

最後更新：2026-03-25

本文件整理「先做後端服務」的模組進度，並標記本輪實作範圍：
- ✅ 依要求已做：1, 2, 3, 4, 6, 7, 8（其中 3/8 為基礎版）
- ⏭️ 依要求暫不做：5（Rust Python sandbox 核心）

---

## 模組清單與狀態

## 1) 使用者註冊與帳號流程（安全強化版）✅

已完成：
- Nodepool HTTP 新增 `POST /api/register`
- Master HTTP 新增 `POST /api/register`（代理到 nodepool HTTP）

目前行為：
- 使用者可建立新帳號（username/password）
- 已改為 bcrypt 儲存密碼
- 舊資料庫若仍是明碼，使用者成功登入後會自動升級成 bcrypt 雜湊

---

## 2) Worker ingress 驗證（authN/authZ）✅

已完成：
- Nodepool `TaskOutputUpload`/`TaskResultUpload`/`TaskUsage` 新增 token 驗證
- 驗證規則：
  - token 必填（可由 `NODEPOOL_WORKER_INGRESS_AUTH` 關閉）
  - token 需可解 JWT
  - token username 需對應到任務 `WorkerID`（若 `WorkerID` 有值）

補齊配套：
- Worker 端自動登入 nodepool 取得 token（`WORKER_PASSWORD`，預設 `worker123`）
- Worker 上報 output/result/usage 會自動帶 token

---

## 3) host_count 多 worker 派發（基礎 fanout）⚠️

已完成（基礎版）：
- UploadTask 支援按 `host_count` 嘗試 fanout 派發到多個 worker
- 會把額外派發結果寫入 task log（`[HOST_FANOUT] ...`）
- 若部分失敗，會留下 partial dispatch 訊息

目前限制：
- 仍是同一 `task_id` 多 worker 執行（尚未拆分 shard/subtask）
- 停止任務與重派策略仍以主要 route 為核心，尚未完整多 worker 協調

---

## 4) 移除 worker 模擬執行回退 ✅

已完成：
- 移除「沒接 executor 時的退化模擬流程」
- 現在一律走外部 executor；若無可用 executor 會回報失敗而非偽造成功結果

---

## 5) Rust Python sandbox 核心 ⏭️（本輪依要求不做）

狀態：
- 本輪依指示不做

補充（已完成串接，不含完整業務沙盒流程）：
- `executor-cli` 已支援 `pydantic/monty` 後端串接模式（`EXECUTOR_SANDBOX_BACKEND=monty`）
- Monty 執行失敗可選擇回退原生後端（`EXECUTOR_MONTY_FALLBACK_NATIVE=true`）
- 使用 `EXECUTOR_MONTY_PYTHON_CMD` 指定 Python 執行命令
- 這是「執行後端切換能力」；真正任務包下載/解壓/main.py 工作流仍待後續完成

---

## 6) Worker 任務持久化（基礎版）✅

已完成：
- Worker `TaskService` 新增可選檔案持久化
- 設定 `WORKER_TASK_PERSIST_PATH` 後，會：
  - 啟動時讀取任務資料
  - 任務更新後寫回檔案

目前限制：
- JSON 檔案持久化（非 DB）
- 未實作 WAL/鎖檔機制

---

## 7) CPT 轉帳帳本查詢 API ✅

已完成：
- Nodepool HTTP 新增 `GET /api/transfers`
  - 支援 `limit`、`task_id`
  - 回傳 `cpt_transfers` 中與當前 user 有關資料（payer/payee）
- Master HTTP 新增 `GET /api/transfers`（代理到 nodepool HTTP）

---

## 8) 後端工程化與觀測（本輪做基礎）⚠️

已完成（基礎）：
- 派發失敗分類碼（`NO_WORKER/PROBE_FAIL/DIAL_FAIL/EXEC_FAIL/REJECTED`）
- 分類碼不只在狀態，也同步寫入 task log
- pre-dispatch probe 已上線（可由環境變數控制）
- HTTP request-id / latency 觀測日誌已接入：
  - Nodepool HTTP
  - Master HTTP
  - Worker control HTTP

未完成（後續）：
- 結構化日志（slog/zap）
- 指標與 trace
- CI/CD pipeline 與部署模板
- Redis/Kafka/PostgreSQL 真正落地

---

## 新增/調整的主要環境變數

- `NODEPOOL_WORKER_INGRESS_AUTH`（預設 true）
- `NODEPOOL_PRE_DISPATCH_PROBE`（既有，預設 true）
- `NODEPOOL_TRANSFERS_LIMIT`（預設 100）
- `NODEPOOL_HTTP_BASE`（Master 代理 nodepool HTTP）
- `WORKER_PASSWORD`（worker 自動登入拿 token）
- `WORKER_TASK_PERSIST_PATH`（啟用 worker 任務持久化）
- `EXECUTOR_SANDBOX_BACKEND`（`native` 或 `monty`）
- `EXECUTOR_MONTY_PYTHON_CMD`（預設 `python`）
- `EXECUTOR_MONTY_FALLBACK_NATIVE`（Monty 失敗時回退 native，預設 true）

---

## 驗證

- 當前 workspace 測試：`passed=18, failed=0`
- 主要變更檔案均無編譯/語法錯誤

---

## 下一步建議（後端）

1. host_count 升級為「subtask 拆分與聚合回收」
2. 帳號密碼改為 bcrypt/argon2，補 Register 的 gRPC 版本
3. transfers 增加彙總查詢（按 task / 按時間區間）
4. worker 持久化升級到 SQLite + migration
5. 加入 observability（metrics/tracing）與 CI 流程

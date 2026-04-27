# HiveMind RPC Contract Notes（開發者備忘）

> 目的：補齊「proto 有了，但實作端與語意容易踩坑」的地方。
> 來源：`node_pool/nodepool.proto` + NodePool/Worker/Master 的實作。

---

## 1. 角色與連線方向（誰是 client / server）

### 1.1 NodePool

- 提供 gRPC server（`node_pool/node_pool_server.py`）。
- 對外暴露：
  - `UserService`
  - `NodeManagerService`
  - `MasterNodeService`
  - `WorkerNodeService`（此端主要用途是接收 worker 回傳 output/result/usage）

### 1.2 Worker

- 同時扮演：
  - **gRPC server**：提供 `WorkerNodeService.ExecuteTask`（`worker/src/hivemind_worker/grpc_servicer.py`）
  - **gRPC client**：呼叫 NodePool 的 `UserService/NodeManagerService/MasterNodeService/WorkerNodeService`

### 1.3 Master UI（master_node.py）

- gRPC client：只連 NodePool。
- 主要呼叫：`UserService.Login/GetBalance/RefreshToken` + `MasterNodeService.UploadTask/GetAllUserTasks/GetTaskResult/StopTask/GetTasklog`

---

## 2. 已知「語意陷阱」與建議

## 2.1 `WorkerNodeService` 名稱混淆（高）

- 同名 service 在 worker/nodepool 兩端都有實作。
- 建議：
  - 文檔中固定用語：
    - **Worker-facing WorkerNodeService**：worker 提供 ExecuteTask。
    - **NodePool-ingest WorkerNodeService**：nodepool 接收 output/result/usage。
  - 長期：拆 service 名稱。

## 2.2 `TaskUsageRequest.token` 欄位被用作 `node_id`（高）

- NodePool `WorkerNodeService.TaskUsage` 中有特殊規則：`task_id == "0"` 代表節點層級上報。
- 此情況下，NodePool 端用 `request.token` 當作 `node_id`。
- Worker 端目前確實把 `token` 塞 `username`（而非 JWT），來讓 nodepool 找到節點。

風險：
- token 欄位命名誤導，且與 JWT token 概念混在一起。

建議：
- proto 增加新欄位 `node_id` 或 `reporter_id`，並逐步淘汰濫用 token。

## 2.3 `RegisterWorkerNode` 被當作 heartbeat refresh（中高）

- Worker 每 30 秒左右會呼叫 `_refresh_registration()`，其實是再打一遍 `RegisterWorkerNodeRequest`。
- NodePool 端 register 寫入大量欄位（含 total/available），即使有「保留 available」的嘗試，也可能導致競態或資料覆寫。

建議：
- 增加獨立 Heartbeat RPC（僅更新 `last_heartbeat/updated_at`）。

## 2.4 計量單位建議（中）

- `RunningStatusRequest.cpu_usage/memory_usage/gpu_usage/...`：程式碼多處以「百分比」使用。
- `TaskUsageRequest.*_usage`：NodePool 端會 clamp 到 0-100。

建議：
- 在 proto 註解或欄位名明確標示為 percent。

---

## 3. RPC 超時與大檔案限制（觀察）

- NodePool gRPC server 與 Master/Worker client 端都設定 `grpc.max_receive_message_length` / `grpc.max_send_message_length` 到 ~1GB。
- Master UI 對單檔案限制：1000MB（`master_node.py` 上傳檢查）。
- Worker ExecuteTask 檔案限制：1500MB（`grpc_servicer.py` 檢查）。

建議：
- 若要一致：把上限集中成單一 config 常數，避免 client/server 不一致。

---

## 4. 建議新增（以提升可維護性）

- 在 docs 中明確列出：
  - 哪些 RPC 需要 `authorization: Bearer <jwt>`
  - 哪些 RPC 目前未強制驗證（如果要上線，應補齊）
- 建議引入 gRPC interceptor 做統一 logging（method、peer、latency、code）。

# HiveMind 開發者架構文檔（以程式碼為準）

> 本文件以 repo 內現有程式碼行為為準（主要來源：
> `node_pool/`、`worker/src/hivemind_worker/`、`master/hivemind_master/src/hivemind_master/master_node.py`、`node_pool/nodepool.proto`）。
> 
> 範圍聲明：
> - 本文件**不**保證依賴版本、部署腳本、或雲端端點可用性。
> - 本文件著重：模組責任、RPC 契約、資料/狀態模型、執行流程與可證明的風險。

---

## 1. Repo 模組地圖（高層）

- `node_pool/`
  - 控制平面（Control Plane）。
  - 提供 gRPC：使用者登入/餘額、Worker 註冊與狀態回報、任務儲存/查詢/停止/結果下載。
  - 依賴 Redis 作為節點與任務的主要狀態儲存；使用 SQLite（`users.db`）保存使用者/餘額/信用等。

- `worker/`
  - 計算節點（Data Plane）。
  - 同時包含：
    - gRPC server：接受外部呼叫 `WorkerNodeService.ExecuteTask` 以啟動任務。
    - gRPC client：把任務輸出/結果/使用率回傳到 NodePool。
    - Flask UI +（可選）WebView 殼：登入、監控、設定效能上限。

- `master/`
  - 目前 repo 中可執行的「Master UI」是 `master/hivemind_master/src/hivemind_master/master_node.py`。
  - 它本質上是 NodePool 的**gRPC client** + 一個 Flask Web UI（port 5002），用來登入、上傳 ZIP 任務、查任務、下載結果、停止任務。
  - 注意：`master/README.md` 內容與 `master_node.py` 的實作不一致（README 談 openapi / /health 等，但程式碼是 Flask UI + gRPC）。

---

## 2. RPC 契約（node_pool/nodepool.proto）

### 2.1 Services

- `UserService`
  - `Login`, `Transfer`, `GetBalance`, `RefreshToken`

- `NodeManagerService`
  - `RegisterWorkerNode(RegisterWorkerNodeRequest) -> StatusResponse`
  - `ReportStatus(RunningStatusRequest) -> RunningStatusResponse`

- `MasterNodeService`
  - `UploadTask(UploadTaskRequest)`
  - `GetTaskResult(GetTaskResultRequest)`
  - `GetAllUserTasks(GetAllUserTasksRequest)`
  - `StopTask(StopTaskRequest)`
  - `GetTasklog(TasklogRequest)`

- `WorkerNodeService`
  - `ExecuteTask(ExecuteTaskRequest)`
  - `TaskOutputUpload`, `TaskResultUpload`, `TaskOutput`, `StopTaskExecution`, `TaskUsage`

### 2.2 重要現況：同名 Service 的雙向用途

- Worker 端啟動 gRPC server 並註冊 `WorkerNodeServiceServicer`（見 `worker/src/hivemind_worker/worker_node.py` + `grpc_servicer.py`），用於接收 `ExecuteTask`。
- NodePool 端也註冊 `WorkerNodeServiceServicer`（見 `node_pool/worker_node_service.py`），但此端的 `ExecuteTask` 是 stub；核心用途在：
  - 接收 worker 回傳的 `TaskOutputUpload` / `TaskResultUpload` / `TaskUsage`

這會造成新進開發者容易混淆「到底誰提供 WorkerNodeService」。

---

## 3. NodePool（控制平面）

### 3.1 gRPC server 入口

- `node_pool/node_pool_server.py`
  - 建立 gRPC server，註冊：
    - `UserServiceServicer`（`user_service.py`）
    - `NodeManagerServiceServicer`（`node_manager_service.py`）
    - `MasterNodeServiceServicer`（`master_node_service.py`）
    - `WorkerNodeServiceServicer`（`worker_node_service.py`）

### 3.2 節點狀態模型（Redis）

- 節點 key：`node:<node_id>`
- 主要欄位（部分）：
  - 身份/連線：`hostname`、`port`、`location`、`gpu_name`、`docker_status`
  - 心跳：`last_heartbeat`、`updated_at`
  - 資源總量/可用量：
    - `total_cpu_score` / `available_cpu_score`
    - `total_memory_gb` / `available_memory_gb`
    - `total_gpu_score` / `available_gpu_score`
    - `total_gpu_memory_gb` / `available_gpu_memory_gb`
  - 任務：`current_tasks`、`running_task_ids`
  - 信任：`credit_score`、`trust_level`

來源：`node_pool/node_manager.py: register_worker_node`、`update_node_usage`、`get_node_list`。

### 3.3 任務狀態模型（Redis + 檔案系統）

- 任務 key：`task:<task_id>`
- 任務 ZIP / 結果 ZIP：由 `FileStorageManager` 存到 `TASK_STORAGE_PATH` 底下（見 `node_pool/master_node_service.py`）
- 任務狀態（從程式碼可見至少包含）：`PENDING`、`RUNNING`、`COMPLETED`、`STOPPED`

### 3.4 Worker 回報資料入口（NodePool 的 WorkerNodeService）

- `node_pool/worker_node_service.py`
  - `TaskOutputUpload`：把 output append 到 Redis（TaskManager.store_output）
  - `TaskResultUpload`：結果 zip 寫硬碟，更新狀態，並呼叫 `NodeManager.release_node_resources(...)`（釋放資源）
  - `TaskUsage`
    - 若 `task_id == "0"`：視為節點層級上報，使用 `request.token` 當 `node_id`
    - 否則：更新特定任務 `task:<id>` 的即時使用率

---

## 4. Worker（計算節點）

### 4.1 啟動與內部結構

- 入口：
  - `worker/main.py` -> `run_worker_node()`
  - `worker/src/hivemind_worker/__main__.py` -> `run_worker_node()`

- `WorkerNode` 初始化（`worker/src/hivemind_worker/worker_node.py`）會做：
  - 硬體偵測、Docker 偵測
  - gRPC client 連 NodePool（`worker/src/hivemind_worker/grpc_client.py:init_grpc`）
  - Flask UI 初始化（`worker/src/hivemind_worker/webapp.py`）
  -（可選）VPN / auto update thread
  -（可選）自動登入與自動註冊

### 4.2 登入與註冊

- `WorkerNode._login`：呼叫 NodePool `UserService.Login` 取得 token，並 `GetBalance` 更新餘額。
- `WorkerNode._register`：呼叫 NodePool `NodeManagerService.RegisterWorkerNode`。
  - 注意：request 的 `node_id` 被設定成 `self.username`（也就是 user name）。

### 4.3 心跳與狀態回報（worker/src/hivemind_worker/heartbeat.py）

- 迴圈頻率：每秒 tick。
- 節點已註冊且有 token 時：
  - 每 ~30s：`_refresh_registration()`（實作上是再次呼叫 `RegisterWorkerNode` 作為心跳刷新）
  - 每 ~60s：`_update_balance()`
  - 每 ~10s 或 CPU/MEM 有顯著變動：`node.node_stub.ReportStatus(RunningStatusRequest)`
  - 每 ~15s：`node.worker_stub.TaskUsage(task_id="0")` 上報節點層級使用率

### 4.4 任務接收與執行（Worker 的 WorkerNodeService）

- `worker/src/hivemind_worker/grpc_servicer.py: WorkerNodeServicer.ExecuteTask`
  - 會做資源檢查、ZIP 驗證、資源分配，然後開 thread 執行 `worker_node._execute_task(...)`。

---

## 5. Master（目前 repo 內的 Master UI）

### 5.1 實際入口與角色

- 檔案：`master/hivemind_master/src/hivemind_master/master_node.py`
- 角色：NodePool gRPC client + Flask UI（預設 port 5002）
  - 登入：`UserService.Login`
  - 上傳任務：`MasterNodeService.UploadTask`
  - 查任務：`MasterNodeService.GetAllUserTasks`
  - 停止任務：`MasterNodeService.StopTask`
  - 下載結果：`MasterNodeService.GetTaskResult`
  - 查任務 logs：`MasterNodeService.GetTasklog`

### 5.2 Token 維護策略（程式碼可見）

- Master UI 內維護 `user_list`（多使用者），每個 user 保存 token + login_time。
- 若 token 年齡 > 50 分鐘會主動 `RefreshToken`。

---

## 6. 已知風險與建議（可由程式碼證明/高度一致）

### 6.1 Service 命名與責任混淆（高）

- `WorkerNodeService` 同名 service 在 worker/nodepool 兩側都實作，但用途不同。
- 建議：
  - 文檔層面先強制畫清：
    - worker 端提供 ExecuteTask
    - nodepool 端提供 output/result/usage ingest
  - 長期可拆分 proto service 名稱以降低認知成本。

### 6.2 身份/授權未一致落實（高）

- worker 端大量 RPC 帶 `authorization: Bearer <token>` metadata，但 node_pool 的部分 RPC 實作未強制驗證。
- `TaskUsage(task_id=0)` 把 `token` 欄位當 node_id 使用（命名與用途不符）。
- 建議：
  - 統一所有需要身份的 RPC：從 metadata 驗 JWT，並把 JWT 的 username / node_id 與 request 內容對齊。
  - 規劃 proto 破壞性調整：`TaskUsageRequest` 增加明確欄位 `node_id`，避免濫用 token。

### 6.3 心跳使用 Register 造成資料覆蓋/競態（中高）

- worker 每 30 秒用 `RegisterWorkerNode` 當 heartbeat refresh。
- NodePool 註冊路徑會更新大量欄位（包含 available/total 等），即使嘗試保留，也可能出現 race。
- 建議：
  - 增加獨立 Heartbeat RPC：只更新 `last_heartbeat/updated_at`。

### 6.4 計量單位（% vs MB/GB）易混（中）

- `ReportStatus` / `TaskUsage` 對 memory_usage 的語意應明確規範（目前多處以百分比處理）。
- 建議：統一為百分比並命名為 `*_percent`；或統一為 MB 並全鏈路調整。

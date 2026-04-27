# NodePool 現有功能流程總覽（以程式碼為準）

> 本文件整理 repo 目前「實際存在的功能與端到端流程」，以程式碼行為為準。
>
> 主要參考：
> - `node_pool/nodepool.proto`
> - `node_pool/node_pool_server.py`
> - `node_pool/user_service.py`, `node_pool/user_manager.py`
> - `node_pool/node_manager_service.py`, `node_pool/node_manager.py`
> - `node_pool/master_node_service.py`, `node_pool/worker_node_service.py`
> - `master/hivemind_master/src/hivemind_master/master_node.py`
> - `worker/src/hivemind_worker/*`
>
> 相關文件：
> - `docs/developer-architecture.md`
> - `docs/rpc-contract-notes.md`
>
---

## 1. 角色與連線方向（誰是 client / server）

### 1.1 NodePool（控制平面 / gRPC Server）

- **入口**：`node_pool/node_pool_server.py`（預設 port `50051`）
- **提供的 gRPC services**（見 `nodepool.proto`）：
  - `UserService`
  - `NodeManagerService`
  - `MasterNodeService`
  - `WorkerNodeService`

### 1.2 Master UI（純 client）

- **入口**：`master/hivemind_master/src/hivemind_master/master_node.py`
- **角色**：NodePool 的 gRPC client + Flask UI
- **主要呼叫**：
  - `UserService.Login/GetBalance/RefreshToken`
  - `MasterNodeService.UploadTask/GetAllUserTasks/StopTask/GetTaskResult/GetTasklog`

### 1.3 Worker（同時是 client + server）

- **Worker 作為 gRPC server**：提供 `WorkerNodeService.ExecuteTask`，接受 NodePool 推送任務
- **Worker 作為 gRPC client**：呼叫 NodePool 的 services，上報節點/任務狀態、上傳 output/result

### 1.4 重要現況：`WorkerNodeService` 同名 service 的雙向用途

- **Worker-side WorkerNodeService**：worker 提供 `ExecuteTask` / `StopTaskExecution`（接收控制平面的命令）
- **NodePool-ingest WorkerNodeService**：nodepool 接收 `TaskOutputUpload` / `TaskResultUpload` / `TaskUsage`（接收資料回傳）

這個命名會造成閱讀上的混淆；追流程時請先分清「呼叫方向」。

---

## 2. RPC 對照表（proto -> NodePool 實作位置）

> proto 定義：`node_pool/nodepool.proto`

### 2.1 `UserService`

- `Login` -> `node_pool/user_service.py::UserServiceServicer.Login`
- `Transfer` -> `node_pool/user_service.py::UserServiceServicer.Transfer`
- `GetBalance` -> `node_pool/user_service.py::UserServiceServicer.GetBalance`
- `RefreshToken` -> `node_pool/user_service.py::UserServiceServicer.RefreshToken`

### 2.2 `NodeManagerService`

- `RegisterWorkerNode` -> `node_pool/node_manager_service.py::NodeManagerServiceServicer.RegisterWorkerNode`
- `ReportStatus` -> `node_pool/node_manager_service.py::NodeManagerServiceServicer.ReportStatus`

### 2.3 `MasterNodeService`

- `UploadTask` -> `node_pool/master_node_service.py::MasterNodeServiceServicer.UploadTask`
- `GetTaskResult` -> `node_pool/master_node_service.py::MasterNodeServiceServicer.GetTaskResult`
- `GetAllUserTasks` -> `node_pool/master_node_service.py::MasterNodeServiceServicer.GetAllUserTasks`
- `StopTask` -> `node_pool/master_node_service.py::MasterNodeServiceServicer.StopTask`
- `GetTasklog` -> `node_pool/master_node_service.py::MasterNodeServiceServicer.GetTasklog`

### 2.4 `WorkerNodeService`（NodePool ingest 端）

- `TaskOutputUpload` -> `node_pool/worker_node_service.py::WorkerNodeServiceServicer.TaskOutputUpload`
- `TaskResultUpload` -> `node_pool/worker_node_service.py::WorkerNodeServiceServicer.TaskResultUpload`
- `TaskUsage` -> `node_pool/worker_node_service.py::WorkerNodeServiceServicer.TaskUsage`

（`node_pool/worker_node_service.py::ExecuteTask` 在 nodepool 端是 stub 性質，任務的真正執行在 worker 端。）

---

## 3. 狀態/儲存模型（你追流程最常查的資料）

### 3.1 Redis：節點 key `node:<node_id>`

由 `node_pool/node_manager.py` 維護。常見欄位：

- **身份/連線**：
  - `hostname`, `port`, `location`, `gpu_name`, `docker_status`
- **心跳**：
  - `last_heartbeat`, `updated_at`
- **資源總量/可用量**：
  - `total_cpu_score` / `available_cpu_score`
  - `total_memory_gb` / `available_memory_gb`
  - `total_gpu_score` / `available_gpu_score`
  - `total_gpu_memory_gb` / `available_gpu_memory_gb`
- **任務**：
  - `current_tasks`, `running_task_ids`
- **信任**：
  - `credit_score`, `trust_level`

### 3.2 Redis：任務 key `task:<task_id>`

由 `node_pool/master_node_service.py::TaskManager` 維護。常見欄位：

- **需求**：`memory_gb`, `cpu_score`, `gpu_score`, `gpu_memory_gb`, `location`, `gpu_name`
- **狀態**：`status`（常見：`PENDING`, `RUNNING`, `COMPLETED`, `FAILED`, `STOPPED`）
- **派送**：`assigned_node`
- **歸屬**：`user_id`（現況：多處實作實際存的是 `username`）
- **log/output**：`logs`, `output`
- **即時使用率**：`cpu_usage`, `memory_usage`, `gpu_usage`, `gpu_memory_usage`
- **成本**：`cpt_cost`（上傳時計算，後續每分鐘固定扣款/轉帳）

### 3.3 檔案系統：任務/結果 ZIP

由 `node_pool/master_node_service.py::FileStorageManager` 管理。

- 任務 ZIP：`TASK_STORAGE_PATH/task_<task_id>.zip`
- 結果 ZIP：`TASK_STORAGE_PATH/result_<task_id>.zip`

---

## 4. 端到端主要流程（任務生命週期）

以下用一條主線串起「你看到的所有功能」：

### 4.1 登入與 token 維護（Master UI）

- Master UI -> NodePool `UserService.Login`
- Master UI 內部維護 token 並在接近過期時呼叫 `UserService.RefreshToken`

### 4.2 上傳任務

- Master UI -> NodePool `MasterNodeService.UploadTask`
- NodePool：
  - 驗 token -> 取 username
  - 檢查 task_id 是否重複
  - 檢查 ZIP 大小（>500MB 拒絕）
  - `TaskManager.store_task`：
    - ZIP 寫硬碟
    - 任務 meta 寫 Redis（`status=PENDING`）
    - 計算固定 `cpt_cost`

### 4.3 NodePool 後台派送（dispatcher thread）

- 啟動位置：`MasterNodeServiceServicer.__init__` -> `start_task_dispatcher()`
- 週期：每 `dispatch_interval` 秒（預設 10 秒）
- 核心邏輯：`_dispatch_pending_tasks_once()`

派送步驟：

1. 從 Redis 找出 `PENDING` 任務
2. 依「用戶信任分數」排序（高分優先）
3. 用 `NodeManager.get_available_nodes_by_trust_group(...)` 找到符合條件的節點群組
4. 選節點後，先 `allocate_node_resources(...)`（扣可用資源、登記 running_task_ids）
5. gRPC 連到 worker 的 `hostname:port` 呼叫 `WorkerNodeService.ExecuteTask(task_zip...)`
6. 成功後：
   - `task.status=RUNNING`
   - `task.assigned_node=<node_id>`
   - 記錄 `task_health[task_id]` 供後續健康檢查/資源釋放

### 4.4 Worker 執行期間回報

- Worker -> NodePool `NodeManagerService.ReportStatus`：
  - 更新節點 status/heartbeat + 即時使用率
- Worker -> NodePool `WorkerNodeService.TaskUsage`：
  - `task_id == "0"`：節點層級上報（NodePool 端把 `request.token` 當 `node_id` 使用）
  - `task_id != "0"`：任務層級上報，寫入 `task:<id>` 的 usage 欄位
- Worker -> NodePool `WorkerNodeService.TaskOutputUpload`：
  - append 任務 output 到 `task:<id>.output`

### 4.5 每分鐘扣款/轉帳（reward scheduler thread）

- 啟動位置：`MasterNodeServiceServicer.__init__` -> `start_reward_scheduler()`
- 週期：每 `reward_interval` 秒（預設 60 秒）

流程（針對 RUNNING/EXECUTING 任務）：

1. 讀 `task:<id>.cpt_cost`（固定費率）
2. 檢查發起者是否足夠支付下一分鐘
3. 若不足：
   - `task.status=STOPPED`
   - `node.status=Idle`
   - 記 log（system）
4. 若足夠：
   - `DatabaseManager.transfer_tokens(from=<user_id>, to=<assigned_node>, amount=<cpt_cost>)`

### 4.6 健康檢查（health checker thread）

- 啟動位置：`MasterNodeServiceServicer.__init__` -> `start_health_checker()`
- 週期：每 `health_check_interval` 秒（預設 5 秒）

流程：

1. 掃 `RUNNING` 任務
2. 讀 `assigned_node` 的 `last_heartbeat`
3. 若超時：
   - `task.status=FAILED`
   - `node.status=Idle`
   - 記 log（system）

### 4.7 任務完成與結果回傳

- Worker -> NodePool `WorkerNodeService.TaskResultUpload(result_zip)`
- NodePool：
  - `TaskManager.store_result`：結果 ZIP 寫硬碟，`task.status=COMPLETED`
  - `NodeManager.release_node_resources(...)`：釋放節點資源
  - `task.assigned_node` 會被清空（避免持續顯示占用）

### 4.8 查任務/查 log/停止/下載結果（Master UI）

- `GetAllUserTasks`：
  - token -> username
  - 掃 `task:*` 過濾 owner
  - 回傳 `TaskInfo`（含 usage）

- `GetTasklog`：
  - token -> username
  - 合併 `logs` + `output` 回傳文字

- `StopTask`：
  - token -> username（並查 DB 得 user_id，做雙向 owner 比對）
  - 先把 `task.status=STOPPED`
  - 如有 assigned_node：呼叫 worker `StopTaskExecution(task_id)`

- `GetTaskResult`：
  - 回傳 `result_zip`
  - 目前程式碼中存在「下載後清理 Redis 與延遲清檔」的邏輯（支援 `STOPPED/FAILED` 有部分結果的情境）

---

## 5. 各 RPC 流程細節（摘要版）

### 5.1 UserService

- `Login`：DB 驗證 bcrypt -> 簽 JWT
- `GetBalance`：JWT -> user_id -> DB 查餘額（過期/無效回特殊 message）
- `Transfer`：JWT -> sender -> DB 交易轉帳
- `RefreshToken`：用舊 JWT 解出 user_id -> 簽新 JWT

### 5.2 NodeManagerService

- `RegisterWorkerNode`：寫入 `node:<id>`（含 trust_level 與資源欄位）
- `ReportStatus`：更新 node 狀態/心跳/即時使用率

### 5.3 MasterNodeService

- `UploadTask`：任務 ZIP 存硬碟 + 任務 meta 存 Redis（`PENDING`）
- `GetAllUserTasks`：掃 `task:*` 回傳 list（最多 100）
- `StopTask`：先 STOPPED，再通知 worker 停止
- `GetTasklog`：合併 logs/output
- `GetTaskResult`：回傳 result_zip（並可能觸發任務資料清理）

### 5.4 WorkerNodeService（NodePool ingest）

- `TaskOutputUpload`：append output
- `TaskResultUpload`：存結果 + 釋放資源
- `TaskUsage`：更新 node 或 task 的 usage 欄位

---

## 6. 已知語意陷阱與注意事項（實作層面）

- **`TaskUsageRequest.token` 被拿來當 `node_id`（在 task_id=="0" 時）**：
  - 欄位命名與用途不符，容易跟 JWT token 混淆。

- **`RegisterWorkerNode` 被當作心跳 refresh**：
  - register 會寫很多欄位，雖然嘗試保留 available 資源，但仍可能有競態/覆寫風險。

- **`task.user_id` 實際存的是 username**：
  - 部分邏輯會用 user_id（數字）比對、部分用 username 比對，因此程式碼中常見「雙向比對」處理。

---

## 7. 快速故障定位索引（看 log 時用）

- 任務派送：`node_pool/master_node_service.py::_dispatch_pending_tasks_once`
- 資源扣除/釋放：`node_pool/node_manager.py::allocate_node_resources/release_node_resources`
- 结果落地：`node_pool/master_node_service.py::FileStorageManager` + `TaskManager.store_result`
- worker 回傳入口：`node_pool/worker_node_service.py`
- 任務清理（下載後）：`TaskManager.cleanup_task_data` + `FileStorageManager.delayed_cleanup_task_files`

# HiveMind Developer Runbook（以程式碼為準 / 不含依賴安裝）

> 本文件描述「如何用程式碼可證明的方式」把系統跑起來、驗證核心資料流、以及常見故障定位。
> 
> 範圍聲明：
> - 不包含套件版本或安裝指令（依賴檔未在此工作流核對）。
> - 假設你能自行準備 Redis、Python 環境等。

---

## 1. 服務與預設埠

- NodePool gRPC：`node_pool/node_pool_server.py`
  - 預設：`0.0.0.0:50051`

- Worker Node：`worker/src/hivemind_worker/worker_node.py`（`run_worker_node()`）
  - Worker 自己的 gRPC server：預設 `NODE_PORT=50053`
  - Worker 的 Web UI（Flask）：預設 `FLASK_PORT` 為 5050~6000 隨機
  - Worker 連 NodePool：`NODEPOOL_ADDRESS`（預設在程式碼中為 `172.16.100.148:50051`）

- Master UI（目前 repo 內可用版本）：`master/hivemind_master/src/hivemind_master/master_node.py`
  - Flask UI：`0.0.0.0:5002`
  - 連 NodePool gRPC：`GRPC_SERVER_ADDRESS`（預設 `127.0.0.1:50051`）

---

## 2. 啟動順序（推薦）

### 2.1 啟動 Redis

NodePool 在初始化 `NodeManager()` 時會 `ping()` Redis，失敗會直接 raise。

驗證點：
- NodePool 啟動後不應立刻報 `Redis connection failed`。

### 2.2 啟動 NodePool gRPC

目標檔案：`node_pool/node_pool_server.py`

啟動後預期：
- log 顯示節點池啟動（端口 50051）
- log 顯示任務存儲目錄（`TASK_STORAGE_PATH` 或預設 `/mnt/myusb/hivemind/task_storage`）

### 2.3 啟動 Worker

入口：
- `python -m hivemind_worker`（對應 `worker/src/hivemind_worker/__main__.py`）
- 或直接呼叫 `worker/src/hivemind_worker/worker_node.py: run_worker_node()`

必要設定（至少要確保）：
- `NODEPOOL_ADDRESS` 指到 NodePool（例如 `127.0.0.1:50051`）

啟動後預期：
- Worker 的 gRPC server 在 `NODE_PORT` 監聽
- Worker Web UI 在 `http://127.0.0.1:<FLASK_PORT>`
- 登入成功並註冊成功後：開始心跳/狀態回報（見 `heartbeat.py`）

### 2.4 啟動 Master UI（可選，但方便測試任務流程）

入口：`master/hivemind_master/src/hivemind_master/master_node.py`（`run_master_node()`）

必要設定：
- `GRPC_SERVER_ADDRESS` 指到 NodePool（預設是 `127.0.0.1:50051`）

使用流程：
- 打開 `http://127.0.0.1:5002/login`
- 用 node_pool 的 user 資料登入（實際使用者資料來自 node_pool 的 SQLite）

---

## 3. 核心驗證路徑（黑箱 + 白箱）

### 3.1 驗證 Worker 註冊與心跳

白箱驗證（NodePool/Redis）：
- Redis 中應出現 `node:<username>` key
- `last_heartbeat` 應更新
- `current_cpu_usage/current_memory_usage/...` 應更新（由 worker 回報）

黑箱驗證（Worker 日誌）：
- 看到類似：
  - `Attempting gRPC connect to nodepool address: ...`
  - `Connected to nodepool ...`
  - `Attempting to register node ...`
  - `Registration response received ... Success: True`
  - `Refreshing registration -> target: ...`（每 30 秒）

### 3.2 驗證任務提交（Master -> NodePool）

在 Master UI：
- `/upload` 上傳 zip
- 成功後在 `/` 面板看到 task 出現，狀態先 PENDING

NodePool/Redis：
- 會出現 `task:<task_id>` hash
- `status` 初始通常為 `PENDING`

### 3.3 驗證停止任務 / 下載結果

- Master 呼叫 `MasterNodeService.StopTask`（UI endpoint：`/api/stop_task/<task_id>`）
- Master 下載：`/api/download_result/<task_id>` -> 呼叫 `GetTaskResult`

注意：
- NodePool 的 `GetTaskResult` 在程式碼註解中宣稱支援 STOPPED 狀態。

---

## 4. 常見故障排除（以程式碼行為為準）

### 4.1 NodePool 起不來：Redis 連線失敗

現象：
- `node_pool/node_manager.py` 初始化時 `redis_client.ping()` 失敗 raise。

處置：
- 先確認 Redis 是否在預期位置（程式碼預設 `localhost:6379`）。
- 若你有設定 `REDIS_HOST/REDIS_PORT`，確認 NodePool 程式有使用到（部分程式碼仍硬編 localhost）。

### 4.2 Worker 登入成功但註冊失敗

現象：
- Worker UI 顯示 Login ok 但 Registration failed。

檢查點：
- Worker 是否能連到 NodePool（`NODEPOOL_ADDRESS`）
- NodePool gRPC 是否在 50051

### 4.3 任務結果下載不到

現象：
- Master `/api/download_result/<task_id>` 回 404 或 result_zip 為空。

檢查點：
- NodePool 端是否已收到 `WorkerNodeService.TaskResultUpload`（該 RPC 會把結果 zip 寫硬碟）
- `TASK_STORAGE_PATH` 是否可寫

### 4.4 Token 過期造成 UI 行為不一致

現象：
- Master UI 有做 RefreshToken（>50 分鐘）與 retry；Worker 端目前主要在 GetBalance 時處理 token。

檢查點：
- NodePool `UserService.GetBalance` 回覆 message：`TOKEN_EXPIRED` 或 `INVALID_TOKEN` 時，客戶端是否有 retry。

---

## 5. 開發者建議（最小改動可提升穩定度）

- 將 NodePool 內多處硬編 `redis.Redis(host='localhost', port=6379...)` 改為一致使用 `Config.REDIS_HOST/REDIS_PORT`（目前看起來有混用風險）。
- 將「心跳」從 `RegisterWorkerNode` 拆出獨立 RPC，避免覆蓋資源欄位並降低競態。
- 統一 `memory_usage` 等欄位單位（目前多處以百分比處理，請在 proto/欄位命名上做強約束）。

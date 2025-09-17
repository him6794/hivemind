# Node Pool 模組文檔

## 📋 概述

Node Pool 是 HiveMind 分散式計算平台的核心資源調度系統，基於多層級信任機制，負責管理所有計算節點的註冊、監控、任務分配和資源動態調度。採用 gRPC 協議提供高性能的分散式服務。

## 🏗️ 系統架構

```
┌─────────────────────────────────────────────────────────────┐
│                    Node Pool Service                         │
├─────────────────────────────────────────────────────────────┤
│  User Service     │  Node Manager   │  Master Node Service │
├─────────────────────────────────────────────────────────────┤
│             多層級信任系統 & 動態資源分配                      │
├─────────────────────────────────────────────────────────────┤
│  SQLite Database │  Redis Cache   │  gRPC Services          │
└─────────────────────────────────────────────────────────────┘
        │                    │                    │
   高信任節點群         中信任節點群         低信任節點群
   (信用≥100)          (信用50-99)         (信用<50)
```

## 🔧 核心組件

### 1. Node Pool Server (`node_pool_server.py`)
- **功能**: 主要 gRPC 服務器，多服務整合
- **端口**: 50051 (可配置)
- **協議**: gRPC with Protocol Buffers
- **特性**: 
  - 支援大檔案傳輸 (100MB)
  - 20 個並發工作線程
  - 進階 keep-alive 配置

### 2. Node Manager (`node_manager.py`)
- **功能**: 智慧節點生命週期管理
- **核心特性**:
  - **多層級信任系統**: 基於信用評分的節點分組
  - **動態資源追蹤**: 實時監控總資源和可用資源
  - **多任務支援**: 單節點並行執行多個任務
  - **Docker 感知**: 根據 Docker 狀態調整信任等級
  - **地理位置感知**: 支援按地區優先分配
  - **負載均衡**: 智慧任務分配演算法

### 3. User Service (`user_service.py`)
- **功能**: JWT 基礎的用戶認證服務
- **職責**:
  - 用戶註冊和登入
  - JWT Token 管理
  - CPT 代幣轉帳
  - 餘額查詢

### 4. Master Node Service (`master_node_service.py`)
- **功能**: 任務調度和管理
- **職責**:
  - 任務上傳和分發
  - 執行狀態監控
  - 結果收集
  - 任務停止控制

### 5. Configuration (`config.py`)
- **功能**: 統一配置管理
- **支援**: 環境變數和 .env 檔案

## 🔗 信任等級系統

### 信任等級分類
```python
# 高信任等級 (High Trust)
- 信用評分 >= 100
- Docker 狀態: enabled
- 優先級: 最高

# 中信任等級 (Normal Trust)  
- 信用評分 50-99
- Docker 狀態: enabled
- 優先級: 中等

# 低信任等級 (Low Trust)
- 信用評分 < 50 或 Docker disabled
- 限制: 只接受高信任用戶任務
- 優先級: 最低
```

### 任務分配演算法
1. **信任群組優先**: 高信任 → 中信任 → 低信任
2. **資源匹配**: CPU、記憶體、GPU 需求檢查
3. **負載均衡**: 優先選擇負載較低的節點
4. **Docker 相容性**: 無 Docker 節點需用戶信任分數 > 50
5. **地理位置**: 支援指定地區偏好

## 📡 gRPC 服務定義

### UserService
```protobuf
service UserService {
    rpc Login(LoginRequest) returns (LoginResponse);
    rpc Register(RegisterRequest) returns (RegisterResponse);
    rpc Transfer(TransferRequest) returns (TransferResponse);
    rpc GetBalance(GetBalanceRequest) returns (GetBalanceResponse);
}
```

**主要訊息類型**:
```protobuf
message LoginRequest {
    string username = 1;
    string password = 2;
}

message LoginResponse {
    bool success = 1;
    string message = 2;
    string token = 3;
}

message GetBalanceRequest {
    string username = 1;
    string token = 2;
}

message TransferRequest {
    string token = 1;
    string receiver_username = 2;
    int64 amount = 3;
}
```

### NodeManagerService
```protobuf
service NodeManagerService {
    rpc RegisterWorkerNode(RegisterWorkerNodeRequest) returns (StatusResponse);
    rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
    rpc ReportStatus(ReportStatusRequest) returns (StatusResponse);
    rpc GetNodeList(GetNodeListRequest) returns (GetNodeListResponse);
}
```

**節點註冊訊息**:
```protobuf
message RegisterWorkerNodeRequest {
    string node_id = 1;        // 用戶名
    string hostname = 2;       // IP 地址
    int32 cpu_cores = 3;
    int32 memory_gb = 4;
    int32 cpu_score = 5;
    int32 gpu_score = 6;
    int32 gpu_memory_gb = 7;
    string location = 8;
    int32 port = 9;
    string gpu_name = 12;
    string docker_status = 13;
}

message WorkerNodeInfo {
    string node_id = 1;
    string hostname = 2;
    int32 cpu_cores = 3;
    int32 memory_gb = 4;
    string status = 5;
    double last_heartbeat = 6;
    int32 cpu_score = 7;
    int32 gpu_score = 8;
    int32 gpu_memory_gb = 9;
    string location = 10;
    int32 port = 11;
    string gpu_name = 12;
}
```

### MasterNodeService
```protobuf
service MasterNodeService {
    rpc UploadTask(UploadTaskRequest) returns (UploadTaskResponse);
    rpc PollTaskStatus(PollTaskStatusRequest) returns (PollTaskStatusResponse);
    rpc StoreOutput(StoreOutputRequest) returns (StatusResponse);
    rpc StoreResult(StoreResultRequest) returns (StatusResponse);
    rpc GetTaskResult(GetTaskResultRequest) returns (GetTaskResultResponse);
    rpc TaskCompleted(TaskCompletedRequest) returns (StatusResponse);
    rpc StoreLogs(StoreLogsRequest) returns (StatusResponse);
    rpc GetTaskLogs(GetTaskLogsRequest) returns (GetTaskLogsResponse);
    rpc GetAllTasks(GetAllTasksRequest) returns (GetAllTasksResponse);
    rpc StopTask(StopTaskRequest) returns (StopTaskResponse);
    rpc ReturnTaskResult(ReturnTaskResultRequest) returns (ReturnTaskResultResponse);
}
```

**任務相關訊息**:
```protobuf
message UploadTaskRequest {
    string task_id = 1;
    bytes task_zip = 2;
    int32 memory_gb = 3;
    int32 cpu_score = 4;
    int32 gpu_score = 5;
    int32 gpu_memory_gb = 6;
    string location = 7;
    string gpu_name = 8;
    string user_id = 9;
}

message TaskStatus {
    string task_id = 1;
    string status = 2;
    string created_at = 3;
    string updated_at = 4;
    string assigned_node = 5;
}

message PollTaskStatusResponse {
    string task_id = 1;
    string status = 2;
    repeated string output = 3;
    string message = 4;
}
```

### WorkerNodeService
```protobuf
service WorkerNodeService {
    rpc ExecuteTask(ExecuteTaskRequest) returns (ExecuteTaskResponse);
    rpc ReportOutput(ReportOutputRequest) returns (StatusResponse);
    rpc ReportRunningStatus(RunningStatusRequest) returns (RunningStatusResponse);
    rpc StopTaskExecution(StopTaskExecutionRequest) returns (StopTaskExecutionResponse);
}
```

**工作節點訊息**:
```protobuf
message ExecuteTaskRequest {
    string node_id = 1;
    string task_id = 2;
    bytes task_zip = 3;
    int32 cpu = 4;
    int32 gpu = 5;
    int32 memory_gb = 6;
    int32 gpu_memory_gb = 7;
}

message RunningStatusRequest {
    string node_id = 1;
    string task_id = 2;
    int32 cpu_usage = 3;      // CPU 使用率
    int32 memory_usage = 4;   // 記憶體總使用量
    int32 gpu_usage = 5;      // GPU 使用率
    int32 gpu_memory_usage = 6; // GPU 記憶體使用量
}

message RunningStatusResponse {
    bool success = 1;
    string message = 2;
    int64 cpt_reward = 3;
}
```

## 🗄️ 資料儲存架構

### SQLite 用戶資料庫
- **檔案**: `users.db`
- **功能**: 用戶認證、信用評分、代幣餘額

### Redis 節點狀態快取
- **用途**: 即時節點狀態、資源追蹤
- **鍵值結構**:
  ```
  node:{node_id} -> {
    "hostname": "worker01",
    "status": "Idle",
    "cpu_score": "1000",
    "available_cpu_score": "800",
    "current_tasks": "2",
    "trust_level": "high",
    "docker_status": "enabled",
    ...
  }
  ```

## � 資源管理系統

### 資源類型定義
```python
# CPU 評分: 處理器運算能力評分
# 記憶體: GB 為單位的記憶體容量  
# GPU 評分: 圖形處理器運算能力評分
# GPU 記憶體: GB 為單位的顯示記憶體容量
```

### 動態資源分配
```python
def allocate_node_resources(node_id, task_id, cpu_score, memory_gb, gpu_score, gpu_memory_gb):
    """分配節點資源給任務"""
    # 檢查可用資源
    # 扣除分配的資源
    # 更新任務列表
    # 記錄分配狀態
```

### 資源釋放機制
```python
def release_node_resources(node_id, task_id, cpu_score, memory_gb, gpu_score, gpu_memory_gb):
    """釋放節點資源"""
    # 歸還資源到可用資源池
    # 移除任務記錄
    # 更新節點狀態
```

## 🚀 部署和配置

### 環境變數配置
```bash
# gRPC 服務配置
GRPC_SERVER_HOST=0.0.0.0
GRPC_SERVER_PORT=50051

# Redis 配置
REDIS_HOST=localhost
REDIS_PORT=6379
REDIS_DB=0

# JWT 認證配置
JWT_SECRET_KEY=your-secret-key
TOKEN_EXPIRATION_HOURS=24

# 儲存配置
TASK_STORAGE_PATH=/mnt/myusb/hivemind/task_storage
MAX_FILE_SIZE=10485760  # 10MB

# 數據庫配置
DB_PATH=./users.db
```

### 啟動服務
```bash
cd node_pool
pip install -r requirements.txt
python node_pool_server.py
```

## 🔍 監控和日誌

### 關鍵指標監控
- **節點指標**: 活躍節點數、信任等級分布
- **任務指標**: 待處理任務、完成率、失敗率
- **資源指標**: 總資源、可用資源、使用率
- **系統指標**: gRPC 響應時間、錯誤率

### 日誌分類
```python
# 節點管理日誌
logging.info(f"節點 {node_id} (GPU: {gpu_name}, Docker: {docker_status}) 註冊成功")

# 任務分配日誌  
logging.info(f"任務 {task_id} 分配給節點 {node_id}，信任等級: {trust_level}")

# 資源管理日誌
logging.info(f"節點 {node_id} 資源分配: CPU-{cpu_score}, Memory-{memory_gb}GB")
```

## 🛠️ API 使用範例

### 節點註冊範例
```python
import grpc
import nodepool_pb2
import nodepool_pb2_grpc

channel = grpc.insecure_channel('localhost:50051')
stub = nodepool_pb2_grpc.NodeManagerServiceStub(channel)

request = nodepool_pb2.RegisterWorkerNodeRequest(
    node_id="worker-001",        # 用戶名
    hostname="192.168.1.100",    # IP 地址
    cpu_cores=8,
    memory_gb=16,
    cpu_score=1000,
    gpu_score=2000,
    gpu_memory_gb=8,
    location="Asia/Taipei",
    port=50052,
    gpu_name="RTX 4090",
    docker_status="enabled"
)

response = stub.RegisterWorkerNode(request)
print(f"註冊結果: {response.message}")
```

### 用戶登入範例
```python
user_stub = nodepool_pb2_grpc.UserServiceStub(channel)

# 用戶登入
login_request = nodepool_pb2.LoginRequest(
    username="user123",
    password="password123"
)

login_response = user_stub.Login(login_request)
if login_response.success:
    token = login_response.token
    print(f"登入成功，Token: {token}")
    
    # 查詢餘額
    balance_request = nodepool_pb2.GetBalanceRequest(
        username="user123",
        token=token
    )
    
    balance_response = user_stub.GetBalance(balance_request)
    if balance_response.success:
        print(f"帳戶餘額: {balance_response.balance} CPT")
```

### 任務上傳範例
```python
master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(channel)

with open("task.zip", "rb") as f:
    task_data = f.read()

request = nodepool_pb2.UploadTaskRequest(
    task_id="task-001",
    task_zip=task_data,
    memory_gb=4,
    cpu_score=500,
    gpu_score=1000,
    gpu_memory_gb=4,
    location="Any",
    gpu_name="Any",
    user_id="user123"
)

response = master_stub.UploadTask(request)
print(f"任務上傳: {response.message}")

# 查詢任務狀態
status_request = nodepool_pb2.PollTaskStatusRequest(task_id="task-001")
status_response = master_stub.PollTaskStatus(status_request)
print(f"任務狀態: {status_response.status}")
print(f"輸出: {status_response.output}")
```

### 工作節點狀態報告範例
```python
worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(channel)

# 報告運行狀態
status_request = nodepool_pb2.RunningStatusRequest(
    node_id="worker-001",
    task_id="task-001",
    cpu_usage=75,      # CPU 使用率 75%
    memory_usage=60,   # 記憶體使用率 60%
    gpu_usage=85,      # GPU 使用率 85%
    gpu_memory_usage=70  # GPU 記憶體使用率 70%
)

status_response = worker_stub.ReportRunningStatus(status_request)
if status_response.success:
    print(f"狀態報告成功，獲得 CPT 獎勵: {status_response.cpt_reward}")
```

## 🔧 故障排除

### 1. gRPC 連接問題
```bash
# 檢查服務狀態
netstat -an | grep 50051

# 測試連接
grpcurl -plaintext localhost:50051 nodepool.NodeManagerService/HealthCheck
```

### 2. Redis 連接失敗
```bash
# 檢查 Redis 服務
redis-cli ping

# 重啟 Redis
sudo systemctl restart redis-server
```

### 3. 節點註冊失敗
```python
# 檢查節點資訊
node_info = node_manager.get_node_info(node_id)
if not node_info:
    print(f"節點 {node_id} 不存在或未註冊")
```

### 4. 任務分配失敗
```python
# 檢查可用節點
available_nodes = node_manager.get_available_nodes(
    memory_gb_req=4, cpu_score_req=500, 
    gpu_score_req=1000, gpu_memory_gb_req=4,
    location_req="Any", gpu_name_req="Any",
    user_trust_score=100
)
print(f"可用節點數: {len(available_nodes)}")
```

## 📊 效能調優

### gRPC 伺服器優化
```python
options = [
    ('grpc.keepalive_time_ms', 10000),
    ('grpc.keepalive_timeout_ms', 5000),
    ('grpc.max_receive_message_length', 100 * 1024 * 1024),  # 100MB
    ('grpc.max_send_message_length', 100 * 1024 * 1024),     # 100MB
]

server = grpc.server(
    futures.ThreadPoolExecutor(max_workers=20),
    options=options
)
```

### Redis 記憶體優化
```bash
# 設定 Redis 記憶體限制
maxmemory 2gb
maxmemory-policy allkeys-lru

# 監控記憶體使用
redis-cli info memory
```

### 資料庫索引優化
```sql
-- 為關鍵欄位建立索引（如果使用 SQL 查詢）
CREATE INDEX idx_user_username ON users(username);
CREATE INDEX idx_user_credit_score ON users(credit_score);
```

## � 安全性

### JWT 認證機制
- **密鑰管理**: 環境變數安全儲存
- **過期時間**: 可配置的 token 有效期
- **權限控制**: 基於用戶身份的 API 存取控制

### gRPC 安全性
- **TLS 加密**: 支援 HTTPS/gRPC-TLS
- **訊息驗證**: 防止偽造請求
- **速率限制**: 防止 DDoS 攻擊

### 資料保護
- **密碼加密**: bcrypt 雜湊加密
- **敏感資料**: 加密儲存機密資訊
- **存取日誌**: 記錄所有 API 呼叫

---

**相關文檔**:
- [API 文檔](../api.md)
- [部署指南](../deployment.md)
- [故障排除](../troubleshooting.md)
- [開發者指南](../developer.md)

# HiveMind API 接口文檔

## 概述

HiveMind 使用 gRPC 協議進行服務間通訊，所有 API 接口都基於 Protocol Buffers 定義。本文檔詳細描述了各個服務的 API 接口規範。

## 服務架構

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Master Node   │    │   Node Pool     │    │  Worker Node    │
│                 │    │                 │    │                 │
│ 端口: 50051     │◄──►│ 端口: 50051     │◄──►│ 動態端口        │
│ 用戶服務        │    │ 節點管理服務    │    │ 任務執行服務    │
│ 任務管理服務    │    │ 任務調度服務    │    │ 狀態報告服務    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## gRPC 服務定義

基於實際的 `worker/nodepool.proto` 文件，HiveMind 包含四個主要的 gRPC 服務：

### UserService (用戶服務)

**文件**: `worker/nodepool.proto`

```protobuf
service UserService {
    rpc Login(LoginRequest) returns (LoginResponse);
    rpc Register(RegisterRequest) returns (RegisterResponse);
    rpc Transfer(TransferRequest) returns (TransferResponse);
    rpc GetBalance(GetBalanceRequest) returns (GetBalanceResponse);
}
```

### NodeManagerService (節點管理服務)

```protobuf
service NodeManagerService {
    rpc RegisterWorkerNode(RegisterWorkerNodeRequest) returns (StatusResponse);
    rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
    rpc ReportStatus(ReportStatusRequest) returns (StatusResponse);
    rpc GetNodeList(GetNodeListRequest) returns (GetNodeListResponse);
}
```

### MasterNodeService (主節點服務)

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

### WorkerNodeService (工作節點服務)

```protobuf
service WorkerNodeService {
    rpc ExecuteTask(ExecuteTaskRequest) returns (ExecuteTaskResponse);
    rpc ReportOutput(ReportOutputRequest) returns (StatusResponse);
    rpc ReportRunningStatus(RunningStatusRequest) returns (RunningStatusResponse);
    rpc StopTaskExecution(StopTaskExecutionRequest) returns (StopTaskExecutionResponse);
}
```

## 用戶管理 API

### 用戶註冊

**方法**: `Register`
**服務**: `UserService`

**請求參數**:
```protobuf
message RegisterRequest {
    string username = 1;     // 用戶名
    string password = 2;     // 密碼
}
```

**回應參數**:
```protobuf
message RegisterResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
}
```

### 用戶登入

**方法**: `Login`
**服務**: `UserService`

**請求參數**:
```protobuf
message LoginRequest {
    string username = 1;     // 用戶名
    string password = 2;     // 密碼
}
```

**回應參數**:
```protobuf
message LoginResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
    string token = 3;        // JWT 認證令牌
}
```

### 餘額查詢

**方法**: `GetBalance`
**服務**: `UserService`

**請求參數**:
```protobuf
message GetBalanceRequest {
    string username = 1;     // 用戶名
    string token = 2;        // 認證令牌
}
```

**回應參數**:
```protobuf
message GetBalanceResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
    int64 balance = 3;       // 用戶餘額
}
```

### 轉帳交易

**方法**: `Transfer`
**服務**: `UserService`

**請求參數**:
```protobuf
message TransferRequest {
    string token = 1;            // 認證令牌
    string receiver_username = 2; // 接收者用戶名
    int64 amount = 3;           // 轉帳金額
}
```

**回應參數**:
```protobuf
message TransferResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
}
```

## 節點管理 API

### 工作節點註冊

**方法**: `RegisterWorkerNode`
**服務**: `NodeManagerService`

**請求參數**:
```protobuf
message RegisterWorkerNodeRequest {
    string node_id = 1;          // 節點ID (用戶名)
    string hostname = 2;         // IP 地址
    int32 cpu_cores = 3;         // CPU 核心數
    int32 memory_gb = 4;         // 記憶體容量 (GB)
    int32 cpu_score = 5;         // CPU 效能分數
    int32 gpu_score = 6;         // GPU 效能分數
    int32 gpu_memory_gb = 7;     // GPU 記憶體容量 (GB)
    string location = 8;         // 地理位置
    int32 port = 9;             // 服務端口
    string gpu_name = 10;        // GPU 名稱
    int32 trust_level = 11;      // 信任等級
    double last_heartbeat = 12;   // 最後心跳時間
    string docker_status = 13;    // Docker 狀態
}
```

**回應參數**:
```protobuf
message StatusResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
}
```

### 健康檢查

**方法**: `HealthCheck`
**服務**: `NodeManagerService`

**請求參數**:
```protobuf
message HealthCheckRequest {
    string node_id = 1;      // 節點ID
}
```

**回應參數**:
```protobuf
message HealthCheckResponse {
    string status = 1;       // 健康狀態
    string message = 2;      // 狀態訊息
}
```

### 狀態報告

**方法**: `ReportStatus`
**服務**: `NodeManagerService`

**請求參數**:
```protobuf
message ReportStatusRequest {
    string node_id = 1;      // 節點ID
    string status = 2;       // 節點狀態
}
```

### 獲取節點列表

**方法**: `GetNodeList`
**服務**: `NodeManagerService`

**請求參數**:
```protobuf
message GetNodeListRequest {
    // 空請求，獲取所有節點
}
```

**回應參數**:
```protobuf
message GetNodeListResponse {
    repeated WorkerNodeInfo nodes = 1; // 節點列表
}

message WorkerNodeInfo {
    string node_id = 1;          // 節點ID
    string hostname = 2;         // IP 地址
    int32 cpu_cores = 3;         // CPU 核心數
    int32 memory_gb = 4;         // 記憶體容量
    int32 cpu_score = 5;         // CPU 效能分數
    int32 gpu_score = 6;         // GPU 效能分數
    int32 gpu_memory_gb = 7;     // GPU 記憶體容量
    string location = 8;         // 地理位置
    int32 port = 9;             // 服務端口
    string gpu_name = 10;        // GPU 名稱
    int32 trust_level = 11;      // 信任等級
    double last_heartbeat = 12;   // 最後心跳時間
    string docker_status = 13;    // Docker 狀態
}
```

## 任務管理 API

### 上傳任務

**方法**: `UploadTask`
**服務**: `MasterNodeService`

**請求參數**:
```protobuf
message UploadTaskRequest {
    string task_id = 1;      // 任務ID
    bytes task_data = 2;     // 任務數據
    string user_id = 3;      // 用戶ID
}
```

**回應參數**:
```protobuf
message UploadTaskResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
}
```

### 查詢任務狀態

**方法**: `PollTaskStatus`
**服務**: `MasterNodeService`

**請求參數**:
```protobuf
message PollTaskStatusRequest {
    string task_id = 1;      // 任務ID
}
```

**回應參數**:
```protobuf
message PollTaskStatusResponse {
    bool success = 1;            // 是否成功
    string message = 2;          // 回應訊息
    string status = 3;           // 任務狀態
    repeated string output = 4;   // 輸出內容
}
```

### 獲取任務結果

**方法**: `GetTaskResult`
**服務**: `MasterNodeService`

**請求參數**:
```protobuf
message GetTaskResultRequest {
    string task_id = 1;      // 任務ID
}
```

**回應參數**:
```protobuf
message GetTaskResultResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
    bytes result_data = 3;   // 結果數據
}
```

### 停止任務

**方法**: `StopTask`
**服務**: `MasterNodeService`

**請求參數**:
```protobuf
message StopTaskRequest {
    string task_id = 1;      // 任務ID
}
```

**回應參數**:
```protobuf
message StopTaskResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
}
```

### 獲取所有任務

**方法**: `GetAllTasks`
**服務**: `MasterNodeService`

**請求參數**:
```protobuf
message GetAllTasksRequest {
    // 空請求，獲取所有任務
}
```

**回應參數**:
```protobuf
message GetAllTasksResponse {
    repeated TaskStatus tasks = 1; // 任務列表
}

message TaskStatus {
    string task_id = 1;       // 任務ID
    string status = 2;        // 任務狀態
    string assigned_node = 3; // 分配的節點
    double created_at = 4;    // 創建時間
    double started_at = 5;    // 開始時間
    double completed_at = 6;  // 完成時間
    string user_id = 7;       // 用戶ID
}
```

## 工作節點 API

### 執行任務

**方法**: `ExecuteTask`
**服務**: `WorkerNodeService`

**請求參數**:
```protobuf
message ExecuteTaskRequest {
    string task_id = 1;      // 任務ID
    bytes task_data = 2;     // 任務數據
}
```

**回應參數**:
```protobuf
message ExecuteTaskResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
}
```

### 報告運行狀態

**方法**: `ReportRunningStatus`
**服務**: `WorkerNodeService`

**請求參數**:
```protobuf
message RunningStatusRequest {
    string node_id = 1;          // 節點ID
    string task_id = 2;          // 任務ID
    int32 cpu_usage = 3;         // CPU 使用率
    int32 memory_usage = 4;      // 記憶體使用率
    int32 gpu_usage = 5;         // GPU 使用率
    int32 gpu_memory_usage = 6;  // GPU 記憶體使用率
}
```

**回應參數**:
```protobuf
message RunningStatusResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
    int64 cpt_reward = 3;    // CPT 獎勵
}
```

### 停止任務執行

**方法**: `StopTaskExecution`
**服務**: `WorkerNodeService`

**請求參數**:
```protobuf
message StopTaskExecutionRequest {
    string task_id = 1;      // 任務ID
}
```

**回應參數**:
```protobuf
message StopTaskExecutionResponse {
    bool success = 1;        // 是否成功
    string message = 2;      // 回應訊息
}
```

## TaskWorker gRPC API

### 服務定義

**文件**: `taskworker/protos/taskworker.proto`

```protobuf
service TaskWorkerService {
  rpc ExecuteTask(ExecuteTaskRequest) returns (ExecuteTaskResponse);
  rpc GetTaskStatus(GetTaskStatusRequest) returns (GetTaskStatusResponse);
  rpc CancelTask(CancelTaskRequest) returns (CancelTaskResponse);
  rpc GetSystemInfo(GetSystemInfoRequest) returns (GetSystemInfoResponse);
}
```

### 執行任務

**方法**: `ExecuteTask`

**請求參數**:
```protobuf
message ExecuteTaskRequest {
  string task_id = 1;       // 任務ID
  string task_type = 2;     // 任務類型
  bytes task_data = 3;      // 任務數據
  map<string, string> parameters = 4; // 任務參數
}
```

## 資料結構定義

### 用戶資訊

```protobuf
message UserInfo {
  string user_id = 1;       // 用戶ID
  string username = 2;      // 用戶名
  string email = 3;         // 電子郵件
  int64 created_at = 4;     // 創建時間
  UserRole role = 5;        // 用戶角色
}
```

### 節點能力

```protobuf
message NodeCapabilities {
  int32 cpu_cores = 1;      // CPU 核心數
  int64 memory_mb = 2;      // 記憶體容量 (MB)
  bool has_gpu = 3;         // 是否有 GPU
  repeated string supported_tasks = 4; // 支援的任務類型
}
```

### 資源使用情況

```protobuf
message ResourceUsage {
  float cpu_percent = 1;    // CPU 使用率
  float memory_percent = 2; // 記憶體使用率
  float disk_percent = 3;   // 磁碟使用率
  float network_mbps = 4;   // 網路使用量 (Mbps)
}
```

## 錯誤代碼

| 代碼 | 名稱 | 描述 |
|------|------|------|
| 0 | SUCCESS | 操作成功 |
| 1001 | USER_NOT_FOUND | 用戶不存在 |
| 1002 | INVALID_PASSWORD | 密碼錯誤 |
| 1003 | USER_EXISTS | 用戶已存在 |
| 1004 | INVALID_SESSION | 無效的會話 |
| 2001 | NODE_NOT_FOUND | 節點不存在 |
| 2002 | NODE_OFFLINE | 節點離線 |
| 2003 | INSUFFICIENT_RESOURCES | 資源不足 |
| 3001 | TASK_NOT_FOUND | 任務不存在 |
| 3002 | TASK_FAILED | 任務執行失敗 |
| 3003 | TASK_CANCELLED | 任務已取消 |
| 9999 | INTERNAL_ERROR | 內部服務器錯誤 |

## 認證機制

HiveMind 使用基於令牌的認證機制：

1. 用戶登入後獲得 `session_token`
2. 所有後續 API 請求都需要在 metadata 中包含：
   ```
   authorization: Bearer <session_token>
   ```
3. 令牌有效期為 24 小時
4. 令牌過期後需要重新登入

## 使用範例

### Python 客戶端範例

#### 用戶服務範例

```python
import grpc
from node_pool import nodepool_pb2_grpc, nodepool_pb2

# 創建連接
channel = grpc.insecure_channel('localhost:50051')
user_stub = nodepool_pb2_grpc.UserServiceStub(channel)

# 用戶註冊
register_request = nodepool_pb2.RegisterRequest(
    username="testuser",
    password="password123"
)
register_response = user_stub.Register(register_request)
print(f"註冊結果: {register_response.success}, 訊息: {register_response.message}")

# 用戶登入
login_request = nodepool_pb2.LoginRequest(
    username="testuser",
    password="password123"
)
login_response = user_stub.Login(login_request)
if login_response.success:
    token = login_response.token
    print(f"登入成功，令牌: {token}")
    
    # 查詢餘額
    balance_request = nodepool_pb2.GetBalanceRequest(
        username="testuser",
        token=token
    )
    balance_response = user_stub.GetBalance(balance_request)
    print(f"用戶餘額: {balance_response.balance}")
```

#### 節點管理服務範例

```python
# 節點管理服務
node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(channel)

# 註冊工作節點
register_node_request = nodepool_pb2.RegisterWorkerNodeRequest(
    node_id="worker-001",
    hostname="192.168.1.100",
    cpu_cores=8,
    memory_gb=16,
    cpu_score=1000,
    gpu_score=2000,
    gpu_memory_gb=8,
    location="Asia/Taipei",
    port=50052,
    gpu_name="RTX 4090",
    trust_level=100,
    last_heartbeat=1640995200.0,
    docker_status="enabled"
)
node_response = node_stub.RegisterWorkerNode(register_node_request)
print(f"節點註冊: {node_response.success}")

# 健康檢查
health_request = nodepool_pb2.HealthCheckRequest(node_id="worker-001")
health_response = node_stub.HealthCheck(health_request)
print(f"節點健康狀態: {health_response.status}")
```

#### 主節點服務範例

```python
# 主節點服務
master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(channel)

# 上傳任務
task_data = b"print('Hello, HiveMind!')"
upload_request = nodepool_pb2.UploadTaskRequest(
    task_id="task-001",
    task_data=task_data,
    user_id="user123"
)
upload_response = master_stub.UploadTask(upload_request)
print(f"任務上傳: {upload_response.success}")

# 查詢任務狀態
status_request = nodepool_pb2.PollTaskStatusRequest(task_id="task-001")
status_response = master_stub.PollTaskStatus(status_request)
print(f"任務狀態: {status_response.status}")
print(f"任務輸出: {status_response.output}")
```

#### 工作節點服務範例

```python
# 工作節點服務  
worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(channel)

# 報告運行狀態
status_request = nodepool_pb2.RunningStatusRequest(
    node_id="worker-001",
    task_id="task-001",
    cpu_usage=75,
    memory_usage=60,
    gpu_usage=85,
    gpu_memory_usage=70
)
status_response = worker_stub.ReportRunningStatus(status_request)
print(f"CPT 獎勵: {status_response.cpt_reward}")
```

## 性能考量

- 建議使用連接池來管理 gRPC 連接
- 對於高頻率的狀態更新，考慮使用批處理 API
- 大型任務數據應該使用流式 API 傳輸
- 在生產環境中啟用 TLS 加密

## 限制和注意事項

1. 單個任務數據大小限制為 100MB
2. 併發任務數量受節點資源限制
3. 會話令牌不支援跨節點共享
4. WebSocket 實時通訊功能正在開發中

---

**更新日期**: 2024年1月  
**版本**: v1.0  
**狀態**: 基於實際實現的準確文檔

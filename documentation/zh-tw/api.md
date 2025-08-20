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

### NodePool 服務

**文件**: `nodepool.proto`

```protobuf
service NodePoolService {
  // 節點管理
  rpc RegisterNode(RegisterNodeRequest) returns (RegisterNodeResponse);
  rpc UnregisterNode(UnregisterNodeRequest) returns (UnregisterNodeResponse);
  rpc UpdateNodeStatus(UpdateNodeStatusRequest) returns (UpdateNodeStatusResponse);
  rpc GetNodeInfo(GetNodeInfoRequest) returns (GetNodeInfoResponse);
  
  // 用戶管理
  rpc RegisterUser(RegisterUserRequest) returns (RegisterUserResponse);
  rpc LoginUser(LoginUserRequest) returns (LoginUserResponse);
  rpc LogoutUser(LogoutUserRequest) returns (LogoutUserResponse);
  rpc GetUserInfo(GetUserInfoRequest) returns (GetUserInfoResponse);
  
  // 任務管理
  rpc SubmitTask(SubmitTaskRequest) returns (SubmitTaskResponse);
  rpc GetTaskStatus(GetTaskStatusRequest) returns (GetTaskStatusResponse);
  rpc CancelTask(CancelTaskRequest) returns (CancelTaskResponse);
  rpc GetTaskResult(GetTaskResultRequest) returns (GetTaskResultResponse);
}
```

## 用戶管理 API

### 用戶註冊

**方法**: `RegisterUser`

**請求參數**:
```protobuf
message RegisterUserRequest {
  string username = 1;      // 用戶名 (3-20字符，字母數字下劃線)
  string password = 2;      // 密碼 (最少8字符)
  string email = 3;         // 電子郵件地址
}
```

**回應參數**:
```protobuf
message RegisterUserResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  string user_id = 3;       // 用戶ID (成功時返回)
}
```

**錯誤碼**:
- `USER_EXISTS`: 用戶名已存在
- `INVALID_EMAIL`: 無效的電子郵件格式
- `WEAK_PASSWORD`: 密碼強度不足

### 用戶登入

**方法**: `LoginUser`

**請求參數**:
```protobuf
message LoginUserRequest {
  string username = 1;      // 用戶名
  string password = 2;      // 密碼
}
```

**回應參數**:
```protobuf
message LoginUserResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  string session_token = 3; // 會話令牌
  UserInfo user_info = 4;   // 用戶資訊
}
```

## 節點管理 API

### 節點註冊

**方法**: `RegisterNode`

**請求參數**:
```protobuf
message RegisterNodeRequest {
  string node_id = 1;       // 節點ID
  string node_type = 2;     // 節點類型 (worker/master)
  string hostname = 3;      // 主機名
  int32 port = 4;          // 端口號
  NodeCapabilities capabilities = 5; // 節點能力
}
```

**回應參數**:
```protobuf
message RegisterNodeResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  string assigned_id = 3;   // 分配的節點ID
}
```

### 節點狀態更新

**方法**: `UpdateNodeStatus`

**請求參數**:
```protobuf
message UpdateNodeStatusRequest {
  string node_id = 1;       // 節點ID
  NodeStatus status = 2;    // 節點狀態
  ResourceUsage resource_usage = 3; // 資源使用情況
}
```

## 任務管理 API

### 提交任務

**方法**: `SubmitTask`

**請求參數**:
```protobuf
message SubmitTaskRequest {
  string user_id = 1;       // 用戶ID
  string task_type = 2;     // 任務類型
  bytes task_data = 3;      // 任務數據
  TaskRequirements requirements = 4; // 任務需求
  TaskPriority priority = 5; // 任務優先級
}
```

**回應參數**:
```protobuf
message SubmitTaskResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  string task_id = 3;       // 任務ID
  string estimated_time = 4; // 預估完成時間
}
```

### 查詢任務狀態

**方法**: `GetTaskStatus`

**請求參數**:
```protobuf
message GetTaskStatusRequest {
  string task_id = 1;       // 任務ID
  string user_id = 2;       // 用戶ID
}
```

**回應參數**:
```protobuf
message GetTaskStatusResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  TaskStatus status = 3;    // 任務狀態
  float progress = 4;       // 完成進度 (0.0-1.0)
  string assigned_node = 5; // 分配的節點
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

```python
import grpc
from node_pool import nodepool_pb2_grpc, nodepool_pb2

# 創建連接
channel = grpc.insecure_channel('localhost:50051')
stub = nodepool_pb2_grpc.NodePoolServiceStub(channel)

# 用戶註冊
request = nodepool_pb2.RegisterUserRequest(
    username="testuser",
    password="password123",
    email="test@example.com"
)
response = stub.RegisterUser(request)
print(f"註冊結果: {response.success}, 訊息: {response.message}")

# 用戶登入
login_request = nodepool_pb2.LoginUserRequest(
    username="testuser",
    password="password123"
)
login_response = stub.LoginUser(login_request)
if login_response.success:
    session_token = login_response.session_token
    print(f"登入成功，令牌: {session_token}")
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

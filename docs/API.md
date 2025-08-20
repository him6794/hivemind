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

## 用戶管理服務

### 用戶註冊

**接口**：`RegisterUser`

**請求參數**：
```protobuf
message RegisterRequest {
  string username = 1;      // 用戶名 (3-20字符，字母數字下劃線)
  string password = 2;      // 密碼 (最少8字符)
  string email = 3;         // 電子郵件地址
}
```

**回應參數**：
```protobuf
message RegisterResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  string user_id = 3;       // 用戶ID (成功時返回)
}
```

**錯誤碼**：
- `USER_EXISTS`: 用戶名已存在
- `INVALID_EMAIL`: 無效的電子郵件格式
- `WEAK_PASSWORD`: 密碼強度不足

### 用戶登入

**接口**：`LoginUser`

**請求參數**：
```protobuf
message LoginRequest {
  string username = 1;      // 用戶名或電子郵件
  string password = 2;      // 密碼
}
```

**回應參數**：
```protobuf
message LoginResponse {
  bool success = 1;         // 是否成功
  string token = 2;         // JWT 令牌 (有效期60分鐘)
  string message = 3;       // 回應訊息
  int64 expires_at = 4;     // 令牌過期時間戳
}
```

### 餘額查詢

**接口**：`GetBalance`

**請求參數**：
```protobuf
message BalanceRequest {
  string token = 1;         // JWT 身份驗證令牌
  string username = 2;      // 查詢的用戶名 (可選，默認為當前用戶)
}
```

**回應參數**：
```protobuf
message BalanceResponse {
  bool success = 1;         // 是否成功
  double balance = 2;       // CPT 代幣餘額
  string message = 3;       // 回應訊息
  int64 last_updated = 4;   // 最後更新時間戳
}
```

### 代幣轉帳

**接口**：`TransferTokens`

**請求參數**：
```protobuf
message TransferRequest {
  string token = 1;         // 發送者身份驗證令牌
  string recipient = 2;     // 接收者用戶名
  double amount = 3;        // 轉帳金額 (必須大於0)
  string memo = 4;          // 轉帳備註 (可選)
}
```

**回應參數**：
```protobuf
message TransferResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  double new_balance = 3;   // 轉帳後餘額
  string transaction_id = 4; // 交易ID
}
```

**錯誤碼**：
- `INSUFFICIENT_BALANCE`: 餘額不足
- `USER_NOT_FOUND`: 接收者不存在
- `INVALID_AMOUNT`: 無效的轉帳金額

## 節點管理服務

### 節點註冊

**接口**：`RegisterNode`

**請求參數**：
```protobuf
message RegisterNodeRequest {
  string node_id = 1;       // 節點唯一識別符
  string hostname = 2;      // 主機名稱
  string ip_address = 3;    // IP 地址
  int32 port = 4;          // 通訊端口
  
  // 硬體資源資訊
  int32 cpu_cores = 5;      // CPU 核心數
  double memory_gb = 6;     // 記憶體容量 (GB)
  string gpu_name = 7;      // GPU 型號名稱
  double gpu_memory_gb = 8; // GPU 記憶體容量 (GB)
  
  // 性能評分
  double cpu_score = 9;     // CPU 性能評分
  double gpu_score = 10;    // GPU 性能評分
  
  // 其他資訊
  string location = 11;     // 地理位置
  bool docker_status = 12;  // Docker 服務狀態
  string os_info = 13;      // 作業系統資訊
}
```

**回應參數**：
```protobuf
message RegisterNodeResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  string node_token = 3;    // 節點認證令牌
  string trust_level = 4;   // 信任等級 (HIGH/MEDIUM/LOW)
}
```

### 狀態報告

**接口**：`ReportStatus`

**請求參數**：
```protobuf
message StatusRequest {
  string node_token = 1;    // 節點認證令牌
  
  // 資源使用情況
  double cpu_usage = 2;     // CPU 使用率 (0-100)
  double memory_usage = 3;  // 記憶體使用率 (0-100)
  double gpu_usage = 4;     // GPU 使用率 (0-100)
  
  // 當前狀態
  string status = 5;        // 節點狀態 (IDLE/BUSY/OFFLINE)
  int32 active_tasks = 6;   // 活動任務數量
  double network_latency = 7; // 網路延遲 (ms)
}
```

**回應參數**：
```protobuf
message StatusResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  repeated TaskAssignment tasks = 3; // 分配的新任務
}
```

### 心跳檢測

**接口**：`Heartbeat`

**請求參數**：
```protobuf
message HeartbeatRequest {
  string node_token = 1;    // 節點認證令牌
  int64 timestamp = 2;      // 客戶端時間戳
}
```

**回應參數**：
```protobuf
message HeartbeatResponse {
  bool healthy = 1;         // 節點健康狀態
  int64 server_time = 2;    // 服務器時間戳
  string message = 3;       // 健康狀態訊息
}
```

## 任務管理服務

### 創建任務

**接口**：`CreateTask`

**請求參數**：
```protobuf
message CreateTaskRequest {
  string user_token = 1;    // 用戶身份驗證令牌
  string task_name = 2;     // 任務名稱
  string task_type = 3;     // 任務類型
  bytes task_data = 4;      // 任務數據 (ZIP格式)
  
  // 資源需求
  int32 required_cpu = 5;   // 所需 CPU 核心數
  double required_memory = 6; // 所需記憶體 (GB)
  bool require_gpu = 7;     // 是否需要 GPU
  
  // 執行參數
  string priority = 8;      // 優先級 (LOW/NORMAL/HIGH/URGENT)
  int32 timeout_seconds = 9; // 超時時間 (秒)
  double max_cost = 10;     // 最大成本 (CPT)
}
```

**回應參數**：
```protobuf
message CreateTaskResponse {
  bool success = 1;         // 是否成功
  string message = 2;       // 回應訊息
  string task_id = 3;       // 任務ID
  double estimated_cost = 4; // 預估成本
}
```

### 任務分配

**接口**：`AssignTask` (內部使用)

**請求參數**：
```protobuf
message AssignTaskRequest {
  string task_id = 1;       // 任務ID
  string node_id = 2;       // 目標節點ID
  int64 deadline = 3;       // 截止時間戳
}
```

### 提交結果

**接口**：`SubmitResult`

**請求參數**：
```protobuf
message ResultRequest {
  string node_token = 1;    // 節點認證令牌
  string task_id = 2;       // 任務ID
  bool success = 3;         // 執行是否成功
  bytes result_data = 4;    // 結果數據
  string error_message = 5; // 錯誤訊息 (如果失敗)
  int64 execution_time = 6; // 執行時間 (毫秒)
}
```

**回應參數**：
```protobuf
message ResultResponse {
  bool accepted = 1;        // 結果是否被接受
  string message = 2;       // 回應訊息
  double reward_amount = 3; // 獎勵金額
}
```

## 錯誤處理

### 通用錯誤碼

| 錯誤碼 | 描述 | HTTP 狀態碼 |
|--------|------|-------------|
| `SUCCESS` | 成功 | 200 |
| `INVALID_REQUEST` | 無效請求 | 400 |
| `UNAUTHORIZED` | 未授權 | 401 |
| `FORBIDDEN` | 禁止訪問 | 403 |
| `NOT_FOUND` | 資源不存在 | 404 |
| `INTERNAL_ERROR` | 內部服務器錯誤 | 500 |
| `SERVICE_UNAVAILABLE` | 服務不可用 | 503 |

### gRPC 狀態碼映射

| gRPC 狀態 | 描述 | 錯誤處理 |
|-----------|------|----------|
| `OK` | 成功 | 正常處理 |
| `INVALID_ARGUMENT` | 參數無效 | 檢查請求參數 |
| `UNAUTHENTICATED` | 未認證 | 重新登入獲取令牌 |
| `PERMISSION_DENIED` | 權限不足 | 檢查用戶權限 |
| `DEADLINE_EXCEEDED` | 請求超時 | 重試或增加超時時間 |
| `UNAVAILABLE` | 服務不可用 | 稍後重試 |

## 安全考慮

### 身份驗證
- 所有 API 調用都需要有效的 JWT 令牌
- 令牌有效期為 60 分鐘
- 支援令牌刷新機制

### 數據傳輸
- 所有 gRPC 連接使用 TLS 加密
- 敏感數據在傳輸前進行額外加密
- 支援訊息壓縮以減少帶寬使用

### 速率限制
- 每個用戶每分鐘最多 100 次 API 調用
- 節點心跳檢測不受速率限制
- 大型文件上傳有專門的限制策略

## 開發工具

### 客戶端生成
```bash
# Python 客戶端
python -m grpc_tools.protoc --python_out=. --grpc_python_out=. nodepool.proto

# Go 客戶端
protoc --go_out=. --go-grpc_out=. nodepool.proto

# JavaScript 客戶端
npx grpc_tools_node_protoc --js_out=import_style=commonjs,binary:. --grpc_out=. nodepool.proto
```

### 測試工具
```bash
# 使用 grpcurl 測試 API
grpcurl -plaintext localhost:50051 list
grpcurl -plaintext -d '{"username":"test","password":"password"}' localhost:50051 UserService/LoginUser
```

## 版本控制

- 當前 API 版本：v1.0
- 向後相容性：支援最近 3 個主要版本
- 版本升級策略：平滑遷移，提前通知棄用

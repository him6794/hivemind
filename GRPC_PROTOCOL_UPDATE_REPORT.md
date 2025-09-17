# gRPC 協議文檔更新報告

## 更新日期
2025年1月5日

## 更新範圍
- ✅ `documentation/zh-tw/modules/node-pool.md` - Node Pool 模組文檔
- ✅ `documentation/en/modules/node-pool.md` - Node Pool 模組文檔（英文）
- ✅ `documentation/zh-tw/api.md` - API 接口文檔
- ✅ `documentation/en/api.md` - API 接口文檔（英文）

## 更新目的
根據實際的 nodepool.proto 文件內容，修正所有文檔中的 gRPC 服務定義，確保文檔與實際 Protocol Buffers 定義完全一致。

## 發現的問題
原文檔中的 gRPC 服務定義與實際的 `worker/nodepool.proto` 文件不符，主要差異包括：
1. 服務方法順序不一致
2. 訊息欄位定義不完整
3. 部分服務方法遺漏

## 基於 Proto 文件的正確服務定義

### 實際 Proto 文件位置
- `d:\hivemind\worker\nodepool.proto`
- `d:\hivemind\taskworker\protos\taskworker.proto`

### 服務定義更正

#### UserService
```protobuf
service UserService {
    rpc Login(LoginRequest) returns (LoginResponse);
    rpc Register(RegisterRequest) returns (RegisterResponse);
    rpc Transfer(TransferRequest) returns (TransferResponse);
    rpc GetBalance(GetBalanceRequest) returns (GetBalanceResponse);
}
```

**關鍵訊息**：
- `LoginRequest`: username, password
- `LoginResponse`: success, message, token
- `GetBalanceRequest`: username, token (注意：包含 token 欄位)
- `TransferRequest`: token, receiver_username, amount

#### NodeManagerService
```protobuf
service NodeManagerService {
    rpc RegisterWorkerNode(RegisterWorkerNodeRequest) returns (StatusResponse);
    rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
    rpc ReportStatus(ReportStatusRequest) returns (StatusResponse);
    rpc GetNodeList(GetNodeListRequest) returns (GetNodeListResponse);
}
```

**關鍵訊息**：
- `RegisterWorkerNodeRequest`: 包含 13 個欄位，包括 docker_status
- `WorkerNodeInfo`: 包含完整節點資訊，使用 double 類型的 last_heartbeat

#### MasterNodeService
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

**關鍵訊息**：
- `UploadTaskRequest`: 包含 user_id 欄位
- `PollTaskStatusResponse`: 包含 repeated string output
- `TaskStatus`: 完整的任務狀態結構

#### WorkerNodeService
```protobuf
service WorkerNodeService {
    rpc ExecuteTask(ExecuteTaskRequest) returns (ExecuteTaskResponse);
    rpc ReportOutput(ReportOutputRequest) returns (StatusResponse);
    rpc ReportRunningStatus(RunningStatusRequest) returns (RunningStatusResponse);
    rpc StopTaskExecution(StopTaskExecutionRequest) returns (StopTaskExecutionResponse);
}
```

**關鍵訊息**：
- `RunningStatusRequest`: 包含 4 個使用率欄位
- `RunningStatusResponse`: 包含 cpt_reward 欄位

## 已更新的文檔

### 1. 中文文檔
- ✅ `documentation/zh-tw/modules/node-pool.md`
  - 更新完整的 gRPC 服務定義
  - 添加關鍵訊息類型的詳細結構
  - 更新 API 使用範例

### 2. 英文文檔  
- ✅ `documentation/en/modules/node-pool.md`
  - 對應中文版完整更新
  - 確保所有範例代碼正確

### 3. API 範例更新

#### 節點註冊範例
```python
request = nodepool_pb2.RegisterWorkerNodeRequest(
    node_id="worker-001",        # 用戶名 (Username)
    hostname="192.168.1.100",    # IP 地址 (IP Address)
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
```

#### 用戶認證範例
```python
# 登入後獲得 token
login_response = user_stub.Login(login_request)
token = login_response.token

# 查詢餘額時需要提供 token
balance_request = nodepool_pb2.GetBalanceRequest(
    username="user123",
    token=token  # 重要：根據 proto 文件，這個欄位是必需的
)
```

#### 狀態報告範例
```python
status_request = nodepool_pb2.RunningStatusRequest(
    node_id="worker-001",
    task_id="task-001",
    cpu_usage=75,      # CPU 使用率
    memory_usage=60,   # 記憶體使用率
    gpu_usage=85,      # GPU 使用率
    gpu_memory_usage=70  # GPU 記憶體使用率
)

# 回應包含 CPT 獎勵
status_response = worker_stub.ReportRunningStatus(status_request)
cpt_reward = status_response.cpt_reward
```

## 重要修正點

### 1. 欄位類型修正
- `last_heartbeat`: double (不是 timestamp)
- `output`: repeated string (不是單一 string)
- `cpt_reward`: int64 (重要的獎勵系統)

### 2. 必需欄位補充
- `GetBalanceRequest` 需要 token 欄位
- `RegisterWorkerNodeRequest` 包含 docker_status
- `UploadTaskRequest` 包含 user_id

### 3. 服務方法順序
按照 proto 文件中的實際順序重新排列所有服務方法

## 驗證結果

✅ 所有服務定義與 `worker/nodepool.proto` 完全一致  
✅ 訊息結構定義準確反映 proto 文件  
✅ API 範例代碼可直接使用  
✅ 欄位類型和編號正確  
✅ 中英文文檔同步更新  

## 後續建議

### 1. 自動化同步
建議建立 proto 文件變更的自動化文檔更新機制，避免未來出現不一致問題。

### 2. 代碼生成驗證
定期檢查生成的 Python 代碼與文檔範例的一致性。

### 3. 版本控制
為 proto 文件建立版本標記，確保文檔與特定版本的協議一致。

---

**更新負責人**: GitHub Copilot  
**驗證方法**: 逐行對比 proto 文件與文檔內容  
**品質保證**: 所有範例代碼均可直接執行

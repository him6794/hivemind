# HiveMind 節點池服務 (Node Pool Service)

## 概述

節點池服務是 HiveMind 分散式計算平台的核心元件，負責管理計算節點的註冊、任務分配、資源監控和用戶認證。該服務基於 gRPC 協議，提供高性能的節點間通訊和任務管理。

## 主要功能

### 1. 節點管理 (Node Management)
- **節點註冊**: 工作節點自動註冊到節點池，包含硬體規格和能力資訊
- **心跳監控**: 實時監控節點狀態，自動檢測離線節點
- **資源追蹤**: 追蹤每個節點的 CPU、記憶體、GPU 資源使用情況
- **多任務支援**: 支援單節點同時執行多個任務
- **信任等級管理**: 基於信用評分的多層級節點分組

### 2. 用戶服務 (User Service)
- **用戶註冊/登入**: JWT-based 認證系統
- **CPT 代幣管理**: 計算力代幣轉帳和餘額查詢
- **權限控制**: 基於信用評分的資源存取控制

### 3. 任務調度 (Task Scheduling)
- **智慧任務分配**: 基於資源需求和節點能力的最佳匹配
- **負載均衡**: 考慮節點當前負載和歷史表現
- **Docker 支援**: 支援 Docker 容器化任務執行
- **地理位置感知**: 支援按地理位置優先分配節點

### 4. 資源監控 (Resource Monitoring)
- **實時監控**: 監控節點 CPU、記憶體、GPU 使用率
- **動態資源分配**: 基於實際使用情況動態調整資源分配
- **效能追蹤**: 記錄任務執行效能和資源消耗

## 架構組成

### 核心服務
- `node_pool_server.py` - gRPC 服務主程式
- `node_manager.py` - 節點管理邏輯
- `user_service.py` - 用戶認證服務
- `master_node_service.py` - 主節點任務調度
- `node_manager_service.py` - 節點管理服務

### 配置與資料
- `config.py` - 服務配置管理
- `database_manager.py` - 資料庫操作
- `user_manager.py` - 用戶管理邏輯
- `users.db` - SQLite 用戶資料庫
- `requirements.txt` - Python 相依套件

### gRPC 接口
- `nodepool_pb2.py` - Protocol Buffers 訊息定義
- `nodepool_pb2_grpc.py` - gRPC 服務定義

## 服務接口

### 用戶服務 (UserService)
```protobuf
service UserService {
    rpc Login(LoginRequest) returns (LoginResponse);
    rpc Register(RegisterRequest) returns (RegisterResponse);
    rpc Transfer(TransferRequest) returns (TransferResponse);
    rpc GetBalance(GetBalanceRequest) returns (GetBalanceResponse);
}
```

### 節點管理服務 (NodeManagerService)
```protobuf
service NodeManagerService {
    rpc RegisterWorkerNode(RegisterWorkerNodeRequest) returns (StatusResponse);
    rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
    rpc ReportStatus(ReportStatusRequest) returns (StatusResponse);
    rpc GetNodeList(GetNodeListRequest) returns (GetNodeListResponse);
}
```

### 主節點服務 (MasterNodeService)
```protobuf
service MasterNodeService {
    rpc UploadTask(UploadTaskRequest) returns (UploadTaskResponse);
    rpc PollTaskStatus(PollTaskStatusRequest) returns (PollTaskStatusResponse);
    rpc GetTaskResult(GetTaskResultRequest) returns (GetTaskResultResponse);
    rpc TaskCompleted(TaskCompletedRequest) returns (StatusResponse);
    rpc GetAllTasks(GetAllTasksRequest) returns (GetAllTasksResponse);
    rpc StopTask(StopTaskRequest) returns (StopTaskResponse);
    // ... 更多任務管理接口
}
```

### 工作節點服務 (WorkerNodeService)
```protobuf
service WorkerNodeService {
    rpc ExecuteTask(ExecuteTaskRequest) returns (ExecuteTaskResponse);
    rpc ReportOutput(ReportOutputRequest) returns (StatusResponse);
    rpc ReportRunningStatus(RunningStatusRequest) returns (RunningStatusResponse);
    rpc StopTaskExecution(StopTaskExecutionRequest) returns (StopTaskExecutionResponse);
}
```

## 節點信任等級系統

### 信任等級分類
- **高信任 (High Trust)**: 信用評分 ≥ 100，具備 Docker 環境
- **中信任 (Normal Trust)**: 信用評分 50-99，具備 Docker 環境
- **低信任 (Low Trust)**: 信用評分 < 50 或無 Docker 環境

### 任務分配策略
1. **優先級排序**: 高信任 > 中信任 > 低信任
2. **資源匹配**: 基於 CPU、記憶體、GPU 需求進行匹配
3. **負載均衡**: 優先分配給負載較低的節點
4. **地理位置**: 支援指定地理位置偏好

## 資源管理

### 資源類型
- **CPU 分數**: 處理器運算能力評分
- **記憶體**: GB 為單位的記憶體容量
- **GPU 分數**: 圖形處理器運算能力評分
- **GPU 記憶體**: GB 為單位的顯示記憶體容量

### 動態資源分配
- 追蹤總資源和可用資源
- 支援資源超額分配 (overcommit)
- 任務完成後自動釋放資源
- 實時監控資源使用率

## 配置說明

### 環境變數
```bash
# gRPC 服務配置
GRPC_SERVER_HOST=0.0.0.0
GRPC_SERVER_PORT=50051

# Redis 配置 (節點狀態儲存)
REDIS_HOST=localhost
REDIS_PORT=6379
REDIS_DB=0

# 資料庫配置
DB_PATH=./users.db

# JWT 認證配置
JWT_SECRET_KEY=your-secret-key
TOKEN_EXPIRATION_HOURS=24

# 檔案儲存配置
TASK_STORAGE_PATH=/mnt/myusb/hivemind/task_storage
MAX_FILE_SIZE=10485760  # 10MB
```

## 部署要求

### 系統需求
- Python 3.8+
- Redis 服務器
- 足夠的磁碟空間用於任務檔案儲存

### 必要套件
```
grpcio==1.60.0
grpcio-tools==1.60.0
redis==5.0.1
flask==3.0.0
flask-cors==4.0.0
protobuf==4.25.1
psutil==5.9.6
requests==2.31.0
```

## 啟動服務

### 1. 安裝相依套件
```bash
pip install -r requirements.txt
```

### 2. 配置環境變數
```bash
# 複製環境配置檔
cp .env.example .env
# 編輯配置檔
nano .env
```

### 3. 啟動 Redis 服務
```bash
redis-server
```

### 4. 啟動節點池服務
```bash
python node_pool_server.py
```

服務將在預設端口 50051 啟動 gRPC 服務。

## 監控與維護

### 日誌監控
- 節點註冊/離線事件
- 任務分配和完成狀態
- 資源使用情況
- 錯誤和異常狀況

### 健康檢查
```bash
# 檢查服務狀態
grpcurl -plaintext localhost:50051 nodepool.NodeManagerService/HealthCheck
```

### 資料庫維護
- 定期清理過期的任務記錄
- 備份用戶資料庫
- 監控 Redis 記憶體使用

## 安全性

### 認證機制
- JWT token 認證
- 密碼 bcrypt 加密
- 請求速率限制

### 資料保護
- 敏感資料加密儲存
- 安全的檔案上傳驗證
- 防止路徑遍歷攻擊

## 故障排除

### 常見問題
1. **Redis 連接失敗**: 檢查 Redis 服務是否啟動
2. **節點註冊失敗**: 驗證網路連接和認證資訊
3. **任務分配失敗**: 檢查資源需求和可用節點
4. **檔案上傳失敗**: 確認檔案大小限制和儲存空間

### 調試模式
```bash
# 啟用詳細日誌
export LOG_LEVEL=DEBUG
python node_pool_server.py
```

## 效能調優

### gRPC 配置
- 調整訊息大小限制 (100MB)
- 設定適當的線程池大小 (20 threads)
- 優化 keep-alive 參數

### Redis 優化
- 設定適當的記憶體限制
- 啟用持久化選項
- 監控連接數量

## 擴展性

### 水平擴展
- 支援多個節點池實例
- 使用負載均衡器分散請求
- Redis 集群模式

### 垂直擴展
- 增加服務器記憶體和 CPU
- 優化資料庫查詢效能
- 調整並發處理能力

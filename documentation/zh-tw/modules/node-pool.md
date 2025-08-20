# Node Pool 模組文檔

## 📋 概述

Node Pool 是 HiveMind 分散式計算平台的核心資源調度系統，負責管理所有計算節點的註冊、監控和任務分配。

## 🏗️ 系統架構

```
┌─────────────────────┐
│    Node Pool        │
│     Server          │
├─────────────────────┤
│ • Node Manager      │
│ • User Manager      │
│ • Master Node Svc   │
│ • Database Manager  │
└─────────────────────┘
        │
        ├─ SQLite Database
        ├─ Redis Cache
        └─ gRPC Services
```

## 🔧 核心組件

### 1. Node Pool Server (`node_pool_server.py`)
- **功能**: 主要 gRPC 服務器
- **端口**: 50051
- **協議**: gRPC with Protocol Buffers

### 2. Node Manager (`node_manager.py`)
- **功能**: 節點生命週期管理
- **職責**:
  - 節點註冊和認證
  - 狀態監控和更新
  - 性能指標收集
  - 故障檢測和恢復

### 3. User Manager (`user_manager.py`)
- **功能**: 用戶身份驗證和授權
- **職責**:
  - 用戶註冊和登入
  - 權限管理
  - 會話管理
  - 安全驗證

### 4. Database Manager (`database_manager.py`)
- **功能**: 數據持久化層
- **支援**:
  - SQLite 主數據庫
  - Redis 快取層
  - 數據遷移
  - 備份恢復

## 📡 gRPC 服務定義

### Node Management Service
```protobuf
service NodeManager {
    rpc RegisterNode(RegisterNodeRequest) returns (RegisterNodeResponse);
    rpc UpdateNodeStatus(UpdateNodeStatusRequest) returns (UpdateNodeStatusResponse);
    rpc GetNodeList(GetNodeListRequest) returns (GetNodeListResponse);
    rpc RemoveNode(RemoveNodeRequest) returns (RemoveNodeResponse);
}
```

### User Management Service
```protobuf
service UserManager {
    rpc RegisterUser(RegisterUserRequest) returns (RegisterUserResponse);
    rpc LoginUser(LoginUserRequest) returns (LoginUserResponse);
    rpc UpdateUserProfile(UpdateUserProfileRequest) returns (UpdateUserProfileResponse);
    rpc GetUserInfo(GetUserInfoRequest) returns (GetUserInfoResponse);
}
```

### Master Node Service
```protobuf
service MasterNodeService {
    rpc SubmitTask(SubmitTaskRequest) returns (SubmitTaskResponse);
    rpc GetTaskStatus(GetTaskStatusRequest) returns (GetTaskStatusResponse);
    rpc CancelTask(CancelTaskRequest) returns (CancelTaskResponse);
}
```

## 🗄️ 數據庫架構

### SQLite 表結構

#### users 表
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    email TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_login TIMESTAMP,
    is_active BOOLEAN DEFAULT 1
);
```

#### nodes 表
```sql
CREATE TABLE nodes (
    id TEXT PRIMARY KEY,
    user_id INTEGER,
    ip_address TEXT NOT NULL,
    port INTEGER NOT NULL,
    status TEXT DEFAULT 'offline',
    cpu_cores INTEGER,
    memory_gb REAL,
    disk_gb REAL,
    registered_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_heartbeat TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users (id)
);
```

#### tasks 表
```sql
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    user_id INTEGER,
    task_type TEXT NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    assigned_node_id TEXT,
    result TEXT,
    FOREIGN KEY (user_id) REFERENCES users (id),
    FOREIGN KEY (assigned_node_id) REFERENCES nodes (id)
);
```

## 🚀 部署和配置

### 啟動 Node Pool 服務
```bash
cd node_pool
python node_pool_server.py
```

### 配置文件 (`config.py`)
```python
# 數據庫配置
DATABASE_PATH = "users.db"
REDIS_HOST = "localhost"
REDIS_PORT = 6379

# gRPC 配置
GRPC_PORT = 50051
MAX_WORKERS = 10

# 節點管理配置
NODE_HEARTBEAT_INTERVAL = 30  # 秒
NODE_TIMEOUT = 120  # 秒
MAX_NODES_PER_USER = 10
```

## 🔍 監控和日誌

### 主要指標
- 活躍節點數量
- 待處理任務數量
- 系統響應時間
- 錯誤率統計

### 日誌級別
- **INFO**: 正常操作記錄
- **WARNING**: 性能警告
- **ERROR**: 錯誤和異常
- **DEBUG**: 詳細調試信息

## 🛠️ API 使用範例

### Python 客戶端範例
```python
import grpc
from node_pool import nodepool_pb2, nodepool_pb2_grpc

# 建立連接
channel = grpc.insecure_channel('localhost:50051')
stub = nodepool_pb2_grpc.NodeManagerStub(channel)

# 註冊節點
request = nodepool_pb2.RegisterNodeRequest(
    node_id="worker-001",
    ip_address="192.168.1.100",
    port=50052,
    cpu_cores=4,
    memory_gb=8.0,
    disk_gb=100.0
)

response = stub.RegisterNode(request)
print(f"節點註冊結果: {response.success}")
```

## 🔧 常見問題排除

### 1. gRPC 連接失敗
**問題**: `grpc._channel._InactiveRpcError`
**解決**:
```bash
# 檢查服務是否運行
netstat -an | grep 50051

# 檢查防火牆設置
sudo ufw allow 50051
```

### 2. 數據庫鎖定錯誤
**問題**: `sqlite3.OperationalError: database is locked`
**解決**:
```python
# 增加連接超時
connection = sqlite3.connect('users.db', timeout=20.0)
```

### 3. Redis 連接問題
**問題**: Redis 連接失敗
**解決**:
```bash
# 啟動 Redis 服務
sudo systemctl start redis-server

# 檢查 Redis 狀態
redis-cli ping
```

## 📊 性能調優

### 數據庫優化
```sql
-- 添加索引
CREATE INDEX idx_nodes_status ON nodes(status);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_nodes_last_heartbeat ON nodes(last_heartbeat);
```

### Redis 配置優化
```bash
# 增加 Redis 記憶體限制
maxmemory 2gb
maxmemory-policy allkeys-lru
```

## 🔄 維護和備份

### 定期維護腳本
```bash
#!/bin/bash
# cleanup_old_data.sh

# 清理超過 30 天的離線節點
sqlite3 users.db "DELETE FROM nodes WHERE status='offline' AND last_heartbeat < datetime('now', '-30 days');"

# 清理完成的任務記錄（保留 7 天）
sqlite3 users.db "DELETE FROM tasks WHERE status='completed' AND created_at < datetime('now', '-7 days');"
```

### 數據庫備份
```bash
#!/bin/bash
# backup_database.sh

DATE=$(date +%Y%m%d_%H%M%S)
cp users.db "backup/users_${DATE}.db"
```

## 📈 擴展性考量

### 水平擴展
- 多個 Node Pool 實例
- 負載均衡器配置
- 數據庫分片策略

### 垂直擴展
- 增加 gRPC 工作線程
- 優化數據庫查詢
- Redis 記憶體擴展

## 🔐 安全性

### 身份驗證
- JWT Token 驗證
- API Key 管理
- 角色權限控制

### 網路安全
- TLS 加密通信
- VPN 隧道連接
- 防火牆規則配置

---

**相關文檔**:
- [API 文檔](../api.md)
- [部署指南](../deployment.md)
- [故障排除](../troubleshooting.md)

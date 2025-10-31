# Node Pool 服務

這是 HiveMind 分散式計算系統的節點池管理服務，負責管理工作節點、用戶認證、任務分發等核心功能。

## 功能特性

### 用戶服務 (UserService)
- **Login** - 用戶登錄認證
- **Transfer** - 用戶間代幣轉帳
- **GetBalance** - 查詢用戶餘額
- **RefreshToken** - 刷新認證令牌

### 節點管理服務 (NodeManagerService)  
- **RegisterWorkerNode** - 工作節點註冊

### 主節點服務 (MasterNodeService)
- **UploadTask** - 任務上傳
- **GetTaskResult** - 獲取任務結果
- **GetAllUserTasks** - 獲取用戶所有任務
- **StopTask** - 停止任務執行

### 工作節點服務 (WorkerNodeService)
- **ExecuteTask** - 執行任務
- **TaskOutputUpload** - 任務輸出上傳
- **TaskResultUpload** - 任務結果上傳
- **TaskOutput** - 任務輸出處理
- **StopTaskExecution** - 停止任務執行
- **TaskUsage** - 任務資源使用情況報告

## 安裝與配置

### 1. 安裝依賴
```bash
pip install -r requirements.txt
```

### 2. 配置環境
編輯 `.env` 文件或設定環境變數：
```
DATABASE_PATH=users.db
SECRET_KEY=your-secret-key-here
TOKEN_EXPIRY=60
REDIS_HOST=localhost
REDIS_PORT=6379
TASK_STORAGE_PATH=/path/to/task/storage
```

### 3. 啟動服務
```bash
python start_server.py
```

或直接運行：
```bash
python node_pool_server.py
```

## 服務架構

```
node_pool/
├── config.py              # 配置管理
├── database_manager.py    # 資料庫管理
├── user_manager.py        # 用戶管理
├── node_manager.py        # 節點管理
├── user_service.py        # 用戶服務實現
├── node_manager_service.py # 節點管理服務實現
├── master_node_service.py # 主節點服務實現
├── worker_node_service.py # 工作節點服務實現
├── node_pool_server.py    # gRPC 服務器
├── start_server.py        # 啟動腳本
├── test_services.py       # 測試腳本
└── monitor_service.py     # 監控服務
```

## API 使用說明

### gRPC 連接
服務默認運行在 `localhost:50051`

### 用戶認證流程
1. 使用 `Login` 方法獲取認證令牌
2. 在後續請求中攜帶令牌進行認證
3. 令牌過期時使用 `RefreshToken` 刷新

### 任務執行流程
1. 用戶通過 `UploadTask` 上傳任務
2. 系統自動分配合適的工作節點執行任務
3. 用戶可通過 `GetAllUserTasks` 查看任務狀態
4. 任務完成後通過 `GetTaskResult` 獲取結果

## 測試

運行測試腳本：
```bash
python test_services.py
```

## 監控

Web 監控界面：
```bash
python monitor_service.py
```
然後訪問 `http://localhost:5001`

## 依賴服務

- **Redis**: 用於快取和任務隊列管理
- **SQLite**: 用於用戶資料存儲

## 故障排除

### 常見問題
1. **端口被占用**: 檢查 50051 端口是否被其他服務使用
2. **Redis 連接失敗**: 確保 Redis 服務正在運行
3. **資料庫錯誤**: 檢查資料庫文件權限和路徑

### 日誌查看
服務日誌會輸出到控制台，包含詳細的錯誤信息和操作記錄。

## 開發指南

### 添加新的 RPC 方法
1. 在 `nodepool.proto` 中定義新的 message 和 service
2. 重新生成 protobuf 文件
3. 在對應的服務類中實現新方法
4. 更新測試文件

### 代碼風格
- 使用 Python 3.8+ 
- 遵循 PEP 8 代碼風格
- 添加適當的錯誤處理和日誌記錄
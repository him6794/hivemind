# HiveMind 快速測試指南

## 🚀 快速啟動（5 分鐘）

### 步驟 1: 啟動基礎服務

```powershell
# 啟動 Redis
docker run -d -p 6379:6379 --name redis-hivemind redis:7-alpine

# 啟動 PostgreSQL
docker run -d -p 5432:5432 --name pg-hivemind `
  -e POSTGRES_USER=hivemind `
  -e POSTGRES_PASSWORD=hivemind `
  -e POSTGRES_DB=hivemind postgres:16-alpine

# 等待服務啟動
Start-Sleep -Seconds 3
Write-Host "✓ Redis 和 PostgreSQL 已啟動"
```

### 步驟 2: 啟動 Nodepool（調度中心）

```powershell
# 開啟新終端
cd services/nodepool/cmd/server

# 設定環境變數
$env:NODEPOOL_POSTGRES_DSN = "postgres://hivemind:hivemind@localhost:5432/hivemind?sslmode=disable"
$env:NODEPOOL_REDIS_ADDR = "localhost:6379"
$env:NODEPOOL_GRPC_PORT = "50051"

# 啟動
go run .
```

**預期輸出**:
```
[INFO] Nodepool gRPC server listening on :50051
[INFO] Connected to PostgreSQL
[INFO] Connected to Redis
```

### 步驟 3: 啟動 Master（任務提交端）

```powershell
# 開啟新終端
cd services/master/cmd/server

# 設定環境變數
$env:MASTER_HTTP_PORT = "8082"
$env:NODEPOOL_ADDR = "localhost:50051"

# 啟動
go run .
```

**預期輸出**:
```
[INFO] Master HTTP server listening on :8082
[INFO] Connected to Nodepool at localhost:50051
```

### 步驟 4: 啟動 Worker（計算節點）

```powershell
# 開啟新終端
cd services/worker/cmd/server

# 設定環境變數
$env:WORKER_ID = "worker-001"
$env:NODEPOOL_ADDR = "localhost:50051"
$env:WORKER_GRPC_PORT = "50053"

# 啟動
go run .
```

**預期輸出**:
```
[INFO] Worker gRPC server listening on :50053
[INFO] Registered to Nodepool
[INFO] Worker ID: worker-001
```

---

## 📝 測試流程

### 測試 1: 提交簡單任務

#### 1.1 準備測試任務

建立 `test_task/main.py`:
```python
print("Hello from HiveMind!")
print("Task executed successfully")

# 簡單計算
result = sum(range(1, 101))
print(f"Sum of 1-100: {result}")
```

#### 1.2 打包任務

```powershell
# 建立 ZIP
Compress-Archive -Path test_task/* -DestinationPath task.zip -Force
```

#### 1.3 提交任務（使用 curl）

```powershell
# 提交任務
curl -X POST http://localhost:8082/api/tasks `
  -F "file=@task.zip" `
  -F "cpu_score=2" `
  -F "memory_gb=1"
```

**預期回應**:
```json
{
  "success": true,
  "task_id": "task-abc123",
  "message": "Task submitted successfully"
}
```

#### 1.4 查詢任務狀態

```powershell
# 查詢狀態
curl http://localhost:8082/api/tasks/task-abc123
```

**預期回應**:
```json
{
  "task_id": "task-abc123",
  "status": "COMPLETED",
  "stdout": "Hello from HiveMind!\nTask executed successfully\nSum of 1-100: 5050\n",
  "worker_ip": "worker-001"
}
```

---

### 測試 2: 資源限制測試

#### 2.1 建立記憶體密集任務

建立 `memory_test/main.py`:
```python
# 嘗試分配大量記憶體
data = []
for i in range(1000):
    data.append([0] * 1000000)  # 會超過限制
    print(f"Allocated {i+1} MB")
```

#### 2.2 提交任務（設定低記憶體限制）

```powershell
Compress-Archive -Path memory_test/* -DestinationPath memory_task.zip -Force

curl -X POST http://localhost:8082/api/tasks `
  -F "file=@memory_task.zip" `
  -F "cpu_score=1" `
  -F "memory_gb=1"  # 限制 1GB
```

**預期結果**: 任務會因記憶體超限被終止

---

### 測試 3: 多節點協作（需要 VPN）

#### 3.1 啟動第二個 Worker

```powershell
# 開啟新終端
cd services/worker/cmd/server

$env:WORKER_ID = "worker-002"
$env:NODEPOOL_ADDR = "localhost:50051"
$env:WORKER_GRPC_PORT = "50054"

go run .
```

#### 3.2 提交多節點任務

```powershell
curl -X POST http://localhost:8082/api/tasks `
  -F "file=@task.zip" `
  -F "cpu_score=2" `
  -F "memory_gb=1" `
  -F "host_count=2"  # 需要 2 個 Worker
```

---

## 🎨 使用 Web UI（可選）

### 啟動 Master UI

```powershell
cd frontend/master-ui
npm install
npm run dev
```

訪問: http://localhost:3000

**功能**:
- 登入系統
- 上傳任務 ZIP
- 查詢任務狀態
- 下載結果

### 啟動 Worker UI

```powershell
cd frontend/worker-ui
npm install
npm run dev
```

訪問: http://localhost:3001

**功能**:
- 查看 Worker 狀態
- 硬體資訊
- 註冊 Worker

---

## 🔍 監控與除錯

### 查看 Nodepool 日誌

```powershell
# 即時查看
Get-Content nodepool.log -Tail 20 -Wait

# 搜尋特定任務
Select-String "task-abc123" nodepool.log
```

### 查看 Redis 資料

```powershell
# 連接 Redis
redis-cli

# 查看所有任務
KEYS task:*

# 查看特定任務
HGETALL task:task-abc123

# 查看活躍任務
SMEMBERS tasks:active
```

### 查看 PostgreSQL 資料

```powershell
# 連接資料庫
docker exec -it pg-hivemind psql -U hivemind

# 查看 Worker
SELECT * FROM workers;

# 查看任務
SELECT * FROM tasks;
```

---

## 🛑 停止服務

```powershell
# 停止所有 Go 進程
Get-Process | Where-Object {$_.ProcessName -like "*go*"} | Stop-Process -Force

# 停止 Docker 容器
docker stop redis-hivemind pg-hivemind
docker rm redis-hivemind pg-hivemind
```

---

## 📊 預期效能

### 任務執行時間
- **簡單任務**: < 1 秒
- **中等任務**: 1-10 秒
- **複雜任務**: 10-60 秒

### 資源使用
- **Nodepool**: ~50MB RAM
- **Master**: ~30MB RAM
- **Worker**: ~40MB RAM
- **Monty 執行器**: ~10MB RAM

---

## ❓ 常見問題

### Q: Nodepool 無法連接 Redis
**A**: 確認 Redis 正在運行
```powershell
docker ps | Select-String redis
redis-cli ping
```

### Q: Worker 無法註冊
**A**: 確認 Nodepool 正在運行
```powershell
curl http://localhost:50051
```

### Q: 任務一直 PENDING
**A**: 確認至少有一個 Worker 已註冊
```powershell
redis-cli
KEYS node:*
```

### Q: Monty.exe 找不到
**A**: 確認 Monty 執行檔路徑
```powershell
# 檢查路徑
Test-Path "C:\Users\user\Desktop\monty\dist\monty.exe"

# 如果不存在，需要編譯 Monty
cd executor-rs
cargo build --release
```

---

## 🎯 下一步

1. ✅ 完成基礎測試
2. 📝 嘗試自己的 Python 任務
3. 🔧 調整資源限制
4. 🌐 測試多節點協作
5. 📊 查看監控數據

---

**提示**: 如果遇到問題，查看各服務的日誌輸出，通常會有詳細的錯誤訊息。

# HiveMind 使用指南

## 🚀 三步驟快速開始

### 1. 啟動 HiveMind

```powershell
./start_hivemind.ps1
```

**這個腳本會自動**:
- ✅ 啟動 Redis 和 PostgreSQL (Docker)
- ✅ 啟動 Nodepool (調度中心)
- ✅ 啟動 Master (任務提交端)
- ✅ 啟動 Worker (計算節點)

**預期輸出**:
```
==================================
  HiveMind 已成功啟動！
==================================

服務狀態:
  • Redis:      localhost:6379
  • PostgreSQL: localhost:5432
  • Nodepool:   localhost:50051 (gRPC)
  • Master:     http://localhost:8082
  • Worker:     localhost:50053 (gRPC)
```

---

### 2. 運行測試

```powershell
./test_hivemind.ps1
```

**這個腳本會自動**:
- ✅ 檢查服務狀態
- ✅ 建立測試任務 (Python)
- ✅ 提交任務到 HiveMind
- ✅ 等待執行完成
- ✅ 顯示執行結果

**預期輸出**:
```
==================================
  任務執行結果
==================================

任務 ID: task-abc123
狀態: COMPLETED
Worker: worker-001

標準輸出:
==================================================
HiveMind 測試任務
==================================================

計算結果: 1+2+...+100 = 5050
訊息: Hello from HiveMind!
訊息長度: 20
數字列表: [1, 2, 3, 4, 5]
列表總和: 15

==================================================
任務執行成功！
==================================================
```

---

### 3. 停止服務

```powershell
./stop_hivemind.ps1
```

或在啟動腳本中按 `Ctrl+C`

---

## 📝 手動提交任務

### 步驟 1: 建立 Python 任務

建立 `my_task/main.py`:
```python
# 你的 Python 程式碼
print("Hello from my task!")

# 進行計算
result = sum(range(1, 1001))
print(f"Result: {result}")
```

### 步驟 2: 打包任務

```powershell
Compress-Archive -Path my_task/* -DestinationPath my_task.zip -Force
```

### 步驟 3: 提交任務

```powershell
curl -X POST http://localhost:8082/api/tasks `
  -F "file=@my_task.zip" `
  -F "cpu_score=2" `
  -F "memory_gb=1"
```

**回應**:
```json
{
  "success": true,
  "task_id": "task-xyz789",
  "message": "Task submitted successfully"
}
```

### 步驟 4: 查詢結果

```powershell
curl http://localhost:8082/api/tasks/task-xyz789
```

**回應**:
```json
{
  "task_id": "task-xyz789",
  "status": "COMPLETED",
  "stdout": "Hello from my task!\nResult: 500500\n",
  "worker_ip": "worker-001",
  "resource_usage": {
    "cpu_percent": 15.5,
    "memory_mb": 25
  }
}
```

---

## 🎯 進階功能

### 1. 自定義資源限制

```powershell
curl -X POST http://localhost:8082/api/tasks `
  -F "file=@task.zip" `
  -F "cpu_score=4" `
  -F "memory_gb=2" `
  -F "timeout=300"  # 5 分鐘超時
```

### 2. 多節點任務

```powershell
# 先啟動第二個 Worker
cd services/worker/cmd/server
$env:WORKER_ID = "worker-002"
$env:WORKER_GRPC_PORT = "50054"
go run .

# 提交多節點任務
curl -X POST http://localhost:8082/api/tasks `
  -F "file=@task.zip" `
  -F "host_count=2"  # 需要 2 個 Worker
```

### 3. 查看所有任務

```powershell
curl http://localhost:8082/api/tasks
```

### 4. 停止任務

```powershell
curl -X POST http://localhost:8082/api/tasks/task-xyz789/stop
```

---

## 🔍 監控與除錯

### 查看 Redis 資料

```powershell
# 連接 Redis
docker exec -it redis-hivemind redis-cli

# 查看所有任務
KEYS task:*

# 查看特定任務
HGETALL task:task-xyz789

# 查看所有 Worker
KEYS node:*
```

### 查看資料庫

```powershell
# 連接 PostgreSQL
docker exec -it pg-hivemind psql -U hivemind

# 查看 Worker
SELECT * FROM workers;

# 查看任務
SELECT * FROM tasks;
```

### 查看日誌

如果使用 `start_hivemind.ps1` 啟動，可以查看 Job 輸出:

```powershell
# 列出所有 Jobs
Get-Job

# 查看特定 Job 的輸出
Receive-Job <Job-ID>

# 例如
Receive-Job 1  # Nodepool
Receive-Job 2  # Master
Receive-Job 3  # Worker
```

---

## 📊 效能測試

### 測試 1: 簡單任務

```python
# simple_task.py
print("Simple task")
```

**預期**: < 1 秒完成

### 測試 2: 計算密集任務

```python
# compute_task.py
result = sum(i**2 for i in range(1000000))
print(f"Result: {result}")
```

**預期**: 1-5 秒完成

### 測試 3: 記憶體測試

```python
# memory_task.py
data = [0] * 10000000  # 約 80MB
print(f"Allocated {len(data)} integers")
```

**預期**: 正常完成，資源監控顯示記憶體使用

---

## ❓ 常見問題

### Q: 啟動腳本卡住不動
**A**: 檢查是否有端口衝突
```powershell
netstat -ano | findstr "6379"  # Redis
netstat -ano | findstr "5432"  # PostgreSQL
netstat -ano | findstr "50051" # Nodepool
netstat -ano | findstr "8082"  # Master
```

### Q: 任務一直 PENDING
**A**: 確認 Worker 已註冊
```powershell
docker exec -it redis-hivemind redis-cli
KEYS node:*
```

### Q: 任務執行失敗
**A**: 查看錯誤訊息
```powershell
curl http://localhost:8082/api/tasks/task-xyz789
# 查看 stderr 和 error 欄位
```

### Q: Monty.exe 找不到
**A**: 確認路徑或編譯 Monty
```powershell
# 檢查路徑
Test-Path "C:\Users\user\Desktop\monty\dist\monty.exe"

# 如果不存在，編譯 Monty
cd executor-rs
cargo build --release
```

---

## 📚 相關文檔

- [QUICK_START_GUIDE.md](QUICK_START_GUIDE.md) - 詳細的快速開始指南
- [DEPLOYMENT_GUIDE.md](DEPLOYMENT_GUIDE.md) - 完整部署指南
- [MONTY_RESOURCE_CONTROL_COMPLETION_REPORT.md](MONTY_RESOURCE_CONTROL_COMPLETION_REPORT.md) - Monty 執行器功能
- [VPN_FEATURE_COMPLETION_REPORT.md](VPN_FEATURE_COMPLETION_REPORT.md) - VPN 網路功能
- [PROJECT_FINAL_STATUS_2026_04_30.md](PROJECT_FINAL_STATUS_2026_04_30.md) - 專案狀態

---

## 🎓 下一步

1. ✅ 完成基礎測試
2. 📝 提交自己的 Python 任務
3. 🔧 測試資源限制
4. 🌐 啟動多個 Worker
5. 📊 查看監控數據
6. 🚀 部署到生產環境

---

**提示**: 所有腳本都有詳細的輸出，如果遇到問題，仔細閱讀錯誤訊息通常能找到解決方案。

**需要幫助?** 查看相關文檔或檢查服務日誌。

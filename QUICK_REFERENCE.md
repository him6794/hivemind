# Hivemind 快速參考

## 端口總覽
| 服務 | 端口 | 類型 | 說明 |
|------|------|------|------|
| Master API | 8082 | HTTP | 任務和用戶管理 |
| Nodepool gRPC | 50051 | gRPC | 分佈式調度 |
| Worker gRPC | 50053 | gRPC | 工作節點 |
| Master UI | 3000 | HTTP | 任務管理界面 |
| Worker UI | 3001 | HTTP | Worker 管理界面 |
| Redis | 6379 | TCP | 任務元數據存儲 |

## 用戶登錄
| 應用 | 用戶名 | 密碼 |
|------|--------|------|
| Master UI | testuser | testpass123 |
| Worker UI | worker1 | worker123 |

## 主要 API 端點
```
POST   /api/login              # 用戶登錄
POST   /api/register-worker    # 註冊 Worker 節點
POST   /api/create-torrent     # 上傳 ZIP 生成 Torrent
POST   /api/upload-task        # 提交任務
POST   /api/stop-task          # 停止任務
GET    /api/balance            # 查詢餘額
GET    /api/tasks              # 列舉任務
GET    /api/task/{id}/log      # 查看任務日誌
GET    /api/task/{id}/result   # 查看任務結果
```

## 日誌查看
```powershell
# 實時監控 Nodepool 日誌
Get-Content nodepool.log -Tail 20 -Wait

# 搜索特定任務
Select-String "task-id-123" nodepool.log

# 搜索錯誤
Select-String "ERROR|FAILED" nodepool.log
```

## 任務狀態流轉
```
PENDING  
   ↓ [嘗試分配 Worker]
DISPATCHED  
   ↓ [Worker 響應並執行]
RUNNING  
   ↓ [Worker 上傳結果]
COMPLETED
   ↓ [定期結算]
SETTLED

超時/失敗邏輯:
DISPATCHED → (30秒無響應) → FAILED 或 PENDING (重新分配)
RUNNING    → (連接斷開) → PENDING (重新分配)
```

## 環境變量速查

### 開發環境
```bash
# .env 文件示例
NODEPOOL_REDIS_ADDR=localhost:6379
NODEPOOL_TASK_TIMEOUT_SEC=30
NODEPOOL_MAX_REDISPATCH=2
VITE_API_BASE=http://localhost:8082
```

### 故障排除命令
```powershell
# 檢查端口
netstat -ano | Select-String "8082|50051|6379"

# 檢查進程
Get-Process | Where-Object {$_.Name -match "go|node|redis"}

# 終止進程
Stop-Process -Name "go" -Force

# 清空端口
Get-NetTCPConnection -LocalPort 8082 | Stop-Process -Force
```

## 快速啟動 (複製粘貼)
```powershell
# 終端 1: Redis
docker run -d -p 6379:6379 redis:7-alpine

# 終端 2: Master
cd D:\hivemind\services\master\cmd\server; go run .

# 終端 3: Nodepool
cd D:\hivemind\services\nodepool\cmd\server; $env:NODEPOOL_REDIS_ADDR="localhost:6379"; go run .

# 終端 4: Master UI
cd D:\hivemind\frontend\master-ui; npm install; npm run dev

# 終端 5: Worker UI
cd D:\hivemind\frontend\worker-ui; npm install; npm run dev

# 終端 6: Worker
cd D:\hivemind\services\worker\cmd\server; go run .
```

## 常用 curl 命令
```bash
# 登錄
curl -X POST http://localhost:8082/api/login \
  -H "Content-Type: application/json" \
  -d '{"username":"testuser","password":"testpass123"}'

# 提交任務
curl -X POST http://localhost:8082/api/upload-task \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <TOKEN>" \
  -d '{
    "task_id":"test-1",
    "torrent":"magnet:?xt=urn:btih:...",
    "memory_gb":4,
    "gpu_memory_gb":2
  }'

# 查詢任務
curl http://localhost:8082/api/tasks \
  -H "Authorization: Bearer <TOKEN>"

# 查看日誌
curl http://localhost:8082/api/task/test-1/log \
  -H "Authorization: Bearer <TOKEN>"
```

## Redis 常用命令
```bash
redis-cli

# 查看所有鍵
KEYS *

# 查看特定任務
HGETALL task:task-id-123

# 查看用戶任務
SMEMBERS tasks:owner:testuser

# 查看活躍任務
SMEMBERS tasks:active

# 刪除任務
DEL task:task-id-123

# 監控實時命令
MONITOR
```

## 性能優化

### Redis 內存使用
```bash
# 查看內存使用
redis-cli INFO memory

# 設置最大內存限制
redis-cli CONFIG SET maxmemory 512mb
redis-cli CONFIG SET maxmemory-policy allkeys-lru
```

### 日誌大小管理
```powershell
# 按日期備份日誌
$date = Get-Date -Format "yyyyMMdd"
Copy-Item nodepool.log "nodepool.log.$date"
Clear-Content nodepool.log
```

## 調試技巧

### 啟用詳細日誌
```go
// 在 main.go 中
log.SetFlags(log.LstdFlags | log.Lshortfile)
```

### 監控任務轉移
```bash
# 監控日誌中的 "DISPATCHED" 消息
Select-String "DISPATCHED|RUNNING|FAILED" nodepool.log | Tail -20
```

### 檢查 Worker 健康狀況
```bash
# 查看 Worker 註冊日誌
Select-String "register worker" nodepool.log
```

## 更多文檔
- 完整部署指南: `DEPLOYMENT_GUIDE.md`
- 更新摘要: `docs/UPDATE_SUMMARY_2026_03_25.md`
- Redis 遷移: `docs/REDIS_MIGRATION_PLAN.md`
- UI 分離: `docs/UI_SEPARATION_PLAN.md`

---
**最後更新: 2026-03-25**

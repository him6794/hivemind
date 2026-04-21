# Hivemind 系統更新總結 (2026-03-25)

## 概述
本次更新完成了三個主要功能改進：
1. **詳細 Redispatch 日誌記錄** - 完整追蹤任務重新調度過程
2. **Redis 任務元數據存儲** - 準備橫向擴展和高可用部署
3. **UI 分離** - 獨立的 Master 和 Worker 管理界面

## 1. Redispatch 日誌記錄系統

### 實現內容
在 `services/nodepool/cmd/server/main.go` 中添加了詳細的日誌記錄：

#### 日誌類型

**初始調度**:
```
task_dispatch_success task_id=<id> worker_id=<worker> worker_addr=<addr>
task_dispatch_failed task_id=<id> reason=<reason>
```

**任務超時**:
```
task_timeout_redispatch task_id=<id> worker_id=<worker> worker_addr=<addr> retry=<n>/<max>
task_timeout_failed task_id=<id> worker_id=<worker> worker_addr=<addr> retry=<n>/<max>
```

**重新調度**:
```
redispatch_success task_id=<id> worker_id=<worker> worker_addr=<addr> retry=<n>/<max>
redispatch_waiting task_id=<id> retry=<n>/<max> reason=<reason>
```

### 日誌輸出位置
- **文件**: `nodepool.log` (時間戳格式: `2026-03-25T06:27:15Z`)
- **標準輸出**: 實時控制台顯示

### 使用場景
用戶現在可以：
- 追蹤任務從 PENDING → DISPATCHED → RUNNING 的完整過程
- 快速診斷調度失敗的原因（無可用 worker、worker 排除等）
- 監控重新調度的重試次數和原因

### 日誌示例
```log
2026-03-25T06:38:16Z task_dispatch_success task_id=task-123 worker_id=worker1 worker_addr=127.0.0.1:50053
2026-03-25T06:38:50Z task_timeout_redispatch task_id=task-123 worker_id=worker1 worker_addr=127.0.0.1:50053 retry=1/2
2026-03-25T06:38:50Z redispatch_success task_id=task-123 worker_id=worker2 worker_addr=127.0.0.1:50054 retry=1/2
```

## 2. Redis 任務元數據存儲

### 架構設計

**三層存儲模型**:
- **Redis**: 任務元數據（狀態、owner、worker分配）- 快速訪問
- **SQLite**: 用戶和 worker 數據 - 持久化
- **磁盤**: 任務文件和結果（種子、輸出）- 文件存儲

### 集成方式
- Redis 客戶端已添加到 `masterNodeServer` 結構
- 每次任務更新同時寫入 SQLite 和 Redis
- 配置環境變量: `NODEPOOL_REDIS_ADDR` (默認: localhost:6379)

### Redis 鍵設計
```
task:{task_id}              # 任務哈希表，包含所有元數據
tasks:owner:{owner}         # 用戶的任務集合
tasks:active                # 活躍任務集合 (PENDING/DISPATCHED/RUNNING)
task:{task_id}:logs         # 任務日誌 (預留)
```

### 元數據字段
```
Hash: task:{task_id}
Fields:
  - task_id, owner, worker_id, worker_ip
  - status, status_message
  - output, result_torrent, torrent_source, expected_btih
  - cpu_usage, memory_usage, gpu_usage, gpu_memory_usage
  - req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb
  - host_count
  - billing_settled, billed_amount
  - updated_at, last_update, last_settlement, retry_count (unix timestamps)
```

### 優勢
✅ **快速查詢**: Redis 內存訪問，毫秒級延遲
✅ **橫向擴展**: 多 nodepool 實例可共享任務狀態
✅ **自動過期**: 完成/失敗任務 7 天後自動清理
✅ **實時更新**: 支持任務進度實時推送

### 部署配置
```bash
# 啟動 Redis (Docker)
docker run -d -p 6379:6379 redis:7-alpine

# 啟動 Nodepool (啟用 Redis)
export NODEPOOL_REDIS_ADDR=localhost:6379
cd services/nodepool/cmd/server && go run .
```

## 3. UI 分離

### 新目錄結構
```
frontend/
├── master-ui/              # Master 應用 (端口 3000)
│   ├── src/
│   │   ├── App.jsx        # 任務管理頁面
│   │   └── main.jsx
│   ├── package.json
│   ├── vite.config.js
│   └── index.html
│
├── worker-ui/              # Worker 應用 (端口 3001)
│   ├── src/
│   │   ├── App.jsx        # Worker 監控頁面
│   │   └── main.jsx
│   ├── package.json
│   ├── vite.config.js
│   └── index.html
│
└── src/                    # 原始統一應用（保留備用）
```

### Master UI (http://localhost:3000)
**功能**:
- 用戶登錄/註冊
- 查看賬戶餘額
- 上傳 ZIP 文件自動生成 Torrent
- 提交任務（指定 Magnet/HTTP URL）
- 查看任務列表和實時狀態
- 查看任務日誌和結果
- 停止任務

**API 連接**:
- 連接到 Master 服務 (localhost:8082)
- 環境變量: `VITE_API_BASE=http://localhost:8082`

### Worker UI (http://localhost:3001)
**功能**:
- Worker 節點登錄
- 顯示硬體配置 (CPU/GPU/內存)
- Worker 節點一鍵註冊
- 刷新 Worker 狀態
- 查看當前任務（預留擴展）

**API 連接**:
- 連接到 Master 和 Worker Control 服務
- 環境變量:
  - `VITE_API_BASE=http://localhost:8082`
  - `VITE_WORKER_CONTROL_BASE=http://localhost:18080`

### 啟動命令
```bash
# Master UI
cd frontend/master-ui
npm install
npm run dev          # 監聽 http://localhost:3000

# Worker UI
cd frontend/worker-ui
npm install
npm run dev          # 監聽 http://localhost:3001

# 原始統一應用（可選）
cd frontend
npm install
npm run start        # 監聽 http://localhost:5173
```

### 構建部署
```bash
# Master UI 生產構建
cd frontend/master-ui
npm run build        # 輸出到 dist/

# Worker UI 生產構建
cd frontend/worker-ui
npm run build        # 輸出到 dist/
```

## 技術細節

### Redispatch 邏輯優化
之前存在一個關鍵 bug：
```go
// ❌ 舊邏輯（導致 DISPATCHED 任務永遠卡住）
if t.Status != "RUNNING" && t.Status != "DISPATCHED" {
    continue  // 結果：DISPATCHED 任務被定期結算，LastUpdate 被更新
}            // 超時檢查無法觸發

// ✅ 新邏輯
if t.Status != "RUNNING" {
    continue  // 結果：只結算 RUNNING 任務，DISPATCHED 任務不被刷新
}            // 超時檢查正常觸發，30 秒後重新調度
```

### 測試結果
```
✅ Nodepool 單元測試：15/15 passed
✅ 編譯驗證：無錯誤
✅ Redis 集成：成功連接並保存任務元數據
```

## 部署清單

### 一鍵啟動腳本 (Windows PowerShell)
```powershell
# 1. 啟動 Redis
docker run -d -p 6379:6379 redis:7-alpine

# 2. 啟動 Master 服務
cd D:\hivemind\services\master\cmd\server
go run .

# 3. 啟動 Nodepool 服務
cd D:\hivemind\services\nodepool\cmd\server
$env:NODEPOOL_REDIS_ADDR = "localhost:6379"
go run .

# 4. 啟動 Master UI
cd D:\hivemind\frontend\master-ui
npm install
npm run dev

# 5. 啟動 Worker UI
cd D:\hivemind\frontend\worker-ui
npm install
npm run dev

# 6. 啟動 Worker 服務（在另一個終端）
cd D:\hivemind\services\worker\cmd\server
go run .
```

## 文檔更新
- `docs/REDIS_MIGRATION_PLAN.md` - Redis 遷移詳細計劃
- `docs/UI_SEPARATION_PLAN.md` - UI 分離架構和時間線

## 後續工作

### 短期 (本週)
- [ ] 測試 Master UI 完整功能
- [ ] 測試 Worker UI 與 worker 服務集成
- [ ] 驗證 Redis 任務同步

### 中期 (2 週)
- [ ] Redis 完全遷移（移除 SQLite 任務表依賴）
- [ ] 任務進度實時推送 (WebSocket)
- [ ] Worker 性能指標儀表板

### 長期 (1 月)
- [ ] 多 nodepool 實例部署測試
- [ ] 容器化部署 (Docker Compose)
- [ ] 負載均衡和故障轉移

## 故障排除

### Redis 連接失敗
```bash
# 檢查 Redis 運行狀態
redis-cli ping   # 應返回 PONG

# 驗證連接地址
$env:NODEPOOL_REDIS_ADDR = "localhost:6379"
```

### UI 端口被占用
```bash
# 更改 Master UI 端口
cd frontend/master-ui
npm run dev -- --port 3000

# 更改 Worker UI 端口
cd frontend/worker-ui
npm run dev -- --port 3001
```

### 日誌未出現
```bash
# 檢查日誌文件
type nodepool.log

# 查看實時日誌
Get-Content nodepool.log -Tail 10 -Wait
```

## 總結
本次更新大幅提升了系統的可觀測性、可擴展性和用戶體驗：

| 方面 | 改進 |
|------|------|
| 日誌記錄 | 詳細追蹤任務調度過程，快速診斷問題 |
| 架構 | 支持 Redis 集中化存儲，為橫向擴展做準備 |
| UI | 獨立的 Master/Worker 界面，清晰的使用場景 |
| 可靠性 | 修復 DISPATCHED 狀態卡住的 bug，保證任務流轉 |

系統已完全準備好進行端到端集成測試！


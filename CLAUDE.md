# CLAUDE.md

這個檔案為 Claude Code (claude.ai/code) 提供在此程式碼庫中工作的指引。

## 專案概述

Hivemind 是一個分散式任務調度系統，由 Go 後端服務和 React 前端組成。系統採用三層架構：

- **Master 服務**：HTTP API (8082)，處理用戶認證、任務提交、Torrent 生成
- **Nodepool 服務**：gRPC (50051)，核心調度器，管理 Worker 註冊、任務分配、超時重分配、計費結算
- **Worker 服務**：gRPC (50053)，執行任務並回報結果

資料存儲使用 **Redis** (任務元數據) 和 **PostgreSQL** (持久化資料)。

## 建置與測試

### 啟動服務（開發環境）

**必須按順序啟動**：

```powershell
# 1. 啟動 PostgreSQL
docker run -d -p 5432:5432 --name pg-hivemind `
  -e POSTGRES_USER=hivemind `
  -e POSTGRES_PASSWORD=hivemind `
  -e POSTGRES_DB=hivemind postgres:16-alpine

# 2. 啟動 Redis
docker run -d -p 6379:6379 --name redis-hivemind redis:7-alpine

# 3. 啟動 Nodepool (必須先於 Master 和 Worker)
cd services/nodepool/cmd/server
$env:NODEPOOL_POSTGRES_DSN = "postgres://hivemind:hivemind@localhost:5432/hivemind?sslmode=disable"
$env:NODEPOOL_REDIS_ADDR = "localhost:6379"
$env:NODEPOOL_TASK_TIMEOUT_SEC = "30"
$env:NODEPOOL_MAX_REDISPATCH = "2"
go run .

# 4. 啟動 Master
cd services/master/cmd/server
go run .

# 5. 啟動 Worker
cd services/worker/cmd/server
go run .

# 6. 啟動前端 (可選)
cd frontend/master-ui && npm install && npm run dev  # Port 3000
cd frontend/worker-ui && npm install && npm run dev  # Port 3001
```

### 執行測試

```bash
# 執行所有測試前需要先修復依賴
cd services/nodepool
go mod download
go mod tidy

# 執行測試
cd services/nodepool && go test ./...
cd services/master && go test ./...
cd services/worker && go test ./...

# 執行特定測試
go test -v ./internal/service -run TestWorkerService
```

### 建置生產版本

```bash
# 建置後端服務
cd services/master/cmd/server && go build -o master.exe
cd services/nodepool/cmd/server && go build -o nodepool.exe
cd services/worker/cmd/server && go build -o worker.exe

# 建置前端
cd frontend/master-ui && npm run build
cd frontend/worker-ui && npm run build
```

## 架構關鍵點

### gRPC 通訊模式

**重要陷阱**：`WorkerNodeService` 在 Nodepool 和 Worker **兩側都有實作**，但用途不同：
- **Nodepool 端**：接收 Worker 的狀態回報 (`ReportRunningStatus`, `UploadTaskResult`)
- **Worker 端**：接收 Nodepool 的任務分配 (`ExecuteTask`)

修改 proto 時必須同步更新三個服務的生成檔案。

### Proto 檔案生成

Proto 定義位於 `proto/hivemind.proto` 和 `proto/vpn.proto`。生成 Go 程式碼：

```bash
# 生成 pb 檔案 (需要 protoc 和 protoc-gen-go-grpc)
protoc --go_out=services/nodepool/pb --go-grpc_out=services/nodepool/pb proto/hivemind.proto
protoc --go_out=services/nodepool/pb --go-grpc_out=services/nodepool/pb proto/vpn.proto

# Master 和 Worker 使用 nodepool/pb 作為共享 proto 包
```

### 任務狀態流轉

```
PENDING → DISPATCHED → RUNNING → COMPLETED → SETTLED
         ↓ (超時)      ↓ (失敗)
         FAILED        PENDING (重新分配，最多 MAX_REDISPATCH 次)
```

- **DISPATCHED 超時**：30 秒無 Worker 響應 → 重新分配或標記 FAILED
- **RUNNING 斷線**：Worker 連線中斷 → 重新分配給其他 Worker
- **重分配邏輯**：見 `services/nodepool/cmd/server/main.go` 的 `redispatchLoop()`

### Redis 資料結構

```
task:{task_id}          # Hash - 任務詳情 (status, owner, worker_id, torrent, etc.)
tasks:owner:{username}  # Set - 用戶的所有任務 ID
tasks:active            # Set - 活躍任務 ID (PENDING/DISPATCHED/RUNNING)
worker:{worker_id}      # Hash - Worker 資訊 (ip, cpu_cores, memory_gb, etc.)
```

### 任務檔案傳輸

- **上傳**：用戶上傳 ZIP → Master 生成 Torrent → 存儲到 `api/torrents/` → 返回 Magnet URI
- **下載**：Worker 從 Torrent/Magnet/HTTP URL 下載任務檔案
- **結果**：Worker 執行完成 → 上傳結果 ZIP → 生成 Torrent → 用戶下載

BitTorrent 實作位於 `services/master/internal/bt/` 和 `services/worker/internal/bt/`。

## 常見開發任務

### 新增 API 端點

1. 在 `proto/hivemind.proto` 定義 RPC 方法和訊息
2. 重新生成 pb 檔案
3. 在對應服務實作方法 (例如 `services/master/cmd/server/main.go`)
4. 更新前端 API 呼叫 (例如 `frontend/master-ui/src/App.jsx`)

### 修改任務調度邏輯

核心邏輯位於 `services/nodepool/cmd/server/main.go`：
- `dispatchLoop()` - 從 PENDING 任務中選擇可用 Worker 並分配
- `redispatchLoop()` - 檢測超時任務並重新分配
- `selectWorker()` - Worker 選擇演算法 (根據資源需求匹配)

### 新增環境變數

在對應服務的 `main.go` 中使用 `os.Getenv()` 讀取，並在 `DEPLOYMENT_GUIDE.md` 和 `QUICK_REFERENCE.md` 中記錄。

常用環境變數：
- `NODEPOOL_REDIS_ADDR` - Redis 連線位址
- `NODEPOOL_POSTGRES_DSN` - PostgreSQL DSN
- `NODEPOOL_TASK_TIMEOUT_SEC` - 任務超時秒數
- `NODEPOOL_MAX_REDISPATCH` - 最大重分配次數
- `VITE_API_BASE` - 前端 API 基礎 URL

## 故障排查

### 服務無法啟動

```powershell
# 檢查端口佔用
netstat -ano | Select-String "8082|50051|50053|6379|5432"

# 檢查 Redis 連線
redis-cli ping

# 檢查 PostgreSQL 連線
psql -h localhost -U hivemind -d hivemind
```

### 任務卡在 DISPATCHED

檢查 `nodepool.log` 中的 `redispatchLoop` 日誌：
```powershell
Select-String "DISPATCHED|timeout|redispatch" nodepool.log
```

可能原因：
- Worker 未啟動或無法連線到 Nodepool
- Worker 資源不足 (CPU/GPU/Memory 不符合任務需求)
- 網路問題導致 gRPC 呼叫失敗

### Redis 資料不一致

```bash
redis-cli
KEYS task:*           # 列出所有任務
HGETALL task:{id}     # 檢查任務詳情
SMEMBERS tasks:active # 檢查活躍任務集合
```

## 程式碼慣例

- **錯誤處理**：使用 `log.Printf` 記錄錯誤，關鍵錯誤使用 `log.Fatalf`
- **gRPC 超時**：所有 gRPC 呼叫使用 5 秒 context timeout
- **CORS**：Master HTTP API 允許所有來源 (`Access-Control-Allow-Origin: *`)
- **認證**：使用 JWT Bearer Token，bcrypt 加密密碼
- **日誌格式**：使用結構化日誌，包含 task_id, worker_id, username 等關鍵欄位

## 相關文檔

- [DEPLOYMENT_GUIDE.md](DEPLOYMENT_GUIDE.md) - 完整部署指南
- [QUICK_REFERENCE.md](QUICK_REFERENCE.md) - 快速參考卡
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - 系統架構
- [.github/copilot-instructions.md](.github/copilot-instructions.md) - 舊版 Python 實作的參考資訊

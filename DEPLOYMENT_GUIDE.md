# Hivemind 完整部署指南

## 前置需求
- Go 1.20+ (後端服務)
- Node.js 16+ (前端應用)
- Redis 6.0+ (任務存儲)
- Docker (可選，用於 Redis)

## 快速啟動 (一鍵部署)

### Windows PowerShell

```powershell
# 1. 啟動 PostgreSQL
docker run -d -p 5432:5432 --name pg-hivemind `
  -e POSTGRES_USER=hivemind `
  -e POSTGRES_PASSWORD=hivemind `
  -e POSTGRES_DB=hivemind postgres:16-alpine

# 2. 啟動 Redis
docker run -d -p 6379:6379 --name redis-hivemind redis:7-alpine

# 3. 啟動 Master 服務 (端口 8082)
$Job1 = Start-Job -ScriptBlock {
    cd D:\hivemind\services\master\cmd\server
    go run .
}

# 4. 啟動 Nodepool 服務 (端口 50051, gRPC)
$Job2 = Start-Job -ScriptBlock {
    cd D:\hivemind\services\nodepool\cmd\server
    $env:NODEPOOL_POSTGRES_DSN = "postgres://hivemind:hivemind@localhost:5432/hivemind?sslmode=disable"
    $env:NODEPOOL_REDIS_ADDR = "localhost:6379"
    $env:NODEPOOL_TASK_TIMEOUT_SEC = "30"
    $env:NODEPOOL_MAX_REDISPATCH = "2"
    $env:NODEPOOL_ENABLE_HTTP_AUTH = "1"
    go run .
}

# 4. 等待服務啟動 (5 秒)
Start-Sleep -Seconds 5
Write-Host "✓ Master: http://localhost:8082"
Write-Host "✓ Nodepool: localhost:50051 (gRPC)"

# 5. 啟動 Master UI (端口 3000)
$Job3 = Start-Job -ScriptBlock {
    cd D:\hivemind\frontend\master-ui
    npm install --silent
    npm run dev
}

# 6. 啟動 Worker UI (端口 3001)
$Job4 = Start-Job -ScriptBlock {
    cd D:\hivemind\frontend\worker-ui
    npm install --silent
    npm run dev
}

# 7. 啟動 Worker 服務 (端口 50053, gRPC)
$Job5 = Start-Job -ScriptBlock {
    cd D:\hivemind\services\worker\cmd\server
    $env:WORKER_ADDR = ":50053"
    go run .
}

Write-Host "✓ Master UI: http://localhost:3000"
Write-Host "✓ Worker UI: http://localhost:3001"
Write-Host "✓ Worker Service: localhost:50053 (gRPC)"

# 保持終端運行
Get-Job | Wait-Job
```

## 詳細啟動步驟

### 步驟 1: 啟動 Redis

#### 使用 Docker (推薦)
```bash
docker run -d \
  --name redis-hivemind \
  -p 6379:6379 \
  redis:7-alpine
```

#### 驗證連接
```bash
redis-cli ping
# 預期輸出: PONG
```

### 步驟 2: 啟動後端服務

#### 終端 1: Master 服務
```powershell
cd D:\hivemind\services\master\cmd\server
$env:BT_PUBLIC_BASE_URL = ""  # 留空自動生成
go run .

# 預期輸出:
# 2026/03/25 14:00:00 HTTP master server listening on :8082
```

#### 終端 2: Nodepool 服務
```powershell
cd D:\hivemind\services\nodepool\cmd\server
$env:NODEPOOL_POSTGRES_DSN = "postgres://hivemind:hivemind@localhost:5432/hivemind?sslmode=disable"
$env:NODEPOOL_REDIS_ADDR = "localhost:6379"
$env:NODEPOOL_TASK_TIMEOUT_SEC = "30"
$env:NODEPOOL_MAX_REDISPATCH = "2"
$env:NODEPOOL_SETTLEMENT_INTERVAL_SEC = "60"
$env:NODEPOOL_JWT_SECRET = "dev-secret-change-me"
$env:NODEPOOL_ENABLE_HTTP_AUTH = "1"
go run .

# 預期輸出:
# redis connected at localhost:6379
# gRPC nodepool server listening on :50051
```

#### 終端 3: Worker 服務
```powershell
cd D:\hivemind\services\worker\cmd\server
$env:WORKER_ADDR = ":50053"
go run .

# 預期輸出:
# Worker service starting on :50053
```

### 步驟 3: 啟動前端應用

#### 終端 4: Master UI
```powershell
cd D:\hivemind\frontend\master-ui
npm install
npm run dev

# 預期輸出:
#   VITE v5.4.8  ready in 123 ms
#   ➜  Local:   http://localhost:3000/
```

#### 終端 5: Worker UI
```powershell
cd D:\hivemind\frontend\worker-ui
npm install
npm run dev

# 預期輸出:
#   VITE v5.4.8  ready in 123 ms
#   ➜  Local:   http://localhost:3001/
```

## 環境變量配置

### Master 服務
| 變量 | 默認值 | 說明 |
|------|--------|------|
| `BT_PUBLIC_BASE_URL` | (自動生成) | 公開 Torrent 下載地址前綴 |

### Nodepool 服務
| 變量 | 默認值 | 說明 |
|------|--------|------|
| `NODEPOOL_POSTGRES_DSN` | (必填) | PostgreSQL 連線字串 |
| `NODEPOOL_REDIS_ADDR` | `localhost:6379` | Redis 服務器地址 |
| `NODEPOOL_TASK_TIMEOUT_SEC` | `30` | 任務超時時間（秒） |
| `NODEPOOL_MAX_REDISPATCH` | `2` | 最大重新調度次數 |
| `NODEPOOL_SETTLEMENT_INTERVAL_SEC` | `60` | 計費結算周期（秒） |
| `NODEPOOL_JWT_SECRET` | `dev-secret-change-me` | JWT 密鑰 |
| `NODEPOOL_ENABLE_HTTP_AUTH` | `1` | 啟用 HTTP 認證 |

### Worker 服務
| 變量 | 默認值 | 說明 |
|------|--------|------|
| `WORKER_ADDR` | `:50053` | Worker gRPC 監聽地址 |

### 前端應用
| 變量 | 默認值 | 說明 |
|------|--------|------|
| `VITE_API_BASE` | `http://localhost:8082` | Master API 地址 |
| `VITE_WORKER_CONTROL_BASE` | `http://localhost:18080` | Worker Control API 地址 |

## 驗證部署

### 1. 檢查服務狀態

```powershell
# Master API
Invoke-WebRequest http://localhost:8082/api/health -ErrorAction SilentlyContinue

# Nodepool gRPC (需要 grpcurl)
grpcurl -plaintext localhost:50051 list

# Redis
redis-cli ping
```

### 2. 訪問前端

- **Master UI**: http://localhost:3000
  - 默認用戶: `testuser` / `testpass123`
  - 功能: 任務提交和管理

- **Worker UI**: http://localhost:3001
  - 默認用戶: `worker1` / `worker123`
  - 功能: Worker 節點管理

### 3. 檢查日誌

```powershell
# Nodepool 日誌
Get-Content D:\hivemind\nodepool.log -Tail 20

# Master 服務輸出（在對應終端查看）
```

## 測試工作流

### 1. Master UI - 用戶登錄
1. 打開 http://localhost:3000
2. 使用 `testuser` / `testpass123` 登錄
3. 查看賬戶餘額

### 2. Master UI - 提交任務
1. 選擇 ZIP 文件上傳
2. 系統自動生成 Torrent
3. 點擊"建立任務"
4. 觀察任務狀態變化: PENDING → DISPATCHED → RUNNING

### 3. Worker UI - 註冊節點
1. 打開 http://localhost:3001
2. 使用 `worker1` / `worker123` 登錄
3. 點擊"刷新 Worker 狀態"（讀取本地 Worker 硬體信息）
4. 點擊"一鍵註冊 Worker 節點"

### 4. 監控日誌
```powershell
# 實時監控 Nodepool 日誌
Get-Content D:\hivemind\nodepool.log -Tail 5 -Wait
```

## 常見問題

### Q1: Redis 連接失敗
```
Error: redis connection failed
```
**解決方案**:
```powershell
# 檢查 Redis 運行狀態
docker ps | grep redis-hivemind

# 確認地址正確
$env:NODEPOOL_REDIS_ADDR = "localhost:6379"

# 測試連接
redis-cli ping
```

### Q2: 端口被占用
```
Error: listen tcp :8082: bind: An attempt was made to use a socket address
```
**解決方案**:
```powershell
# 查找占用端口的進程
Get-NetTCPConnection -LocalPort 8082 | Select-Object OwningProcess

# 終止進程
Stop-Process -Id <PID> -Force

# 或更改服務端口
$env:MASTER_ADDR = ":8083"  # 修改為 8083
```

### Q3: 前端無法連接 API
```
連線失敗：Failed to fetch
```
**解決方案**:
```powershell
# 確認 Master 服務已啟動
curl http://localhost:8082/api/balance

# 檢查前端配置
# frontend/master-ui/.env
# VITE_API_BASE=http://localhost:8082
```

### Q4: 日誌文件過大
```powershell
# 清空日誌並重新啟動
Remove-Item D:\hivemind\nodepool.log
# 重啟 Nodepool 服務
```

## 生產部署 (Docker Compose)

```yaml
version: '3.8'

services:
  postgres:
    image: postgres:16-alpine
    ports:
      - "5432:5432"
    environment:
      POSTGRES_USER: hivemind
      POSTGRES_PASSWORD: hivemind
      POSTGRES_DB: hivemind
    volumes:
      - postgres_data:/var/lib/postgresql/data

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data

  master:
    build:
      context: ./services/master
      dockerfile: Dockerfile
    ports:
      - "8082:8082"
    depends_on:
      - postgres

  nodepool:
    build:
      context: ./services/nodepool
      dockerfile: Dockerfile
    ports:
      - "50051:50051"
    depends_on:
      - postgres
      - redis
    environment:
      NODEPOOL_POSTGRES_DSN: postgres://hivemind:hivemind@postgres:5432/hivemind?sslmode=disable
      NODEPOOL_REDIS_ADDR: redis:6379
      NODEPOOL_TASK_TIMEOUT_SEC: 30
      NODEPOOL_MAX_REDISPATCH: 2

  master-ui:
    build:
      context: ./frontend/master-ui
      dockerfile: Dockerfile
    ports:
      - "3000:3000"
    environment:
      VITE_API_BASE: http://localhost:8082

  worker-ui:
    build:
      context: ./frontend/worker-ui
      dockerfile: Dockerfile
    ports:
      - "3001:3000"
    environment:
      VITE_API_BASE: http://localhost:8082

volumes:
  postgres_data:
  redis_data:
```

## 故障轉移和監控

### 健康檢查

```powershell
# 定期檢查服務狀態
$services = @(
    "http://localhost:8082/api/health",
    "redis-cli ping"
)

foreach ($service in $services) {
    try {
        $result = Invoke-WebRequest $service -ErrorAction SilentlyContinue
        Write-Host "✓ $service: OK"
    } catch {
        Write-Host "✗ $service: FAILED" -ForegroundColor Red
    }
}
```

### 自動重啟腳本

```powershell
# 監控服務並在失敗時重啟
while ($true) {
    $redis = & {redis-cli ping 2>&1}
    if ($redis -ne "PONG") {
        Write-Host "Redis 斷開，重新啟動..."
        docker restart redis-hivemind
    }
    Start-Sleep -Seconds 30
}
```

## 清理環境

```powershell
# 停止所有 Job
Get-Job | Stop-Job
Get-Job | Remove-Job

# 停止 Redis
docker stop redis-hivemind
docker rm redis-hivemind

# 清除本地日誌
Remove-Item D:\hivemind\nodepool.log
```

## 支持和文檔

- 日誌記錄文檔: `docs/UPDATE_SUMMARY_2026_03_25.md`
- Redis 遷移計劃: `docs/REDIS_MIGRATION_PLAN.md`
- UI 分離計劃: `docs/UI_SEPARATION_PLAN.md`
- 架構文檔: `docs/ARCHITECTURE.md`

---

**部署完成！系統已準備好進行端到端測試。**

---

## PostgreSQL + Kafka Add-on
The current `nodepool` service uses PostgreSQL for persistent data and Redis for task metadata/cache.

### Quick start

```powershell
# PostgreSQL
docker run -d -p 5432:5432 --name pg-hivemind `
  -e POSTGRES_USER=hivemind `
  -e POSTGRES_PASSWORD=hivemind `
  -e POSTGRES_DB=hivemind postgres:16-alpine

# Kafka-compatible broker using Redpanda
docker run -d -p 9092:9092 --name redpanda-hivemind `
  docker.redpanda.com/redpandadata/redpanda:v25.1.2 `
  redpanda start --overprovisioned --smp 1 --memory 512M --reserve-memory 0M --node-id 0 --check=false `
  --kafka-addr 0.0.0.0:9092 --advertise-kafka-addr localhost:9092
```

### Nodepool environment variables

```powershell
$env:NODEPOOL_POSTGRES_DSN = "postgres://hivemind:hivemind@localhost:5432/hivemind?sslmode=disable"
$env:NODEPOOL_KAFKA_BROKERS = "localhost:9092"
$env:NODEPOOL_KAFKA_TOPIC = "hivemind.nodepool.events"
```

### What gets written

- PostgreSQL tables: `users`, `tasks`, `cpt_transfers`
- Kafka events: task state changes, CPT settlement events, user balance sync events

### Docker Compose snippet

```yaml
postgres:
  image: postgres:16-alpine
  environment:
    POSTGRES_USER: hivemind
    POSTGRES_PASSWORD: hivemind
    POSTGRES_DB: hivemind
  ports:
    - "5432:5432"

redpanda:
  image: docker.redpanda.com/redpandadata/redpanda:v25.1.2
  command:
    - redpanda
    - start
    - --overprovisioned
    - --smp=1
    - --memory=512M
    - --reserve-memory=0M
    - --node-id=0
    - --check=false
    - --kafka-addr=0.0.0.0:9092
    - --advertise-kafka-addr=redpanda:9092
  ports:
    - "9092:9092"

nodepool:
  environment:
    NODEPOOL_POSTGRES_DSN: postgres://hivemind:hivemind@postgres:5432/hivemind?sslmode=disable
    NODEPOOL_KAFKA_BROKERS: redpanda:9092
    NODEPOOL_KAFKA_TOPIC: hivemind.nodepool.events
```

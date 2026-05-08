# HiveMind VPN 部署指南

本指南詳細說明如何部署和配置 HiveMind 的 VPN 功能，實現 Worker 節點之間的安全通訊。

## 目錄

- [架構概述](#架構概述)
- [前置需求](#前置需求)
- [環境變數配置](#環境變數配置)
- [部署方式](#部署方式)
- [驗證部署](#驗證部署)
- [監控與日誌](#監控與日誌)
- [故障排除](#故障排除)
- [安全建議](#安全建議)

## 架構概述

HiveMind VPN 使用 Headscale（Tailscale 的開源實現）建立 Worker 節點之間的 mesh 網路：

```
┌─────────────────────────────────────────────────────────┐
│                      Nodepool                           │
│  ┌──────────────────────────────────────────────────┐  │
│  │         Embedded Headscale Server                │  │
│  │  - 節點註冊與認證                                  │  │
│  │  - IP 地址分配 (100.64.0.0/10)                    │  │
│  │  │  - DERP 中繼協調                                │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                          │
        ┌─────────────────┼─────────────────┐
        │                 │                 │
   ┌────▼────┐       ┌────▼────┐       ┌────▼────┐
   │ Worker1 │◄─────►│ Worker2 │◄─────►│ Worker3 │
   │ 100.64  │       │ 100.64  │       │ 100.64  │
   │  .0.1   │       │  .0.2   │       │  .0.3   │
   └─────────┘       └─────────┘       └─────────┘
        VPN Mesh Network (Encrypted P2P)
```

### 核心組件

1. **Nodepool with Headscale**
   - 管理 VPN 網路
   - 處理 Worker 註冊
   - 分配虛擬 IP 地址
   - 協調 DERP 中繼

2. **Worker Nodes**
   - 使用 tsnet 客戶端連接 VPN
   - 自動註冊到 Nodepool
   - 建立 P2P 加密連接
   - 支援多節點任務執行

3. **支援服務**
   - PostgreSQL: Headscale 資料庫
   - Redis: 任務佇列和快取

## 前置需求

### 系統需求

- **作業系統**: Linux (推薦 Ubuntu 20.04+, Debian 11+, CentOS 8+)
- **Docker**: 20.10+ 或 Docker Compose v2
- **記憶體**: 最少 2GB (Nodepool), 1GB per Worker
- **磁碟空間**: 10GB+
- **網路**: 
  - 開放 TCP 50051 (Nodepool gRPC)
  - 開放 TCP 8080 (Headscale HTTP)
  - 開放 UDP 41641 (DERP/STUN，可選)

### Linux 核心需求

VPN 功能需要 TUN/TAP 支援：

```bash
# 檢查 TUN 模組
lsmod | grep tun

# 如果沒有，載入模組
sudo modprobe tun

# 永久啟用
echo "tun" | sudo tee -a /etc/modules
```

### Docker 權限

Worker 容器需要網路管理權限：

```yaml
cap_add:
  - NET_ADMIN
  - NET_RAW
devices:
  - /dev/net/tun:/dev/net/tun
```

## 環境變數配置

### Nodepool 配置

#### 基本配置

```bash
# Nodepool gRPC 埠
NODEPOOL_GRPC_PORT=":50051"

# VPN 功能開關
VPN_ENABLED="true"
```

#### Headscale 伺服器配置

```bash
# Headscale 伺服器 URL（Worker 用來連接）
VPN_SERVER_URL="http://nodepool:8080"

# HTTP API 監聽地址
VPN_LISTEN_ADDR="0.0.0.0:8080"

# gRPC 監聽地址
VPN_GRPC_LISTEN_ADDR="0.0.0.0:50443"
```

#### 網路配置

```bash
# VPN IP 地址範圍
VPN_IP_PREFIX="100.64.0.0/10"

# 基礎域名
VPN_BASE_DOMAIN="hivemind.local"
```

#### 節點管理

```bash
# Worker 離線後自動清理
VPN_EPHEMERAL_NODES="true"

# 節點過期時間
VPN_NODE_EXPIRY="24h"
```

#### 資料庫配置

**SQLite (開發/測試)**

```bash
VPN_DB_TYPE="sqlite"
VPN_DB_PATH="/var/lib/headscale/db.sqlite"
```

**PostgreSQL (生產環境推薦)**

```bash
VPN_DB_TYPE="postgres"
VPN_DB_HOST="postgres"
VPN_DB_PORT="5432"
VPN_DB_NAME="headscale"
VPN_DB_USER="headscale"
VPN_DB_PASSWORD="your_secure_password"
```

#### DERP 配置（可選）

```bash
# DERP 伺服器配置 URL
VPN_DERP_MAP_URL=""

# 自動更新 DERP 配置
VPN_DERP_AUTO_UPDATE="true"
```

### Worker 配置

```bash
# Worker 唯一識別碼
WORKER_ID="worker-001"

# Nodepool 地址
NODEPOOL_ADDR="nodepool:50051"

# Worker gRPC 埠
WORKER_GRPC_PORT=":50052"

# VPN 功能開關
VPN_ENABLED="true"

# VPN 狀態目錄
VPN_STATE_DIR="/var/lib/hivemind-vpn"

# VPN 主機名稱
VPN_HOSTNAME="worker-001"
```

### Redis 配置

```bash
REDIS_ADDR="redis:6379"
REDIS_PASSWORD=""  # 生產環境應設定密碼
REDIS_DB="0"
```

## 部署方式

### 方式 1: Docker Compose (推薦)

#### 1. 準備配置文件

```bash
cd hivemind/infra
cp docker-compose.vpn.yml docker-compose.yml
```

#### 2. 配置環境變數

創建 `.env` 文件：

```bash
# .env
POSTGRES_PASSWORD=your_secure_password
VPN_DB_PASSWORD=your_secure_password
REDIS_PASSWORD=your_redis_password
```

#### 3. 啟動服務

```bash
# 啟動所有服務（2 個 Worker）
docker-compose up -d

# 啟動包含第 3 個 Worker
docker-compose --profile extra-workers up -d

# 查看日誌
docker-compose logs -f

# 查看特定服務日誌
docker-compose logs -f nodepool
docker-compose logs -f worker1
```

#### 4. 擴展 Worker 數量

```bash
# 動態增加 Worker
docker-compose up -d --scale worker1=5
```

### 方式 2: 手動部署

#### 1. 啟動 PostgreSQL

```bash
docker run -d \
  --name hivemind-postgres \
  -e POSTGRES_DB=headscale \
  -e POSTGRES_USER=headscale \
  -e POSTGRES_PASSWORD=headscale_password \
  -p 5432:5432 \
  -v postgres-data:/var/lib/postgresql/data \
  postgres:15-alpine
```

#### 2. 啟動 Redis

```bash
docker run -d \
  --name hivemind-redis \
  -p 6379:6379 \
  -v redis-data:/data \
  redis:7-alpine redis-server --appendonly yes
```

#### 3. 啟動 Nodepool

```bash
docker run -d \
  --name hivemind-nodepool \
  -p 50051:50051 \
  -p 8080:8080 \
  -p 50443:50443 \
  -e VPN_ENABLED=true \
  -e VPN_SERVER_URL=http://localhost:8080 \
  -e VPN_DB_TYPE=postgres \
  -e VPN_DB_HOST=postgres \
  -e VPN_DB_PASSWORD=headscale_password \
  -v nodepool-data:/var/lib/headscale \
  --link hivemind-postgres:postgres \
  --link hivemind-redis:redis \
  hivemind/nodepool:latest
```

#### 4. 啟動 Worker

```bash
# Worker 1
docker run -d \
  --name hivemind-worker1 \
  -e WORKER_ID=worker-001 \
  -e NODEPOOL_ADDR=nodepool:50051 \
  -e VPN_ENABLED=true \
  -e VPN_HOSTNAME=worker-001 \
  -v worker1-vpn:/var/lib/hivemind-vpn \
  --cap-add NET_ADMIN \
  --cap-add NET_RAW \
  --device /dev/net/tun:/dev/net/tun \
  --link hivemind-nodepool:nodepool \
  hivemind/worker:latest

# Worker 2
docker run -d \
  --name hivemind-worker2 \
  -e WORKER_ID=worker-002 \
  -e NODEPOOL_ADDR=nodepool:50051 \
  -e VPN_ENABLED=true \
  -e VPN_HOSTNAME=worker-002 \
  -v worker2-vpn:/var/lib/hivemind-vpn \
  --cap-add NET_ADMIN \
  --cap-add NET_RAW \
  --device /dev/net/tun:/dev/net/tun \
  --link hivemind-nodepool:nodepool \
  hivemind/worker:latest
```

### 方式 3: Kubernetes (進階)

參考 `k8s/` 目錄中的 Kubernetes 配置文件（待補充）。

## 驗證部署

### 1. 檢查服務狀態

```bash
# Docker Compose
docker-compose ps

# 應該看到所有服務都是 "Up" 狀態
```

### 2. 檢查 Nodepool 日誌

```bash
docker-compose logs nodepool | grep -i "vpn\|headscale"
```

預期輸出：
```
nodepool  | INFO Headscale manager initialized
nodepool  | INFO VPN server started on 0.0.0.0:8080
nodepool  | INFO Worker worker-001 registered to VPN
nodepool  | INFO Worker worker-002 registered to VPN
```

### 3. 檢查 Worker VPN 連接

```bash
# Worker 1
docker-compose logs worker1 | grep -i "vpn"

# Worker 2
docker-compose logs worker2 | grep -i "vpn"
```

預期輸出：
```
worker1  | INFO VPN manager created
worker1  | INFO Registered to VPN network
worker1  | INFO VPN connected, local IP: 100.64.0.1
worker1  | INFO Discovered 1 peers
```

### 4. 測試 Worker 間通訊

```bash
# 進入 Worker 1 容器
docker-compose exec worker1 sh

# Ping Worker 2 的 VPN IP
ping -c 3 100.64.0.2

# 應該能成功 ping 通
```

### 5. 運行整合測試

```bash
cd hivemind
chmod +x scripts/test_vpn_integration.sh
./scripts/test_vpn_integration.sh
```

## 監控與日誌

### 日誌位置

#### Docker Compose

```bash
# 即時查看所有日誌
docker-compose logs -f

# 查看特定服務
docker-compose logs -f nodepool
docker-compose logs -f worker1

# 查看最近 100 行
docker-compose logs --tail=100 nodepool
```

#### 容器內日誌

- Nodepool: `/var/log/hivemind/nodepool.log`
- Worker: `/var/log/hivemind/worker.log`
- Headscale: 整合在 Nodepool 日誌中

### 關鍵日誌訊息

#### 成功訊息

```
✓ Headscale manager initialized
✓ VPN server started
✓ Worker registered to VPN
✓ VPN connected
✓ Peer discovered
✓ Task peers retrieved
```

#### 警告訊息

```
⚠ Worker heartbeat timeout
⚠ Peer connection degraded
⚠ DERP relay in use (direct connection failed)
```

#### 錯誤訊息

```
✗ Failed to register worker
✗ VPN connection failed
✗ Database connection error
✗ Auth key invalid
```

### 監控指標

#### Nodepool 指標

```bash
# 查看已註冊的 Worker 數量
docker-compose exec nodepool curl http://localhost:8080/metrics

# 或使用 gRPC 健康檢查
grpc_health_probe -addr=localhost:50051
```

#### Worker 指標

- VPN 連接狀態
- 本地 VPN IP
- 已發現的 Peer 數量
- 任務執行統計

### Prometheus 整合（可選）

在 `docker-compose.vpn.yml` 中添加 Prometheus：

```yaml
prometheus:
  image: prom/prometheus:latest
  ports:
    - "9090:9090"
  volumes:
    - ./prometheus.yml:/etc/prometheus/prometheus.yml
    - prometheus-data:/prometheus
  networks:
    - hivemind-net
```

## 故障排除

### 問題 1: Worker 無法註冊到 VPN

**症狀**:
```
ERROR Failed to register to VPN: connection refused
```

**解決方案**:

1. 檢查 Nodepool 是否正常運行：
```bash
docker-compose ps nodepool
docker-compose logs nodepool
```

2. 檢查網路連接：
```bash
docker-compose exec worker1 ping nodepool
docker-compose exec worker1 nc -zv nodepool 50051
```

3. 檢查防火牆規則：
```bash
# 確保 50051 埠開放
sudo ufw allow 50051/tcp
```

### 問題 2: Worker 之間無法通訊

**症狀**:
```
ERROR Failed to dial peer: no route to host
```

**解決方案**:

1. 檢查 VPN 連接狀態：
```bash
docker-compose logs worker1 | grep "VPN connected"
docker-compose logs worker2 | grep "VPN connected"
```

2. 檢查 TUN 設備：
```bash
docker-compose exec worker1 ip addr show tun0
```

3. 檢查容器權限：
```bash
# 確保容器有 NET_ADMIN 權限
docker inspect hivemind-worker1 | grep -A 5 CapAdd
```

4. 重啟 Worker：
```bash
docker-compose restart worker1 worker2
```

### 問題 3: 資料庫連接失敗

**症狀**:
```
ERROR Failed to connect to database: connection refused
```

**解決方案**:

1. 檢查 PostgreSQL 狀態：
```bash
docker-compose ps postgres
docker-compose logs postgres
```

2. 測試資料庫連接：
```bash
docker-compose exec postgres psql -U headscale -d headscale -c "SELECT 1;"
```

3. 檢查資料庫密碼：
```bash
# 確保 .env 文件中的密碼正確
cat .env | grep POSTGRES_PASSWORD
```

### 問題 4: 記憶體不足

**症狀**:
```
ERROR Out of memory
```

**解決方案**:

1. 增加 Docker 記憶體限制：
```yaml
# docker-compose.vpn.yml
services:
  nodepool:
    mem_limit: 2g
    memswap_limit: 2g
```

2. 監控記憶體使用：
```bash
docker stats
```

### 問題 5: VPN IP 衝突

**症狀**:
```
ERROR IP address already assigned
```

**解決方案**:

1. 清理舊的 VPN 狀態：
```bash
docker-compose down -v
docker volume rm hivemind_worker1-vpn hivemind_worker2-vpn
docker-compose up -d
```

2. 更改 IP 前綴：
```bash
# .env
VPN_IP_PREFIX="100.65.0.0/10"
```

### 問題 6: DERP 中繼延遲高

**症狀**:
```
WARN Using DERP relay, latency: 200ms
```

**解決方案**:

1. 檢查防火牆是否阻擋 UDP：
```bash
sudo ufw allow 41641/udp
```

2. 配置自定義 DERP 伺服器：
```bash
VPN_DERP_MAP_URL="https://your-derp-server.com/derp.json"
```

3. 檢查 NAT 穿透：
```bash
# 在 Worker 容器中
docker-compose exec worker1 sh
# 測試 STUN
stunclient stun.l.google.com 19302
```

### 除錯模式

啟用詳細日誌：

```bash
# docker-compose.vpn.yml
services:
  nodepool:
    environment:
      LOG_LEVEL: "debug"
      VPN_DEBUG: "true"

  worker1:
    environment:
      LOG_LEVEL: "debug"
      VPN_DEBUG: "true"
```

## 安全建議

### 1. 使用強密碼

```bash
# 生成安全密碼
openssl rand -base64 32
```

### 2. 啟用 TLS

```yaml
# docker-compose.vpn.yml
services:
  nodepool:
    environment:
      VPN_TLS_ENABLED: "true"
      VPN_TLS_CERT_FILE: "/etc/certs/server.crt"
      VPN_TLS_KEY_FILE: "/etc/certs/server.key"
    volumes:
      - ./certs:/etc/certs:ro
```

### 3. 限制網路訪問

```yaml
# docker-compose.vpn.yml
services:
  postgres:
    ports: []  # 不對外暴露
  redis:
    ports: []  # 不對外暴露
```

### 4. 定期更新

```bash
# 更新 Docker 映像
docker-compose pull
docker-compose up -d
```

### 5. 備份資料

```bash
# 備份 PostgreSQL
docker-compose exec postgres pg_dump -U headscale headscale > backup.sql

# 備份 volumes
docker run --rm -v hivemind_postgres-data:/data -v $(pwd):/backup \
  alpine tar czf /backup/postgres-backup.tar.gz /data
```

### 6. 監控異常活動

- 定期檢查日誌中的失敗登入嘗試
- 監控異常的網路流量
- 設定告警規則

### 7. 網路隔離

```yaml
# docker-compose.vpn.yml
networks:
  hivemind-net:
    driver: bridge
    internal: true  # 隔離外部網路
  public-net:
    driver: bridge
```

## 效能調優

### 1. 資料庫優化

```sql
-- PostgreSQL 調優
ALTER SYSTEM SET shared_buffers = '256MB';
ALTER SYSTEM SET effective_cache_size = '1GB';
ALTER SYSTEM SET maintenance_work_mem = '64MB';
```

### 2. Redis 優化

```bash
# redis.conf
maxmemory 512mb
maxmemory-policy allkeys-lru
```

### 3. Worker 並發

```yaml
# docker-compose.vpn.yml
services:
  worker1:
    environment:
      MAX_CONCURRENT_TASKS: "10"
      TASK_TIMEOUT: "300s"
```

## 生產環境檢查清單

- [ ] 所有密碼已更改為強密碼
- [ ] TLS 已啟用
- [ ] 防火牆規則已配置
- [ ] 日誌輪轉已設定
- [ ] 監控和告警已配置
- [ ] 備份策略已實施
- [ ] 資源限制已設定
- [ ] 健康檢查已配置
- [ ] 文檔已更新
- [ ] 團隊已培訓

## 相關文檔

- [VPN 快速開始指南](./VPN_QUICKSTART.md)
- [VPN 實現摘要](./VPN_IMPLEMENTATION_SUMMARY.md)
- [架構文檔](./ARCHITECTURE.md)
- [開發者手冊](./developer-runbook.md)

## 支援

如遇到問題，請：

1. 查看[故障排除](#故障排除)章節
2. 檢查 GitHub Issues
3. 聯繫技術支援團隊

---

**版本**: 1.0.0  
**最後更新**: 2026-04-30  
**維護者**: HiveMind DevOps Team

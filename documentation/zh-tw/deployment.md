# HiveMind 部署指南

## 概述

本指南將幫助您部署 HiveMind 分布式運算平台。目前支援基本的 Python 環境部署，Docker 容器化部署正在開發中。

## 系統需求

### 最低硬體要求

#### Node Pool 服務器
- **CPU**：4 核心 2.0GHz+
- **記憶體**：8GB RAM
- **儲存**：100GB SSD
- **網路**：1Gbps 頻寬
- **作業系統**：Ubuntu 20.04 LTS / CentOS 8 / Windows Server 2019

#### Master 節點服務器
- **CPU**：2 核心 2.0GHz+
- **記憶體**：4GB RAM
- **儲存**：50GB SSD
- **網路**：100Mbps 頻寬

#### Worker 節點 (推薦配置)
- **CPU**：8+ 核心
- **記憶體**：16GB+ RAM
- **GPU**：NVIDIA GTX 1060 或更高 (可選，需要額外安裝 pynvml)
- **儲存**：20GB 可用空間
- **網路**：50Mbps+ 頻寬

### 軟體依賴

- **Python**：3.8+
- **Redis**：6.0+ (必需)
- **Docker**：20.10+ (僅 Worker 節點使用)

**重要提醒**：目前項目使用 Redis 進行節點狀態管理，SQLite 存儲用戶數據。

## 快速開始

### 1. 克隆專案

```bash
git clone https://github.com/him6794/hivemind.git
cd hivemind
```

### 2. 安裝 Python 依賴

```bash
# 安裝全域依賴
pip install -r requirements.txt

# 安裝各模組特定依賴
pip install -r master/requirements.txt
pip install -r worker/requirements.txt
pip install -r taskworker/requirements.txt
```

### 3. 安裝 Redis

#### Ubuntu/Debian
```bash
sudo apt update
sudo apt install redis-server
sudo systemctl start redis-server
sudo systemctl enable redis-server
```

#### CentOS/RHEL
```bash
sudo yum install epel-release
sudo yum install redis
sudo systemctl start redis
sudo systemctl enable redis
```

#### Windows
```bash
# 使用 Chocolatey
choco install redis-64

# 或下載 Windows 版本並手動安裝
# https://github.com/microsoftarchive/redis/releases
```

### 4. 配置網路 (可選)

HiveMind 支援 WireGuard VPN 來創建安全的節點網路：

```bash
# 安裝 WireGuard (Ubuntu)
sudo apt install wireguard

# 配置會在首次啟動時自動生成
```

## 部署架構

### 標準單機部署

適合測試和小規模使用：

```
┌─────────────────────────────────────┐
│           單一機器                    │
│  ┌─────────────┐ ┌─────────────┐    │
│  │ Node Pool   │ │   Master    │    │
│  │ :50051      │ │   Node      │    │
│  └─────────────┘ └─────────────┘    │
│  ┌─────────────┐ ┌─────────────┐    │
│  │  Worker 1   │ │  Worker 2   │    │
│  │   :動態      │ │   :動態      │    │
│  └─────────────┘ └─────────────┘    │
│  ┌─────────────┐                    │
│  │    Redis    │                    │
│  │   :6379     │                    │
│  └─────────────┘                    │
└─────────────────────────────────────┘
```

### 分布式部署

適合生產環境：

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   服務器 A      │    │   服務器 B      │    │   服務器 C      │
│                │    │                │    │                │
│ ┌─────────────┐ │    │ ┌─────────────┐ │    │ ┌─────────────┐ │
│ │ Node Pool   │ │    │ │   Master    │ │    │ │  Worker 1   │ │
│ │   :50051    │ │    │ │   Node      │ │    │ │    :動態     │ │
│ └─────────────┘ │    │ └─────────────┘ │    │ └─────────────┘ │
│ ┌─────────────┐ │    │ ┌─────────────┐ │    │ ┌─────────────┐ │
│ │    Redis    │ │    │ │ Web 界面    │ │    │ │  Worker 2   │ │
│ │   :6379     │ │    │ │   :5000     │ │    │ │    :動態     │ │
│ └─────────────┘ │    │ └─────────────┘ │    │ └─────────────┘ │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## 詳細部署步驟

### 步驟 1: Node Pool 服務部署

Node Pool 是核心服務，負責節點管理和任務調度。

```bash
# 進入 node_pool 目錄
cd node_pool

# 檢查配置文件
cat config.py

# 啟動服務
python node_pool_server.py
```

**配置說明**：
- 預設端口：50051
- 預設 Redis 地址：localhost:6379
- 用戶數據庫：users.db (SQLite)

### 步驟 2: Master 節點部署

Master 節點提供管理界面和高級功能。

```bash
# 進入 master 目錄
cd master

# 啟動 Master 節點
python master_node.py
```

**配置說明**：
- 自動連接到 Node Pool
- 提供 Web 管理界面
- 支援 VPN 配置

### 步驟 3: Worker 節點部署

Worker 節點執行實際的計算任務。

```bash
# 進入 worker 目錄
cd worker

# 啟動 Worker 節點
python worker_node.py
```

**多 Worker 部署**：
```bash
# 在不同機器或使用不同配置
python worker_node.py --node-id worker-1
python worker_node.py --node-id worker-2
```

### 步驟 4: Web 界面部署 (可選)

提供用戶友好的 Web 管理界面。

```bash
# 進入 web 目錄
cd web

# 啟動 Web 服務
python app.py
```

預設訪問地址：http://localhost:5000

## 進階配置

### 環境變數配置

創建 `.env` 文件來自定義配置：

```bash
# Node Pool 配置
NODEPOOL_HOST=0.0.0.0
NODEPOOL_PORT=50051
REDIS_HOST=localhost
REDIS_PORT=6379

# Master 配置
MASTER_HOST=0.0.0.0
MASTER_PORT=50052

# Worker 配置
WORKER_HOST=0.0.0.0
WORKER_PORT=0  # 使用動態端口

# Web 配置
WEB_HOST=0.0.0.0
WEB_PORT=5000
WEB_DEBUG=false
```

### Redis 配置優化

編輯 `/etc/redis/redis.conf`：

```conf
# 記憶體限制
maxmemory 2gb
maxmemory-policy allkeys-lru

# 持久化配置
save 900 1
save 300 10
save 60 10000

# 網路配置
bind 0.0.0.0
protected-mode no
```

### 防火牆配置

```bash
# Ubuntu UFW
sudo ufw allow 50051  # Node Pool
sudo ufw allow 50052  # Master Node
sudo ufw allow 5000   # Web 界面
sudo ufw allow 6379   # Redis

# CentOS firewalld
sudo firewall-cmd --permanent --add-port=50051/tcp
sudo firewall-cmd --permanent --add-port=50052/tcp
sudo firewall-cmd --permanent --add-port=5000/tcp
sudo firewall-cmd --permanent --add-port=6379/tcp
sudo firewall-cmd --reload
```

## 服務管理

### 使用 systemd (推薦)

創建服務文件：

**Node Pool 服務** (`/etc/systemd/system/hivemind-nodepool.service`)：
```ini
[Unit]
Description=HiveMind Node Pool Service
After=network.target redis.service

[Service]
Type=simple
User=hivemind
WorkingDirectory=/opt/hivemind/node_pool
ExecStart=/usr/bin/python3 node_pool_server.py
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

**Master 服務** (`/etc/systemd/system/hivemind-master.service`)：
```ini
[Unit]
Description=HiveMind Master Node Service
After=network.target hivemind-nodepool.service

[Service]
Type=simple
User=hivemind
WorkingDirectory=/opt/hivemind/master
ExecStart=/usr/bin/python3 master_node.py
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

啟動服務：
```bash
sudo systemctl daemon-reload
sudo systemctl enable hivemind-nodepool
sudo systemctl enable hivemind-master
sudo systemctl start hivemind-nodepool
sudo systemctl start hivemind-master
```

### 使用 Docker Compose (開發中)

```yaml
version: '3.8'
services:
  redis:
    image: redis:6.2-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data

  nodepool:
    build: ./node_pool
    ports:
      - "50051:50051"
    depends_on:
      - redis
    environment:
      - REDIS_HOST=redis

  master:
    build: ./master
    ports:
      - "50052:50052"
    depends_on:
      - nodepool

  worker:
    build: ./worker
    depends_on:
      - nodepool
    deploy:
      replicas: 2

volumes:
  redis_data:
```

## 監控和日誌

### 日誌配置

```python
# 在各服務中添加日誌配置
import logging

logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
    handlers=[
        logging.FileHandler('/var/log/hivemind/service.log'),
        logging.StreamHandler()
    ]
)
```

### 性能監控

```bash
# 監控 Redis 性能
redis-cli info stats

# 監控系統資源
htop
iotop
netstat -tulpn
```

## 安全考量

### 網路安全

1. **使用 VPN**: 啟用 WireGuard VPN 來保護節點間通訊
2. **防火牆配置**: 僅開放必要端口
3. **TLS 加密**: 在生產環境中啟用 gRPC TLS

### 認證配置

```python
# 在生產環境中修改預設密鑰
SECRET_KEY = "your-secret-key-here"
JWT_SECRET = "your-jwt-secret-here"
```

## 故障排除

### 常見問題

1. **連接問題**
   ```bash
   # 檢查服務狀態
   netstat -tulpn | grep :50051
   
   # 檢查防火牆
   sudo ufw status
   ```

2. **Redis 連接失敗**
   ```bash
   # 測試 Redis 連接
   redis-cli ping
   
   # 檢查 Redis 日誌
   sudo journalctl -u redis
   ```

3. **Worker 節點無法註冊**
   ```bash
   # 檢查網路連通性
   telnet nodepool-server 50051
   
   # 檢查日誌
   tail -f /var/log/hivemind/worker.log
   ```

### 性能調優

1. **Redis 優化**
   - 調整 `maxmemory` 設置
   - 使用適當的數據持久化策略

2. **gRPC 優化**
   - 調整連接池大小
   - 啟用壓縮

3. **Python 優化**
   - 使用 PyPy 以提高性能
   - 配置適當的工作進程數量

## 擴展部署

### 水平擴展

1. **添加更多 Worker 節點**
   ```bash
   # 在新機器上重複 Worker 部署步驟
   cd hivemind/worker
   python worker_node.py --node-id worker-new
   ```

2. **負載均衡**
   - 使用 HAProxy 或 Nginx 進行負載均衡
   - 配置 Redis Cluster

### 高可用性

1. **Redis 主從複製**
2. **服務容錯機制**
3. **自動故障轉移**

---

**更新日期**: 2024年1月  
**版本**: v1.0  
**狀態**: 基於實際實現的準確文檔

**注意**: 這是基於當前實際代碼的部署指南。Docker 容器化和某些高級功能仍在開發中。

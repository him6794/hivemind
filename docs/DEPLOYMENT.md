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

## 基本部署 (目前支援的方式)

### 1. 安裝依賴

```bash
# 克隆專案
git clone https://github.com/him6794/hivemind.git
cd hivemind

# 安裝 Python 依賴
pip install -r requirements.txt

# 安裝 Redis (Ubuntu/Debian)
sudo apt update
sudo apt install redis-server

# 啟動 Redis
sudo systemctl start redis-server
sudo systemctl enable redis-server
```

### 2. 配置 Node Pool 服務

```bash
# 進入 node_pool 目錄
cd node_pool

# 配置環境變數（可選）
# 創建 .env 文件或使用預設配置

# 啟動 Node Pool 服務
python node_pool_server.py
```

### 3. 配置 Master 服務

```bash
# 在新的終端機中，進入 master 目錄
cd master

# 啟動 Master 服務
python master_node.py
```

### 4. 配置 Worker 節點

```bash
# 在 Worker 機器上，進入 worker 目錄
cd worker

# 啟動 Worker 節點
python worker_node.py
```

### 5. 啟動 Web 界面 (可選)

```bash
# 在新的終端機中，進入 web 目錄
cd web

# 啟動 Web 服務
python app.py
```

## Docker 部署 (開發中)

Docker Compose 部署配置正在開發中，目前建議使用上述的 Python 直接部署方式。

### 計劃中的 Docker 部署功能：
- 自動化的容器編排
- Redis 和應用服務的統一管理
- 環境變數配置檔案
- 持久化數據存儲

## 配置說明

### 環境變數 (可選配置)

系統會使用預設配置，以下是可調整的環境變數：

```bash
# Node Pool 配置
NODE_POOL_PORT=50051

# Master 節點配置  
MASTER_PORT=5001

# 任務存儲路徑
TASK_STORAGE_PATH=/path/to/task/storage

# JWT 配置
JWT_SECRET_KEY=your-secret-key

# 郵件服務 (Resend API)
RESEND_API_KEY=your-resend-api-key
FROM_EMAIL=noreply@yourdomain.com
```
    environment:
      POSTGRES_DB: hivemind
      POSTGRES_USER: hivemind
      POSTGRES_PASSWORD: ${DB_PASSWORD}
    volumes:
      - postgres_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"
    restart: unless-stopped

  # Node Pool 服務
  node-pool:
    build:
      context: ./node_pool
      dockerfile: Dockerfile
    ports:
      - "50051:50051"
    environment:
      - REDIS_URL=redis://redis:6379/0
      - DATABASE_URL=${DATABASE_URL}
      - JWT_SECRET_KEY=${JWT_SECRET_KEY}
    volumes:
      - task_storage:/data/tasks
    depends_on:
      - redis
      - postgres
    restart: unless-stopped

  # Master 節點服務
  master:
    build:
      context: ./master
      dockerfile: Dockerfile
    ports:
      - "5001:5001"
      - "50052:50052"
    environment:
      - NODE_POOL_URL=node-pool:50051
      - DATABASE_URL=${DATABASE_URL}
      - JWT_SECRET_KEY=${JWT_SECRET_KEY}
      - RESEND_API_KEY=${RESEND_API_KEY}
    volumes:
      - master_data:/app/data
    depends_on:
      - node-pool
    restart: unless-stopped

  # Nginx 反向代理
  nginx:
    image: nginx:alpine
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./nginx/nginx.conf:/etc/nginx/nginx.conf
      - ./nginx/ssl:/etc/nginx/ssl
    depends_on:
      - master
    restart: unless-stopped

volumes:
  redis_data:
  postgres_data:
  task_storage:
  master_data:
```

### 4. 啟動服務

```bash
# 構建並啟動所有服務
docker-compose up -d

# 查看服務狀態
docker-compose ps

# 查看日誌
docker-compose logs -f node-pool
docker-compose logs -f master
```

## Kubernetes 部署

### 1. 準備 Kubernetes 清單

創建命名空間：

```yaml
# namespace.yaml
apiVersion: v1
kind: Namespace
metadata:
  name: hivemind
```

### 2. Redis 部署

```yaml
# redis-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: redis
  namespace: hivemind
spec:
  replicas: 1
  selector:
    matchLabels:
      app: redis
  template:
    metadata:
      labels:
        app: redis
    spec:
      containers:
      - name: redis
        image: redis:7-alpine
        ports:
        - containerPort: 6379
        volumeMounts:
        - name: redis-data
          mountPath: /data
      volumes:
      - name: redis-data
        persistentVolumeClaim:
          claimName: redis-pvc

---
apiVersion: v1
kind: Service
metadata:
  name: redis
  namespace: hivemind
spec:
  selector:
    app: redis
  ports:
  - port: 6379
    targetPort: 6379
```

### 3. Node Pool 部署

```yaml
# node-pool-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: node-pool
  namespace: hivemind
spec:
  replicas: 2
  selector:
    matchLabels:
      app: node-pool
  template:
    metadata:
      labels:
        app: node-pool
    spec:
      containers:
      - name: node-pool
        image: hivemind/node-pool:latest
        ports:
        - containerPort: 50051
        env:
        - name: REDIS_URL
          value: "redis://redis:6379/0"
        - name: JWT_SECRET_KEY
          valueFrom:
            secretKeyRef:
              name: hivemind-secrets
              key: jwt-secret

---
apiVersion: v1
kind: Service
metadata:
  name: node-pool
  namespace: hivemind
spec:
  selector:
    app: node-pool
  ports:
  - port: 50051
    targetPort: 50051
  type: LoadBalancer
```

### 4. 部署到 Kubernetes

```bash
# 應用所有配置
kubectl apply -f namespace.yaml
kubectl apply -f redis-deployment.yaml
kubectl apply -f node-pool-deployment.yaml
kubectl apply -f master-deployment.yaml

# 檢查部署狀態
kubectl get pods -n hivemind
kubectl get services -n hivemind
```

## 傳統服務器部署

### 1. 系統準備

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install python3 python3-pip redis-server postgresql-13

# CentOS/RHEL
sudo yum update
sudo yum install python3 python3-pip redis postgresql-server

# 啟動服務
sudo systemctl start redis
sudo systemctl start postgresql
sudo systemctl enable redis
sudo systemctl enable postgresql
```

### 2. 數據庫設置

```bash
# 創建數據庫用戶和數據庫
sudo -u postgres psql
CREATE USER hivemind WITH PASSWORD 'your_password';
CREATE DATABASE hivemind OWNER hivemind;
GRANT ALL PRIVILEGES ON DATABASE hivemind TO hivemind;
\q
```

### 3. 安裝 Python 依賴

```bash
# 創建虛擬環境
python3 -m venv venv
source venv/bin/activate

# 安裝依賴
pip install -r requirements.txt
```

### 4. 配置服務

創建 systemd 服務文件：

```ini
# /etc/systemd/system/hivemind-node-pool.service
[Unit]
Description=HiveMind Node Pool Service
After=network.target redis.service postgresql.service

[Service]
Type=simple
User=hivemind
WorkingDirectory=/opt/hivemind/node_pool
Environment=PATH=/opt/hivemind/venv/bin
ExecStart=/opt/hivemind/venv/bin/python node_pool_server.py
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

### 5. 啟動服務

```bash
# 重新載入 systemd 配置
sudo systemctl daemon-reload

# 啟動服務
sudo systemctl start hivemind-node-pool
sudo systemctl start hivemind-master

# 開機自啟
sudo systemctl enable hivemind-node-pool
sudo systemctl enable hivemind-master

# 檢查狀態
sudo systemctl status hivemind-node-pool
```

## 網路配置

### 1. 防火牆設置

```bash
# Ubuntu/Debian (ufw)
sudo ufw allow 50051/tcp  # Node Pool gRPC
sudo ufw allow 5001/tcp   # Master Web Interface
sudo ufw allow 51820/udp  # WireGuard VPN

# CentOS/RHEL (firewalld)
sudo firewall-cmd --permanent --add-port=50051/tcp
sudo firewall-cmd --permanent --add-port=5001/tcp
sudo firewall-cmd --permanent --add-port=51820/udp
sudo firewall-cmd --reload
```

### 2. Nginx 反向代理配置

```nginx
# /etc/nginx/sites-available/hivemind
server {
    listen 80;
    server_name hivemind.yourdomain.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name hivemind.yourdomain.com;

    ssl_certificate /etc/nginx/ssl/hivemind.crt;
    ssl_certificate_key /etc/nginx/ssl/hivemind.key;

    # Master Web Interface
    location / {
        proxy_pass http://localhost:5001;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # gRPC API
    location /grpc/ {
        grpc_pass grpc://localhost:50051;
        grpc_set_header Host $host;
    }
}
```

## 監控和日誌

### 1. Prometheus 監控

```yaml
# prometheus.yml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'hivemind-node-pool'
    static_configs:
      - targets: ['localhost:50051']
    metrics_path: /metrics

  - job_name: 'hivemind-master'
    static_configs:
      - targets: ['localhost:5001']
    metrics_path: /metrics
```

### 2. 日誌聚合

```yaml
# docker-compose.override.yml
version: '3.8'

services:
  node-pool:
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"

  master:
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"
```

## 備份和恢復

### 1. 數據備份

```bash
#!/bin/bash
# backup.sh

DATE=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/backup/hivemind"

# 創建備份目錄
mkdir -p $BACKUP_DIR

# 備份數據庫
pg_dump -h localhost -U hivemind hivemind > $BACKUP_DIR/db_$DATE.sql

# 備份 Redis 數據
redis-cli BGSAVE
cp /var/lib/redis/dump.rdb $BACKUP_DIR/redis_$DATE.rdb

# 備份任務數據
tar -czf $BACKUP_DIR/tasks_$DATE.tar.gz /data/tasks

# 清理舊備份 (保留30天)
find $BACKUP_DIR -name "*.sql" -mtime +30 -delete
find $BACKUP_DIR -name "*.rdb" -mtime +30 -delete
find $BACKUP_DIR -name "*.tar.gz" -mtime +30 -delete
```

### 2. 自動備份設置

```bash
# 添加到 crontab
crontab -e

# 每天凌晨2點執行備份
0 2 * * * /opt/hivemind/backup.sh
```

## 性能優化

### 1. Redis 優化

```bash
# /etc/redis/redis.conf
maxmemory 2gb
maxmemory-policy allkeys-lru
save 900 1
save 300 10
save 60 10000
```

### 2. PostgreSQL 優化

```sql
-- /etc/postgresql/13/main/postgresql.conf
shared_buffers = 256MB
effective_cache_size = 1GB
work_mem = 4MB
maintenance_work_mem = 64MB
```

## 故障排除

### 常見問題

1. **服務無法啟動**
   ```bash
   # 檢查日誌
   journalctl -u hivemind-node-pool -f
   
   # 檢查端口佔用
   netstat -tlnp | grep 50051
   ```

2. **節點連接失敗**
   ```bash
   # 檢查防火牆
   sudo ufw status
   
   # 測試連接
   telnet your-server 50051
   ```

3. **內存不足**
   ```bash
   # 監控內存使用
   free -h
   htop
   
   # 優化 Docker 記憶體限制
   docker update --memory=2g container_name
   ```

## 安全加固

### 1. SSL/TLS 配置

```bash
# 生成自簽名證書 (開發環境)
openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
  -keyout hivemind.key -out hivemind.crt

# 或使用 Let's Encrypt (生產環境)
certbot --nginx -d hivemind.yourdomain.com
```

### 2. 訪問控制

```bash
# 使用 iptables 限制訪問
iptables -A INPUT -p tcp --dport 50051 -s trusted_ip -j ACCEPT
iptables -A INPUT -p tcp --dport 50051 -j DROP
```
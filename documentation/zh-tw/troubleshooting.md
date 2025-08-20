# HiveMind 故障排除指南

## 概述

本指南幫助您診斷和解決 HiveMind 分布式運算平台的常見問題。我們將問題按照組件和症狀進行分類，提供詳細的診斷步驟和解決方案。

## 快速診斷

### 系統健康檢查

使用以下命令檢查各組件狀態：

```bash
# 檢查服務狀態
python -c "
import subprocess
import socket

def check_port(host, port):
    try:
        socket.create_connection((host, port), timeout=3)
        return True
    except:
        return False

# 檢查主要端口
print('Node Pool (50051):', '✅' if check_port('localhost', 50051) else '❌')
print('Master Web (5000):', '✅' if check_port('localhost', 5000) else '❌')  
print('Redis (6379):', '✅' if check_port('localhost', 6379) else '❌')
"
```

### 日誌檢查

```bash
# 檢查主要組件日誌
tail -f node_pool/logs/nodepool.log
tail -f master/logs/master.log
tail -f worker/logs/worker.log

# 檢查系統日誌 (Linux)
journalctl -f -u hivemind-*
```

## 常見問題分類

### 1. 連接問題

#### 問題：無法連接到 Node Pool

**症狀**：
- 錯誤：`grpc._channel._InactiveRpcError: <_InactiveRpcError of RPC that terminated with: status = StatusCode.UNAVAILABLE`
- Worker 節點無法註冊
- Master 節點無法連接

**診斷步驟**：

```bash
# 1. 檢查 Node Pool 是否運行
ps aux | grep node_pool_server

# 2. 檢查端口監聽
netstat -tlnp | grep :50051
# 或
ss -tlnp | grep :50051

# 3. 測試網路連通性
telnet localhost 50051
# 或
nc -zv localhost 50051

# 4. 檢查防火牆
sudo ufw status
sudo iptables -L
```

**解決方案**：

1. **重啟 Node Pool 服務**：
   ```bash
   cd node_pool
   python node_pool_server.py
   ```

2. **檢查配置文件**：
   ```python
   # config.py
   GRPC_HOST = "0.0.0.0"  # 確保不是 127.0.0.1
   GRPC_PORT = 50051
   ```

3. **防火牆配置**：
   ```bash
   sudo ufw allow 50051
   sudo firewall-cmd --permanent --add-port=50051/tcp
   ```

#### 問題：Redis 連接失敗

**症狀**：
- 錯誤：`redis.exceptions.ConnectionError: Error connecting to Redis`
- 節點狀態無法更新
- 用戶認證失敗

**診斷步驟**：

```bash
# 1. 檢查 Redis 服務
sudo systemctl status redis
# 或
redis-cli ping

# 2. 檢查 Redis 配置
redis-cli config get bind
redis-cli config get protected-mode

# 3. 測試連接
redis-cli -h localhost -p 6379 ping
```

**解決方案**：

1. **啟動 Redis**：
   ```bash
   sudo systemctl start redis
   sudo systemctl enable redis
   ```

2. **修改 Redis 配置** (`/etc/redis/redis.conf`)：
   ```conf
   bind 0.0.0.0
   protected-mode no
   # 或設置密碼
   requirepass your_password
   ```

3. **重啟 Redis**：
   ```bash
   sudo systemctl restart redis
   ```

### 2. 性能問題

#### 問題：任務執行緩慢

**症狀**：
- 任務排隊時間過長
- 任務執行時間異常
- 系統回應延遲

**診斷步驟**：

```bash
# 1. 檢查系統資源
htop
iotop
free -h
df -h

# 2. 檢查 Worker 節點數量
redis-cli hgetall worker_nodes

# 3. 檢查任務隊列
redis-cli llen task_queue

# 4. 檢查網路延遲
ping worker-node-ip
traceroute worker-node-ip
```

**解決方案**：

1. **增加 Worker 節點**：
   ```bash
   # 在新機器上部署 Worker
   cd worker
   python worker_node.py --node-id worker-new
   ```

2. **優化資源配置**：
   ```python
   # worker_node.py
   MAX_CONCURRENT_TASKS = 4  # 根據 CPU 核心數調整
   TASK_TIMEOUT = 300        # 調整任務超時時間
   ```

3. **優化 Redis 配置**：
   ```conf
   maxmemory 2gb
   maxmemory-policy allkeys-lru
   ```

#### 問題：記憶體使用過高

**症狀**：
- OOM (Out of Memory) 錯誤
- 系統回應緩慢
- 服務自動重啟

**診斷步驟**：

```bash
# 1. 檢查記憶體使用
free -h
cat /proc/meminfo

# 2. 檢查進程記憶體使用
ps aux --sort=-%mem | head -20

# 3. 檢查 Python 記憶體使用
python3 -c "
import psutil
import os
process = psutil.Process(os.getpid())
print(f'Memory: {process.memory_info().rss / 1024 / 1024:.1f} MB')
"
```

**解決方案**：

1. **增加系統記憶體**
2. **優化代碼記憶體使用**：
   ```python
   # 定期清理未使用的對象
   import gc
   gc.collect()
   
   # 限制任務數據大小
   MAX_TASK_DATA_SIZE = 100 * 1024 * 1024  # 100MB
   ```

3. **配置交換文件**：
   ```bash
   sudo dd if=/dev/zero of=/swapfile bs=1G count=4
   sudo mkswap /swapfile
   sudo swapon /swapfile
   ```

### 3. 任務執行問題

#### 問題：任務執行失敗

**症狀**：
- 任務狀態顯示 FAILED
- 錯誤日誌中有異常信息
- 任務結果為空

**診斷步驟**：

```bash
# 1. 檢查任務日誌
tail -f worker/logs/task_execution.log

# 2. 檢查 Worker 節點日誌
tail -f worker/logs/worker.log

# 3. 檢查任務詳情
redis-cli hget task:TASK_ID status
redis-cli hget task:TASK_ID error_message
```

**解決方案**：

1. **檢查任務代碼**：
   ```python
   # 確保任務函數正確實現
   def task_function(data):
       try:
           # 處理邏輯
           return result
       except Exception as e:
           logging.error(f"任務執行失敗: {e}")
           raise
   ```

2. **增加錯誤處理**：
   ```python
   # worker_node.py
   try:
       result = execute_task(task_data)
   except Exception as e:
       logging.exception("任務執行異常")
       task_result = {"error": str(e), "status": "failed"}
   ```

3. **檢查依賴項**：
   ```bash
   pip install -r requirements.txt
   python -c "import numpy; print('numpy OK')"
   ```

#### 問題：任務超時

**症狀**：
- 任務長時間處於 RUNNING 狀態
- 錯誤日誌顯示超時
- Worker 節點無回應

**診斷步驟**：

```bash
# 1. 檢查任務執行時間
redis-cli hget task:TASK_ID start_time
redis-cli hget task:TASK_ID last_update

# 2. 檢查 Worker 節點狀態
redis-cli hget worker:WORKER_ID last_heartbeat
redis-cli hget worker:WORKER_ID status

# 3. 檢查系統負載
uptime
top
```

**解決方案**：

1. **調整超時設置**：
   ```python
   # config.py
   TASK_TIMEOUT = 600  # 10分鐘
   HEARTBEAT_INTERVAL = 30  # 30秒
   ```

2. **優化任務分割**：
   ```python
   # 將大任務分割為小任務
   def split_large_task(large_task):
       chunks = []
       for i in range(0, len(large_task), CHUNK_SIZE):
           chunks.append(large_task[i:i+CHUNK_SIZE])
       return chunks
   ```

3. **實現任務檢查點**：
   ```python
   def long_running_task(data):
       for i, item in enumerate(data):
           # 處理項目
           if i % 100 == 0:
               # 報告進度
               report_progress(i / len(data))
   ```

### 4. 網路問題

#### 問題：節點間通訊失敗

**症狀**：
- Worker 節點頻繁離線
- 任務分發失敗
- 網路超時錯誤

**診斷步驟**：

```bash
# 1. 檢查網路連通性
ping node-pool-server
ping worker-node-ip

# 2. 檢查端口可達性
telnet node-pool-server 50051
nmap -p 50051 node-pool-server

# 3. 檢查網路延遲
mtr node-pool-server
traceroute node-pool-server

# 4. 檢查防火牆規則
sudo iptables -L
sudo ufw status verbose
```

**解決方案**：

1. **配置防火牆**：
   ```bash
   # 開放必要端口
   sudo ufw allow from worker-subnet to any port 50051
   sudo ufw allow from worker-subnet to any port 6379
   ```

2. **優化網路配置**：
   ```python
   # 增加重試機制
   import grpc
   
   channel = grpc.insecure_channel(
       'node-pool:50051',
       options=[
           ('grpc.keepalive_time_ms', 30000),
           ('grpc.keepalive_timeout_ms', 5000),
           ('grpc.http2.max_pings_without_data', 0),
       ]
   )
   ```

3. **使用 VPN**：
   ```bash
   # 配置 WireGuard VPN
   cd master
   python vpn.py --setup
   ```

### 5. 數據問題

#### 問題：數據庫連接失敗

**症狀**：
- 用戶認證失敗
- 數據保存失敗
- SQLite 錯誤

**診斷步驟**：

```bash
# 1. 檢查數據庫文件
ls -la node_pool/users.db
file node_pool/users.db

# 2. 檢查數據庫權限
sqlite3 node_pool/users.db ".tables"
sqlite3 node_pool/users.db "SELECT count(*) FROM users;"

# 3. 檢查文件權限
ls -la node_pool/users.db
```

**解決方案**：

1. **修復數據庫權限**：
   ```bash
   chmod 664 node_pool/users.db
   chown hivemind:hivemind node_pool/users.db
   ```

2. **重建數據庫**：
   ```bash
   cd node_pool
   python database_migration.py
   ```

3. **檢查磁盤空間**：
   ```bash
   df -h
   # 確保有足夠的磁盤空間
   ```

#### 問題：數據不一致

**症狀**：
- Redis 和數據庫數據不匹配
- 節點狀態異常
- 任務狀態錯誤

**診斷步驟**：

```bash
# 1. 比較數據源
redis-cli hgetall worker_nodes
sqlite3 users.db "SELECT * FROM users;"

# 2. 檢查數據同步日誌
grep "sync" node_pool/logs/*.log
```

**解決方案**：

1. **手動同步數據**：
   ```python
   # 執行數據同步腳本
   python node_pool/sync_data.py
   ```

2. **重置系統狀態**：
   ```bash
   # 清除 Redis 緩存
   redis-cli flushall
   
   # 重啟服務
   systemctl restart hivemind-*
   ```

## 性能監控

### 設置監控

```bash
# 安裝監控工具
pip install psutil prometheus-client

# 創建監控腳本
cat > monitor.py << 'EOF'
import psutil
import time
import redis

r = redis.Redis()

while True:
    # CPU 使用率
    cpu_percent = psutil.cpu_percent()
    
    # 記憶體使用率
    memory = psutil.virtual_memory()
    
    # 磁盤使用率
    disk = psutil.disk_usage('/')
    
    # Worker 節點數量
    worker_count = r.hlen('worker_nodes')
    
    print(f"CPU: {cpu_percent}%, Memory: {memory.percent}%, "
          f"Disk: {disk.percent}%, Workers: {worker_count}")
    
    time.sleep(10)
EOF

python monitor.py
```

### 效能調優建議

1. **Redis 優化**：
   ```conf
   # /etc/redis/redis.conf
   maxmemory 2gb
   maxmemory-policy allkeys-lru
   save 900 1
   save 300 10
   save 60 10000
   ```

2. **Python 優化**：
   ```python
   # 使用連接池
   import redis.connection
   pool = redis.ConnectionPool(host='localhost', port=6379, db=0)
   r = redis.Redis(connection_pool=pool)
   ```

3. **系統優化**：
   ```bash
   # 增加文件描述符限制
   echo "hivemind soft nofile 65536" >> /etc/security/limits.conf
   echo "hivemind hard nofile 65536" >> /etc/security/limits.conf
   ```

## 日誌分析

### 重要日誌文件

```bash
# 主要日誌位置
node_pool/logs/nodepool.log      # Node Pool 服務日誌
master/logs/master.log           # Master 節點日誌
worker/logs/worker.log           # Worker 節點日誌
/var/log/redis/redis-server.log  # Redis 日誌
```

### 常見錯誤模式

```bash
# 搜索常見錯誤
grep -i "error\|exception\|failed" */logs/*.log
grep -i "timeout\|connection" */logs/*.log
grep -i "memory\|disk" */logs/*.log
```

## 恢復程序

### 系統完全重啟

```bash
#!/bin/bash
# recovery.sh

echo "開始系統恢復程序..."

# 1. 停止所有服務
sudo systemctl stop hivemind-*
sudo systemctl stop redis

# 2. 清理臨時文件
rm -f */logs/*.log.old
rm -f */tmp/*

# 3. 重啟基礎服務
sudo systemctl start redis
sleep 5

# 4. 重啟 HiveMind 服務
sudo systemctl start hivemind-nodepool
sleep 10
sudo systemctl start hivemind-master

# 5. 驗證服務狀態
systemctl is-active hivemind-nodepool
systemctl is-active hivemind-master

echo "恢復程序完成"
```

### 數據備份和恢復

```bash
# 備份
tar -czf hivemind-backup-$(date +%Y%m%d).tar.gz \
    node_pool/users.db \
    master/config/ \
    worker/config/

# 恢復
tar -xzf hivemind-backup-YYYYMMDD.tar.gz
```

---

**更新日期**: 2024年1月  
**版本**: v1.0  
**狀態**: 基於實際部署經驗的故障排除指南

**注意**: 本指南基於實際運行環境的問題和解決方案。如遇到此處未涵蓋的問題，請檢查最新的日誌文件並聯繫技術支援。

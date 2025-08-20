# HiveMind 故障排除指南

## 概述

本指南幫助您診斷和解決 HiveMind 分布式運算平台的常見問題。我們將問題按照組件和症狀進行分類，提供詳細的診斷步驟和解決方案。

## 快速診斷工具

### 系統健康檢查腳本

創建一個自動化診斷腳本：

```bash
#!/bin/bash
# health_check.sh

echo "=== HiveMind 系統健康檢查 ==="
echo "時間: $(date)"
echo

# 檢查服務狀態
echo "1. 服務狀態檢查:"
systemctl is-active hivemind-node-pool && echo "✓ Node Pool: 運行中" || echo "✗ Node Pool: 停止"
systemctl is-active hivemind-master && echo "✓ Master: 運行中" || echo "✗ Master: 停止"
systemctl is-active redis && echo "✓ Redis: 運行中" || echo "✗ Redis: 停止"
systemctl is-active postgresql && echo "✓ PostgreSQL: 運行中" || echo "✗ PostgreSQL: 停止"
echo

# 檢查端口監聽
echo "2. 端口監聽檢查:"
netstat -tlnp | grep ":50051" > /dev/null && echo "✓ Node Pool gRPC (50051): 監聽中" || echo "✗ Node Pool gRPC (50051): 未監聽"
netstat -tlnp | grep ":5001" > /dev/null && echo "✓ Master Web (5001): 監聽中" || echo "✗ Master Web (5001): 未監聽"
netstat -tlnp | grep ":6379" > /dev/null && echo "✓ Redis (6379): 監聽中" || echo "✗ Redis (6379): 未監聽"
echo

# 檢查磁盤空間
echo "3. 磁盤空間檢查:"
df -h | grep -E "/$|/data|/var" | while read line; do
    usage=$(echo $line | awk '{print $5}' | sed 's/%//')
    mount=$(echo $line | awk '{print $6}')
    if [ $usage -gt 80 ]; then
        echo "⚠ $mount: ${usage}% (警告)"
    else
        echo "✓ $mount: ${usage}%"
    fi
done
echo

# 檢查內存使用
echo "4. 內存使用檢查:"
free -h | grep "Mem:" | awk '{
    used = $3; total = $2; 
    print "內存使用: " used "/" total
}'
echo

# 檢查 Docker 狀態
echo "5. Docker 狀態檢查:"
if command -v docker &> /dev/null; then
    docker ps --format "table {{.Names}}\t{{.Status}}" | grep hivemind
else
    echo "Docker 未安裝或不可訪問"
fi
echo

echo "=== 檢查完成 ==="
```

## Node Pool 問題

### 問題 1: Node Pool 服務無法啟動

**症狀**：
- Node Pool 服務啟動失敗
- gRPC 端口 50051 無法訪問
- 工作節點無法註冊

**診斷步驟**：

```bash
# 檢查服務狀態
systemctl status hivemind-node-pool

# 查看詳細日誌
journalctl -u hivemind-node-pool -f

# 檢查端口佔用
netstat -tlnp | grep 50051
lsof -i :50051
```

**常見原因和解決方案**：

1. **端口被佔用**
   ```bash
   # 查找佔用進程
   lsof -i :50051
   
   # 終止進程
   kill -9 <PID>
   
   # 或修改配置使用其他端口
   ```

2. **Redis 連接失敗**
   ```bash
   # 檢查 Redis 狀態
   systemctl status redis
   redis-cli ping
   
   # 檢查連接配置
   cat /opt/hivemind/node_pool/.env | grep REDIS
   ```

3. **權限問題**
   ```bash
   # 檢查文件權限
   ls -la /opt/hivemind/node_pool/
   
   # 修正權限
   sudo chown -R hivemind:hivemind /opt/hivemind/
   chmod +x /opt/hivemind/node_pool/node_pool_server.py
   ```

4. **Python 依賴缺失**
   ```bash
   # 激活虛擬環境並檢查依賴
   source /opt/hivemind/venv/bin/activate
   pip check
   
   # 重新安裝依賴
   pip install -r requirements.txt
   ```

### 問題 2: 節點註冊失敗

**症狀**：
- Worker 節點無法向 Node Pool 註冊
- 節點顯示為 "未連接" 狀態
- gRPC 連接錯誤

**診斷步驟**：

```bash
# 測試 gRPC 連接
grpcurl -plaintext localhost:50051 list

# 檢查防火牆設置
sudo ufw status
iptables -L

# 測試網路連通性
telnet <node_pool_ip> 50051
```

**解決方案**：

1. **防火牆配置**
   ```bash
   # Ubuntu/Debian
   sudo ufw allow 50051/tcp
   
   # CentOS/RHEL
   sudo firewall-cmd --permanent --add-port=50051/tcp
   sudo firewall-cmd --reload
   ```

2. **網路連接問題**
   ```bash
   # 檢查路由
   traceroute <node_pool_ip>
   
   # 檢查 DNS 解析
   nslookup <node_pool_hostname>
   ```

### 問題 3: 任務分配失敗

**症狀**：
- 任務創建成功但未分配給節點
- 節點顯示閒置但未接收任務
- 任務停留在佇列中

**診斷步驟**：

```bash
# 檢查 Redis 中的任務數據
redis-cli
> KEYS task:*
> HGETALL task:<task_id>

# 檢查節點狀態
> KEYS node:*
> HGETALL node:<node_id>

# 檢查任務佇列
> LLEN task_queue
> LRANGE task_queue 0 -1
```

**解決方案**：

1. **節點資源不足**
   ```bash
   # 檢查節點資源使用
   redis-cli HGET node:<node_id> cpu_usage
   redis-cli HGET node:<node_id> memory_usage
   
   # 調整資源閾值
   # 編輯 node_pool/config.py
   ```

2. **節點信任等級問題**
   ```bash
   # 檢查節點信任等級
   redis-cli HGET node:<node_id> trust_level
   
   # 檢查用戶信用評分
   sqlite3 /opt/hivemind/node_pool/users.db
   SELECT username, credit_score FROM users;
   ```

## Master 節點問題

### 問題 4: Web 界面無法訪問

**症狀**：
- 無法訪問 http://localhost:5001
- 頁面載入失敗或超時
- 502/503 錯誤

**診斷步驟**：

```bash
# 檢查 Master 服務狀態
systemctl status hivemind-master

# 檢查端口監聽
netstat -tlnp | grep 5001

# 檢查 Nginx 配置 (如果使用)
nginx -t
systemctl status nginx
```

**解決方案**：

1. **Flask 應用程序錯誤**
   ```bash
   # 查看應用程序日誌
   journalctl -u hivemind-master -f
   
   # 手動啟動以查看錯誤
   cd /opt/hivemind/master
   source ../venv/bin/activate
   python master_node.py
   ```

2. **Nginx 反向代理問題**
   ```bash
   # 檢查 Nginx 配置
   nginx -t
   
   # 重新載入配置
   sudo systemctl reload nginx
   
   # 檢查 Nginx 錯誤日誌
   tail -f /var/log/nginx/error.log
   ```

### 問題 5: 用戶認證失敗

**症狀**：
- 用戶無法登入
- JWT 令牌驗證失敗
- 註冊過程中斷

**診斷步驟**：

```bash
# 檢查數據庫連接
sqlite3 /opt/hivemind/node_pool/users.db
.tables
SELECT * FROM users LIMIT 5;

# 檢查 JWT 配置
grep JWT_SECRET /opt/hivemind/master/.env
```

**解決方案**：

1. **密碼哈希問題**
   ```python
   # 測試密碼哈希
   import bcrypt
   password = "test_password"
   hashed = bcrypt.hashpw(password.encode('utf-8'), bcrypt.gensalt())
   print(bcrypt.checkpw(password.encode('utf-8'), hashed))
   ```

2. **JWT 密鑰不匹配**
   ```bash
   # 確保所有服務使用相同的 JWT 密鑰
   grep JWT_SECRET /opt/hivemind/*/'.env'
   ```

## Worker 節點問題

### 問題 6: Docker 容器啟動失敗

**症狀**：
- 任務執行失敗
- Docker 容器無法創建
- 資源限制錯誤

**診斷步驟**：

```bash
# 檢查 Docker 服務
systemctl status docker

# 檢查 Docker 映像
docker images | grep hivemind

# 測試容器創建
docker run --rm hello-world

# 檢查磁盤空間
df -h
docker system df
```

**解決方案**：

1. **Docker 映像缺失**
   ```bash
   # 拉取所需映像
   docker pull justin308/hivemind-worker:latest
   
   # 或構建本地映像
   cd /opt/hivemind/worker
   docker build -t hivemind-worker .
   ```

2. **磁盤空間不足**
   ```bash
   # 清理 Docker 資源
   docker system prune -f
   docker volume prune -f
   
   # 清理舊映像
   docker image prune -a -f
   ```

3. **資源限制問題**
   ```bash
   # 檢查系統資源
   free -h
   nproc
   
   # 調整容器資源限制
   # 編輯 worker/worker_node.py 中的資源設定
   ```

### 問題 7: VPN 連接失敗

**症狀**：
- WireGuard 配置生成失敗
- 節點間無法通信
- VPN 隧道建立失敗

**診斷步驟**：

```bash
# 檢查 WireGuard 狀態
sudo wg show

# 檢查配置文件
cat /opt/hivemind/worker/wg0.conf

# 測試網路連通性
ping 10.8.0.1

# 檢查路由表
ip route | grep 10.8.0
```

**解決方案**：

1. **WireGuard 未安裝**
   ```bash
   # Ubuntu/Debian
   sudo apt install wireguard
   
   # CentOS/RHEL
   sudo yum install wireguard-tools
   ```

2. **配置文件權限**
   ```bash
   # 設置正確權限
   sudo chmod 600 /opt/hivemind/worker/wg0.conf
   sudo chown root:root /opt/hivemind/worker/wg0.conf
   ```

3. **防火牆阻擋 UDP 流量**
   ```bash
   # 允許 WireGuard 端口
   sudo ufw allow 51820/udp
   ```

## 性能問題

### 問題 8: 系統響應緩慢

**症狀**：
- API 響應時間過長
- 任務分配延遲
- Web 界面載入緩慢

**診斷步驟**：

```bash
# 檢查系統負載
uptime
htop

# 檢查內存使用
free -h
cat /proc/meminfo

# 檢查磁盤 I/O
iostat -x 1 5
iotop

# 檢查網路連接
netstat -an | grep ESTABLISHED | wc -l
```

**解決方案**：

1. **內存不足**
   ```bash
   # 增加 swap 空間
   sudo fallocate -l 2G /swapfile
   sudo chmod 600 /swapfile
   sudo mkswap /swapfile
   sudo swapon /swapfile
   
   # 永久啟用
   echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
   ```

2. **Redis 性能優化**
   ```bash
   # 編輯 Redis 配置
   sudo nano /etc/redis/redis.conf
   
   # 調整以下參數：
   # maxmemory 2gb
   # maxmemory-policy allkeys-lru
   # tcp-keepalive 60
   
   sudo systemctl restart redis
   ```

3. **資料庫查詢優化**
   ```sql
   -- 為常用查詢添加索引
   CREATE INDEX idx_users_username ON users(username);
   CREATE INDEX idx_nodes_status ON nodes(status);
   
   -- 分析查詢性能
   EXPLAIN ANALYZE SELECT * FROM users WHERE username = 'test';
   ```

### 問題 9: 高併發處理問題

**症狀**：
- 大量節點註冊時系統崩潰
- 併發任務執行失敗
- gRPC 連接超時

**解決方案**：

1. **增加 gRPC 線程池大小**
   ```python
   # 在 node_pool_server.py 中調整
   server = grpc.server(
       futures.ThreadPoolExecutor(max_workers=50),  # 增加到 50
       options=[
           ('grpc.keepalive_time_ms', 30000),
           ('grpc.keepalive_timeout_ms', 5000),
           ('grpc.keepalive_permit_without_calls', True),
           ('grpc.http2.max_pings_without_data', 0),
           ('grpc.http2.min_time_between_pings_ms', 10000),
           ('grpc.http2.min_ping_interval_without_data_ms', 300000)
       ]
   )
   ```

2. **配置連接池**
   ```python
   # Redis 連接池配置
   import redis
   
   pool = redis.ConnectionPool(
       host='localhost',
       port=6379,
       db=0,
       max_connections=20,
       retry_on_timeout=True
   )
   redis_client = redis.Redis(connection_pool=pool)
   ```

## 數據問題

### 問題 10: 數據不一致

**症狀**：
- 節點狀態顯示錯誤
- 任務狀態不正確
- 用戶餘額異常

**診斷和修復**：

```bash
# 數據一致性檢查腳本
#!/bin/bash
# data_consistency_check.sh

echo "檢查 Redis 和 SQLite 數據一致性..."

# 檢查用戶數據
echo "用戶數據檢查:"
sqlite3 /opt/hivemind/node_pool/users.db "SELECT COUNT(*) FROM users;" | \
  while read count; do echo "SQLite 用戶數: $count"; done

# 檢查節點數據
echo "節點數據檢查:"
redis-cli KEYS "node:*" | wc -l | \
  while read count; do echo "Redis 節點數: $count"; done

# 清理過期數據
echo "清理過期任務數據..."
redis-cli EVAL "
  local keys = redis.call('KEYS', 'task:*')
  local expired = 0
  for i=1,#keys do
    local created = redis.call('HGET', keys[i], 'created_at')
    if created and (os.time() - tonumber(created)) > 86400 then
      redis.call('DEL', keys[i])
      expired = expired + 1
    end
  end
  return expired
" 0
```

## 監控和預防

### 設置監控告警

1. **系統資源監控**
   ```bash
   # 創建監控腳本
   #!/bin/bash
   # monitor.sh
   
   # CPU 使用率告警
   CPU_USAGE=$(top -bn1 | grep "Cpu(s)" | awk '{print $2}' | sed 's/%us,//')
   if (( $(echo "$CPU_USAGE > 80" | bc -l) )); then
       echo "CPU 使用率過高: $CPU_USAGE%" | mail -s "HiveMind 告警" admin@example.com
   fi
   
   # 內存使用率告警
   MEM_USAGE=$(free | grep Mem | awk '{printf("%.2f", $3/$2 * 100.0)}')
   if (( $(echo "$MEM_USAGE > 85" | bc -l) )); then
       echo "內存使用率過高: $MEM_USAGE%" | mail -s "HiveMind 告警" admin@example.com
   fi
   ```

2. **服務健康檢查**
   ```bash
   # 添加到 crontab
   */5 * * * * /opt/hivemind/scripts/health_check.sh
   0 */6 * * * /opt/hivemind/scripts/data_consistency_check.sh
   ```

### 日誌管理

1. **配置日誌輪轉**
   ```bash
   # /etc/logrotate.d/hivemind
   /var/log/hivemind/*.log {
       daily
       missingok
       rotate 30
       compress
       delaycompress
       notifempty
       copytruncate
   }
   ```

2. **集中日誌收集**
   ```yaml
   # docker-compose.yml 日誌配置
   services:
     node-pool:
       logging:
         driver: "json-file"
         options:
           max-size: "100m"
           max-file: "5"
   ```

## 緊急恢復程序

### 服務快速恢復

```bash
#!/bin/bash
# emergency_recovery.sh

echo "開始緊急恢復程序..."

# 停止所有服務
sudo systemctl stop hivemind-node-pool
sudo systemctl stop hivemind-master
sudo systemctl stop nginx

# 檢查並修復文件系統
sudo fsck -y /dev/sda1

# 清理臨時文件
sudo rm -rf /tmp/hivemind_*
sudo rm -rf /var/log/hivemind/*.log.1

# 重置 Redis 數據 (謹慎使用)
# redis-cli FLUSHALL

# 重啟服務
sudo systemctl start redis
sudo systemctl start postgresql
sudo systemctl start hivemind-node-pool
sudo systemctl start hivemind-master
sudo systemctl start nginx

# 驗證服務狀態
sleep 10
./health_check.sh
```

## 聯繫支援

如果以上解決方案都無法解決您的問題，請聯繫技術支援：

- **GitHub Issues**: https://github.com/him6794/hivemind/issues
- **電子郵件**: support@hivemind.com
- **社群論壇**: https://forum.hivemind.com

提交問題時，請包含：
1. 詳細的錯誤描述
2. 相關的日誌輸出
3. 系統環境資訊
4. 重現步驟
5. 已嘗試的解決方案

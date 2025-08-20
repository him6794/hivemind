# HiveMind Troubleshooting Guide

## Overview

This guide helps you diagnose and resolve common issues with the HiveMind distributed computing platform. Problems are categorized by component and symptoms, with detailed diagnostic steps and solutions.

## Quick Diagnostics

### System Health Check

Use the following commands to check component status:

```bash
# Check service status
python -c "
import subprocess
import socket

def check_port(host, port):
    try:
        socket.create_connection((host, port), timeout=3)
        return True
    except:
        return False

# Check main ports
print('Node Pool (50051):', '✅' if check_port('localhost', 50051) else '❌')
print('Master Web (5000):', '✅' if check_port('localhost', 5000) else '❌')  
print('Redis (6379):', '✅' if check_port('localhost', 6379) else '❌')
"
```

### Log Checking

```bash
# Check main component logs
tail -f node_pool/logs/nodepool.log
tail -f master/logs/master.log
tail -f worker/logs/worker.log

# Check system logs (Linux)
journalctl -f -u hivemind-*
```

## Common Problem Categories

### 1. Connection Issues

#### Problem: Cannot Connect to Node Pool

**Symptoms**:
- Error: `grpc._channel._InactiveRpcError: <_InactiveRpcError of RPC that terminated with: status = StatusCode.UNAVAILABLE`
- Worker nodes cannot register
- Master node cannot connect

**Diagnostic Steps**:

```bash
# 1. Check if Node Pool is running
ps aux | grep node_pool_server

# 2. Check port listening
netstat -tlnp | grep :50051
# or
ss -tlnp | grep :50051

# 3. Test network connectivity
telnet localhost 50051
# or
nc -zv localhost 50051

# 4. Check firewall
sudo ufw status
sudo iptables -L
```

**Solutions**:

1. **Restart Node Pool Service**:
   ```bash
   cd node_pool
   python node_pool_server.py
   ```

2. **Check Configuration File**:
   ```python
   # config.py
   GRPC_HOST = "0.0.0.0"  # Ensure it's not 127.0.0.1
   GRPC_PORT = 50051
   ```

3. **Firewall Configuration**:
   ```bash
   sudo ufw allow 50051
   sudo firewall-cmd --permanent --add-port=50051/tcp
   ```

#### Problem: Redis Connection Failure

**Symptoms**:
- Error: `redis.exceptions.ConnectionError: Error connecting to Redis`
- Node status cannot update
- User authentication fails

**Diagnostic Steps**:

```bash
# 1. Check Redis service
sudo systemctl status redis
# or
redis-cli ping

# 2. Check Redis configuration
redis-cli config get bind
redis-cli config get protected-mode

# 3. Test connection
redis-cli -h localhost -p 6379 ping
```

**Solutions**:

1. **Start Redis**:
   ```bash
   sudo systemctl start redis
   sudo systemctl enable redis
   ```

2. **Modify Redis Configuration** (`/etc/redis/redis.conf`):
   ```conf
   bind 0.0.0.0
   protected-mode no
   # or set password
   requirepass your_password
   ```

3. **Restart Redis**:
   ```bash
   sudo systemctl restart redis
   ```

### 2. Performance Issues

#### Problem: Slow Task Execution

**Symptoms**:
- Long task queue times
- Abnormal task execution times
- System response delays

**Diagnostic Steps**:

```bash
# 1. Check system resources
htop
iotop
free -h
df -h

# 2. Check Worker node count
redis-cli hgetall worker_nodes

# 3. Check task queue
redis-cli llen task_queue

# 4. Check network latency
ping worker-node-ip
traceroute worker-node-ip
```

**Solutions**:

1. **Add Worker Nodes**:
   ```bash
   # Deploy Worker on new machine
   cd worker
   python worker_node.py --node-id worker-new
   ```

2. **Optimize Resource Configuration**:
   ```python
   # worker_node.py
   MAX_CONCURRENT_TASKS = 4  # Adjust based on CPU cores
   TASK_TIMEOUT = 300        # Adjust task timeout
   ```

3. **Optimize Redis Configuration**:
   ```conf
   maxmemory 2gb
   maxmemory-policy allkeys-lru
   ```

#### Problem: High Memory Usage

**Symptoms**:
- OOM (Out of Memory) errors
- Slow system response
- Service auto-restart

**Diagnostic Steps**:

```bash
# 1. Check memory usage
free -h
cat /proc/meminfo

# 2. Check process memory usage
ps aux --sort=-%mem | head -20

# 3. Check Python memory usage
python3 -c "
import psutil
import os
process = psutil.Process(os.getpid())
print(f'Memory: {process.memory_info().rss / 1024 / 1024:.1f} MB')
"
```

**Solutions**:

1. **Increase System Memory**
2. **Optimize Code Memory Usage**:
   ```python
   # Periodically clean unused objects
   import gc
   gc.collect()
   
   # Limit task data size
   MAX_TASK_DATA_SIZE = 100 * 1024 * 1024  # 100MB
   ```

3. **Configure Swap File**:
   ```bash
   sudo dd if=/dev/zero of=/swapfile bs=1G count=4
   sudo mkswap /swapfile
   sudo swapon /swapfile
   ```

### 3. Task Execution Issues

#### Problem: Task Execution Failure

**Symptoms**:
- Task status shows FAILED
- Exception information in error logs
- Empty task results

**Diagnostic Steps**:

```bash
# 1. Check task logs
tail -f worker/logs/task_execution.log

# 2. Check Worker node logs
tail -f worker/logs/worker.log

# 3. Check task details
redis-cli hget task:TASK_ID status
redis-cli hget task:TASK_ID error_message
```

**Solutions**:

1. **Check Task Code**:
   ```python
   # Ensure task function is correctly implemented
   def task_function(data):
       try:
           # Processing logic
           return result
       except Exception as e:
           logging.error(f"Task execution failed: {e}")
           raise
   ```

2. **Add Error Handling**:
   ```python
   # worker_node.py
   try:
       result = execute_task(task_data)
   except Exception as e:
       logging.exception("Task execution exception")
       task_result = {"error": str(e), "status": "failed"}
   ```

3. **Check Dependencies**:
   ```bash
   pip install -r requirements.txt
   python -c "import numpy; print('numpy OK')"
   ```

#### Problem: Task Timeout

**Symptoms**:
- Tasks stay in RUNNING status for long periods
- Error logs show timeout
- Worker nodes unresponsive

**Diagnostic Steps**:

```bash
# 1. Check task execution time
redis-cli hget task:TASK_ID start_time
redis-cli hget task:TASK_ID last_update

# 2. Check Worker node status
redis-cli hget worker:WORKER_ID last_heartbeat
redis-cli hget worker:WORKER_ID status

# 3. Check system load
uptime
top
```

**Solutions**:

1. **Adjust Timeout Settings**:
   ```python
   # config.py
   TASK_TIMEOUT = 600  # 10 minutes
   HEARTBEAT_INTERVAL = 30  # 30 seconds
   ```

2. **Optimize Task Splitting**:
   ```python
   # Split large tasks into smaller ones
   def split_large_task(large_task):
       chunks = []
       for i in range(0, len(large_task), CHUNK_SIZE):
           chunks.append(large_task[i:i+CHUNK_SIZE])
       return chunks
   ```

3. **Implement Task Checkpoints**:
   ```python
   def long_running_task(data):
       for i, item in enumerate(data):
           # Process item
           if i % 100 == 0:
               # Report progress
               report_progress(i / len(data))
   ```

### 4. Network Issues

#### Problem: Inter-Node Communication Failure

**Symptoms**:
- Worker nodes frequently go offline
- Task distribution fails
- Network timeout errors

**Diagnostic Steps**:

```bash
# 1. Check network connectivity
ping node-pool-server
ping worker-node-ip

# 2. Check port accessibility
telnet node-pool-server 50051
nmap -p 50051 node-pool-server

# 3. Check network latency
mtr node-pool-server
traceroute node-pool-server

# 4. Check firewall rules
sudo iptables -L
sudo ufw status verbose
```

**Solutions**:

1. **Configure Firewall**:
   ```bash
   # Open necessary ports
   sudo ufw allow from worker-subnet to any port 50051
   sudo ufw allow from worker-subnet to any port 6379
   ```

2. **Optimize Network Configuration**:
   ```python
   # Add retry mechanism
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

3. **Use VPN**:
   ```bash
   # Configure WireGuard VPN
   cd master
   python vpn.py --setup
   ```

### 5. Data Issues

#### Problem: Database Connection Failure

**Symptoms**:
- User authentication fails
- Data save failures
- SQLite errors

**Diagnostic Steps**:

```bash
# 1. Check database file
ls -la node_pool/users.db
file node_pool/users.db

# 2. Check database permissions
sqlite3 node_pool/users.db ".tables"
sqlite3 node_pool/users.db "SELECT count(*) FROM users;"

# 3. Check file permissions
ls -la node_pool/users.db
```

**Solutions**:

1. **Fix Database Permissions**:
   ```bash
   chmod 664 node_pool/users.db
   chown hivemind:hivemind node_pool/users.db
   ```

2. **Rebuild Database**:
   ```bash
   cd node_pool
   python database_migration.py
   ```

3. **Check Disk Space**:
   ```bash
   df -h
   # Ensure sufficient disk space
   ```

#### Problem: Data Inconsistency

**Symptoms**:
- Redis and database data mismatch
- Abnormal node status
- Incorrect task status

**Diagnostic Steps**:

```bash
# 1. Compare data sources
redis-cli hgetall worker_nodes
sqlite3 users.db "SELECT * FROM users;"

# 2. Check data sync logs
grep "sync" node_pool/logs/*.log
```

**Solutions**:

1. **Manual Data Sync**:
   ```python
   # Execute data sync script
   python node_pool/sync_data.py
   ```

2. **Reset System State**:
   ```bash
   # Clear Redis cache
   redis-cli flushall
   
   # Restart services
   systemctl restart hivemind-*
   ```

## Performance Monitoring

### Setup Monitoring

```bash
# Install monitoring tools
pip install psutil prometheus-client

# Create monitoring script
cat > monitor.py << 'EOF'
import psutil
import time
import redis

r = redis.Redis()

while True:
    # CPU usage
    cpu_percent = psutil.cpu_percent()
    
    # Memory usage
    memory = psutil.virtual_memory()
    
    # Disk usage
    disk = psutil.disk_usage('/')
    
    # Worker node count
    worker_count = r.hlen('worker_nodes')
    
    print(f"CPU: {cpu_percent}%, Memory: {memory.percent}%, "
          f"Disk: {disk.percent}%, Workers: {worker_count}")
    
    time.sleep(10)
EOF

python monitor.py
```

### Performance Tuning Recommendations

1. **Redis Optimization**:
   ```conf
   # /etc/redis/redis.conf
   maxmemory 2gb
   maxmemory-policy allkeys-lru
   save 900 1
   save 300 10
   save 60 10000
   ```

2. **Python Optimization**:
   ```python
   # Use connection pool
   import redis.connection
   pool = redis.ConnectionPool(host='localhost', port=6379, db=0)
   r = redis.Redis(connection_pool=pool)
   ```

3. **System Optimization**:
   ```bash
   # Increase file descriptor limit
   echo "hivemind soft nofile 65536" >> /etc/security/limits.conf
   echo "hivemind hard nofile 65536" >> /etc/security/limits.conf
   ```

## Log Analysis

### Important Log Files

```bash
# Main log locations
node_pool/logs/nodepool.log      # Node Pool service logs
master/logs/master.log           # Master node logs
worker/logs/worker.log           # Worker node logs
/var/log/redis/redis-server.log  # Redis logs
```

### Common Error Patterns

```bash
# Search for common errors
grep -i "error\|exception\|failed" */logs/*.log
grep -i "timeout\|connection" */logs/*.log
grep -i "memory\|disk" */logs/*.log
```

## Recovery Procedures

### Complete System Restart

```bash
#!/bin/bash
# recovery.sh

echo "Starting system recovery procedure..."

# 1. Stop all services
sudo systemctl stop hivemind-*
sudo systemctl stop redis

# 2. Clean temporary files
rm -f */logs/*.log.old
rm -f */tmp/*

# 3. Restart base services
sudo systemctl start redis
sleep 5

# 4. Restart HiveMind services
sudo systemctl start hivemind-nodepool
sleep 10
sudo systemctl start hivemind-master

# 5. Verify service status
systemctl is-active hivemind-nodepool
systemctl is-active hivemind-master

echo "Recovery procedure completed"
```

### Data Backup and Recovery

```bash
# Backup
tar -czf hivemind-backup-$(date +%Y%m%d).tar.gz \
    node_pool/users.db \
    master/config/ \
    worker/config/

# Recovery
tar -xzf hivemind-backup-YYYYMMDD.tar.gz
```

---

**Updated**: January 2024  
**Version**: v1.0  
**Status**: Troubleshooting guide based on actual deployment experience

**Note**: This guide is based on issues and solutions from actual running environments. For problems not covered here, please check the latest log files and contact technical support.

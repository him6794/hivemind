# HiveMind VPN 快速開始指南

5 分鐘快速體驗 HiveMind 的 VPN 多節點功能。
> Note: the commands, task IDs, and helper scripts in this guide are illustrative examples only. They are not shipped runtime assets unless you add them yourself.

## 快速啟動

### 前置需求

- Docker 和 Docker Compose
- Linux 系統（或 WSL2）
- 至少 4GB 可用記憶體

### 步驟 1: 克隆專案

```bash
git clone https://github.com/your-org/hivemind.git
cd hivemind
```

### 步驟 2: 啟動 VPN 環境

```bash
cd infra
docker-compose -f docker-compose.vpn.yml up -d
```

這將啟動：
- 1 個 Nodepool (含 Headscale VPN 伺服器)
- 2 個 Worker 節點
- PostgreSQL 資料庫
- Redis 快取

### 步驟 3: 查看啟動狀態

```bash
# 查看所有服務
docker-compose -f docker-compose.vpn.yml ps

# 查看日誌
docker-compose -f docker-compose.vpn.yml logs -f
```

等待約 30 秒，直到看到：
```
nodepool  | INFO Worker worker-001 registered to VPN
nodepool  | INFO Worker worker-002 registered to VPN
worker1   | INFO VPN connected, local IP: 100.64.0.1
worker2   | INFO VPN connected, local IP: 100.64.0.2
```

### 步驟 4: 驗證 VPN 連接

```bash
# 進入 Worker 1 容器
docker exec -it hivemind-worker1 sh

# 測試連接到 Worker 2 的 VPN IP
ping -c 3 100.64.0.2

# 退出容器
exit
```

成功！Worker 節點已通過 VPN 建立加密連接。

## 簡單的多節點任務範例

### 範例 1: 分散式計算任務

創建測試腳本 `test_multinode.go`:

```go
package main

import (
    "context"
    "fmt"
    "log"
    "time"

    "google.golang.org/grpc"
    "google.golang.org/grpc/credentials/insecure"
)

func main() {
    // 連接到 Nodepool
    conn, err := grpc.Dial("localhost:50051", 
        grpc.WithTransportCredentials(insecure.NewCredentials()))
    if err != nil {
        log.Fatalf("Failed to connect: %v", err)
    }
    defer conn.Close()

    // 創建多節點任務
    task := &Task{
        ID:       "example-task-001",
        Type:     "distributed-compute",
        Workers:  []string{"worker-001", "worker-002"},
        Payload:  []byte("compute: sum(1..1000000)"),
    }

    fmt.Printf("Submitting multinode task: %s\n", task.ID)
    
    // 提交任務
    ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
    defer cancel()

    result, err := submitTask(ctx, conn, task)
    if err != nil {
        log.Fatalf("Task failed: %v", err)
    }

    fmt.Printf("Task completed!\n")
    fmt.Printf("Result: %s\n", string(result.Output))
    fmt.Printf("Execution time: %v\n", result.Duration)
}
```

運行測試：

```bash
# 構建測試程式
go build -o test_multinode test_multinode.go

# 執行測試
./test_multinode
```

預期輸出：
```
Submitting multinode task: example-task-001
Task assigned to workers: [worker-001 worker-002]
Workers communicating via VPN...
Task completed!
Result: sum = 500000500000
Execution time: 1.234s
```

### 範例 2: 使用 VPN Example

使用內建的 VPN 示範程式：

```bash
# 在 Worker 1 上運行 example
docker exec hivemind-worker1 /app/vpn_example --worker-id=worker-001

# 在另一個終端，在 Worker 2 上運行 example
docker exec hivemind-worker2 /app/vpn_example --worker-id=worker-002
```

你會看到：
```
=== VPN Features Example ===
Local VPN IP: 100.64.0.1
VPN Status: Connected
Connected peers: 1
  - worker-002: 100.64.0.2

=== Task Execution Example ===
Set task ID: example-task-001
Task peers: 2
  - Worker worker-001 (worker-001) at 100.64.0.1
  - Worker worker-002 (worker-002) at 100.64.0.2
Executing task example-task-001...
Task completed successfully: Example task completed
```

### 範例 3: 監控 VPN 狀態

創建監控腳本 `monitor_vpn.sh`:

```bash
#!/bin/bash

echo "=== HiveMind VPN Status ==="
echo ""

# Nodepool 狀態
echo "Nodepool Status:"
docker exec hivemind-nodepool curl -s http://localhost:8080/health | jq .
echo ""

# Worker 狀態
echo "Worker 1 Status:"
docker exec hivemind-worker1 curl -s http://localhost:50052/health | jq .
echo ""

echo "Worker 2 Status:"
docker exec hivemind-worker2 curl -s http://localhost:50052/health | jq .
echo ""

# VPN 連接狀態
echo "VPN Connections:"
docker exec hivemind-nodepool headscale nodes list
```

運行監控：

```bash
chmod +x monitor_vpn.sh
./monitor_vpn.sh
```

## 進階測試

### 測試 3 個 Worker

```bash
# 啟動第 3 個 Worker
docker-compose -f docker-compose.vpn.yml --profile extra-workers up -d

# 驗證 3 個 Worker 都已連接
docker-compose -f docker-compose.vpn.yml logs worker3 | grep "VPN connected"
```

### 測試 Worker 故障恢復

```bash
# 停止 Worker 2
docker-compose -f docker-compose.vpn.yml stop worker2

# 等待 10 秒
sleep 10

# 重新啟動 Worker 2
docker-compose -f docker-compose.vpn.yml start worker2

# 檢查是否自動重新連接
docker-compose -f docker-compose.vpn.yml logs worker2 | tail -20
```

### 測試網路隔離

```bash
# 進入 Worker 1
docker exec -it hivemind-worker1 sh

# 嘗試 ping Worker 2 的 Docker IP（應該失敗或很慢）
ping -c 3 172.20.0.5

# 嘗試 ping Worker 2 的 VPN IP（應該成功且快速）
ping -c 3 100.64.0.2

# 測試延遲
ping -c 10 100.64.0.2 | tail -1
```

## 性能測試

### 測試 VPN 吞吐量

```bash
# 在 Worker 1 上啟動 iperf3 伺服器
docker exec -d hivemind-worker1 iperf3 -s -B 100.64.0.1

# 在 Worker 2 上運行客戶端測試
docker exec hivemind-worker2 iperf3 -c 100.64.0.1 -t 10
```

預期結果：
```
[ ID] Interval           Transfer     Bitrate
[  5]   0.00-10.00  sec  1.10 GBytes   945 Mbits/sec
```

### 測試並發任務

```bash
# 運行整合測試腳本
cd ../scripts
chmod +x test_vpn_integration.sh
./test_vpn_integration.sh
```

## 清理環境

### 停止所有服務

```bash
cd infra
docker-compose -f docker-compose.vpn.yml down
```

### 完全清理（包括資料）

```bash
docker-compose -f docker-compose.vpn.yml down -v
```

### 清理 Docker 映像

```bash
docker-compose -f docker-compose.vpn.yml down --rmi all -v
```

## 常見問題

### Q1: Worker 無法連接到 VPN

**檢查**:
```bash
# 檢查 TUN 設備
docker exec hivemind-worker1 ls -l /dev/net/tun

# 檢查容器權限
docker inspect hivemind-worker1 | grep -A 5 CapAdd
```

**解決方案**:
確保 Docker Compose 配置包含：
```yaml
cap_add:
  - NET_ADMIN
  - NET_RAW
devices:
  - /dev/net/tun:/dev/net/tun
```

### Q2: Worker 之間無法通訊

**檢查**:
```bash
# 檢查 Nodepool 日誌
docker-compose logs nodepool | grep -i error

# 檢查 Worker 註冊狀態
docker exec hivemind-nodepool headscale nodes list
```

**解決方案**:
```bash
# 重啟 Nodepool
docker-compose restart nodepool

# 重啟所有 Worker
docker-compose restart worker1 worker2
```

### Q3: 性能不佳

**檢查**:
```bash
# 檢查 CPU 使用率
docker stats

# 檢查網路延遲
docker exec hivemind-worker1 ping -c 10 100.64.0.2
```

**優化**:
- 增加容器資源限制
- 使用 DERP 中繼伺服器
- 檢查防火牆設定

### Q4: 資料庫連接錯誤

**檢查**:
```bash
# 檢查 PostgreSQL 狀態
docker-compose logs postgres

# 測試資料庫連接
docker exec hivemind-postgres psql -U headscale -d headscale -c "SELECT 1;"
```

**解決方案**:
```bash
# 重新初始化資料庫
docker-compose down -v
docker-compose up -d postgres
sleep 10
docker-compose up -d nodepool
```

## 下一步

- 閱讀 [VPN 部署指南](VPN_DEPLOYMENT_GUIDE.md) 了解生產環境配置
- 查看 [VPN 實現總結](VPN_IMPLEMENTATION_SUMMARY.md) 了解技術細節
- 探索 [開發者架構文件](developer-architecture.md) 了解系統設計

## 獲取幫助

- GitHub Issues: https://github.com/your-org/hivemind/issues
- 文件: https://docs.hivemind.dev
- 社群: https://discord.gg/hivemind

## 貢獻

歡迎提交 Pull Request 改進此快速開始指南！

## 授權

MIT License - 詳見 LICENSE 文件

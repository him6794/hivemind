# Worker VPN Integration

本文件說明 HiveMind Worker 的 VPN 功能整合。

## 概述

Worker 使用 tsnet (Tailscale 的嵌入式客戶端) 連接到 Nodepool 的 Headscale 服務器，實現 Worker 節點間的 P2P 加密通訊。

## 架構

```
Worker
├── VPN Manager (pkg/vpn/manager.go)
│   ├── 與 Nodepool VPN 服務通訊
│   ├── 管理 VPN 生命週期
│   └── 提供 Peer 資訊
│
├── Tsnet Client (pkg/vpn/tsnet_client.go)
│   ├── 初始化 tsnet 客戶端
│   ├── 連接到 Headscale
│   └── 提供 Listen/Dial 介面
│
├── Registration Handler (pkg/handlers/registration.go)
│   ├── Worker 註冊時自動加入 VPN
│   └── Worker 關閉時離開 VPN
│
└── Multinode Executor (pkg/executor/multinode_executor.go)
    ├── 支援跨 Worker 通訊
    └── 使用虛擬 IP 連接其他 Worker
```

## 配置

### 環境變數

```bash
# Nodepool 地址
NODEPOOL_ADDR=nodepool.example.com:50051

# Worker ID (必須唯一)
WORKER_ID=worker-001

# Worker gRPC 端口
WORKER_GRPC_PORT=:50052

# VPN 配置
VPN_ENABLED=true
VPN_STATE_DIR=/var/lib/hivemind/vpn
VPN_HOSTNAME=worker-001
```

### 配置文件

```go
cfg := &config.Config{
    NodepoolAddr:   "nodepool.example.com:50051",
    WorkerGRPCPort: ":50052",
    WorkerID:       "worker-001",
    VPN: config.VPNConfig{
        Enabled:  true,
        StateDir: "/var/lib/hivemind/vpn",
        Hostname: "worker-001",
    },
}
```

## 使用方式

### 1. 啟動 Worker

```go
package main

import (
    "log"
    "hivemind/services/worker/pkg/config"
    "hivemind/services/worker/pkg/server"
)

func main() {
    // 載入配置
    cfg := config.LoadConfig()
    
    // 創建並啟動 Worker 服務器
    srv, err := server.NewServer(cfg)
    if err != nil {
        log.Fatalf("Failed to create server: %v", err)
    }
    
    // 啟動服務器 (會自動註冊並加入 VPN)
    if err := srv.Start(); err != nil {
        log.Fatalf("Failed to start server: %v", err)
    }
}
```

### 2. 執行多節點任務

```go
package main

import (
    "context"
    "hivemind/services/worker/pkg/executor"
    "hivemind/services/worker/pkg/vpn"
)

func executeTask(vpnMgr *vpn.Manager) {
    // 創建多節點執行器
    localExec := &MyLocalExecutor{}
    multinodeExec := executor.NewMultinodeExecutor(vpnMgr, localExec)
    
    // 創建任務
    task := &executor.Task{
        ID:            "task-123",
        Type:          "distributed-compute",
        Payload:       []byte("task data"),
        RequiresPeers: true,
    }
    
    // 執行任務 (會自動協調 Peer Workers)
    result, err := multinodeExec.Execute(context.Background(), task)
    if err != nil {
        log.Printf("Task failed: %v", err)
        return
    }
    
    log.Printf("Task completed: %v", result)
}
```

### 3. 直接連接 Peer Worker

```go
func connectToPeer(vpnMgr *vpn.Manager) {
    // 獲取任務的 Peer 列表
    peers, err := vpnMgr.GetTaskPeers("task-123")
    if err != nil {
        log.Printf("Failed to get peers: %v", err)
        return
    }
    
    // 連接到第一個 Peer
    if len(peers) > 0 {
        peer := peers[0]
        tsnetClient := vpnMgr.GetTsnetClient()
        
        // 使用虛擬 IP 連接
        conn, err := tsnetClient.Dial("tcp", peer.VirtualIP+":50052")
        if err != nil {
            log.Printf("Failed to connect to peer: %v", err)
            return
        }
        defer conn.Close()
        
        // 使用連接進行通訊
        // ...
    }
}
```

## VPN 生命週期

### 啟動流程

1. Worker 啟動時載入配置
2. 創建 VPN Manager
3. 向 Nodepool 註冊並獲取 Auth Key
4. 初始化 tsnet 客戶端
5. 連接到 Headscale 服務器
6. 獲取虛擬 IP 地址
7. 開始心跳循環

### 關閉流程

1. 接收關閉信號
2. 停止心跳循環
3. 向 Nodepool 註銷
4. 關閉 tsnet 客戶端
5. 清理狀態文件

## API 參考

### VPN Manager

```go
type Manager struct {
    // ...
}

// 創建 VPN Manager
func NewManager(cfg *ManagerConfig) (*Manager, error)

// 啟動 VPN 連接
func (m *Manager) Start() error

// 停止 VPN 連接
func (m *Manager) Stop() error

// 獲取任務的 Peer 列表
func (m *Manager) GetTaskPeers(taskID string) ([]*PeerInfo, error)

// 設置當前任務 ID
func (m *Manager) SetTaskID(taskID string)

// 獲取 tsnet 客戶端
func (m *Manager) GetTsnetClient() *TsnetClient

// 檢查是否已註冊
func (m *Manager) IsRegistered() bool
```

### Tsnet Client

```go
type TsnetClient struct {
    // ...
}

// 創建 tsnet 客戶端
func NewTsnetClient(cfg *TsnetConfig) (*TsnetClient, error)

// 啟動客戶端
func (c *TsnetClient) Start() error

// 停止客戶端
func (c *TsnetClient) Stop() error

// 在 VPN 網路上監聽
func (c *TsnetClient) Listen(network, address string) (net.Listener, error)

// 連接到 VPN 網路上的 Peer
func (c *TsnetClient) Dial(network, address string) (net.Conn, error)

// 帶 Context 的連接
func (c *TsnetClient) DialContext(ctx context.Context, network, address string) (net.Conn, error)

// 獲取本地虛擬 IP
func (c *TsnetClient) GetLocalIP() string

// 檢查是否已連接
func (c *TsnetClient) IsConnected() bool

// 獲取 Peer 狀態
func (c *TsnetClient) GetPeerStatus() (map[string]string, error)
```

### Multinode Executor

```go
type MultinodeExecutor struct {
    // ...
}

// 創建多節點執行器
func NewMultinodeExecutor(vpnMgr *vpn.Manager, localExec Executor) *MultinodeExecutor

// 執行任務
func (e *MultinodeExecutor) Execute(ctx context.Context, task *Task) (*TaskResult, error)

// 連接到 Peer Worker
func (e *MultinodeExecutor) DialPeer(ctx context.Context, peerIP string, port int) (net.Conn, error)

// 獲取 Peer 列表
func (e *MultinodeExecutor) GetPeerList(taskID string) ([]*PeerInfo, error)
```

## 測試

### 運行單元測試

```bash
cd services/worker

# 運行所有測試
go test ./...

# 運行 VPN 相關測試
go test ./pkg/vpn/...

# 運行特定測試
go test -v ./pkg/vpn -run TestManager

# 運行測試並顯示覆蓋率
go test -cover ./pkg/vpn/...
```

### 集成測試

集成測試需要運行 Headscale 服務器：

```bash
# 跳過集成測試
go test -short ./...

# 運行集成測試 (需要 Headscale)
go test -v ./pkg/vpn -run TestTsnetClientStartStop
```

## 故障排除

### VPN 連接失敗

1. 檢查 Nodepool 地址是否正確
2. 確認 Headscale 服務器正在運行
3. 檢查網路連接和防火牆設置
4. 查看日誌: `grep "VPN" /var/log/hivemind/worker.log`

### Peer 連接失敗

1. 確認兩個 Worker 都已連接到 VPN
2. 檢查虛擬 IP 是否正確
3. 測試 Peer 連通性: `ping <peer-virtual-ip>`
4. 檢查 NAT 穿透狀態

### 心跳失敗

1. 檢查 Nodepool 服務是否正常
2. 確認網路連接穩定
3. 查看心跳間隔設置 (默認 30 秒)

## 性能優化

### 連接池

對於頻繁的 Peer 通訊，建議使用連接池：

```go
type PeerConnectionPool struct {
    conns map[string]net.Conn
    mu    sync.RWMutex
}

func (p *PeerConnectionPool) Get(peerIP string) (net.Conn, error) {
    p.mu.RLock()
    conn, ok := p.conns[peerIP]
    p.mu.RUnlock()
    
    if ok && conn != nil {
        return conn, nil
    }
    
    // 創建新連接
    // ...
}
```

### 批量操作

對於多個 Peer 的操作，使用並發處理：

```go
func broadcastToPeers(peers []*vpn.PeerInfo, data []byte) {
    var wg sync.WaitGroup
    for _, peer := range peers {
        wg.Add(1)
        go func(p *vpn.PeerInfo) {
            defer wg.Done()
            sendToPeer(p, data)
        }(peer)
    }
    wg.Wait()
}
```

## 安全考慮

1. **加密**: 所有 VPN 流量使用 WireGuard 加密
2. **認證**: 使用 Auth Key 進行節點認證
3. **隔離**: 每個任務的 Worker 組成獨立的虛擬網路
4. **臨時節點**: Worker 離線後自動清理 (Ephemeral=true)

## 依賴

- `tailscale.com/tsnet`: Tailscale 嵌入式客戶端
- `google.golang.org/grpc`: gRPC 通訊
- `google.golang.org/protobuf`: Protocol Buffers

## 相關文件

- [VPN_NAT_TRAVERSAL_IMPLEMENTATION_PLAN.md](../../VPN_NAT_TRAVERSAL_IMPLEMENTATION_PLAN.md) - 完整實作計畫
- [Headscale Documentation](https://headscale.net/)
- [Tailscale tsnet Guide](https://tailscale.com/kb/1244/tsnet/)

## 授權

本專案使用與 HiveMind 相同的授權。

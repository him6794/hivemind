# HiveMind VPN NAT 穿透實作計畫

## 執行摘要

本文件定義基於 Tailscale 開源技術的 NAT 穿透解決方案，用於 HiveMind Worker 節點間的直接通訊。採用 Headscale + tsnet 架構，由 Nodepool 作為協調中心，無需獨立帳戶系統。

**預計工作量**: 4-6 週（1 位 Go 後端工程師）  
**技術棧**: Headscale, WireGuard-go, tsnet, gRPC  
**授權**: 全部為寬鬆開源授權（BSD/MIT/Apache 2.0）

---

## 1. 技術架構設計

### 1.1 整體架構圖

```
┌─────────────────────────────────────────────────────────────┐
│                        Nodepool                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  Headscale Embedded Server                           │   │
│  │  - 節點註冊與認證                                      │   │
│  │  - 虛擬 IP 分配 (100.64.0.0/10)                       │   │
│  │  - Peer 配置同步                                       │   │
│  │  - DERP 中繼協調                                       │   │
│  └──────────────────────────────────────────────────────┘   │
│                          ↕ gRPC                              │
└─────────────────────────────────────────────────────────────┘
                           ↕
        ┌──────────────────┴──────────────────┐
        ↓                                      ↓
┌───────────────────┐                  ┌───────────────────┐
│   Worker Node A   │                  │   Worker Node B   │
│  ┌──────────────┐ │                  │  ┌──────────────┐ │
│  │ tsnet Client │ │ ←─ WireGuard ──→ │  │ tsnet Client │ │
│  │ VIP: 100.64  │ │    P2P Tunnel    │  │ VIP: 100.64  │ │
│  │      .0.1    │ │                  │  │      .0.2    │ │
│  └──────────────┘ │                  │  └──────────────┘ │
│         ↕         │                  │         ↕         │
│  ┌──────────────┐ │                  │  ┌──────────────┐ │
│  │Task Executor │ │                  │  │Task Executor │ │
│  └──────────────┘ │                  │  └──────────────┘ │
└───────────────────┘                  └───────────────────┘
```

### 1.2 核心組件

#### A. Nodepool 端 - Headscale 整合

**位置**: `services/nodepool/internal/vpn/`

```go
// headscale_manager.go
package vpn

import (
    "github.com/juanfont/headscale/hscontrol"
    "github.com/juanfont/headscale/hscontrol/types"
)

type HeadscaleManager struct {
    app    *hscontrol.Headscale
    config *Config
}

type Config struct {
    ServerURL      string // Nodepool 的公開 URL
    IPPrefix       string // 預設 "100.64.0.0/10"
    DERPMap        string // DERP 伺服器配置
    EphemeralNodes bool   // Worker 離線後自動清理
}

func NewHeadscaleManager(cfg *Config) (*HeadscaleManager, error) {
    // 初始化 Headscale 實例
    app, err := hscontrol.NewHeadscale(&hscontrol.Config{
        ServerURL:      cfg.ServerURL,
        IPPrefixes:     []netip.Prefix{netip.MustParsePrefix(cfg.IPPrefix)},
        EphemeralNodes: cfg.EphemeralNodes,
        // 禁用 OIDC/OAuth，使用內部認證
        DisableAuth: true,
    })
    if err != nil {
        return nil, err
    }
    
    return &HeadscaleManager{app: app, config: cfg}, nil
}

// 註冊 Worker 節點並分配虛擬 IP
func (h *HeadscaleManager) RegisterWorker(workerID, machineKey string) (*WorkerVPNInfo, error) {
    // 1. 創建或獲取用戶（使用 workerID 作為用戶名）
    user, err := h.app.GetUser(workerID)
    if err != nil {
        user, err = h.app.CreateUser(workerID)
        if err != nil {
            return nil, err
        }
    }
    
    // 2. 註冊機器
    machine, err := h.app.RegisterMachine(machineKey, user.Name)
    if err != nil {
        return nil, err
    }
    
    // 3. 返回虛擬 IP 和配置
    return &WorkerVPNInfo{
        VirtualIP:   machine.IPAddresses[0].String(),
        NodeKey:     machine.NodeKey,
        Peers:       h.getPeerList(machine),
        DERPMap:     h.config.DERPMap,
    }, nil
}

// 獲取 Worker 的 Peer 列表（用於多節點任務）
func (h *HeadscaleManager) GetTaskPeers(taskID string, workerIDs []string) ([]*PeerInfo, error) {
    var peers []*PeerInfo
    for _, wid := range workerIDs {
        machine, err := h.app.GetMachineByUser(wid)
        if err != nil {
            continue
        }
        peers = append(peers, &PeerInfo{
            WorkerID:  wid,
            VirtualIP: machine.IPAddresses[0].String(),
            NodeKey:   machine.NodeKey,
        })
    }
    return peers, nil
}
```

#### B. Worker 端 - tsnet 客戶端

**位置**: `services/worker/pkg/vpn/`

```go
// tsnet_client.go
package vpn

import (
    "tailscale.com/tsnet"
    "net"
)

type TsnetClient struct {
    server    *tsnet.Server
    virtualIP string
}

func NewTsnetClient(workerID, controlURL, authKey string) (*TsnetClient, error) {
    // 初始化 tsnet 伺服器（作為客戶端使用）
    srv := &tsnet.Server{
        Hostname:   workerID,
        ControlURL: controlURL, // Nodepool Headscale URL
        AuthKey:    authKey,    // 從 Nodepool 獲取的預授權密鑰
        Ephemeral:  true,       // Worker 離線後自動清理
        Logf:       log.Printf,
    }
    
    // 啟動並連接到 Nodepool
    if err := srv.Start(); err != nil {
        return nil, err
    }
    
    // 獲取分配的虛擬 IP
    status, err := srv.Up(context.Background())
    if err != nil {
        return nil, err
    }
    
    return &TsnetClient{
        server:    srv,
        virtualIP: status.TailscaleIPs[0].String(),
    }, nil
}

// 監聽虛擬網路上的 TCP 連接
func (c *TsnetClient) Listen(port int) (net.Listener, error) {
    return c.server.Listen("tcp", fmt.Sprintf(":%d", port))
}

// 連接到其他 Worker 的虛擬 IP
func (c *TsnetClient) Dial(peerVirtualIP string, port int) (net.Conn, error) {
    return c.server.Dial(context.Background(), "tcp", 
        fmt.Sprintf("%s:%d", peerVirtualIP, port))
}

// 獲取本機虛擬 IP
func (c *TsnetClient) GetVirtualIP() string {
    return c.virtualIP
}

// 關閉連接
func (c *TsnetClient) Close() error {
    return c.server.Close()
}
```

### 1.3 gRPC API 擴展

**位置**: `proto/vpn.proto`

```protobuf
syntax = "proto3";

package nodepool;

option go_package = "./pb";

// VPN 管理服務
service VPNService {
    // Worker 請求加入 VPN 網路
    rpc JoinVPN(JoinVPNRequest) returns (JoinVPNResponse);
    
    // 獲取任務相關的 Peer 列表
    rpc GetTaskPeers(GetTaskPeersRequest) returns (GetTaskPeersResponse);
    
    // Worker 離開 VPN 網路
    rpc LeaveVPN(LeaveVPNRequest) returns (LeaveVPNResponse);
}

message JoinVPNRequest {
    string worker_id = 1;
    string machine_key = 2;  // WireGuard 公鑰
    string token = 3;        // Worker 認證 token
}

message JoinVPNResponse {
    bool success = 1;
    string status_message = 2;
    string virtual_ip = 3;       // 分配的虛擬 IP
    string auth_key = 4;         // Headscale 預授權密鑰
    string control_url = 5;      // Headscale 控制伺服器 URL
    repeated PeerInfo peers = 6; // 初始 Peer 列表
}

message PeerInfo {
    string worker_id = 1;
    string virtual_ip = 2;
    string node_key = 3;
}

message GetTaskPeersRequest {
    string task_id = 1;
    string token = 2;
}

message GetTaskPeersResponse {
    bool success = 1;
    string status_message = 2;
    repeated PeerInfo peers = 3;
}

message LeaveVPNRequest {
    string worker_id = 1;
    string token = 2;
}

message LeaveVPNResponse {
    bool success = 1;
    string status_message = 2;
}
```

---

## 2. 實作階段規劃

### Phase 1: 基礎設施 (週 1-2)

#### 任務 1.1: Nodepool Headscale 整合
- [ ] 安裝 Headscale 依賴
  ```bash
  cd services/nodepool
  go get github.com/juanfont/headscale/hscontrol@latest
  ```
- [ ] 實作 `HeadscaleManager`
- [ ] 配置 Headscale 嵌入模式（無需獨立進程）
- [ ] 實作 VPN gRPC 服務端

#### 任務 1.2: Worker tsnet 整合
- [ ] 安裝 tsnet 依賴
  ```bash
  cd services/worker
  go get tailscale.com/tsnet@latest
  ```
- [ ] 實作 `TsnetClient`
- [ ] Worker 啟動時自動加入 VPN
- [ ] 實作 VPN gRPC 客戶端

#### 任務 1.3: Proto 定義與生成
- [ ] 創建 `proto/vpn.proto`
- [ ] 生成 Go 程式碼
  ```bash
  protoc --go_out=. --go-grpc_out=. proto/vpn.proto
  ```
- [ ] 更新 Nodepool 和 Worker 的 proto 引用

**交付物**:
- `services/nodepool/internal/vpn/headscale_manager.go`
- `services/worker/pkg/vpn/tsnet_client.go`
- `proto/vpn.proto` + 生成的 pb 檔案

---

### Phase 2: 核心功能實作 (週 2-3)

#### 任務 2.1: Worker 註冊流程
```go
// services/worker/pkg/handlers/vpn_registration.go
func (w *Worker) RegisterToVPN() error {
    // 1. 生成 WireGuard 密鑰對
    privateKey, publicKey := generateWireGuardKeys()
    
    // 2. 呼叫 Nodepool JoinVPN
    resp, err := w.vpnClient.JoinVPN(context.Background(), &pb.JoinVPNRequest{
        WorkerId:   w.config.WorkerID,
        MachineKey: publicKey,
        Token:      w.authToken,
    })
    if err != nil {
        return err
    }
    
    // 3. 初始化 tsnet 客戶端
    tsClient, err := vpn.NewTsnetClient(
        w.config.WorkerID,
        resp.ControlUrl,
        resp.AuthKey,
    )
    if err != nil {
        return err
    }
    
    w.vpnClient = tsClient
    w.virtualIP = resp.VirtualIp
    
    log.Printf("Worker joined VPN with virtual IP: %s", w.virtualIP)
    return nil
}
```

#### 任務 2.2: 多節點任務協調
```go
// services/nodepool/internal/orchestrator/multi_node_task.go
type MultiNodeTask struct {
    TaskID    string
    Workers   []string
    VPNPeers  []*vpn.PeerInfo
}

func (o *Orchestrator) AssignMultiNodeTask(task *MultiNodeTask) error {
    // 1. 獲取任務的 VPN Peer 列表
    peers, err := o.vpnManager.GetTaskPeers(task.TaskID, task.Workers)
    if err != nil {
        return err
    }
    
    // 2. 向每個 Worker 發送任務，附帶 Peer 資訊
    for _, workerID := range task.Workers {
        workerConn := o.getWorkerConnection(workerID)
        _, err := workerConn.ExecuteTask(context.Background(), &pb.ExecuteTaskRequest{
            TaskId:   task.TaskID,
            Torrent:  task.Torrent,
            VpnPeers: peers, // 新增欄位
        })
        if err != nil {
            return err
        }
    }
    
    return nil
}
```

#### 任務 2.3: Worker 間通訊測試
```go
// services/worker/pkg/executor/multi_node_executor.go
func (e *MultiNodeExecutor) RunDistributedTask(task *Task) error {
    // 1. 啟動本地服務監聽虛擬 IP
    listener, err := e.vpnClient.Listen(8080)
    if err != nil {
        return err
    }
    go e.handlePeerConnections(listener)
    
    // 2. 連接到其他 Worker
    for _, peer := range task.VPNPeers {
        if peer.WorkerId == e.workerID {
            continue // 跳過自己
        }
        
        conn, err := e.vpnClient.Dial(peer.VirtualIp, 8080)
        if err != nil {
            log.Printf("Failed to connect to peer %s: %v", peer.WorkerId, err)
            continue
        }
        defer conn.Close()
        
        // 3. 執行分散式任務邏輯
        e.coordinateWithPeer(conn, peer)
    }
    
    return nil
}
```

**交付物**:
- Worker VPN 註冊流程
- 多節點任務分配邏輯
- Worker 間 P2P 通訊測試

---

### Phase 3: DERP 中繼與優化 (週 4)

#### 任務 3.1: 內建 DERP 伺服器
```go
// services/nodepool/internal/vpn/derp_server.go
package vpn

import (
    "tailscale.com/derp"
    "tailscale.com/derp/derphttp"
)

type DERPServer struct {
    server *derp.Server
    config *DERPConfig
}

type DERPConfig struct {
    Hostname string
    Port     int
    STUNPort int
}

func NewDERPServer(cfg *DERPConfig) (*DERPServer, error) {
    // 初始化 DERP 伺服器（用於 NAT 穿透失敗時的中繼）
    srv := derp.NewServer(key.NewNode(), log.Printf)
    
    // 啟動 HTTP 伺服器
    go func() {
        err := derphttp.ServeHTTP(fmt.Sprintf(":%d", cfg.Port), srv)
        if err != nil {
            log.Fatalf("DERP server failed: %v", err)
        }
    }()
    
    return &DERPServer{server: srv, config: cfg}, nil
}

// 生成 DERP Map 配置
func (d *DERPServer) GetDERPMap() string {
    return fmt.Sprintf(`{
        "Regions": {
            "1": {
                "RegionID": 1,
                "RegionCode": "nodepool",
                "Nodes": [{
                    "Name": "nodepool-derp",
                    "RegionID": 1,
                    "HostName": "%s",
                    "DERPPort": %d,
                    "STUNPort": %d
                }]
            }
        }
    }`, d.config.Hostname, d.config.Port, d.config.STUNPort)
}
```

#### 任務 3.2: 連接品質監控
```go
// services/worker/pkg/vpn/connection_monitor.go
func (c *TsnetClient) MonitorConnectionQuality() *ConnectionStats {
    status, _ := c.server.Status()
    
    stats := &ConnectionStats{
        VirtualIP:     c.virtualIP,
        DirectPeers:   0,
        RelayedPeers:  0,
        TotalPeers:    len(status.Peer),
    }
    
    for _, peer := range status.Peer {
        if peer.Relay != "" {
            stats.RelayedPeers++
        } else {
            stats.DirectPeers++
        }
    }
    
    return stats
}
```

**交付物**:
- DERP 中繼伺服器
- 連接品質監控
- 效能測試報告

---

### Phase 4: 測試與文檔 (週 5-6)

#### 任務 4.1: 單元測試
```go
// services/nodepool/internal/vpn/headscale_manager_test.go
func TestRegisterWorker(t *testing.T) {
    manager := setupTestHeadscaleManager(t)
    
    info, err := manager.RegisterWorker("worker-1", "test-machine-key")
    assert.NoError(t, err)
    assert.NotEmpty(t, info.VirtualIP)
    assert.True(t, strings.HasPrefix(info.VirtualIP, "100.64."))
}

func TestGetTaskPeers(t *testing.T) {
    manager := setupTestHeadscaleManager(t)
    
    // 註冊多個 Worker
    manager.RegisterWorker("worker-1", "key-1")
    manager.RegisterWorker("worker-2", "key-2")
    
    // 獲取 Peer 列表
    peers, err := manager.GetTaskPeers("task-123", []string{"worker-1", "worker-2"})
    assert.NoError(t, err)
    assert.Len(t, peers, 2)
}
```

#### 任務 4.2: 整合測試
```bash
# test_vpn_integration.sh
#!/bin/bash

# 1. 啟動 Nodepool（含 Headscale）
cd services/nodepool
go run cmd/server/main.go &
NODEPOOL_PID=$!

# 2. 啟動兩個 Worker
cd ../worker
go run cmd/server/main.go --worker-id=worker-1 --port=50053 &
WORKER1_PID=$!

go run cmd/server/main.go --worker-id=worker-2 --port=50054 &
WORKER2_PID=$!

# 3. 等待 VPN 連接建立
sleep 10

# 4. 測試 Worker 間通訊
curl -X POST http://localhost:8082/api/tasks \
  -d '{"task_id":"test-multi-node","workers":["worker-1","worker-2"]}'

# 5. 驗證連接
docker exec worker-1 ping -c 3 100.64.0.2
docker exec worker-2 ping -c 3 100.64.0.1

# 6. 清理
kill $NODEPOOL_PID $WORKER1_PID $WORKER2_PID
```

#### 任務 4.3: 文檔撰寫
- [ ] 使用者指南：如何啟用 VPN 功能
- [ ] 開發者文檔：VPN API 參考
- [ ] 故障排除指南：常見 NAT 穿透問題
- [ ] 效能調優指南：DERP vs 直連

**交付物**:
- 完整測試套件（覆蓋率 > 80%）
- 整合測試腳本
- 使用者與開發者文檔

---

## 3. 配置範例

### 3.1 Nodepool 配置

```yaml
# services/nodepool/config.yaml
vpn:
  enabled: true
  server_url: "https://nodepool.example.com"
  ip_prefix: "100.64.0.0/10"
  ephemeral_nodes: true
  
  derp:
    enabled: true
    hostname: "nodepool.example.com"
    port: 3478
    stun_port: 3479
```

### 3.2 Worker 配置

```yaml
# services/worker/config.yaml
vpn:
  enabled: true
  auto_join: true  # 啟動時自動加入 VPN
  control_url: "https://nodepool.example.com"
```

---

## 4. 安全考量

### 4.1 認證機制
- ✅ Worker 使用現有的 JWT token 認證
- ✅ Headscale 預授權密鑰（ephemeral，單次使用）
- ✅ WireGuard 密鑰對（每個 Worker 獨立生成）

### 4.2 加密
- ✅ WireGuard 提供端到端加密（ChaCha20-Poly1305）
- ✅ DERP 中繼流量也經過加密
- ✅ 控制平面使用 TLS (gRPC over HTTPS)

### 4.3 隔離
- ✅ 每個任務可選擇性地建立獨立虛擬網路
- ✅ Worker 只能看到同任務的 Peer
- ✅ Nodepool 可強制斷開異常連接

---

## 5. 效能指標

### 5.1 預期效能
- **連接建立時間**: < 2 秒（直連）/ < 5 秒（DERP 中繼）
- **延遲**: 
  - 直連: 接近物理網路延遲
  - DERP 中繼: +20-50ms
- **吞吐量**: 
  - 直連: 接近網卡頻寬
  - DERP 中繼: ~100 Mbps（受 Nodepool 頻寬限制）

### 5.2 可擴展性
- **支援節點數**: 10,000+ Worker（Headscale 已驗證）
- **並發任務**: 1,000+ 多節點任務
- **DERP 中繼負載**: 建議 < 10% 流量走中繼

---

## 6. 故障排除

### 6.1 常見問題

#### Q: Worker 無法加入 VPN
```bash
# 檢查 Nodepool Headscale 狀態
curl http://nodepool:8080/health

# 檢查 Worker 日誌
journalctl -u hivemind-worker -f | grep VPN

# 驗證網路連通性
telnet nodepool.example.com 443
```

#### Q: Worker 間無法直連（全部走 DERP）
```bash
# 檢查 UDP 端口是否開放
nc -u -v nodepool.example.com 3479

# 檢查防火牆規則
iptables -L -n | grep 41641  # WireGuard 預設端口

# 強制使用直連（測試用）
export TS_DEBUG_FORCE_DIRECT_CONN=1
```

#### Q: DERP 中繼負載過高
- 增加 DERP 伺服器數量（多區域部署）
- 優化防火牆規則以提高直連成功率
- 使用 Tailscale 的公共 DERP 伺服器（可選）

---

## 7. 未來擴展

### 7.1 短期（3 個月內）
- [ ] 支援 IPv6
- [ ] 多區域 DERP 伺服器
- [ ] VPN 連接品質儀表板

### 7.2 中期（6 個月內）
- [ ] 子網路路由（Subnet Routing）
- [ ] Exit Node 支援（Worker 作為代理）
- [ ] MagicDNS（虛擬 DNS 解析）

### 7.3 長期（1 年內）
- [ ] 與 Kubernetes CNI 整合
- [ ] 支援 WASM Runtime 的網路隔離
- [ ] 零信任網路架構（Zero Trust）

---

## 8. 參考資源

### 8.1 官方文檔
- [Headscale Documentation](https://headscale.net/)
- [Tailscale tsnet Guide](https://tailscale.com/kb/1244/tsnet/)
- [WireGuard Protocol](https://www.wireguard.com/protocol/)

### 8.2 範例專案
- [Headscale Examples](https://github.com/juanfont/headscale/tree/main/docs/examples)
- [tsnet Examples](https://github.com/tailscale/tailscale/tree/main/tsnet)
- [DERP Server Setup](https://tailscale.com/kb/1118/custom-derp-servers/)

### 8.3 社群資源
- [Headscale Discord](https://discord.gg/headscale)
- [Tailscale Forum](https://forum.tailscale.com/)
- [WireGuard Mailing List](https://lists.zx2c4.com/mailman/listinfo/wireguard)

---

## 9. 結論

本實作計畫提供了完整的 VPN NAT 穿透解決方案，基於成熟的 Tailscale 開源技術。透過 Headscale + tsnet 的架構，HiveMind 可以實現：

✅ **零配置 P2P 連接** - Worker 自動建立加密隧道  
✅ **NAT 穿透** - 支援各種網路環境（家用路由器、企業防火牆）  
✅ **高效能** - 優先使用直連，降低延遲  
✅ **可擴展** - 支援數千個 Worker 節點  
✅ **安全** - WireGuard 加密 + JWT 認證  

預計 **4-6 週**即可完成核心功能，使 HiveMind 具備與 Golem Network 相當的多節點協作能力。

---

**文件版本**: 1.0  
**最後更新**: 2026-04-30  
**作者**: HiveMind VPN Team  
**審核狀態**: 待審核

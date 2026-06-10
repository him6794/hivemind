# HiveMind VPN 功能文件索引

本目錄包含 HiveMind VPN 功能的完整文件。

## 文件列表

### 快速開始
- **[VPN_QUICKSTART.md](./VPN_QUICKSTART.md)** - 5 分鐘快速體驗指南
  - 快速啟動步驟
  - 簡單的多節點任務範例
  - 常見問題解答

### 部署指南
- **[VPN_DEPLOYMENT_GUIDE.md](./VPN_DEPLOYMENT_GUIDE.md)** - 完整部署指南
  - 架構概述
  - 環境變數配置
  - 部署方式（Docker Compose、手動、Kubernetes）
  - 監控與日誌
  - 故障排除
  - 安全建議

### 實現細節
- **[VPN_IMPLEMENTATION_SUMMARY.md](./VPN_IMPLEMENTATION_SUMMARY.md)** - VPN 實現總結
  - 技術架構
  - 核心組件
  - API 設計

- **[VPN_PROTO_GENERATION.md](./VPN_PROTO_GENERATION.md)** - Protocol Buffers 生成指南

### 測試與驗證
- **[VPN_TESTING_DEPLOYMENT_SUMMARY.md](./VPN_TESTING_DEPLOYMENT_SUMMARY.md)** - 測試與部署總結
  - 已創建文件概述
  - 使用流程
  - 測試場景

## 相關文件

### 配置文件
- `../infra/docker-compose.vpn.yml` - Docker Compose 配置
- `../services/nodepool/Dockerfile` - Nodepool Docker 映像
- `../services/worker/Dockerfile` - Worker Docker 映像

### 測試腳本
- `../scripts/test_vpn_integration.sh` - VPN 整合測試腳本
- `../scripts/generate_vpn_proto.sh` - Protocol Buffers 生成腳本

### 程式碼範例
- `../services/worker/examples/vpn_demo.go` - VPN 功能範例程式

## 快速導航

### 我想...

#### 快速體驗 VPN 功能
→ 閱讀 [VPN_QUICKSTART.md](./VPN_QUICKSTART.md)

#### 部署到生產環境
→ 閱讀 [VPN_DEPLOYMENT_GUIDE.md](./VPN_DEPLOYMENT_GUIDE.md)

#### 了解技術實現
→ 閱讀 [VPN_IMPLEMENTATION_SUMMARY.md](./VPN_IMPLEMENTATION_SUMMARY.md)

#### 運行整合測試
→ 執行 `../scripts/test_vpn_integration.sh`

#### 配置環境變數
→ 參考 [VPN_DEPLOYMENT_GUIDE.md](./VPN_DEPLOYMENT_GUIDE.md#環境變數配置)

#### 解決問題
→ 參考 [VPN_DEPLOYMENT_GUIDE.md](./VPN_DEPLOYMENT_GUIDE.md#故障排除)

## 架構概覽

```
┌─────────────────────────────────────────────────────────┐
│                      Nodepool                           │
│  ┌──────────────────────────────────────────────────┐  │
│  │         Embedded Headscale Server                │  │
│  │  - 節點註冊與認證                                  │  │
│  │  - IP 地址分配 (100.64.0.0/10)                    │  │
│  │  - DERP 中繼協調                                   │  │
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

## 核心功能

- ✅ **自動 VPN 網路建立**: Worker 自動加入 VPN mesh 網路
- ✅ **P2P 加密通訊**: Worker 間直接加密連接
- ✅ **動態節點管理**: 自動註冊、心跳、清理
- ✅ **任務 Peer 發現**: 自動發現同任務的 Worker 節點
- ✅ **多節點任務執行**: 支援分散式任務協調
- ✅ **故障恢復**: Worker 重啟自動重連
- ✅ **DERP 中繼**: NAT 穿透和中繼支援

## 環境需求

- **作業系統**: Linux (Ubuntu 20.04+, Debian 11+, CentOS 8+)
- **Docker**: 20.10+ 或 Docker Compose v2
- **記憶體**: 最少 2GB (Nodepool), 1GB per Worker
- **網路**: 開放 TCP 50051, 8080, 50443

## 快速開始

```bash
# 1. 啟動 VPN 環境
cd hivemind/infra
docker-compose -f docker-compose.vpn.yml up -d

# 2. 查看狀態
docker-compose -f docker-compose.vpn.yml logs -f

# 3. 驗證連接
docker exec hivemind-worker1 ping -c 3 100.64.0.2

# 4. 運行測試
cd ../scripts
./test_vpn_integration.sh
```

## 支援

如有問題，請參考：
1. [故障排除指南](./VPN_DEPLOYMENT_GUIDE.md#故障排除)
2. [常見問題](./VPN_QUICKSTART.md#常見問題)
3. 提交 GitHub Issue

## 更新日誌

- **2026-04-30**: 創建完整的 VPN 測試和部署文件
  - 整合測試腳本
  - Docker Compose 配置
  - 部署指南
  - 快速開始指南
  - Dockerfile 配置

## 貢獻

歡迎提交改進建議和 Pull Request！

## 授權

請參考專案根目錄的 LICENSE 文件。

# HiveMind 分布式運算平台

[![Project Status](https://img.shields.io/badge/status-active-brightgreen.svg)](https://github.com/him6794/hivemind)
[![License](https://img.shields.io/badge/license-GPL%20v3-blue.svg)](LICENSE.txt)
[![Python Version](https://img.shields.io/badge/python-3.8%2B-blue.svg)](https://www.python.org/)
[![Docker](https://img.shields.io/badge/docker-required-blue.svg)](https://www.docker.com/)

HiveMind 是一個開源的分布式運算平台，旨在構建一個去中心化的計算網絡，讓用戶可以共享閒置的計算資源並獲得代幣獎勵。

## 系統架構

HiveMind 採用三層分布式架構，由以下核心組件構成：

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Master    │    │ Node Pool   │    │   Worker    │
│   主控節點    │◄──►│   節點池     │◄──►│   工作節點   │
│             │    │             │    │             │
│ 任務管理     │    │ 資源調度     │    │ 任務執行     │
│ 用戶界面     │    │ 負載均衡     │    │ 狀態監控     │
│ VPN 管理    │    │ 獎勵分發     │    │ 結果回傳     │
└─────────────┘    └─────────────┘    └─────────────┘
```

### 核心功能模組

#### Node Pool (節點池)
- **資源調度**：智能分配計算任務到最適合的工作節點
- **任務分發**：維護任務佇列，處理任務優先級和依賴關係
- **獎勵系統**：基於貢獻度計算並發放 CPT 代幣獎勵
- **節點管理**：監控節點狀態，維護節點信任等級

#### Worker (工作節點)
- **任務執行**：在 Docker 容器中安全執行計算任務
- **資源監控**：實時監控 CPU、記憶體、GPU 使用情況
- **狀態回報**：定期向節點池報告節點健康狀態
- **結果傳輸**：安全地將任務結果回傳給節點池

#### Master (主控節點)
- **任務管理**：提供任務創建、監控和管理界面
- **用戶認證**：處理用戶註冊、登入和權限管理
- **VPN 服務**：管理節點間的安全通訊隧道
- **監控面板**：提供系統狀態和任務執行的可視化界面

### 輔助模組

#### 🤖 AI (人工智慧)
- **模型分割**：將大型 AI 模型拆分為可分布式執行的小任務
- **智能調度**：基於模型特徵優化任務分配策略
- **狀態**：🚧 開發中

#### 📁 BT (P2P 傳輸)
- **大檔案傳輸**：支援大型任務文件的點對點傳輸
- **種子管理**：創建和管理 BitTorrent 種子文件
- **狀態**：已完成，待整合

#### Web (官方網站)
- **項目展示**：項目介紹和功能說明
- **用戶註冊**：在線用戶註冊和管理
- **狀態監控**：實時系統狀態展示
- **訪問地址**：https://hivemind.justin0711.com

## 🔌 通訊協議與資料格式

HiveMind 使用 **gRPC** 作為節點間通訊協議，採用 **Protocol Buffers** 定義資料格式，確保高效能和跨平台相容性。

### 主要 API 接口

#### 用戶認證服務
```protobuf
// 用戶登入
message LoginRequest {
  string username = 1;    // 用戶名
  string password = 2;    // 密碼
}

message LoginResponse {
  bool success = 1;       // 登入是否成功
  string token = 2;       // JWT 身份驗證令牌
  string message = 3;     // 回應訊息
}

// 用戶註冊
message RegisterRequest {
  string username = 1;    // 用戶名
  string password = 2;    // 密碼
  string email = 3;       // 電子郵件地址
}

message RegisterResponse {
  bool success = 1;       // 註冊是否成功
  string message = 2;     // 回應訊息
}
```

#### 代幣管理服務
```protobuf
// 代幣轉帳
message TransferRequest {
  string token = 1;           // 用戶身份驗證令牌
  string recipient = 2;       // 接收者用戶名
  double amount = 3;          // 轉帳金額
}

message TransferResponse {
  bool success = 1;           // 轉帳是否成功
  string message = 2;         // 回應訊息
  double new_balance = 3;     // 轉帳後餘額
}

// 餘額查詢
message GetBalanceRequest {
  string token = 1;           // 用戶身份驗證令牌
  string username = 2;        // 查詢的用戶名
}

message GetBalanceResponse {
  bool success = 1;           // 查詢是否成功
  double balance = 2;         // 用戶餘額
  string message = 3;         // 回應訊息
}
```

#### 節點管理服務
```protobuf
// 工作節點註冊
message RegisterWorkerNodeRequest {
  string node_id = 1;         // 節點唯一識別符
  string hostname = 2;        // 主機名稱
  int32 cpu_cores = 3;        // CPU 核心數
  double memory_gb = 4;       // 記憶體容量 (GB)
  double cpu_score = 5;       // CPU 性能評分
  double gpu_score = 6;       // GPU 性能評分
  double gpu_memory_gb = 7;   // GPU 記憶體容量 (GB)
  string location = 8;        // 地理位置
  int32 port = 9;            // 通訊埠號
  string gpu_name = 10;       // GPU 型號名稱
  bool docker_status = 11;    // Docker 服務狀態
}

// 節點狀態回報
message StatusResponse {
  bool success = 1;           // 操作是否成功
  string message = 2;         // 狀態訊息
  string status = 3;          // 節點狀態
}

// 心跳檢測
message HealthCheckRequest {}   // 無需參數，持續發送以保持連線

message HealthCheckResponse {
  bool healthy = 1;           // 節點健康狀態
  int64 timestamp = 2;        // 時間戳
  string message = 3;         // 健康狀態訊息
}
```

## 快速開始

### 系統需求

- **作業系統**：Windows 10/11, Ubuntu 18.04+, macOS 10.15+
- **Python**：3.8 或更高版本
- **Docker**：20.10 或更高版本
- **記憶體**：最少 4GB RAM
- **網路**：穩定的網際網路連線

### 環境準備

1. **安裝 Python 依賴**
```bash
pip install -r requirements.txt
```

2. **安裝 Docker**
```bash
# Ubuntu/Debian
sudo apt-get install docker.io docker-compose

# Windows/macOS
# 請下載並安裝 Docker Desktop
```

3. **安裝 Redis (用於節點池)**
```bash
# Ubuntu/Debian
sudo apt-get install redis-server

# Windows (使用 Docker)
docker run -d --name redis -p 6379:6379 redis:latest

# macOS (使用 Homebrew)
brew install redis
```

### 啟動服務

#### 1. 啟動節點池 (Node Pool)
```bash
cd node_pool
python node_pool_server.py
```

#### 2. 啟動工作節點 (Worker)
```bash
cd worker
python worker_node.py
```

#### 3. 啟動主控節點 (Master)
```bash
cd master
python master_node.py
```

### 訪問 Web 界面

- **主控面板**：http://localhost:5001

## 詳細文檔

### 模組文檔
- [Node Pool 詳細說明](node_pool/README.md) - 資源調度和任務分發
- [Worker 節點配置](worker/README.md) - 工作節點部署和管理
- [Master 節點設置](master/README.md) - 主控節點功能和配置

### 開發文檔
- [API 接口文檔](docs/API.md) - gRPC 接口詳細說明
- [部署指南](docs/DEPLOYMENT.md) - 生產環境部署
- [故障排除](docs/TROUBLESHOOTING.md) - 常見問題解決方案

## 貢獻指南

歡迎參與 HiveMind 的開發！請遵循以下步驟：

1. **Fork** 本專案
2. 創建功能分支：`git checkout -b feature/AmazingFeature`
3. 提交變更：`git commit -m 'Add some AmazingFeature'`
4. 推送到分支：`git push origin feature/AmazingFeature`
5. 開啟 **Pull Request**

### 開發規範
- 遵循 PEP 8 編碼規範
- 添加適當的測試案例
- 更新相關文檔
- 提交訊息使用英文並遵循 [Conventional Commits](https://www.conventionalcommits.org/)

## 問題回報

如果您發現任何問題，請在 [GitHub Issues](https://github.com/him6794/hivemind/issues) 中提交問題報告。

提交問題時請包含：
- 詳細的問題描述
- 重現步驟
- 系統環境資訊
- 相關的錯誤日誌

## 📜 授權條款

本專案採用 **GNU General Public License v3.0** 授權條款。
詳細內容請參閱 [LICENSE.txt](LICENSE.txt) 檔案。

## 聯絡資訊

- **專案維護者**：Justin
- **電子郵件**：[justin0711@example.com]
- **專案首頁**：https://hivemind.justin0711.com
- **GitHub**：https://github.com/him6794/hivemind

## 🙏 致謝

感謝所有為 HiveMind 專案做出貢獻的開發者和社群成員！

---

> **注意**：本專案仍在積極開發中，部分功能可能尚未完全穩定。歡迎提供反饋和建議！


# 模組說明

本目錄包含 HiveMind 各個模組的詳細技術文檔。

## 模組列表

###  核心服務模組

- **[Node Pool](node-pool.md)** - 節點池服務
  - 多層級信任系統管理
  - 智慧任務調度和分配
  - 動態資源追蹤和分配
  - JWT 用戶認證系統
  - gRPC 高性能通訊

- **[Master Node](master-node.md)** - 主控節點
  - Web 管理界面
  - VPN 網路管理
  - 系統監控和報告
  - 任務協調和分發

- **[Worker Node](worker-node.md)** - 工作節點
  - 多任務並行執行引擎
  - Docker 容器化安全執行
  - VPN 自動連接管理
  - 信任評分系統
  - 現代化 Web 管理介面
  - 即時資源監控和健康檢查

### 📦 任務執行框架

- **[TaskWorker](taskworker.md)** - 任務執行庫
  - 輕量級任務執行框架
  - gRPC 服務介面
  - 獨立部署能力
  - 與 Node Pool 可選整合

### 🌐 Web 服務模組

- **[Web Module](web.md)** - Web 服務界面
  - Flask Web 應用程式
  - VPN 服務管理
  - WireGuard 伺服器整合
  - 用戶介面和管理工具

### 🤖 人工智慧模組

- **[AI Module](ai.md)** - 機器學習服務
  - 模型識別和分析
  - 強化學習演算法
  - 分散式訓練支援
  - Q-learning 任務調度優化

### 🌐 檔案傳輸模組

- **[BT Module](bt.md)** - BitTorrent 點對點傳輸
  - 種子檔案創建和管理
  - P2P 網路整合
  - 分散式檔案共享
  - 追蹤器和播種器服務

## 模組依賴關係

```
Node Pool (核心調度服務)
├── Master Node (依賴 Node Pool API)
├── Worker Node (依賴 Node Pool 註冊)
└── TaskWorker (獨立，可選整合)

AI Module (依賴 Node Pool 資源管理)
BT Module (獨立 P2P 服務)
Web Module (獨立 Web 服務，可選整合)
```

## 開發狀態

| 模組 | 狀態 | 完成度 | 關鍵特性 |
|------|------|--------|----------|
| Node Pool | ✅ 生產就緒 | 100% | 多層信任、動態資源、gRPC |
| Master Node | ✅ 生產就緒 | 100% | VPN 管理、Web 介面 |
| Worker Node | ✅ 生產就緒 | 100% | 多任務、Docker、監控 |
| TaskWorker | ✅ 生產就緒 | 100% | 輕量級、獨立部署 |
| AI Module | 🔄 開發中 | 30% | Q-learning、模型分析 |
| BT Module | ✅ Beta 版本 | 85% | P2P 傳輸、種子管理 |
| Web Module | ✅ 生產就緒 | 90% | Flask 應用、VPN 服務 |

## 架構概覽

### 信任等級系統 (Node Pool 核心)
```
高信任節點 (信用 ≥ 100, Docker 啟用)
├── 優先任務分配
├── 完整功能存取
└── 最高獎勵係數

中信任節點 (信用 50-99, Docker 啟用)
├── 標準任務分配
├── 一般功能存取
└── 標準獎勵係數

低信任節點 (信用 < 50 或無 Docker)
├── 限制任務類型
├── 基本功能存取
└── 基本獎勵係數
```

### 資源管理系統
- **動態分配**: 實時追蹤總資源和可用資源
- **多任務支援**: 單節點並行執行多個任務
- **負載均衡**: 智慧分配演算法
- **地理感知**: 支援按地區優先分配

## 快速導航

### 按開發角色

**系統管理員**：
- [部署指南](../deployment.md)
- [故障排除](../troubleshooting.md)
- [Node Pool 配置](node-pool.md#部署和配置)

**開發者**：
- [開發指南](../developer.md)
- [TaskWorker API](taskworker.md)
- [gRPC API 文檔](../api.md)

**用戶**：
- [快速開始](../README.md#快速開始)
- [Master Node 使用](master-node.md)
- [Web 介面](web.md)

### 按功能類別

**核心架構**：
- [Node Pool](node-pool.md) - 中央調度服務
- [Master Node](master-node.md) - 系統控制台
- [Worker Node](worker-node.md) - 計算執行單元

**擴展功能**：
- [AI Module](ai.md) - 智慧優化功能
- [BT Module](bt.md) - P2P 檔案分享
- [Web Module](web.md) - Web 管理介面

**開發工具**：
- [TaskWorker](taskworker.md) - 任務執行庫
- [API 文檔](../api.md) - 完整介面規範

### 按技術堆疊

**Python + gRPC**：
- Node Pool, Worker Node, TaskWorker

**Python + Flask**：
- Master Node, Web Module

**Python + ML/AI**：
- AI Module

**Python + P2P**：
- BT Module

---

**最後更新**: 2025年9月5日  
**維護狀態**: 所有模組文檔與實際代碼同步  
**文檔版本**: v2.0 (基於實際代碼分析)

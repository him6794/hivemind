# 模組說明

本目錄包含 HiveMind 各個模組的詳細技術文檔。

## 模組列表

### 📦 核心模組

- **[TaskWorker](taskworker.md)** - 任務執行庫
  - 輕量級任務執行框架
  - gRPC 服務介面
  - 獨立部署能力

### 🔄 核心服務

- **[Node Pool](node-pool.md)** - 節點池服務
  - 節點管理和註冊
  - 任務調度和分配
  - 用戶認證和管理

- **[Master Node](master-node.md)** - 主控節點
  - Web 管理界面
  - VPN 網路管理
  - 系統監控和報告

- **[Worker Node](worker-node.md)** - 工作節點
  - 任務執行引擎
  - 資源監控
  - 狀態報告

### 🤖 人工智慧模組

- **[AI Module](ai.md)** - 機器學習服務
  - 模型識別和分析
  - 強化學習演算法
  - 分散式訓練支援

### 🌐 檔案傳輸模組

- **[BT Module](bt.md)** - BitTorrent 點對點傳輸
  - 種子檔案創建和管理
  - P2P 網路整合
  - 分散式檔案共享

### 🧠 AI 模組

- **[AI Module](ai.md)** - 人工智慧模組
  - 模型分割和分布
  - 智能任務調度
  - 身份識別系統
  - 狀態：開發中

### 📂 檔案傳輸

- **[BT Module](bt.md)** - BitTorrent 模組
  - P2P 檔案傳輸
  - 種子管理和追蹤
  - 分散式存儲
  - 狀態：已完成

### 🌐 Web 服務

- **[Web Module](web.md)** - Web 服務模組
  - Flask Web 應用
  - VPN 服務管理
  - WireGuard 服務器
  - 用戶界面

## 模組依賴關係

```
Node Pool (核心)
├── Master Node (依賴 Node Pool)
├── Worker Node (依賴 Node Pool)
└── TaskWorker (獨立，可選整合)

AI Module (依賴 Node Pool)
BT Module (獨立)
Web Module (獨立，可選整合)
```

## 開發狀態

| 模組 | 狀態 | 完成度 | 說明 |
|------|------|--------|------|
| Node Pool | ✅ 完成 | 100% | 核心功能完整 |
| Master Node | ✅ 完成 | 100% | 管理功能完整 |
| Worker Node | ✅ 完成 | 100% | 執行功能完整 |
| TaskWorker | ✅ 完成 | 100% | 獨立庫完整 |
| AI Module | 🔄 開發中 | 30% | 基礎功能實現 |
| BT Module | ✅ 完成 | 100% | P2P 功能完整 |
| Web Module | ✅ 完成 | 90% | 基本功能完整 |

## 快速導航

### 按開發角色

**系統管理員**：
- [部署指南](../deployment.md)
- [故障排除](../troubleshooting.md)
- [Node Pool](node-pool.md)

**開發者**：
- [開發指南](../developer.md)
- [TaskWorker](taskworker.md)
- [API 文檔](../api.md)

**用戶**：
- [快速開始](../README.md#快速開始)
- [Master Node](master-node.md)
- [Web Module](web.md)

### 按功能類別

**核心架構**：
- [Node Pool](node-pool.md) - 節點管理
- [Master Node](master-node.md) - 系統控制
- [Worker Node](worker-node.md) - 任務執行

**擴展功能**：
- [AI Module](ai.md) - 智能功能
- [BT Module](bt.md) - 檔案傳輸
- [Web Module](web.md) - Web 服務

**開發工具**：
- [TaskWorker](taskworker.md) - 任務庫
- [API 文檔](../api.md) - 介面規範

---

**更新日期**: 2024年1月  
**維護狀態**: 所有模組文檔與實際代碼同步

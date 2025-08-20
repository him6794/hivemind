# HiveMind 分布式運算平台文檔

歡迎來到 HiveMind 分布式運算平台的完整文檔！

## 快速導航

### 📚 核心文檔
- [項目概述](#項目概述)
- [系統架構](#系統架構)
- [快速開始](deployment.md#快速開始)
- [API 參考](api.md)

### 🔧 部署運維
- [部署指南](deployment.md)
- [故障排除](troubleshooting.md)
- [性能調優](deployment.md#性能優化)

### 👨‍💻 開發文檔
- [開發者指南](developer.md)
- [貢獻指南](developer.md#貢獻指南)
- [編碼規範](developer.md#編碼規範)

### 📖 模組文檔
- [Node Pool 模組](modules/node-pool.md)
- [Master 節點模組](modules/master.md)
- [Worker 節點模組](modules/worker.md)
- [AI 模組](modules/ai.md)
- [BT 模組](modules/bt.md)
- [TaskWorker 模組](modules/taskworker.md)
- [Web 介面模組](modules/web.md)

## 項目概述

HiveMind 是一個開源的分布式運算平台，旨在建立去中心化的運算網路，允許用戶分享閒置的運算資源並賺取代幣獎勵。

### 核心特性

| 功能 | 描述 | 狀態 |
|------|------|------|
| 🌐 **節點池管理** | 分布式節點註冊和管理 | ✅ 已實現 |
| 🔄 **主從架構** | 階層式任務分配系統 | ✅ 已實現 |
| 💾 **持久化存儲** | SQLite 數據庫和 Redis 快取 | ✅ 已實現 |
| 🌍 **Web 儀表板** | 任務監控和系統狀態 | 🚧 開發中 |
| 🔒 **用戶認證** | 安全的用戶和權限管理 | ✅ 已實現 |
| 📡 **gRPC 通訊** | 高性能服務間通訊 | ✅ 已實現 |
| 🔧 **任務工作系統** | 分布式任務執行框架 | ✅ 已實現 |
| 📦 **BitTorrent 協議** | P2P 文件分享和分發 | 🚧 開發中 |

## 系統架構

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   🎛️ Master     │    │   🌐 Node Pool   │    │   ⚙️ Worker     │
│    Node         │◄──►│    Service      │◄──►│    Node         │
│                 │    │                 │    │                 │
│ • 任務管理       │    │ • 資源調度       │    │ • 任務執行       │
│ • 用戶介面       │    │ • 節點管理       │    │ • 狀態監控       │
│ • VPN 管理       │    │ • 獎勵分發       │    │ • 結果回報       │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### 核心組件

| 組件 | 用途 | 實現狀態 |
|------|------|----------|
| **🌐 Node Pool** | 資源調度和任務分配 | ✅ 完成 |
| **⚙️ Worker Node** | 任務執行和資源監控 | ✅ 完成 |
| **🎛️ Master Node** | 任務管理和用戶介面 | ✅ 完成 |
| **🤖 AI Module** | 分布式 AI 模型訓練 | 🚧 開發中 |
| **📦 BT Module** | P2P 文件傳輸系統 | 🚧 開發中 |

## 快速開始

### 系統需求

- **作業系統**: Linux (Ubuntu 18.04+), Windows 10+, macOS 10.15+
- **Python**: 3.8 或更高版本
- **Docker**: 20.10 或更高版本
- **記憶體**: 最低 2GB，建議 4GB+

### 安裝步驟

```bash
# 克隆專案
git clone https://github.com/him6794/hivemind.git
cd hivemind

# 安裝依賴
pip install -r requirements.txt

# 啟動節點池服務
cd node_pool
python node_pool_server.py
```

詳細的安裝和部署指南請參考 [部署指南](deployment.md)。

## 獎勵系統

通過貢獻運算資源賺取 **CPT (Computing Power Token)**：

| 獎勵類型 | 費率 | 描述 |
|----------|------|------|
| **🏆 基礎獎勵** | 10 CPT/小時 | 標準參與費率 |
| **⚡ 性能獎勵** | +50% 最高 | 高效任務完成 |
| **🔄 穩定性獎勵** | +30% | 穩定在線維護 |
| **💎 品質獎勵** | 可變 | 複雜任務執行 |

**注意**: 代幣獎勵系統目前正在開發中。

## 開發狀態

### ✅ 當前實現 (2025年8月)

| 組件 | 狀態 | 描述 |
|------|------|------|
| **🏗️ 核心架構** | ✅ 完成 | Node Pool, Master, Worker 模組 |
| **🔧 基本功能** | ✅ 完成 | 節點註冊、任務分配、用戶管理 |
| **📡 通訊** | ✅ 完成 | 基於 gRPC 的節點間通訊 |
| **💾 數據存儲** | ✅ 完成 | SQLite 用戶數據，Redis 節點狀態 |

### 🎯 未來開發目標

| 目標 | 優先級 | 描述 |
|------|--------|------|
| **🧪 測試框架** | 高 | 全面的測試套件實現 |
| **🤖 AI 整合** | 中 | 分布式 AI 模型訓練能力 |
| **🌍 Web 儀表板** | 中 | 增強的監控和管理介面 |
| **🐳 Docker Compose** | 低 | 簡化的部署配置 |

## 社群與支援

### 🌟 加入我們的社群

| 平台 | 用途 | 連結 |
|------|------|------|
| **🔗 GitHub** | 主要倉庫 | [Repository](https://github.com/him6794/hivemind) |
| **🐛 Issues** | 錯誤報告與功能請求 | [GitHub Issues](https://github.com/him6794/hivemind/issues) |
| **💬 Discussions** | 社群問答 | [GitHub Discussions](https://github.com/him6794/hivemind/discussions) |

**注意**: Discord 和 Telegram 社群規劃在未來開發中。

### 🆘 支援管道

| 類型 | 平台 | 狀態 |
|------|------|------|
| **錯誤報告** | [GitHub Issues](https://github.com/him6794/hivemind/issues) | ✅ 活躍 |
| **功能請求** | [GitHub Discussions](https://github.com/him6794/hivemind/discussions) | ✅ 活躍 |
| **聯繫方式** | GitHub Profile | ✅ 可用 |

**注意**: 專門的支援電子郵件地址規劃在未來開發中。

## 授權

本專案採用 **GNU General Public License v3.0** 授權 - 詳見 [LICENSE](../LICENSE.txt) 文件。

---

<div align="center">

**加入 HiveMind 分布式運算網路**

*將您的閒置運算能力轉化為價值，幫助建立更強大的運算生態系統*

[![GitHub Stars](https://img.shields.io/github/stars/him6794/hivemind?style=social)](https://github.com/him6794/hivemind)
[![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](../LICENSE.txt)

**[GitHub Repository](https://github.com/him6794/hivemind) | [問題與討論](https://github.com/him6794/hivemind/issues)**

</div>

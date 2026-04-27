# Hivemind 文檔索引

## 🎯 快速開始
- **[QUICK_REFERENCE.md](QUICK_REFERENCE.md)** - 快速參考卡，常用命令和端口
- **[DEPLOYMENT_GUIDE.md](DEPLOYMENT_GUIDE.md)** - 完整部署指南，一鍵啟動腳本

## 📊 系統狀態
- **[STATUS_REPORT.md](STATUS_REPORT.md)** - 當前系統狀態、功能清單、測試覆蓋

## 📋 最近更新
- **[docs/UPDATE_SUMMARY_2026_03_25.md](docs/UPDATE_SUMMARY_2026_03_25.md)** - 詳細的本週更新說明

## 📐 架構和設計
- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** - 系統架構概覽
- **[docs/REDIS_MIGRATION_PLAN.md](docs/REDIS_MIGRATION_PLAN.md)** - Redis 遷移計劃
- **[docs/UI_SEPARATION_PLAN.md](docs/UI_SEPARATION_PLAN.md)** - UI 分離架構

## 📈 開發進度
- **[docs/DEVELOPMENT_PROGRESS.md](docs/DEVELOPMENT_PROGRESS.md)** - 總體開發進度
- **[docs/BACKEND_SERVICE_PROGRESS.md](docs/BACKEND_SERVICE_PROGRESS.md)** - 後端服務進度詳情

## 🔄 遷移文檔
- **[docs/master_python_to_go_mapping.md](docs/master_python_to_go_mapping.md)** - Master 服務 Python→Go 遷移映射
- **[docs/nodepool_python_to_go_mapping.md](docs/nodepool_python_to_go_mapping.md)** - Nodepool 服務 Python→Go 遷移映射
- **[docs/worker_python_to_go_mapping.md](docs/worker_python_to_go_mapping.md)** - Worker 服務 Python→Go 遷移映射

## 🎨 前端應用

### Master UI (任務管理)
- **位置**: `frontend/master-ui/`
- **端口**: http://localhost:3000
- **用途**: 用戶提交任務、查詢狀態、管理任務
- **啟動**: 
  ```bash
  cd frontend/master-ui
  npm install
  npm run dev
  ```

### Worker UI (節點管理)
- **位置**: `frontend/worker-ui/`
- **端口**: http://localhost:3001
- **用途**: Worker 節點註冊、硬體信息展示、狀態監控
- **啟動**:
  ```bash
  cd frontend/worker-ui
  npm install
  npm run dev
  ```

## 🔧 後端服務

### Master 服務
- **位置**: `services/master/cmd/server/`
- **端口**: 8082 (HTTP)
- **責任**: 
  - 用戶認證和授權
  - 任務 API 端點
  - Torrent 生成和存儲
  - 日誌上傳接收

### Nodepool 服務
- **位置**: `services/nodepool/cmd/server/`
- **端口**: 50051 (gRPC)
- **責任**:
  - Worker 節點管理
  - 任務分配和監控
  - 超時檢測和重分配
  - 計費結算
  - Redis 任務存儲

### Worker 服務
- **位置**: `services/worker/cmd/server/`
- **端口**: 50053 (gRPC)
- **責任**:
  - 任務執行
  - 結果上傳
  - 狀態報告

### Executor-rs (可選)
- **位置**: `executor-rs/`
- **責任**: 高性能任務執行引擎（Rust 實現）

## 📦 數據存儲

### Redis (6379)
- **用途**: 任務元數據存儲
- **Key 格式**:
  - `task:{task_id}` - 任務詳情 (Hash)
  - `tasks:owner:{owner}` - 用戶任務集 (Set)
  - `tasks:active` - 活躍任務集 (Set)
- **配置**: `NODEPOOL_REDIS_ADDR`

### PostgreSQL (5432)
- **用途**: 關聯式持久化資料（使用者、Worker、結算與任務資料）
- **配置**: `NODEPOOL_POSTGRES_DSN`

### 磁盤存儲
- **種子文件**: `api/torrents/` 目錄
- **日誌文件**: `nodepool.log` (UTF-8 日誌輸出)

## 🚀 部署環境

### 開發環境
- **Redis**: `localhost:6379`
- **Master**: `http://localhost:8082`
- **Nodepool**: `localhost:50051`
- **Master UI**: `http://localhost:3000`
- **Worker UI**: `http://localhost:3001`

### 生產環境
- Docker Compose 配置: 見 `DEPLOYMENT_GUIDE.md`
- 環境變量: 見 `QUICK_REFERENCE.md`

## 📝 使用指南

### 第一次啟動
1. 閱讀 [DEPLOYMENT_GUIDE.md](DEPLOYMENT_GUIDE.md) 的「快速啟動」部分
2. 複製啟動腳本到 PowerShell
3. 打開 http://localhost:3000 (Master UI)
4. 登錄: `testuser` / `testpass123`

### 監控日誌
```powershell
# 實時監控
Get-Content nodepool.log -Tail 20 -Wait

# 搜索特定任務
Select-String "task-id-123" nodepool.log

# 統計任務分配成功率
(Select-String "task_dispatch_success" nodepool.log).Count
```

### 故障排除
- 見 [DEPLOYMENT_GUIDE.md](DEPLOYMENT_GUIDE.md) 的「常見問題」部分
- 見 [QUICK_REFERENCE.md](QUICK_REFERENCE.md) 的「故障排除命令」

## 📊 關鍵指標

| 指標 | 值 |
|------|-----|
| 系統可用性 | ✅ 生產就緒 |
| 單元測試覆蓋 | 16/16 ✅ |
| 日誌記錄 | 詳細的 Redispatch 日誌 |
| 存儲層 | PostgreSQL + Redis + 磁盤 |
| UI 應用 | Master + Worker 分離 |

## 🔄 工作流程

### 用戶提交任務 (Master UI)
1. 登錄系統
2. 上傳 ZIP 文件（自動生成 Torrent）或指定 Magnet/HTTP URL
3. 設置資源要求（CPU/GPU/內存）
4. 提交任務
5. 查詢任務狀態和日誌

### Worker 節點管理 (Worker UI)
1. 登錄系統
2. 刷新 Worker 狀態（讀取本地硬體配置）
3. 一鍵註冊 Worker 節點
4. 監控節點狀態

### 後台任務執行
1. Nodepool 分配任務到可用 Worker
2. Worker 執行任務
3. Worker 上傳結果
4. Nodepool 進行計費結算
5. 用戶查詢結果

## 🎓 技術棧

### 後端
- **語言**: Go 1.20+
- **通訊**: gRPC + HTTP
- **數據庫**: PostgreSQL + Redis
- **認證**: JWT (bcrypt 加密)

### 前端
- **框架**: React 18 + Vite
- **語言**: JavaScript (JSX)
- **樣式**: 內聯 CSS

### 基礎設施
- **容器**: Docker (Redis)
- **編程語言**: Go, JavaScript, Rust (Executor-rs)

## 📞 支援

### 常見問題
見 [DEPLOYMENT_GUIDE.md](DEPLOYMENT_GUIDE.md) 中的「常見問題」和「故障轉移和監控」

### 獲取幫助
1. 查看相關文檔
2. 檢查 `nodepool.log` 日誌文件
3. 運行故障排除命令 (見 [QUICK_REFERENCE.md](QUICK_REFERENCE.md))

### 報告 Bug
在提交 bug 報告時，請包括:
- 錯誤消息
- 相關日誌摘錄
- 重現步驟
- 系統信息

---

## 文檔更新時間線

| 日期 | 文檔 | 變更 |
|------|------|------|
| 2026-03-25 | UPDATE_SUMMARY_2026_03_25.md | 首次發布 |
| 2026-03-25 | REDIS_MIGRATION_PLAN.md | 詳細的 Redis 遷移計劃 |
| 2026-03-25 | UI_SEPARATION_PLAN.md | Master/Worker UI 分離方案 |
| 2026-03-25 | DEPLOYMENT_GUIDE.md | 完整部署指南 |
| 2026-03-25 | QUICK_REFERENCE.md | 快速參考卡 |
| 2026-03-25 | STATUS_REPORT.md | 系統狀態報告 |
| 2026-03-25 | 本文件 | 文檔索引 |

---

**最後更新**: 2026-03-25 14:35 UTC  
**維護者**: Hivemind 開發團隊  
**許可證**: MIT (待定)



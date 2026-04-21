# UI 分離計劃

## 目標
將統一的前端分離為兩個獨立的 UI 應用：
- **Master UI** (端口 3000) - 用於任務提交和管理
- **Worker UI** (端口 3001) - 用於工作節點控制面板

## 架構設計

### Master UI
**功能**:
- 用戶登錄/註冊
- 查看賬戶餘額
- 提交新任務（上傳 ZIP 或指定 Magnet/HTTP URL）
- 查看任務列表和狀態
- 查看任務日誌和結果
- 停止任務

**構成**:
- 基於當前 App.jsx
- 連接到 Master API (localhost:8082)
- 不需要 Worker Control API

### Worker UI
**功能**:
- Worker 節點註冊和登錄
- 查看 Worker 硬件信息
- 實時監控 CPU/GPU/內存使用
- 查看當前任務和進度
- 查看歷史任務記錄

**構成**:
- 簡化版本，專注於工作節點監控
- 連接到 Worker Service (localhost:50053 gRPC)
- 本地儀表板展示

## 實現步驟

### Phase 1: 項目結構
```
frontend/
├── master-ui/          # Master 應用
│   ├── src/
│   │   ├── App.jsx
│   │   ├── components/
│   │   └── main.jsx
│   ├── package.json
│   └── vite.config.js
├── worker-ui/          # Worker 應用
│   ├── src/
│   │   ├── App.jsx
│   │   ├── components/
│   │   └── main.jsx
│   ├── package.json
│   └── vite.config.js
└── shared/             # 共享組件和工具
    ├── api/
    ├── components/
    └── utils/
```

### Phase 2: Master UI 開發
1. 複製當前 App.jsx 內容作為基礎
2. 移除所有 Worker 相關邏輯
3. 保留：
   - 登錄/註冊
   - 任務提交（ZIP 上傳）
   - 任務管理
   - 餘額查詢

### Phase 3: Worker UI 開發
1. 從零開始建構新 UI
2. 實現：
   - Worker 節點登錄
   - 硬件信息展示（CPU/GPU/內存）
   - 實時任務監控
   - 性能指標儀表板

### Phase 4: 部署配置
**Master UI**:
- npm run dev: localhost:3000
- npm run build: 生產構建
- VITE_API_BASE = http://localhost:8082

**Worker UI**:
- npm run dev: localhost:3001
- npm run build: 生產構建
- VITE_WORKER_API_BASE = http://localhost:50053

## 環境變量

**Master UI (.env)**:
```
VITE_API_BASE=http://localhost:8082
```

**Worker UI (.env)**:
```
VITE_WORKER_API_BASE=http://localhost:50053
VITE_WORKER_CONTROL_BASE=http://localhost:18080
```

## 時間線
- Phase 1-2: 本週內完成
- Phase 3: 下週
- Phase 4: 整合測試和部署

## 技術棧
- React 18 + Vite
- TailwindCSS (UI 框架)
- axios (HTTP 客戶端)
- protobufjs (gRPC 支持，如需要)

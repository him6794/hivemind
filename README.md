# HiveMind 分布式計算平台

## 項目概述
HiveMind是一個高效的分布式計算平台，旨在通過將計算任務分配到多個節點來實現高性能計算。平台採用模塊化架構，支持動態節點分配、資源監控和安全隔離，為用戶提供靈活且強大的分布式計算解決方案。

## 核心功能
- **模塊化架構**：將任務分解為可管理的子任務，實現計算能力的自由擴展
- **即時節點分配**：節點池自動分配最合適的節點，支持自定義任務標準
- **安全隔離**：任務在Docker容器中執行，確保節點之間的獨立性和安全性
- **高性能處理**：利用分布式架構提供卓越的計算能力和可伸縮性
- **資源監控**：實時追蹤CPU、內存和GPU使用率，優化資源分配
- **獎勵機制**：根據節點貢獻（CPU、內存和GPU使用率）計算獎勵，通過使用率倍數(0.8-1.5)和GPU獎勵係數(使用率的1%)計算總獎勵

## 系統架構
### 主要模組
1. **主控節點 (master)**
   - 負責任務分配、節點管理和協調：基於節點負載值動態分配任務
   - 提供Web管理界面和API接口：使用Flask框架構建RESTful API
   - 維護節點狀態和任務進度：通過gRPC與工作節點通信，使用Protocol Buffers定義數據結構
   - 任務存儲管理：將任務數據存儲在/mnt/myusb/hivemind/task_storage目錄

2. **工作節點 (worker)**
   - 執行分配的計算任務：通過Docker容器化運行任務，使用justin308/hivemind-worker鏡像
   - 監控本地資源使用情況：實時採集CPU、內存和GPU使用率，每30秒向主控節點報告
   - 任務生命週期管理：負責任務的啟動、監控、終止和結果回傳
   - 自動重連機制：斷網後自動嘗試重新連接主控節點

3. **節點池管理 (node_pool)**
   - 管理節點註冊和狀態追蹤，通過定期心跳檢查節點健康狀態
   - 實現節點負載均衡：基於CPU使用率、內存佔用率計算節點負載值(max(cpu_usage, memory_usage))，優先選擇低負載節點分配任務
   - 處理節點間通信和數據傳輸，使用gRPC協議進行節點間高效通信
   - 任務狀態監控：記錄任務執行狀態和資源使用統計，用於獎勵計算和故障恢復

4. **Web界面 (web)**
   - 用戶交互界面，支持任務提交和監控：使用Flask框架和模板引擎構建
   - 節點狀態可視化：實時顯示CPU、內存和GPU使用率統計
   - 用戶賬戶和權限管理：支持註冊、登錄和餘額管理功能
   - VPN配置生成：自動生成WireGuard VPN配置文件，支持節點安全通信

## 安裝指南
### 先決條件
- Python 3.8+ 
- Docker
- Git
- pip (Python包管理工具)
- 網絡連接

### 快速安裝
1. 克隆倉庫
```bash
```

2. 安裝依賴
```bash
pip install -r requirements.txt
```

3. 啟動主控節點
```bash
cd master
python master_node.py
# 或運行打包好的可執行文件
HiveMind-Master.exe
```

4. 啟動工作節點
```bash
cd worker
python worker_node.py
# 或運行打包好的可執行文件
HiveMind-Worker.exe
```

5. 訪問Web界面
打開瀏覽器訪問: http://localhost:5000

## 使用方法
### 基本流程
1. **註冊賬戶**
   - 在Web界面完成用戶註冊
   - 登錄系統獲取認證令牌

2. **提交任務**
   - 準備任務代碼和數據
   - 通過Web界面或API上傳任務
   - 配置資源需求（CPU、內存、GPU等）

3. **監控任務**
   - 在儀表板查看任務狀態
   - 監控資源使用情況
   - 查看任務日誌和輸出

4. **獲取結果**
   - 任務完成後下載結果
   - 查看計算統計和資源消耗報告

### 示例代碼
通過API提交任務：
```python
import requests

API_URL = "http://localhost:5000/api/tasks"
TOKEN = "your_auth_token"

payload = {
    "task_name": "image_processing",
    "task_file": open("task.zip", "rb"),
    "cpu_cores": 4,
    "memory_gb": 8,
    "gpu_required": True
}

headers = {"Authorization": f"Bearer {TOKEN}"}
response = requests.post(API_URL, files=payload, headers=headers)
print(response.json())
```

## 技術細節
### 通信協議
- 使用gRPC進行節點間高效通信
- 定義了嚴格的Protobuf數據結構
- 支持同步和異步任務處理

### 容器化技術
- 所有任務在隔離的Docker容器中執行
- 預定義多種基礎鏡像（Python、CUDA等）
- 自動資源限制和隔離

### 資源管理
- 實時監控CPU、內存和GPU使用率
- 基於負載的動態任務調度
- 資源使用統計和獎勵計算

### 安全特性
- 節點身份驗證和授權
- 數據傳輸加密
- 任務隔離和資源限制

## 開發和貢獻
### 項目結構
```
hivemind/
├── master/         # 主控節點
├── node_pool/      # 節點池管理
├── web/            # Web界面
├── worker/         # 工作節點
└── requirements.txt # 依賴管理
```

### 構建可執行文件
主控節點打包：
```bash
cd master
python build.py
```

工作節點打包：
```bash
cd worker
python make.py
```

## 許可證
本項目採用GNU General Public License v3.0許可證 - 詳見LICENSE.txt文件。

## 聯繫我們
- 項目網站: https://hivemind.justin0711.com
- 支持郵箱: hivemind@justin0711.com